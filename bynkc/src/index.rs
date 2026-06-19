//! v0.25: the project-wide binding index (ADR 0053).
//!
//! [`RefSink`] collects use→def edges at the resolution sites themselves —
//! the resolver's reference walk, the checker's capability/service call
//! dispatch, and the project driver's clause wiring — mirroring v0.24's
//! `ErrorSink` collection-point pattern. The project pass then qualifies
//! bare names per unit and assembles a [`ProjectIndex`]: every in-scope
//! symbol's definition site plus all of its reference sites, binding-correct
//! (never name-matched).
//!
//! In-scope symbol kinds this increment: top-level types, free `fn`s,
//! capabilities, services, agents, and providers. Instance methods, record
//! fields, capability op names, and local bindings are deferred (no edges
//! are recorded for them).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::span::Span;

/// The kind half of a symbol's structural key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolKind {
    Type,
    Fn,
    Capability,
    Service,
    Agent,
    Provider,
    /// v0.36 (ADR 0069): an instance method, keyed by the compound name
    /// `"Type.method"` in the type's defining unit. The first parent-scoped
    /// index kind (see the v0.36 members slice).
    Method,
    /// v0.36 (ADR 0069, slice 2): a record field, keyed by `"Type.field"`.
    Field,
    /// v0.36 (ADR 0069, slice 2): a capability operation, keyed by `"Cap.op"`.
    CapabilityOp,
    /// v0.45: an actor declaration — a boundary contract consumed by a
    /// handler's `by` clause.
    Actor,
}

impl SymbolKind {
    pub fn display(self) -> &'static str {
        match self {
            SymbolKind::Type => "type",
            SymbolKind::Fn => "fn",
            SymbolKind::Capability => "capability",
            SymbolKind::Service => "service",
            SymbolKind::Agent => "agent",
            SymbolKind::Provider => "provider",
            SymbolKind::Method => "method",
            SymbolKind::Field => "field",
            SymbolKind::CapabilityOp => "operation",
            SymbolKind::Actor => "actor",
        }
    }
}

/// Structural symbol identity (no `DefId` plumbing): the defining unit's
/// qualified name, the declaration kind, and the declared name. Top-level
/// names are unique within a unit, so the key is unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolKey {
    pub unit: String,
    pub kind: SymbolKind,
    pub name: String,
}

/// One recorded use→def edge, in collection-point context.
///
/// `unit: None` means the name resolved through the recording namespace's
/// merged tables (local declarations + `uses` imports) and is qualified at
/// assembly; `Some` means the resolution site already knew the defining
/// unit (cross-context capability/service references, flattened caps).
#[derive(Debug, Clone)]
pub struct RefEdge {
    /// The name-segment span (for dotted `B.Cap`, just `Cap`).
    pub span: Span,
    pub kind: SymbolKind,
    pub name: String,
    pub unit: Option<String>,
    /// Project-relative file the span is an offset into (collection point).
    pub file: PathBuf,
    /// The unit whose merged namespace resolves a bare (`unit: None`) name.
    /// For test/integration files this is the *target* unit.
    pub namespace: Option<String>,
    /// Display name of the enclosing top-level declaration, when known
    /// (`"f"`, `"T.m"`, a service/provider name). Used at assembly to
    /// re-attribute spans to the file that declares the owner — sibling-file
    /// methods and unit-level handler tables are processed under a different
    /// file than the one their spans index into.
    pub owner: Option<String>,
    /// v0.35 (ADR 0068): set only on the `Cap` of a `provides Cap = Provider`
    /// clause (never on a `given Cap` dependency). With `owner` the provider,
    /// this marks a capability→provider implementation edge — distinguishing
    /// the provided capability from the provider's own `given` deps, which are
    /// also capability refs owned by the same provider.
    pub provides: bool,
}

/// Collection-point sink for use→def edges (the `ErrorSink` analogue).
/// The pipeline sets the ambient file/namespace before each per-file phase;
/// resolution sites only supply the span and target. A sink left in its
/// default state (no file) discards edges — the single-file entry points
/// resolve without recording.
#[derive(Debug, Default)]
pub struct RefSink {
    pub edges: Vec<RefEdge>,
    /// Synthetic namespaces (integration-test harness roots) → their `uses`
    /// resolution order, merged with the project's `uses` table at assembly.
    pub extra_uses: HashMap<String, Vec<String>>,
    file: Option<PathBuf>,
    namespace: Option<String>,
    owner: Option<String>,
    /// Set while processing synthetic (toolchain-injected) files: edges are
    /// discarded — first-party units are not user-editable and out of index.
    muted: bool,
}

impl RefSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a synthetic namespace's `uses` resolution order (integration
    /// harness roots are not project units, so the project's `uses` table
    /// has no entry for them).
    pub fn declare_namespace(&mut self, namespace: &str, uses: Vec<String>) {
        self.extra_uses.insert(namespace.to_string(), uses);
    }

    /// Enter a per-file recording context. `namespace` is the unit whose
    /// merged tables resolve bare names in this file (the file's own unit,
    /// or a test file's target unit).
    pub fn enter_file(&mut self, file: &Path, namespace: &str, muted: bool) {
        self.file = Some(file.to_path_buf());
        self.namespace = Some(namespace.to_string());
        self.owner = None;
        self.muted = muted;
    }

    /// Set the enclosing top-level declaration for subsequent edges.
    pub fn set_owner(&mut self, owner: impl Into<String>) {
        self.owner = Some(owner.into());
    }

    pub fn clear_owner(&mut self) {
        self.owner = None;
    }

    /// Record an edge whose defining unit is found at assembly.
    pub fn record(&mut self, span: Span, kind: SymbolKind, name: &str) {
        self.push(span, kind, name, None, false);
    }

    /// Record an edge whose defining unit the resolution site already knows.
    pub fn record_in_unit(&mut self, span: Span, kind: SymbolKind, name: &str, unit: &str) {
        self.push(span, kind, name, Some(unit.to_string()), false);
    }

    /// v0.35 (ADR 0068): record the `Cap` of a `provides Cap = Provider` clause
    /// — a capability reference also flagged as an implementation edge (the
    /// owner is the provider). `unit` is `Some` for a cross-context provided
    /// capability, `None` when it resolves at assembly.
    pub fn record_provides(&mut self, span: Span, name: &str, unit: Option<&str>) {
        self.push(
            span,
            SymbolKind::Capability,
            name,
            unit.map(str::to_string),
            true,
        );
    }

    fn push(
        &mut self,
        span: Span,
        kind: SymbolKind,
        name: &str,
        unit: Option<String>,
        provides: bool,
    ) {
        if self.muted {
            return;
        }
        let Some(file) = &self.file else {
            return; // single-file mode: no recording context.
        };
        self.edges.push(RefEdge {
            span,
            kind,
            name: name.to_string(),
            unit,
            file: file.clone(),
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
            provides,
        });
    }
}

/// One occurrence of a symbol: the file (project-relative) and the
/// name-segment span within that file's analysed snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SiteRef {
    pub path: PathBuf,
    pub span: Span,
}

/// v0.28 (ADR 0057): the Bynk-specific semantic-token modifiers recorded on
/// a symbol at assemble time. `refined` only when a refinement is present —
/// `type Age = Int` parses as `Refined { refinement: None }` and is a plain
/// alias, carrying neither; `opaque` is orthogonal, so `opaque B where …`
/// carries both. `platform_native` when the declaring unit is a platform
/// adapter (`firstparty::platform_of` is `Some`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SymbolModifiers {
    pub refined: bool,
    pub opaque: bool,
    pub platform_native: bool,
}

/// A symbol's definition site plus every reference site.
#[derive(Debug, Clone, Default)]
pub struct SymbolEntry {
    /// The declaration's name span. `None` only transiently during assembly;
    /// symbols without a located definition are dropped from the index.
    pub def: Option<SiteRef>,
    /// Sorted, deduplicated. Does not include the definition site.
    pub refs: Vec<SiteRef>,
    /// v0.28 (ADR 0057): semantic-token modifiers, set from the declaration.
    pub modifiers: SymbolModifiers,
}

/// v0.34 (ADR 0067): one resolved caller→callee call edge — a `Fn` reference
/// (`callee`) occurring inside a known top-level declaration (`caller`), at
/// `site` (the callee-name span, in the caller's file). The backing data for
/// call hierarchy: incoming calls group edges by `callee`, outgoing by
/// `caller`. v0.36 (ADR 0069): `Fn` and `Method` callees/callers; op-call and
/// agent-dispatch edges are still absent (those callees aren't index symbols —
/// the remaining deferred index kinds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEdge {
    pub caller: SymbolKey,
    pub callee: SymbolKey,
    pub site: SiteRef,
}

/// v0.35 (ADR 0068): one capability→provider implementation edge — a `provides
/// Cap = P` clause records a `Capability` reference (`capability`) whose
/// enclosing owner is the provider (`provider`), at `site` (the capability-name
/// span in the `provides` clause). The backing data for implementation
/// navigation: `implementation` on a capability returns its providers' defs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplEdge {
    pub capability: SymbolKey,
    pub provider: SymbolKey,
    pub site: SiteRef,
}

/// v0.28 (ADR 0057): one reference to a first-party (`bynk.*`) symbol.
/// Tokens-only: first-party defs point at synthetic files not on disk, so
/// these sites are **never** read by definition/rename/workspace-symbol —
/// the v0.25 exclusion of synthetic units from `symbols` stands untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRef {
    pub site: SiteRef,
    pub kind: SymbolKind,
    pub modifiers: SymbolModifiers,
}

/// The project-wide binding index: every in-scope symbol's definition and
/// references, keyed structurally. Built by the v0.24 project pass in
/// analyse mode; empty in build mode.
#[derive(Debug, Clone, Default)]
pub struct ProjectIndex {
    pub symbols: HashMap<SymbolKey, SymbolEntry>,
    /// v0.28 (ADR 0057): references to first-party symbols, sorted by
    /// (path, span), deduplicated — read only by the semantic-tokens
    /// producer (see [`ForeignRef`]).
    pub foreign_refs: Vec<ForeignRef>,
    /// v0.34 (ADR 0067): caller→callee call edges (`Fn` callees only), sorted
    /// by (caller, callee, site). The call-hierarchy graph (see [`CallEdge`]).
    pub calls: Vec<CallEdge>,
    /// v0.35 (ADR 0068): capability→provider implementation edges, sorted by
    /// (capability, provider, site). The implementation-nav graph (see
    /// [`ImplEdge`]).
    pub impls: Vec<ImplEdge>,
}

impl ProjectIndex {
    /// The symbol whose definition or reference name-segment contains
    /// `offset` within `path`. Spans are half-open; name segments never
    /// overlap, so the first hit is the only hit.
    pub fn symbol_at(&self, path: &Path, offset: usize) -> Option<(&SymbolKey, &SiteRef)> {
        for (key, entry) in &self.symbols {
            if let Some(def) = &entry.def
                && def.path == path
                && def.span.range().contains(&offset)
            {
                return Some((key, def));
            }
            for site in &entry.refs {
                if site.path == path && site.span.range().contains(&offset) {
                    return Some((key, site));
                }
            }
        }
        None
    }

    /// Definition + references for `key`, definition first.
    pub fn sites(&self, key: &SymbolKey) -> Vec<&SiteRef> {
        let Some(entry) = self.symbols.get(key) else {
            return Vec::new();
        };
        entry.def.iter().chain(entry.refs.iter()).collect()
    }

    /// v0.34 (ADR 0067): call edges whose callee is `key` — its callers.
    pub fn calls_into<'a>(&'a self, key: &SymbolKey) -> impl Iterator<Item = &'a CallEdge> {
        let key = key.clone();
        self.calls.iter().filter(move |e| e.callee == key)
    }

    /// v0.34 (ADR 0067): call edges whose caller is `key` — what it calls.
    pub fn calls_from<'a>(&'a self, key: &SymbolKey) -> impl Iterator<Item = &'a CallEdge> {
        let key = key.clone();
        self.calls.iter().filter(move |e| e.caller == key)
    }

    /// v0.35 (ADR 0068): impl edges whose capability is `key` — its providers.
    pub fn impls_of<'a>(&'a self, key: &SymbolKey) -> impl Iterator<Item = &'a ImplEdge> {
        let key = key.clone();
        self.impls.iter().filter(move |e| e.capability == key)
    }

    /// Structural equality after mapping `self`'s sites through `remap`
    /// and renaming `from` to `to_name` — the rename capture/escape
    /// validator. `remap` converts a pre-edit site to its post-edit
    /// position (rename edits shift spans within edited files).
    pub fn equals_modulo_rename(
        &self,
        post: &ProjectIndex,
        from: &SymbolKey,
        to_name: &str,
        mut remap: impl FnMut(&SiteRef) -> SiteRef,
    ) -> bool {
        if self.symbols.len() != post.symbols.len() {
            return false;
        }
        for (key, entry) in &self.symbols {
            let expect_key = if key == from {
                SymbolKey {
                    unit: key.unit.clone(),
                    kind: key.kind,
                    name: to_name.to_string(),
                }
            } else {
                key.clone()
            };
            let Some(post_entry) = post.symbols.get(&expect_key) else {
                return false;
            };
            let expect_def = entry.def.as_ref().map(&mut remap);
            if expect_def != post_entry.def {
                return false;
            }
            let mut expect_refs: Vec<SiteRef> = entry.refs.iter().map(&mut remap).collect();
            expect_refs.sort();
            let mut post_refs = post_entry.refs.clone();
            post_refs.sort();
            if expect_refs != post_refs {
                return false;
            }
        }
        true
    }
}

/// Assembles the index from per-file declaration walks plus the sink's
/// edges. Built by the project pass, which alone knows unit membership,
/// `uses` targets, and which file declares each top-level item.
#[derive(Debug, Default)]
pub struct IndexBuilder {
    /// (unit, kind, name) → definition site + modifiers.
    defs: HashMap<SymbolKey, (SiteRef, SymbolModifiers)>,
    /// v0.28 (ADR 0057): first-party (`bynk.*`) symbols — kind + modifiers
    /// only, no usable def site (synthetic files are not on disk). Edges
    /// qualifying here route into [`ProjectIndex::foreign_refs`].
    first_party_defs: HashMap<SymbolKey, SymbolModifiers>,
    /// (unit, owner display name) → declaring file, for span re-attribution.
    /// Includes methods (`"T.m"`), which are not index symbols.
    owner_files: HashMap<(String, String), PathBuf>,
    /// v0.34 (ADR 0067): (unit, owner display name) → the owner's symbol key,
    /// for resolving a call edge's caller. Only index symbols (every
    /// `add_def`); method owners (`add_owner`) are absent, so their call edges
    /// are not recorded — same boundary as the deferred index kinds.
    owner_keys: HashMap<(String, String), SymbolKey>,
    /// unit → `uses` targets, resolution order.
    uses: HashMap<String, Vec<String>>,
    /// unit → `consumes` targets — bare names can also resolve to a consumed
    /// unit's exported types (the consumer's merged table layers them after
    /// `uses` imports).
    consumes: HashMap<String, Vec<String>>,
}

impl IndexBuilder {
    pub fn add_def(
        &mut self,
        unit: &str,
        kind: SymbolKind,
        name: &str,
        site: SiteRef,
        modifiers: SymbolModifiers,
    ) {
        self.owner_files
            .insert((unit.to_string(), name.to_string()), site.path.clone());
        let key = SymbolKey {
            unit: unit.to_string(),
            kind,
            name: name.to_string(),
        };
        self.owner_keys
            .insert((unit.to_string(), name.to_string()), key.clone());
        self.defs.insert(key, (site, modifiers));
    }

    /// v0.28 (ADR 0057): register a first-party symbol for the second
    /// qualification pass — kind + modifiers only, no def site.
    pub fn add_first_party_def(
        &mut self,
        unit: &str,
        kind: SymbolKind,
        name: &str,
        modifiers: SymbolModifiers,
    ) {
        self.first_party_defs.insert(
            SymbolKey {
                unit: unit.to_string(),
                kind,
                name: name.to_string(),
            },
            modifiers,
        );
    }

    /// Register a non-symbol owner (a method) for attribution only.
    pub fn add_owner(&mut self, unit: &str, owner: &str, path: &Path) {
        self.owner_files
            .insert((unit.to_string(), owner.to_string()), path.to_path_buf());
    }

    pub fn set_uses(&mut self, uses: HashMap<String, Vec<String>>) {
        self.uses = uses;
    }

    pub fn set_consumes(&mut self, consumes: HashMap<String, Vec<String>>) {
        self.consumes = consumes;
    }

    /// Qualify, attribute, dedupe, and assemble.
    pub fn build(self, edges: Vec<RefEdge>) -> ProjectIndex {
        let mut index = ProjectIndex::default();
        for (key, (def, modifiers)) in &self.defs {
            index.symbols.insert(
                key.clone(),
                SymbolEntry {
                    def: Some(def.clone()),
                    refs: Vec::new(),
                    modifiers: *modifiers,
                },
            );
        }
        let mut seen: HashSet<(PathBuf, Span, SymbolKey)> = HashSet::new();
        let mut foreign_seen: HashSet<(PathBuf, Span, SymbolKind)> = HashSet::new();
        let mut calls: Vec<CallEdge> = Vec::new();
        let mut impls: Vec<ImplEdge> = Vec::new();
        for edge in edges {
            // Re-attribute to the owner's declaring file when the owner
            // lives in a different file than the collection point: sibling-
            // file methods and unit-level handler tables are processed under
            // a file other than the one their spans index into. The owner is
            // declared in the *namespace* unit (the unit being processed).
            let path = edge
                .owner
                .as_ref()
                .zip(edge.namespace.as_ref())
                .and_then(|(o, ns)| self.owner_files.get(&(ns.clone(), o.clone())))
                .cloned()
                .unwrap_or_else(|| edge.file.clone());
            let Some(key) = self.qualify(&edge) else {
                // v0.28 (ADR 0057): second pass — a positive match against
                // the first-party defs routes into the tokens-only side
                // table; genuinely unresolved targets stay dropped.
                if let Some(key) =
                    self.qualify_with(&edge, |k| self.first_party_defs.contains_key(k))
                    && foreign_seen.insert((path.clone(), edge.span, key.kind))
                {
                    index.foreign_refs.push(ForeignRef {
                        site: SiteRef {
                            path,
                            span: edge.span,
                        },
                        kind: key.kind,
                        modifiers: self.first_party_defs[&key],
                    });
                }
                continue;
            };
            let entry = index.symbols.entry(key.clone()).or_default();
            let Some(def) = &entry.def else {
                continue;
            };
            let site = SiteRef {
                path,
                span: edge.span,
            };
            // The definition's own name span is not also a reference.
            if site == *def {
                continue;
            }
            if seen.insert((site.path.clone(), site.span, key.clone())) {
                // v0.34 (ADR 0067): a `Fn` call inside a known top-level owner
                // is a call edge. The caller resolves via `owner_keys` exactly
                // as the file re-attribution above resolves `owner_files`.
                // v0.36 (ADR 0069): methods are call targets too, now that they
                // are `add_def`'d index symbols (and callers, since `add_def`
                // populates `owner_keys` for `"T.m"` owners).
                if matches!(key.kind, SymbolKind::Fn | SymbolKind::Method)
                    && let Some(caller) = edge
                        .owner
                        .as_ref()
                        .zip(edge.namespace.as_ref())
                        .and_then(|(o, ns)| self.owner_keys.get(&(ns.clone(), o.clone())))
                {
                    calls.push(CallEdge {
                        caller: caller.clone(),
                        callee: key.clone(),
                        site: site.clone(),
                    });
                }
                // v0.35 (ADR 0068): a `provides Cap = Provider` clause — a
                // provides-flagged `Capability` ref whose owner is the provider.
                // The flag distinguishes it from the provider's `given` deps,
                // which are also capability refs owned by the same provider.
                if edge.provides
                    && let Some(provider) = edge
                        .owner
                        .as_ref()
                        .zip(edge.namespace.as_ref())
                        .and_then(|(o, ns)| self.owner_keys.get(&(ns.clone(), o.clone())))
                    && provider.kind == SymbolKind::Provider
                {
                    impls.push(ImplEdge {
                        capability: key.clone(),
                        provider: provider.clone(),
                        site: site.clone(),
                    });
                }
                entry.refs.push(site);
            }
        }
        for entry in index.symbols.values_mut() {
            entry.refs.sort();
        }
        index.symbols.retain(|_, e| e.def.is_some());
        index.foreign_refs.sort_by(|a, b| a.site.cmp(&b.site));
        calls.sort_by(|a, b| (&a.caller, &a.callee, &a.site).cmp(&(&b.caller, &b.callee, &b.site)));
        index.calls = calls;
        impls.sort_by(|a, b| {
            (&a.capability, &a.provider, &a.site).cmp(&(&b.capability, &b.provider, &b.site))
        });
        index.impls = impls;
        index
    }

    fn qualify(&self, edge: &RefEdge) -> Option<SymbolKey> {
        self.qualify_with(edge, |k| self.defs.contains_key(k))
    }

    /// The merged-table qualification against an arbitrary def set: a
    /// site-known unit is looked up directly; a bare name layers local
    /// first, then `uses` imports, then consumed units' exported types —
    /// first hit wins, matching the pipeline's `or_insert` merge priority.
    fn qualify_with(&self, edge: &RefEdge, has: impl Fn(&SymbolKey) -> bool) -> Option<SymbolKey> {
        if let Some(unit) = &edge.unit {
            let key = SymbolKey {
                unit: unit.clone(),
                kind: edge.kind,
                name: edge.name.clone(),
            };
            return has(&key).then_some(key);
        }
        let ns = edge.namespace.as_ref()?;
        let local = SymbolKey {
            unit: ns.clone(),
            kind: edge.kind,
            name: edge.name.clone(),
        };
        if has(&local) {
            return Some(local);
        }
        for target in self
            .uses
            .get(ns)
            .into_iter()
            .flatten()
            .chain(self.consumes.get(ns).into_iter().flatten())
        {
            let imported = SymbolKey {
                unit: target.clone(),
                kind: edge.kind,
                name: edge.name.clone(),
            };
            if has(&imported) {
                return Some(imported);
            }
        }
        None
    }
}
