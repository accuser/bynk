//! Name resolution (spec §5.1, v0.1 §4.1, v0.2 §4.1).
//!
//! Builds symbol tables for the commons and validates that:
//! - No two top-level items share a name (types, fns, methods are all named).
//! - Every `TypeRef::Named` resolves to a declared type.
//! - Every free function call resolves to a function declaration.
//! - Every identifier in expression position resolves to a parameter, a
//!   `let` binding, or `self` (inside a method).
//! - Constructor / static calls (`TypeName.method(args)`) resolve either to
//!   the built-in `T.of` of a refined type, a static method on `T`, or a
//!   variant constructor when `T` is a sum type.
//! - Record construction targets a declared record type and uses only
//!   declared fields.
//! - Method calls resolve via the receiver's nominal type (the actual type
//!   check happens in the type checker).
//!
//! On success returns a [`ResolvedCommons`] — the original AST plus
//! symbol tables the type checker consumes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::index::{RefSink, SymbolKind};
use bynk_project::UnitKind;
use bynk_syntax::ast::*;
use bynk_syntax::error::{Applicability, CompileError};
use bynk_syntax::span::Span;

/// Is `name` a type imported via `uses` of a *commons* specifically — the
/// exact predicate [`ResolvedCommons::is_uses_commons_type`] caches as
/// `uses_commons_type_names`, and `bynk-emit` needs at *two* real call
/// sites that must never disagree: `emit_context_rebrands`'s own two steps,
/// "alias the import" and "rebrand the type" (its own doc comment, step 1
/// "Done in imports", step 2 the rebrand itself, both in
/// `bynk-emit/src/emitter.rs`) — an import narrower than the rebrand leaves
/// an undefined name in the generated module; a rebrand narrower than the
/// import leaves an alias imported and never used.
///
/// R4.10/R8.2 (`design/bynk-greenfield-compiler.md`): before this function
/// existed, `prepare_unit_check_ctx` (`check_pipeline.rs`) and *both*
/// `bynk-emit` call sites above each independently inlined this same
/// two-condition check, linked only by a doc comment promising they all
/// matched exactly — a real risk ADR 0226 names (#655: "a single named
/// binder took the entire test run down, pointing at generated code the
/// author never wrote"). One definition, every caller reads it — an edit to
/// either condition can no longer silently update only one side.
pub fn compute_is_uses_commons_type(
    imported_from_kind: &HashMap<String, UnitKind>,
    types: &HashMap<String, Arc<TypeDecl>>,
    name: &str,
) -> bool {
    matches!(imported_from_kind.get(name), Some(UnitKind::Commons)) && types.contains_key(name)
}

#[cfg(test)]
mod compute_is_uses_commons_type_tests {
    use super::compute_is_uses_commons_type;
    use bynk_project::UnitKind;
    use bynk_syntax::ast::{Ident, RecordBody, Trivia, TypeBody, TypeDecl};
    use bynk_syntax::span::Span;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn bare_record_type(name: &str) -> Arc<TypeDecl> {
        Arc::new(TypeDecl {
            type_params: Vec::new(),
            name: Ident {
                name: name.to_string(),
                span: Span::default(),
            },
            body: TypeBody::Record(RecordBody {
                fields: Vec::new(),
                span: Span::default(),
            }),
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        })
    }

    /// The four combinations the real callers' own doc comments describe:
    /// a `uses`-imported commons type (the only `true` case), a
    /// `uses`-imported commons *function* (v0.20b's own carve-out — not
    /// rebranded, a value not a type), a name imported from a non-commons
    /// unit kind (a consumed context, say), and a name absent from
    /// `imported_from_kind` entirely (a local declaration).
    #[test]
    fn matches_a_commons_imported_type_only() {
        let mut kinds = HashMap::new();
        kinds.insert("Money".to_string(), UnitKind::Commons);
        kinds.insert("traverse".to_string(), UnitKind::Commons);
        kinds.insert("Order".to_string(), UnitKind::Context);
        let mut types = HashMap::new();
        types.insert("Money".to_string(), bare_record_type("Money"));
        types.insert("Order".to_string(), bare_record_type("Order"));

        assert!(compute_is_uses_commons_type(&kinds, &types, "Money"));
        assert!(
            !compute_is_uses_commons_type(&kinds, &types, "traverse"),
            "a uses-imported commons function is a value, not a type — v0.20b"
        );
        assert!(
            !compute_is_uses_commons_type(&kinds, &types, "Order"),
            "imported from a context, not a commons"
        );
        assert!(
            !compute_is_uses_commons_type(&kinds, &types, "Local"),
            "absent from imported_from_kind entirely — a local declaration"
        );
    }
}

/// The resolver's two collection points, bundled so the reference walk
/// threads one parameter (v0.25, ADR 0053). `push` forwards to the error
/// list, keeping the walk's error sites unchanged; binding edges record
/// via `refs` at the site that resolved them.
pub(crate) struct Sinks<'a> {
    errs: &'a mut Vec<CompileError>,
    pub(crate) refs: &'a mut RefSink,
}

impl Sinks<'_> {
    fn push(&mut self, e: CompileError) {
        self.errs.push(e);
    }
}

/// Per-type method table built during resolution: keyed by method name,
/// values are clones of the [`FnDecl`] for that method.
#[derive(Debug, Default, Clone)]
pub struct MethodTable {
    pub instance: HashMap<String, Arc<FnDecl>>,
    pub statics: HashMap<String, Arc<FnDecl>>,
}

/// Output of resolution: the AST plus the symbol tables the checker needs.
pub struct ResolvedCommons {
    pub commons: Commons,
    /// Finding #10/#51: `Arc`-wrapped (not owned) so cloning this map — done
    /// once per synthetic per-handler `ResolvedCommons` during emission — is
    /// a pointer bump, not a deep copy of every declaration body in the unit.
    pub types: HashMap<String, Arc<TypeDecl>>,
    /// Finding #10/#51: `Arc`-wrapped for the same reason as `types`.
    pub fns: HashMap<String, Arc<FnDecl>>,
    /// Per-type method tables (instance + static).
    pub methods: HashMap<String, MethodTable>,
    /// Names of types declared in *this* commons (as opposed to imported via
    /// `uses`). Used by the checker to gate access to `.raw` and `.unsafe()`
    /// on opaque types. Private: this field's contract is specifically
    /// "declared here, not merely visible here", and a builder outside this
    /// crate that populates it from the wrong (merged, rather than
    /// pre-merge) table silently over-widens those gates — read it via
    /// [`ResolvedCommons::is_local_type`], and build a `ResolvedCommons` via
    /// [`ResolvedCommons::new`], which derives it correctly by construction.
    pub(crate) local_type_names: std::collections::HashSet<String>,
    /// Cross-context call information for v0.6. None for commons and for
    /// single-file mode. For contexts, supplies the set of consumed contexts
    /// and any aliases introduced via `consumes ... as Alias`.
    pub cross_context: CrossContextInfo,
    /// Agents declared in this context. Used to recognise the `Agent(key)`
    /// construction shape and the `agent_instance.handler(args)` method-call
    /// shape in handler bodies that mention other agents.
    pub agents: HashMap<String, AgentDecl>,
    /// v0.91 (ADR 0116 D6): for each imported function name, the qualified unit
    /// it came from (`map` → `bynk.list`). Lets the checker flag deprecated
    /// first-party free functions at their call sites. Empty in single-file
    /// mode and in synthetic handler-validation resolveds.
    pub imported_from: HashMap<String, String>,
    /// True iff this unit is a `context` (as opposed to a commons, adapter, or
    /// test/integration scaffold). `bynk-check` has no dependency on
    /// `bynk-emit`'s `UnitKind`, so callers set this directly from their own
    /// unit-kind knowledge. Used to gate the context-rebrand construction
    /// check (#907): only a context's emission rebrands a `uses`-sourced
    /// commons sum type's variant constructors out of value scope.
    pub is_context: bool,
    /// Names of types brought into scope via `uses` of a *commons*
    /// specifically (as opposed to a local declaration, or a type surfaced
    /// via `consumes`) — every name [`compute_is_uses_commons_type`] accepts
    /// against this unit's own `imported_from_kind`/`combined_types`, the
    /// single shared definition both `bynk-emit`'s `emit_context_rebrands`
    /// (rebrand + its own import-aliasing step) and this crate's own
    /// `prepare_unit_check_ctx` (which populates this set) read (R4.10/R8.2,
    /// closing what used to be two independently hand-maintained copies
    /// linked only by a doc comment promising they matched). A type surfaced
    /// via `consumes` (a capability signature from an adapter or another
    /// context) is *not* rebranded and must not be gated by #907's check —
    /// only this narrower set may be. Private for the same reason as
    /// `local_type_names`; read via [`ResolvedCommons::is_uses_commons_type`].
    pub(crate) uses_commons_type_names: std::collections::HashSet<String>,
    /// Events track, slice 0 (spine #936): names of `event` declarations in
    /// *this* commons specifically — as opposed to `local_type_names`, which
    /// answers "declared here" for any type, event-derived or not. Backs the
    /// `Events.emit[E]` check that `E` names a real event, not merely any
    /// local type (owner-only emission alone can't tell the two apart, since
    /// an event's synthetic `TypeDecl` sits in the same `types` table as
    /// every ordinary type). Private for the same reason as
    /// `local_type_names`; read via [`ResolvedCommons::is_local_event`].
    pub(crate) event_type_names: std::collections::HashSet<String>,
}

/// Static information about the consuming context: the set of contexts it
/// `consumes`, and any aliases introduced via `as Alias` clauses. Used by
/// the resolver to recognise cross-context service calls and by the checker
/// to type them (v0.6 §4.2).
#[derive(Debug, Default, Clone)]
pub struct CrossContextInfo {
    /// The qualified name of the consuming context, if this unit is a context.
    pub self_context: Option<String>,
    /// Qualified names of every consumed context.
    pub consumed_contexts: Vec<String>,
    /// alias → consumed-context qualified name.
    pub aliases: HashMap<String, String>,
    /// For each consumed context, its service surface plus the structural
    /// shapes of each service handler's params and return type (as seen
    /// from the consumed context's own namespace). Populated by the project
    /// driver; empty in single-file mode.
    pub consumed_services: HashMap<String, HashMap<String, CrossContextService>>,
    /// For each consumed context, its full type table (the consumed
    /// context's local types, plus the types it brings in via `uses`).
    /// Used by the checker for structural shape comparisons across the
    /// boundary (v0.6 §4.3).
    pub consumed_types: HashMap<String, HashMap<String, Arc<TypeDecl>>>,
    /// v0.15: for each consumed context, the capabilities it `exports
    /// capability { … }` — keyed by capability name. Used to resolve and
    /// type-check `given B.Cap` references and `B.Cap.op(…)` calls, and by
    /// the emitter to instantiate the provider locally.
    pub consumed_capabilities: HashMap<String, HashMap<String, CrossContextCapability>>,
    /// v0.17: `consumes U { Cap, … }` flattens selected capabilities into the
    /// consumer's local namespace under their bare names (§3.3). Maps each bare
    /// capability name to the consumed unit (context or adapter) providing it,
    /// so bare `given Cap` / `Cap.op(…)` resolve, the deps type imports from the
    /// right module, and compose instantiates the provider.
    pub flattened_caps: HashMap<String, String>,
    /// Events track, slice 0 (spine #936): for each consumed context, the
    /// names of its own `event` declarations. Lets a subscriber's `from
    /// Events(E)` header be checked against a foreign owner too — `E` is
    /// legitimate if it's a local event *or* a declared event of some
    /// consumed context, mirroring how `discover_event_subscribers`
    /// (`bynk-emit/src/project.rs`) already resolves ownership for wiring.
    pub consumed_event_names: HashMap<String, HashSet<String>>,
}

/// Snapshot of one exported capability in a consumed context, as needed for
/// v0.15 cross-context capability resolution. Operation signatures are
/// expressed in the consumed context's own namespace (resolved against
/// `consumed_types` at the call site, mirroring [`CrossContextService`]).
#[derive(Debug, Clone)]
pub struct CrossContextCapability {
    pub name: String,
    /// Each operation's parameter type-refs and return type-ref.
    pub ops: Vec<CrossContextCapabilityOp>,
    /// The provider that implements this capability in the providing context
    /// (its generated class name), so the consumer can instantiate it.
    pub provider_name: String,
    /// The provider's own `given` capabilities (intra-providing-context),
    /// needed to wire the provider's constructor when instantiated locally.
    pub provider_given: Vec<String>,
    pub span: bynk_syntax::span::Span,
}

#[derive(Debug, Clone)]
pub struct CrossContextCapabilityOp {
    pub name: String,
    /// #926: the op's own type parameters (empty for a non-generic op),
    /// spelled the same as the consumed context's own declaration. A cross-
    /// context call resolves these from an explicit call-site type argument,
    /// same as the local-capability path.
    pub type_params: Vec<String>,
    pub params: Vec<(String, TypeRef)>,
    pub return_type: TypeRef,
}

/// Snapshot of one service in a consumed context, as needed for v0.6
/// cross-context type checking. The params and return type are expressed
/// in the consumed context's own namespace.
#[derive(Debug, Clone)]
pub struct CrossContextService {
    pub name: String,
    /// Surface (parsed) type-refs of the `on call` handler's parameters.
    pub params: Vec<(String, TypeRef)>,
    pub return_type: TypeRef,
    pub span: bynk_syntax::span::Span,
}

/// Project one local `on call` handler into the [`CrossContextService`] shape
/// both sides of a cross-context contract check need — a caller resolving a
/// *consumed* service ([`crate::symbols::build_cross_context_info`]) and a
/// callee stamping its *own* `X-Bynk-Contract` constant
/// ([`crate::contract::own_contract_hashes`]). Sharing this one projection is
/// the whole correctness argument for that symmetry: if the two sides ever
/// diverged, a working deployment would 409 on every call instead of only on
/// real skew. `None` when `sdecl` has no `on call` handler (e.g. an
/// events-only or queue-only service).
pub fn cross_context_service_for(name: &str, sdecl: &ServiceDecl) -> Option<CrossContextService> {
    let handler = sdecl
        .handlers
        .iter()
        .find(|h| matches!(h.kind, HandlerKind::Call))?;
    Some(CrossContextService {
        name: name.to_string(),
        params: handler
            .params
            .iter()
            .map(|p| (p.name.name.clone(), p.type_ref.clone()))
            .collect(),
        return_type: handler.return_type.clone(),
        span: sdecl.span,
    })
}

impl CrossContextInfo {
    /// Returns the qualified name of the consumed context this prefix refers
    /// to, treating `prefix` as either an alias or a full qualified name.
    pub fn resolve_prefix(&self, prefix: &str) -> Option<String> {
        if let Some(q) = self.aliases.get(prefix) {
            return Some(q.clone());
        }
        if self.consumed_contexts.iter().any(|c| c == prefix) {
            return Some(prefix.to_string());
        }
        None
    }

    /// v0.15: resolve a dotted receiver chain like `platform.time.Clock` or
    /// `Time.Clock` to `(consumed_context, capability)` when the leading
    /// segments name a consumed context (or alias) that exports the trailing
    /// capability. Returns `None` if the chain is not a cross-context
    /// capability reference.
    pub fn resolve_cross_capability(&self, chain: &str) -> Option<(String, String)> {
        let (prefix, cap) = chain.rsplit_once('.')?;
        let ctx = self.resolve_prefix(prefix)?;
        let caps = self.consumed_capabilities.get(&ctx)?;
        if caps.contains_key(cap) {
            Some((ctx, cap.to_string()))
        } else {
            None
        }
    }
}

impl ResolvedCommons {
    /// Returns true if `name` is a type declared in the current commons
    /// (rather than imported via `uses`). Local types alone may reach into
    /// their opaque representation (`.raw`) or call `.unsafe(value)`.
    pub fn is_local_type(&self, name: &str) -> bool {
        self.local_type_names.contains(name)
    }

    /// Events track, slice 0: is `name` a declared `event` in this commons —
    /// not merely any local type?
    pub fn is_local_event(&self, name: &str) -> bool {
        self.event_type_names.contains(name)
    }

    /// Is `name` in scope via `uses` of a *commons* specifically? See
    /// `uses_commons_type_names`'s field doc for the exact predicate.
    pub fn is_uses_commons_type(&self, name: &str) -> bool {
        self.uses_commons_type_names.contains(name)
    }

    /// Build a `ResolvedCommons` from a merged (local + `uses`/`consumes`)
    /// symbol table, deriving `local_type_names`/`event_type_names` from
    /// `local_types`/`local_events` — the *pre-merge* tables — rather than
    /// from `types`/`agents` (already merged). This is the one thing every
    /// hand-rolled construction outside this crate got a chance to disagree
    /// on: the pre-merge/merged distinction is exactly what backs
    /// `.raw`/`.unsafe()`/owner-only-event-emission gating, and reusing the
    /// merged table there silently widens all three to any consumed/used
    /// type or event (found during the events track, slice 0, spine #936).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        commons: Commons,
        types: HashMap<String, Arc<TypeDecl>>,
        local_types: &HashMap<String, Arc<TypeDecl>>,
        fns: HashMap<String, Arc<FnDecl>>,
        methods: HashMap<String, MethodTable>,
        agents: HashMap<String, AgentDecl>,
        local_events: &HashMap<String, EventDecl>,
        cross_context: CrossContextInfo,
        imported_from: HashMap<String, String>,
        is_context: bool,
        uses_commons_type_names: HashSet<String>,
    ) -> Self {
        Self {
            commons,
            local_type_names: local_types.keys().cloned().collect(),
            event_type_names: local_events.keys().cloned().collect(),
            types,
            fns,
            methods,
            cross_context,
            agents,
            imported_from,
            is_context,
            uses_commons_type_names,
        }
    }
}

/// Resolve names in a single-file (or already-merged) commons. Use this
/// entry point only for self-contained Bynk programs. For multi-file
/// projects and `uses`-resolving commons, use [`resolve_file`] against a
/// pre-built combined symbol table.
pub fn resolve(commons: Commons) -> Result<ResolvedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
    let mut fns: HashMap<String, Arc<FnDecl>> = HashMap::new();
    let mut methods: HashMap<String, MethodTable> = HashMap::new();

    // First pass: collect declarations and detect duplicates / name overlap.
    for item in &commons.items {
        match item {
            // v0.5 declaration kinds — these don't introduce types/fns into
            // the symbol space. They go through the context-level v0.5 path
            // in project.rs. Skip them at the per-commons level.
            CommonsItem::Capability(_)
            | CommonsItem::Provider(_)
            | CommonsItem::Service(_)
            | CommonsItem::Agent(_)
            | CommonsItem::Actor(_)
            // `messages` entries are plain string literals with no type refs
            // to resolve here; commons-only legality and the reference/
            // duplicate-code checks live in bynk-emit's project validation.
            | CommonsItem::Messages(_) => {}
            CommonsItem::Type(t) => {
                if let Some(prev) = types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.duplicate_type",
                            t.name.span,
                            format!("type `{}` is already declared", t.name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else if let Some(prev) = fns.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.name_conflict",
                            t.name.span,
                            format!(
                                "type `{}` conflicts with a function of the same name",
                                t.name.name
                            ),
                        )
                        .with_label(prev.name.ident().span, "function declared here"),
                    );
                } else {
                    types.insert(t.name.name.clone(), Arc::new(t.clone()));
                    methods.insert(t.name.name.clone(), MethodTable::default());
                }
            }
            // Events track, slice 0 (spine #936): an `event` registers into
            // the same `types` table as an ordinary `type` — via the
            // synthetic `TypeDecl` `EventDecl::as_type_decl` builds — so it
            // reuses every existing type-reference/construction check.
            // Context-only legality (`bynk.event.outside_context`) and
            // event-vs-plain-type distinctions live in bynk-emit's project
            // validation, the same split `messages` already uses.
            CommonsItem::Event(e) => {
                let t = e.as_type_decl();
                if let Some(prev) = types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.duplicate_type",
                            t.name.span,
                            format!("type `{}` is already declared", t.name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else if let Some(prev) = fns.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.name_conflict",
                            t.name.span,
                            format!(
                                "type `{}` conflicts with a function of the same name",
                                t.name.name
                            ),
                        )
                        .with_label(prev.name.ident().span, "function declared here"),
                    );
                } else {
                    methods.insert(t.name.name.clone(), MethodTable::default());
                    types.insert(t.name.name.clone(), Arc::new(t));
                }
            }
            CommonsItem::Fn(f) => match &f.name {
                FnName::Free(id) => {
                    if let Some(prev) = fns.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_fn",
                                id.span,
                                format!("function `{}` is already declared", id.name),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else if let Some(prev) = types.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.name_conflict",
                                id.span,
                                format!(
                                    "function `{}` conflicts with a type of the same name",
                                    id.name
                                ),
                            )
                            .with_label(prev.name.span, "type declared here"),
                        );
                    } else {
                        fns.insert(id.name.clone(), Arc::new(f.clone()));
                    }
                }
                FnName::Method {
                    type_name,
                    method_name,
                } => {
                    // The type the method is attached to must be declared.
                    if !types.contains_key(&type_name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.method_unknown_type",
                                type_name.span,
                                format!(
                                    "method `{}.{}` attached to an unknown type `{}`",
                                    type_name.name, method_name.name, type_name.name
                                ),
                            )
                            .with_note(
                                "methods can only be declared on types defined in the same commons",
                            ),
                        );
                        continue;
                    }
                    // #594: an *instance* method on a generic type is a generic
                    // method — the receiver's type arguments supply the type's
                    // parameters (`self: Box[A]`), so it resolves and emits as an
                    // erased TS generic method. A *static* method has no receiver
                    // to supply those parameters, so it stays deferred (it would
                    // need free-function-style inference of the type's params);
                    // reject it rather than emit an under-applied `Box` signature.
                    if !f.has_self
                        && types
                            .get(&type_name.name)
                            .is_some_and(|d| !d.type_params.is_empty())
                    {
                        errors.push(
                            CompileError::new(
                                "bynk.generics.method_on_generic_type",
                                type_name.span,
                                format!(
                                    "static method `{}.{}` is attached to generic type `{}` — static methods on generic types are deferred (instance methods are supported)",
                                    type_name.name, method_name.name, type_name.name
                                ),
                            )
                            .with_note(
                                "give the method a `self` receiver, or use a free function taking the generic value as a parameter instead",
                            ),
                        );
                        continue;
                    }
                    let table = methods.entry(type_name.name.clone()).or_default();
                    let bucket = if f.has_self {
                        &mut table.instance
                    } else {
                        &mut table.statics
                    };
                    if let Some(prev) = bucket.get(&method_name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_method",
                                method_name.span,
                                format!(
                                    "method `{}.{}` is already declared",
                                    type_name.name, method_name.name
                                ),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else {
                        bucket.insert(method_name.name.clone(), Arc::new(f.clone()));
                    }
                }
            },
        }
    }

    // Second pass: validate references inside type-refs and function bodies.
    let mut refs = RefSink::new(); // single-file mode: no recording context.
    let mut sinks = Sinks {
        errs: &mut errors,
        refs: &mut refs,
    };
    for item in &commons.items {
        match item {
            CommonsItem::Type(t) => {
                check_type_decl_refs(t, &types, &mut sinks);
            }
            CommonsItem::Event(e) => {
                check_type_decl_refs(&e.as_type_decl(), &types, &mut sinks);
            }
            CommonsItem::Fn(f) => {
                check_fn_refs(f, &types, &fns, &methods, &mut sinks);
            }
            // v0.5 items are resolved via a separate context-level pass.
            CommonsItem::Capability(_)
            | CommonsItem::Provider(_)
            | CommonsItem::Service(_)
            | CommonsItem::Agent(_)
            | CommonsItem::Actor(_)
            // `messages` entries are plain string literals with no type refs
            // to resolve here; commons-only legality and the reference/
            // duplicate-code checks live in bynk-emit's project validation.
            | CommonsItem::Messages(_) => {}
        }
    }

    if errors.is_empty() {
        let local_type_names = types.keys().cloned().collect();
        let event_type_names = commons
            .items
            .iter()
            .filter_map(|item| match item {
                CommonsItem::Event(e) => Some(e.name.name.clone()),
                _ => None,
            })
            .collect();
        Ok(ResolvedCommons {
            commons,
            types,
            fns,
            methods,
            local_type_names,
            cross_context: CrossContextInfo::default(),
            agents: HashMap::new(),
            // Single-file mode has no `uses`-imported functions.
            imported_from: HashMap::new(),
            // Single-file mode has no `uses` at all — the rebrand this flag
            // gates is unreachable here.
            is_context: false,
            uses_commons_type_names: HashSet::new(),
            event_type_names,
        })
    } else {
        Err(errors)
    }
}

/// Validate name references inside a single file's items against an
/// already-built symbol table (`resolved.types`, `resolved.fns`,
/// `resolved.methods`). Used by the project-level driver after combining
/// declarations from every file in a multi-file commons and from every
/// commons brought in by `uses`.
pub fn resolve_file(resolved: &ResolvedCommons) -> Result<(), Vec<CompileError>> {
    resolve_file_record(resolved, &mut RefSink::new())
}

/// [`resolve_file`], recording binding edges into `refs` as the walk
/// resolves them (v0.25). The project pass sets the sink's per-file context;
/// a fresh sink records nothing.
pub fn resolve_file_record(
    resolved: &ResolvedCommons,
    refs: &mut RefSink,
) -> Result<(), Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut sinks = Sinks {
        errs: &mut errors,
        refs,
    };
    for item in &resolved.commons.items {
        match item {
            CommonsItem::Type(t) => {
                sinks.refs.set_owner(&t.name.name);
                check_type_decl_refs(t, &resolved.types, &mut sinks);
            }
            CommonsItem::Event(e) => {
                sinks.refs.set_owner(&e.name.name);
                check_type_decl_refs(&e.as_type_decl(), &resolved.types, &mut sinks);
            }
            CommonsItem::Fn(f) => {
                sinks.refs.set_owner(f.name.display());
                check_fn_refs(
                    f,
                    &resolved.types,
                    &resolved.fns,
                    &resolved.methods,
                    &mut sinks,
                );
            }
            CommonsItem::Capability(_)
            | CommonsItem::Provider(_)
            | CommonsItem::Service(_)
            | CommonsItem::Agent(_)
            | CommonsItem::Actor(_)
            // `messages` entries are plain string literals with no type refs
            // to resolve here; commons-only legality and the reference/
            // duplicate-code checks live in bynk-emit's project validation.
            | CommonsItem::Messages(_) => {}
        }
        sinks.refs.clear_owner();
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// v0.157 (ADR 0183): the name a record field *directly contains* — a top-level
/// `Named` (`f: A`) or a generic application (`f: A[T]`). Both are direct
/// containment edges for the cycle guards; a `List[…]`/`Option[…]` wrapper is
/// not (its empty/`None` inhabitant breaks the cycle).
fn direct_record_head(tr: &TypeRef) -> Option<&str> {
    match tr {
        TypeRef::Named(id) => Some(&id.name),
        TypeRef::App { name, .. } => Some(&name.name),
        _ => None,
    }
}

/// Whether `target` is reachable from `start` over direct record-field edges
/// (bare `Named` or generic `App` heads) — the record-containment graph. Used
/// to reject indirect record cycles (`A = { b: B }`, `B = { a: A }`); a
/// `visited` set bounds the walk on graphs that already contain cycles
/// elsewhere.
fn record_field_reaches(start: &str, target: &str, types: &HashMap<String, Arc<TypeDecl>>) -> bool {
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(name) = stack.pop() {
        if name == target {
            return true;
        }
        if !visited.insert(name.clone()) {
            continue;
        }
        if let Some(decl) = types.get(&name)
            && let TypeBody::Record(r) = &decl.body
        {
            for f in &r.fields {
                if let Some(head) = direct_record_head(&f.type_ref) {
                    stack.push(head.to_string());
                }
            }
        }
    }
    false
}

/// v0.157 (ADR 0183): reject a repeated type-parameter name — a duplicate would
/// collapse silently in the substitution map (the later argument winning), so a
/// `Pair[T, T]` mis-checks its fields. Shared by `type` and `fn` declarations.
fn check_duplicate_type_params(params: &[TypeParam], owner: &str, errors: &mut Sinks) {
    let mut seen: HashMap<&str, bynk_syntax::span::Span> = HashMap::new();
    for tp in params {
        if let Some(prev) = seen.get(tp.name.name.as_str()) {
            errors.push(
                CompileError::new(
                    "bynk.generics.duplicate_type_param",
                    tp.span,
                    format!(
                        "type parameter `{}` is declared more than once on {owner}",
                        tp.name.name
                    ),
                )
                .with_label(*prev, "previously declared here"),
            );
        } else {
            seen.insert(tp.name.name.as_str(), tp.span);
        }
    }
}

/// Recursively walk a type declaration to check that every type reference
/// inside it resolves.
fn check_type_decl_refs(t: &TypeDecl, types: &HashMap<String, Arc<TypeDecl>>, errors: &mut Sinks) {
    // A `type` declaration may not reuse a compiler-known built-in type name
    // (`List`, `Map`, `Query`, …). Those names are dispatched on by the type
    // parser (`parser/types.rs`), so any *reference* to the alias would be
    // intercepted as the built-in — the declaration would be silently shadowed
    // (`QueueResult`) or fail with an incoherent message at the use site. Reject
    // it here, at the declaration, with a message the user can act on. Base
    // types and other reserved *keywords* (`Int`, `Result`, …) are already
    // rejected earlier, by `expect_ident` at parse time.
    if bynk_syntax::keywords::is_builtin_type_name(&t.name.name) {
        errors.push(
            CompileError::new(
                "bynk.resolve.reserved_builtin_type",
                t.name.span,
                format!(
                    "`{}` is a built-in type name and cannot be redeclared",
                    t.name.name
                ),
            )
            .with_note("rename the type — built-in type names are reserved in type position"),
        );
    }
    // v0.157 (ADR 0183): a record body may be generic. #593: a sum body may too
    // — its variant payloads resolve the parameters as rigid vars, exactly as
    // record fields do. Type parameters on a refined / opaque body are still
    // rejected; a parameter shadowing a declared type is diagnosed (mirrors the
    // function-generics rule).
    let type_params: HashSet<String> = t.type_params.iter().map(|p| p.name.name.clone()).collect();
    if !t.type_params.is_empty() {
        check_duplicate_type_params(&t.type_params, &format!("type `{}`", t.name.name), errors);
        if !matches!(t.body, TypeBody::Record(_) | TypeBody::Sum(_)) {
            errors.push(
                CompileError::new(
                    "bynk.generics.generic_non_record",
                    t.type_params[0].span,
                    format!(
                        "type `{}` declares type parameters, but only a record (`{{ … }}`) or sum (`| … | …`) type may be generic",
                        t.name.name
                    ),
                )
                .with_note("refined and opaque types cannot be generic — their base is a fixed primitive"),
            );
        }
        // #593: a generic sum may not carry an `embeds` clause. Embedding folds
        // another sum's variants in by name; composing that with per-parameter
        // substitution (the embedded source could itself be generic, or mention
        // the host's parameters) is out of scope for this increment.
        if let TypeBody::Sum(s) = &t.body
            && let Some(clause) = s.embeds.first()
        {
            errors.push(
                CompileError::new(
                    "bynk.generics.generic_sum_embeds",
                    clause.span,
                    format!("generic sum `{}` cannot use an `embeds` clause", t.name.name),
                )
                .with_note("embedding into a generic sum is not supported — declare the variants directly, or make the sum non-generic"),
            );
        }
        for tp in &t.type_params {
            if types.contains_key(&tp.name.name) {
                errors.push(
                    CompileError::new(
                        "bynk.generics.type_arg_mismatch",
                        tp.span,
                        format!(
                            "type parameter `{}` shadows the declared type of the same name",
                            tp.name.name
                        ),
                    )
                    .with_note("rename the type parameter"),
                );
            }
        }
    }
    match &t.body {
        TypeBody::Refined { .. } => {
            // Refined-type bodies only reference base types directly.
        }
        TypeBody::Opaque { .. } => {
            // Opaque-type bodies only reference base types directly.
        }
        TypeBody::Record(r) => {
            let mut seen = HashMap::new();
            for f in &r.fields {
                if let Some(prev_span) = seen.get(&f.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.duplicate_field",
                            f.name.span,
                            format!("field `{}` is declared more than once", f.name.name),
                        )
                        .with_label(*prev_span, "previously declared here"),
                    );
                } else {
                    seen.insert(f.name.name.clone(), f.name.span);
                }
                // Detect containment cycles: a direct `type A = { f: A }`,
                // and indirect cycles through direct record fields
                // (`A = { b: B }`, `B = { a: A }`). Such a cycle admits no finite
                // value, and defeats every structural walk downstream (zero-value
                // emission, codecs). A `List[...]`/`Option[...]` wrapper (whose
                // empty/`None` inhabitant breaks the cycle) is not a direct edge.
                // v0.157 (ADR 0183): a generic self-reference `f: A[T]` is a
                // `TypeRef::App` direct edge — caught here in the checker (and so
                // in the standalone LSP), not only by the emit-side boundary pass.
                if let Some(head) = direct_record_head(&f.type_ref) {
                    if head == t.name.name {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.recursive_record_field",
                                f.name.span,
                                format!(
                                    "record `{}` cannot directly contain a field of its own type",
                                    t.name.name
                                ),
                            )
                            .with_label(t.name.span, "type declared here")
                            .with_note(
                                "wrap the recursive reference in `Option[...]` to break the cycle",
                            ),
                        );
                    } else if record_field_reaches(head, &t.name.name, types) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.recursive_record_field",
                                f.name.span,
                                format!(
                                    "record `{}` contains itself through this field — `{}` leads back to `{}`",
                                    t.name.name, head, t.name.name
                                ),
                            )
                            .with_label(t.name.span, "type declared here")
                            .with_note(
                                "wrap one field in the cycle in `Option[...]` to break it",
                            ),
                        );
                    }
                }
                check_type_ref_resolves_in(&f.type_ref, types, &type_params, errors);
            }
        }
        TypeBody::Sum(s) => {
            let mut seen = HashMap::new();
            for v in &s.variants {
                if let Some(prev_span) = seen.get(&v.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.duplicate_variant",
                            v.name.span,
                            format!("variant `{}` is declared more than once", v.name.name),
                        )
                        .with_label(*prev_span, "previously declared here"),
                    );
                } else {
                    seen.insert(v.name.name.clone(), v.name.span);
                }
                let mut payload_seen = HashMap::new();
                for f in &v.payload {
                    if let Some(prev) = payload_seen.get(&f.name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_field",
                                f.name.span,
                                format!(
                                    "payload field `{}` is declared more than once in variant `{}`",
                                    f.name.name, v.name.name
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        payload_seen.insert(f.name.name.clone(), f.name.span);
                    }
                    // #593: a generic sum's declared type parameters are in scope
                    // in its variant payloads, resolving as rigid vars (empty set
                    // for a non-generic sum — the same reference walk as before).
                    check_type_ref_resolves_in(&f.type_ref, types, &type_params, errors);
                }
            }
            // v0.154 (ADR 0178): the `embeds E as V` clauses' source types must
            // resolve (the target variant is checked in `check_embeds`).
            for clause in &s.embeds {
                check_type_ref_resolves(&clause.source_type, types, errors);
            }
        }
    }
}

fn check_fn_refs(
    f: &FnDecl,
    types: &HashMap<String, Arc<TypeDecl>>,
    fns: &HashMap<String, Arc<FnDecl>>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Sinks,
) {
    // Parameter types resolve.
    // v0.20a: the fn's type parameters are legal named references in its
    // own signature and body annotations.
    let mut type_params: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    check_duplicate_type_params(
        &f.type_params,
        &format!("function `{}`", f.name.display()),
        errors,
    );
    // #594: an instance method on a generic type inherits the receiver type's
    // parameters into scope, so `fn Box.map[U](self, f: A -> U) -> Box[U]` may
    // name the type's own parameter `A` alongside the method's `U`. A method
    // parameter that reuses one of the type's parameter names would shadow it
    // ambiguously in the substitution — diagnose the collision.
    if let FnName::Method { type_name, .. } = &f.name
        && let Some(recv) = types.get(&type_name.name)
    {
        for tp in &recv.type_params {
            if type_params.contains(&tp.name.name) {
                errors.push(
                    CompileError::new(
                        "bynk.generics.duplicate_type_param",
                        f.type_params
                            .iter()
                            .find(|mp| mp.name.name == tp.name.name)
                            .map_or(tp.span, |mp| mp.span),
                        format!(
                            "type parameter `{}` is already a parameter of the receiver type `{}`",
                            tp.name.name, type_name.name
                        ),
                    )
                    .with_label(tp.span, "declared on the type here"),
                );
            }
            type_params.insert(tp.name.name.clone());
        }
    }
    let mut seen_params: HashMap<&str, &Ident> = HashMap::new();
    for p in &f.params {
        check_type_ref_resolves_in(&p.type_ref, types, &type_params, errors);
        if let Some(prev) = seen_params.get(p.name.name.as_str()) {
            errors.push(
                CompileError::new(
                    "bynk.resolve.duplicate_param",
                    p.name.span,
                    format!("parameter `{}` is declared more than once", p.name.name),
                )
                .with_label(prev.span, "previously declared here"),
            );
        } else {
            seen_params.insert(p.name.name.as_str(), &p.name);
        }
    }
    check_type_ref_resolves_in(&f.return_type, types, &type_params, errors);

    // Build the initial scope: parameters plus `self` (for instance methods).
    let mut params: HashMap<String, ()> =
        f.params.iter().map(|p| (p.name.name.clone(), ())).collect();
    if f.has_self {
        params.insert("self".to_string(), ());
    }
    let in_method = matches!(f.name, FnName::Method { .. });
    let mut cx = RefCheckCtx {
        params: &params,
        in_method,
        types,
        type_params: &type_params,
        fns,
        methods,
        scopes: Vec::new(),
        errors,
    };
    check_block_references(&f.body, &mut cx);
}

fn unknown_type_error(id: &Ident) -> CompileError {
    CompileError::new(
        "bynk.resolve.unknown_type",
        id.span,
        format!("unknown type `{}`", id.name),
    )
    .with_note(
        "only base types (Int, String, Bool), types declared in this commons, \
         `Result[T, E]`, `Option[T]`, and `ValidationError` are in scope",
    )
}

/// v0.157 (ADR 0183): a generic type named without its `[…]` arguments.
fn bare_generic_type_error(id: &Ident, arity: usize) -> CompileError {
    CompileError::new(
        "bynk.generics.type_arg_count",
        id.span,
        format!(
            "generic type `{}` must be applied to {} type argument{} — write `{}[…]`",
            id.name,
            arity,
            if arity == 1 { "" } else { "s" },
            id.name
        ),
    )
    .with_note("a generic type is used only through a concrete instantiation")
}

/// Recursively check that every type reference resolves.
fn check_type_ref_resolves(
    r: &TypeRef,
    types: &HashMap<String, Arc<TypeDecl>>,
    errors: &mut Sinks,
) {
    check_type_ref_resolves_in(r, types, &HashSet::new(), errors)
}

/// v0.20a: like [`check_type_ref_resolves`], with the enclosing function's
/// type parameters in scope — a `Named` reference matching one is a type
/// variable, not an unknown type.
fn check_type_ref_resolves_in(
    r: &TypeRef,
    types: &HashMap<String, Arc<TypeDecl>>,
    type_params: &HashSet<String>,
    errors: &mut Sinks,
) {
    match r {
        TypeRef::Base(_, _) => {}
        // v0.20a: a function type's components must each resolve.
        TypeRef::Fn(params, ret, _) => {
            for p in params {
                check_type_ref_resolves_in(p, types, type_params, errors);
            }
            check_type_ref_resolves_in(ret, types, type_params, errors);
        }
        TypeRef::Named(id) => {
            if let Some(decl) = types.get(&id.name) {
                errors.refs.record(id.span, SymbolKind::Type, &id.name);
                // v0.157 (ADR 0183): a generic type must be applied to its type
                // arguments — a bare `Paginated` (declared `Paginated[T]`) is an
                // under-application.
                if !decl.type_params.is_empty() {
                    errors.push(bare_generic_type_error(id, decl.type_params.len()));
                }
            } else if !type_params.contains(&id.name) {
                errors.push(unknown_type_error(id));
            }
        }
        // v0.157 (ADR 0183): `Name[Arg, …]` — a user generic-type application.
        // Validate existence, that the target is generic, and arity; then walk
        // the arguments.
        TypeRef::App { name, args, span } => {
            match types.get(&name.name) {
                None if type_params.contains(&name.name) => {
                    // A type parameter applied to arguments (`T[Int]`) — a type
                    // parameter is not itself generic (no higher-kinded types).
                    errors.push(
                        CompileError::new(
                            "bynk.generics.type_arg_count",
                            *span,
                            format!(
                                "type parameter `{}` cannot take type arguments — it is not a generic type",
                                name.name
                            ),
                        )
                        .with_note("higher-kinded type parameters are not supported"),
                    );
                }
                None => errors.push(unknown_type_error(name)),
                Some(decl) => {
                    errors.refs.record(name.span, SymbolKind::Type, &name.name);
                    let expected = decl.type_params.len();
                    // Finding #46: `decl` comes from the combined cross-file
                    // symbol table (`uses`/multi-file siblings), so its span
                    // may belong to a different file than `name` — a label
                    // can't express that without per-label file identity (a
                    // Wave 8 follow-up). A note keeps the same conservative
                    // choice `bynk-emit/src/project/consistency.rs` already
                    // makes for its own always-cross-file diagnostics,
                    // rather than risk underlining unrelated text.
                    if expected == 0 {
                        errors.push(
                            CompileError::new(
                                "bynk.generics.type_arg_count",
                                *span,
                                format!(
                                    "type `{}` is not generic — it takes no type arguments",
                                    name.name
                                ),
                            )
                            .with_note("type declared here"),
                        );
                    } else if expected != args.len() {
                        errors.push(
                            CompileError::new(
                                "bynk.generics.type_arg_count",
                                *span,
                                format!(
                                    "type `{}` expects {} type argument{}, but {} {} given",
                                    name.name,
                                    expected,
                                    if expected == 1 { "" } else { "s" },
                                    args.len(),
                                    if args.len() == 1 { "was" } else { "were" },
                                ),
                            )
                            .with_note("type declared here"),
                        );
                    }
                }
            }
            for a in args {
                check_type_ref_resolves_in(a, types, type_params, errors);
            }
        }
        TypeRef::Result(t, e, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
            check_type_ref_resolves_in(e, types, type_params, errors);
        }
        TypeRef::Option(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::Effect(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::HttpResult(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::QueueResult(_) => {}
        TypeRef::List(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::Query(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::Stream(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        TypeRef::Connection(t, _) => {
            check_type_ref_resolves_in(t, types, type_params, errors);
        }
        // v0.119 (ADR 0155): `History[Agent]` is a test-only generator, legal only
        // as a `for all` binding inside a `property` (validated in
        // `check_property_body`). A `History[…]` reaching this declared-type walk —
        // a field, parameter, return, or local annotation — is out of place.
        TypeRef::History(_, span) => {
            errors.push(
                CompileError::new(
                    "bynk.history.outside_property",
                    *span,
                    "`History[…]` is only valid as a `for all` generator inside a `property`",
                )
                .with_note(
                    "bind a driven call-history with `for all run: History[Agent]` in a `property`",
                ),
            );
        }
        TypeRef::Map(k, v, _) => {
            check_type_ref_resolves_in(k, types, type_params, errors);
            check_type_ref_resolves_in(v, types, type_params, errors);
            check_map_key_keyable(k, types, type_params, errors);
        }
        TypeRef::ValidationError(_) | TypeRef::JsonError(_) => {}
        TypeRef::Unit(_) => {}
    }
}

/// v0.20b: `Map` keys are confined to value-keyable types — `String`, `Int`,
/// and refined/opaque types over them — so the emitted `ReadonlyMap` keeps
/// value equality (object keys would compare by reference). A type parameter
/// is admitted in key position: it can only ever be instantiated through a
/// concrete `Map[K, V]` reference elsewhere, and that site is checked.
fn check_map_key_keyable(
    k: &TypeRef,
    types: &HashMap<String, Arc<TypeDecl>>,
    type_params: &HashSet<String>,
    errors: &mut Sinks,
) {
    let keyable = match k {
        TypeRef::Base(BaseType::String | BaseType::Int, _) => true,
        TypeRef::Named(id) => {
            // A type parameter is admitted (see above). An unknown name has
            // already been reported by the resolution walk; don't pile a
            // keyability error on top of it.
            if type_params.contains(&id.name) || !types.contains_key(&id.name) {
                return;
            }
            matches!(
                types.get(&id.name).map(|t| &t.body),
                Some(TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. })
                    if matches!(base, BaseType::String | BaseType::Int)
            )
        }
        _ => false,
    };
    if !keyable {
        errors.push(
            CompileError::new(
                "bynk.types.unkeyable_map_key",
                k.span(),
                "a `Map` key must be value-keyable — `String`, `Int`, or a refined/opaque type over them",
            )
            .with_note(
                "record, sum, collection, and function keys are rejected in v0.20b; value-equality keys need bounded generics",
            ),
        );
    }
}

/// Lookup a name across scopes. Returns true if it's bound somewhere
/// (param, self, or any let-scope).
fn name_in_scope(name: &str, params: &HashMap<String, ()>, scopes: &[HashMap<String, ()>]) -> bool {
    if params.contains_key(name) {
        return true;
    }
    scopes.iter().rev().any(|s| s.contains_key(name))
}

/// Validate a record construction's *field set* — every required field present,
/// no undeclared extra field, no field initialised twice, and every shorthand
/// `{ name }` bound in scope. Pure over the declaration and the provided fields;
/// the caller supplies its own scope predicate (the resolver's lexical scope via
/// [`name_in_scope`], the checker's binding table via `Ctx::lookup`) and its own
/// diagnostic sink.
///
/// #711: this walk skips `Service`/`Agent`/`Actor` items, so their handler
/// bodies never pass through it — the checker's `check_record_construction` is
/// their only backstop and calls this same function. A single implementation is
/// the point: an earlier fix copied three of these four checks into the checker
/// and dropped the shorthand one, re-opening the gap for shorthand fields. Both
/// callers now share this, so the two cannot re-diverge.
pub(crate) fn check_record_field_set(
    type_name: &Ident,
    fields: &[FieldInit],
    record: &RecordBody,
    // #852: the span of the whole `TypeName { … }` literal, so the missing-field
    // quick-fix knows where to insert a new field (before the closing brace when
    // the literal is empty).
    construction_span: Span,
    in_scope: impl Fn(&str) -> bool,
    errors: &mut Vec<CompileError>,
) {
    let declared: HashMap<&str, &RecordField> = record
        .fields
        .iter()
        .map(|f| (f.name.name.as_str(), f))
        .collect();
    let mut provided: HashMap<&str, bynk_syntax::span::Span> = HashMap::new();
    for f in fields {
        if !declared.contains_key(f.name.name.as_str()) {
            errors.push(
                CompileError::new(
                    "bynk.resolve.unknown_field",
                    f.name.span,
                    format!(
                        "record type `{}` has no field `{}`",
                        type_name.name, f.name.name
                    ),
                )
                // Finding #46: `decl_name_span` may name a declaration in a
                // different file than this construction site (both callers
                // resolve against the combined cross-file symbol table) — a
                // note instead of a label, matching the same conservative
                // choice made elsewhere for cross-file provenance without
                // per-label file identity (a Wave 8 follow-up).
                .with_note("type declared here"),
            );
        }
        if let Some(prev) = provided.get(f.name.name.as_str()) {
            errors.push(
                CompileError::new(
                    "bynk.resolve.duplicate_field_init",
                    f.name.span,
                    format!("field `{}` is initialised more than once", f.name.name),
                )
                .with_label(*prev, "previously initialised here"),
            );
        } else {
            provided.insert(f.name.name.as_str(), f.name.span);
        }
        // A shorthand `{ name }` (no `: value`) reads the binding `name` from
        // scope — it must exist. The full `field: value` form is checked by the
        // caller (the resolver recurses into the value, the checker types it).
        if f.value.is_none() && !in_scope(&f.name.name) {
            errors.push(
                CompileError::new(
                    "bynk.resolve.unknown_name",
                    f.name.span,
                    format!(
                        "shorthand field initialiser `{}` requires a binding of that name in scope",
                        f.name.name
                    ),
                )
                .with_note("either bring `{name}` into scope or use the full `field: value` form"),
            );
        }
    }
    // Missing required fields. Each is a diagnostic anchored at the type name;
    // a field whose type has a safe default additionally carries a
    // machine-applicable "add field `x`" quick-fix (#852, DECISIONS B/C) that
    // inserts `x: <default>` at a fmt-stable position, and — when more than one
    // field is missing and every missing field is defaultable — the first such
    // diagnostic also carries an "add all missing fields" convenience.
    let missing: Vec<&RecordField> = record
        .fields
        .iter()
        .filter(|f| !provided.contains_key(f.name.name.as_str()))
        .collect();
    // The edit for a `body` of one or more `name: default` entries. With
    // existing fields it appends `, body` right after the last one. With an
    // *empty* literal there is no field span to anchor to and the interior
    // spacing/trailing punctuation is unknown, so instead the whole ` { … }`
    // tail (from the end of the type name through the closing brace) is
    // **replaced** with a canonical ` { body }` — fmt-stable regardless of how
    // the empty braces were originally spelled (`{}`, `{ }`, `{  }`).
    let field_edit = |body: &str| -> (Span, String) {
        match fields.iter().map(|f| f.span.end).max() {
            Some(end) => (Span::new(end, end), format!(", {body}")),
            None => (
                Span::new(type_name.span.end, construction_span.end),
                format!(" {{ {body} }}"),
            ),
        }
    };
    // Defaultable missing fields, in declaration order, as `name: default`.
    let defaultable: Vec<String> = missing
        .iter()
        .filter_map(|f| field_default_init(f))
        .collect();
    let all_defaultable = defaultable.len() == missing.len();

    for (i, decl_field) in missing.iter().enumerate() {
        let mut err = CompileError::new(
            "bynk.resolve.missing_field",
            type_name.span,
            format!(
                "missing required field `{}` for record `{}`",
                decl_field.name.name, type_name.name
            ),
        )
        .with_label(decl_field.name.span, "field declared here");
        if let Some(piece) = field_default_init(decl_field) {
            err = err.with_suggestion(
                format!("add field `{}`", decl_field.name.name),
                vec![field_edit(&piece)],
                Applicability::MachineApplicable,
            );
        }
        // The "add all missing fields" convenience rides on the first missing
        // diagnostic (they all share `type_name.span`, so it surfaces together
        // with the single-field fixes), and only when the whole set is
        // defaultable and there is more than one to add.
        if i == 0 && missing.len() > 1 && all_defaultable {
            err = err.with_suggestion(
                "add all missing fields",
                vec![field_edit(&defaultable.join(", "))],
                Applicability::MachineApplicable,
            );
        }
        errors.push(err);
    }
}

/// The `name: <default>` initialiser for a missing record field, or `None` when
/// the field's type has no value that is guaranteed to re-check clean (#852,
/// DECISION B). Deliberately conservative: an inline-refined field or a
/// user-named type (which may itself be refined, a sum, or opaque) has no
/// synthesised default — only the unrefined built-in scalars, `Option` (`None`),
/// and `List` (`[]`) do, so the inserted value always type-checks.
fn field_default_init(field: &RecordField) -> Option<String> {
    if field.refinement.is_some() {
        return None;
    }
    let default = match &field.type_ref {
        TypeRef::Base(BaseType::Int, _) => "0",
        TypeRef::Base(BaseType::Float, _) => "0.0",
        TypeRef::Base(BaseType::String, _) => "\"\"",
        TypeRef::Base(BaseType::Bool, _) => "false",
        TypeRef::Option(..) => "None",
        TypeRef::List(..) => "[]",
        _ => return None,
    };
    Some(format!("{}: {}", field.name.name, default))
}

#[allow(clippy::too_many_arguments)]
/// Bundles the reference-walk's read-only lookup tables and mutable
/// traversal state (finding #37): threading nine positional parameters
/// through a ~900-line walk meant 313 of resolver.rs's 2,346 lines were
/// argument names at recursive call sites.
struct RefCheckCtx<'a, 'b> {
    params: &'a HashMap<String, ()>,
    in_method: bool,
    types: &'a HashMap<String, Arc<TypeDecl>>,
    type_params: &'a HashSet<String>,
    fns: &'a HashMap<String, Arc<FnDecl>>,
    methods: &'a HashMap<String, MethodTable>,
    scopes: Vec<HashMap<String, ()>>,
    errors: &'a mut Sinks<'b>,
}

fn check_block_references(block: &Block, cx: &mut RefCheckCtx) {
    cx.scopes.push(HashMap::new());
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                check_expr_references(&l.value, cx);
                if let Some(annot) = &l.type_annot {
                    check_type_ref_resolves_in(annot, cx.types, cx.type_params, cx.errors);
                }
                if let Some(prev) = cx.types.get(&l.name.name) {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.let_shadows_type",
                            l.name.span,
                            format!(
                                "`let {}` shadows the declared type `{}`",
                                l.name.name, l.name.name
                            ),
                        )
                        .with_label(prev.name.span, "type declared here")
                        .with_note("choose a different name for the let binding"),
                    );
                } else if let Some(prev) = cx.fns.get(&l.name.name) {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.let_shadows_fn",
                            l.name.span,
                            format!(
                                "`let {}` shadows the declared function `{}`",
                                l.name.name, l.name.name
                            ),
                        )
                        .with_label(prev.name.ident().span, "function declared here")
                        .with_note("choose a different name for the let binding"),
                    );
                } else if l.name.name != "_" {
                    cx.scopes
                        .last_mut()
                        .unwrap()
                        .insert(l.name.name.clone(), ());
                }
            }
            Statement::Expect(a) => {
                check_expr_references(&a.value, cx);
            }
            Statement::Send(s) => {
                check_expr_references(&s.value, cx);
            }
            Statement::Do(d) => {
                check_expr_references(&d.value, cx);
            }
            Statement::Assign(a) => {
                // v0.81: walk the RHS for references; the target resolves to a
                // `store` field, handled in the storage-track checker slice.
                check_expr_references(&a.value, cx);
            }
        }
    }
    check_expr_references(&block.tail, cx);
    cx.scopes.pop();
}

#[allow(clippy::too_many_lines)]
fn check_expr_references(expr: &Expr, cx: &mut RefCheckCtx) {
    match &expr.kind {
        // v0.43: resolve names referenced inside each interpolation hole.
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(hole) = part {
                    check_expr_references(hole, cx);
                }
            }
        }
        ExprKind::IntLit { .. }
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        // v0.20b: a list literal — each element resolves as a value.
        ExprKind::ListLit(elems) => {
            for el in elems {
                check_expr_references(el, cx);
            }
        }
        // Slice C: `Wire(<String>)` — the raw inner expression resolves as an
        // ordinary value (a string literal in practice).
        ExprKind::Wire(inner) => {
            check_expr_references(inner, cx);
        }
        // v0.20a: a lambda introduces a scope frame holding its params; the
        // body walks with the frame in place. Annotated param types resolve
        // through the ordinary type-ref check.
        ExprKind::Lambda(lambda) => {
            for p in &lambda.params {
                if let Some(tr) = &p.type_ref {
                    check_type_ref_resolves_in(tr, cx.types, cx.type_params, cx.errors);
                }
            }
            let mut frame: HashMap<String, ()> = HashMap::new();
            for p in &lambda.params {
                frame.insert(p.name.name.clone(), ());
            }
            cx.scopes.push(frame);
            check_expr_references(&lambda.body, cx);
            cx.scopes.pop();
        }
        ExprKind::EffectPure(inner) => {
            check_expr_references(inner, cx);
        }
        ExprKind::Expect(inner) => {
            check_expr_references(inner, cx);
        }
        ExprKind::Val { args, .. } => {
            // v0.9.4: the mocked type is validated by the checker; resolve any
            // pin-argument references here.
            for a in args {
                check_expr_references(a, cx);
            }
        }
        ExprKind::Observation(_) => {
            // v0.117: a `with` predicate's free names are the operation's
            // parameters, bound during type checking and not visible to name
            // resolution; a count is a literal. Nothing to resolve here.
        }
        ExprKind::Trace { .. } => {
            // v0.117: `Cap.op` names a capability seam, not value references.
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => {
            if let Some(tn) = type_name
                && !cx.types.contains_key(&tn.name)
            {
                cx.errors.push(unknown_type_error(tn));
            }
            check_expr_references(base, cx);
            for f in overrides {
                if let Some(v) = &f.value {
                    check_expr_references(v, cx);
                }
            }
        }
        ExprKind::Ident(id) => {
            if id.name == "self" {
                if !cx.in_method {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.self_outside_method",
                            id.span,
                            "`self` can only be used inside a method body",
                        )
                        .with_note(
                            "declare the function as `fn TypeName.method(self, ...)` if you intended a method",
                        ),
                    );
                }
                return;
            }
            if name_in_scope(&id.name, cx.params, &cx.scopes) {
                // OK.
            } else if http_variant(&id.name).is_some() {
                // v0.9: predeclared HttpResult variant (e.g. `NoContent`,
                // `Unauthorized`). The checker validates payload arity and
                // expected-type disambiguation.
            } else if let Some(sum_owner) = find_unique_variant_owner(&id.name, cx.types) {
                // It's a bare variant reference. We treat it as a valid
                // expression in resolver — the type checker will assign
                // the correct sum type. Mark with no error.
                let _ = sum_owner;
            } else if cx.types.contains_key(&id.name) {
                cx.errors.push(
                    CompileError::new(
                        "bynk.resolve.type_in_expr",
                        id.span,
                        format!("`{}` is a type, not a value", id.name),
                    )
                    .with_note(
                        "types cannot appear in expression position; \
                         use `TypeName.of(value)` or `TypeName { ... }` to construct values",
                    ),
                );
            } else if cx.fns.contains_key(&id.name) {
                // v0.20a: a bare named-function reference may be a function
                // VALUE where a function type is expected. The resolver has
                // no type information, so the judgment (and the
                // `bynk.resolve.fn_without_call` diagnostic for non-function
                // positions) now lives in the checker's ident rule. Silent
                // pass here keeps `unknown_name` from misfiring.
                cx.errors.refs.record(id.span, SymbolKind::Fn, &id.name);
            } else if find_ambiguous_variant_owners(&id.name, cx.types).len() > 1 {
                cx.errors.push(
                    CompileError::new(
                        "bynk.resolve.ambiguous_variant",
                        id.span,
                        format!(
                            "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}`",
                            id.name, id.name
                        ),
                    ),
                );
            } else {
                cx.errors.push(
                    CompileError::new(
                        "bynk.resolve.unknown_name",
                        id.span,
                        format!("unknown name `{}`", id.name),
                    )
                    .with_note(
                        "only parameters, `let` bindings, and functions declared \
                         in this commons are in scope",
                    ),
                );
            }
        }
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            // #712: explicit type arguments (`identity[T](…)`) are type
            // references and must resolve — the checker's `check_generic_call`
            // otherwise dropped an unknown one silently. Validated here so
            // `fn`/method bodies are covered; the checker backstops handler
            // bodies (which never reach this walk).
            for ta in type_args {
                check_type_ref_resolves_in(ta, cx.types, cx.type_params, cx.errors);
            }
            match cx.fns.get(&name.name) {
                Some(decl) => {
                    cx.errors.refs.record(name.span, SymbolKind::Fn, &name.name);
                    if decl.params.len() != args.len() {
                        cx.errors.push(
                            CompileError::new(
                                "bynk.resolve.arity_mismatch",
                                name.span,
                                format!(
                                    "function `{}` expects {} argument(s), but {} were given",
                                    name.name,
                                    decl.params.len(),
                                    args.len()
                                ),
                            )
                            // Finding #46: `decl` is looked up in the
                            // combined cross-file symbol table, so its span
                            // may belong to a different file than this call
                            // — see the same note at the type-arity checks
                            // above.
                            .with_note("function declared here"),
                        );
                    }
                }
                None => {
                    // Maybe it's a variant constructor with a payload (e.g., `Placed(at, total)`).
                    let owners = find_ambiguous_variant_owners(&name.name, cx.types);
                    if http_variant(&name.name).is_some() {
                        // v0.9: predeclared HttpResult variant constructor.
                    } else if owners.len() == 1 {
                        // Single owner — treat as variant construction. Type
                        // checker validates arg count and types.
                    } else if owners.len() > 1 {
                        cx.errors.push(CompileError::new(
                            "bynk.resolve.ambiguous_variant",
                            name.span,
                            format!(
                                "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}(...)`",
                                name.name, name.name
                            ),
                        ));
                    } else if cx.types.contains_key(&name.name) {
                        cx.errors.push(CompileError::new(
                            "bynk.resolve.type_as_function",
                            name.span,
                            format!(
                                "`{}` is a type, not a function — use `{}.of(value)` or `{} {{ ... }}` instead",
                                name.name, name.name, name.name
                            ),
                        ));
                    } else if name_in_scope(&name.name, cx.params, &cx.scopes) {
                        // v0.20a: an in-scope value being called may be a
                        // legal value application if its type is a function
                        // type. The resolver has no type information, so the
                        // judgment (and `bynk.resolve.param_as_function` for
                        // non-function-typed values) lives in the checker's
                        // call dispatch. Silent pass.
                    } else {
                        cx.errors.push(
                            CompileError::new(
                                "bynk.resolve.unknown_function",
                                name.span,
                                format!("unknown function `{}`", name.name),
                            )
                            .with_note("only functions declared in this commons are callable"),
                        );
                    }
                }
            }
            for a in args {
                check_expr_references(a, cx);
            }
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            check_expr_references(lhs, cx);
            check_expr_references(rhs, cx);
        }
        ExprKind::UnaryOp(_, e) => check_expr_references(e, cx),
        ExprKind::Paren(e) => check_expr_references(e, cx),
        ExprKind::Block(b) => check_block_references(b, cx),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            check_expr_references(cond, cx);
            // `is`-pattern bindings inside the condition flow into the
            // then-branch's scope (v0.2 §3.9).
            let mut then_extra: HashMap<String, ()> = HashMap::new();
            collect_is_binding_names(cond, &mut then_extra);
            cx.scopes.push(then_extra);
            check_block_references(then_block, cx);
            cx.scopes.pop();
            check_block_references(else_block, cx);
        }
        ExprKind::Ok(inner) | ExprKind::Err(inner) | ExprKind::Question(inner) => {
            check_expr_references(inner, cx);
        }
        ExprKind::Some(inner) => {
            check_expr_references(inner, cx);
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            // The expression `T.name(args)` may be:
            //   - a static method call (or refined-type `of`),
            //   - a qualified variant constructor on a sum,
            //   - a qualified HttpResult variant (v0.9).
            // The resolver only needs to ensure that *something* matches.
            if type_name.name == "HttpResult" {
                if http_variant(&method.name).is_none() {
                    cx.errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                }
                for a in args {
                    check_expr_references(a, cx);
                }
                return;
            }
            if let Some(decl) = cx.types.get(&type_name.name) {
                cx.errors
                    .refs
                    .record(type_name.span, SymbolKind::Type, &type_name.name);
                let table = cx.methods.get(&type_name.name).cloned().unwrap_or_default();
                let is_static_method = table.statics.contains_key(&method.name);
                let is_of_constructor = method.name == "of"
                    && matches!(
                        decl.body,
                        TypeBody::Refined { .. } | TypeBody::Opaque { .. }
                    );
                let is_unsafe_constructor =
                    method.name == "unsafe" && matches!(decl.body, TypeBody::Opaque { .. });
                let is_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == method.name),
                    _ => false,
                };
                if !(is_static_method || is_of_constructor || is_unsafe_constructor || is_variant) {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                type_name.name, method.name
                            ),
                        )
                        // Finding #46: cross-file table lookup — see resolver.rs:1029.
                        .with_note("type declared here"),
                    );
                }
            } else {
                cx.errors.push(unknown_type_error(type_name));
            }
            for a in args {
                check_expr_references(a, cx);
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            match cx.types.get(&type_name.name) {
                Some(decl) => {
                    cx.errors
                        .refs
                        .record(type_name.span, SymbolKind::Type, &type_name.name);
                    match &decl.body {
                        TypeBody::Record(r) => {
                            // Field-set validation (missing / unknown / duplicate
                            // / shorthand-in-scope) is shared with the checker's
                            // `check_record_construction` so the two cannot
                            // re-diverge (#711). The value recursion below stays
                            // here — it is the resolver's reference walk.
                            check_record_field_set(
                                type_name,
                                fields,
                                r,
                                expr.span,
                                |n| name_in_scope(n, cx.params, &cx.scopes),
                                cx.errors.errs,
                            );
                            for f in fields {
                                if let Some(v) = &f.value {
                                    check_expr_references(v, cx);
                                }
                            }
                        }
                        TypeBody::Opaque { .. } => {
                            cx.errors.push(
                            CompileError::new(
                                "bynk.resolve.opaque_record_construction",
                                type_name.span,
                                format!(
                                    "opaque type `{}` cannot be constructed with record-literal syntax",
                                    type_name.name
                                ),
                            )
                            // Finding #46: cross-file table lookup — see resolver.rs:1029.
                            .with_note("type declared here")
                            .with_note(
                                "construct opaque values via `T.of(value)` (validated) or `T.unsafe(value)` (inside the defining commons)",
                            ),
                        );
                        }
                        _ => {
                            cx.errors.push(
                            CompileError::new(
                                "bynk.resolve.not_a_record_type",
                                type_name.span,
                                format!(
                                    "`{}` is not a record type — only record types can be constructed with `{{ ... }}`",
                                    type_name.name
                                ),
                            )
                            // Finding #46: cross-file table lookup — see resolver.rs:1029.
                            .with_note("type declared here"),
                        );
                        }
                    }
                }
                None => cx.errors.push(unknown_type_error(type_name)),
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // v0.9: `HttpResult.Variant` qualified nullary variant.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, cx.params, &cx.scopes)
                && id.name == "HttpResult"
            {
                if http_variant(&field.name).is_none() {
                    cx.errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        field.span,
                        format!("`HttpResult` has no variant named `{}`", field.name),
                    ));
                }
                return;
            }
            // `TypeName.Variant` — qualified nullary variant reference.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, cx.params, &cx.scopes)
                && let Some(decl) = cx.types.get(&id.name)
            {
                cx.errors.refs.record(id.span, SymbolKind::Type, &id.name);
                let known_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == field.name),
                    _ => false,
                };
                if !known_variant {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            field.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, field.name
                            ),
                        )
                        // Finding #46: cross-file table lookup — see resolver.rs:1029.
                        .with_note("type declared here"),
                    );
                }
            } else {
                check_expr_references(receiver, cx);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // v0.9: `HttpResult.Variant(args)` — qualified HttpResult constructor.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, cx.params, &cx.scopes)
                && id.name == "HttpResult"
            {
                if http_variant(&method.name).is_none() {
                    cx.errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                }
                for a in args {
                    check_expr_references(a, cx);
                }
                return;
            }
            // v0.20b: `List.empty()` / `Map.empty()` — qualified statics on
            // the built-in collection types (no user declaration to resolve
            // against; the checker owns their typing). v0.22a adds the
            // numeric parse statics, `Int.parse(…)` / `Float.parse(…)`.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, cx.params, &cx.scopes)
                && matches!(
                    id.name.as_str(),
                    "List"
                        | "Map"
                        | "Int"
                        | "Float"
                        | "Json"
                        | "Duration"
                        | "Instant"
                        | "Stream"
                        | "Bytes"
                )
                && !cx.types.contains_key(&id.name)
            {
                let allowed: &[&str] = match id.name.as_str() {
                    "List" | "Map" => &["empty"],
                    "Json" => &["encode", "decode"],
                    // v0.86 (ADR 0112): `Duration.millis(n)`.
                    "Duration" => &["millis"],
                    // v0.90 (ADR 0114): `Instant.fromEpochMillis(n)`.
                    "Instant" => &["fromEpochMillis"],
                    // v0.100: `Stream.of(xs)`.
                    "Stream" => &["of"],
                    // v0.110 (ADR 0142): `Bytes.fromUtf8(s)`/`fromBase64(s)`/`empty()`.
                    "Bytes" => &["fromUtf8", "fromBase64", "empty"],
                    _ => &["parse"],
                };
                let only = allowed.join("`/`");
                if !allowed.contains(&method.name.as_str()) {
                    cx.errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!(
                            "the built-in `{}` type has no static method named `{}` — the statics are `{only}`",
                            id.name, method.name
                        ),
                    ));
                }
                for a in args {
                    check_expr_references(a, cx);
                }
                return;
            }
            // If the receiver is a bare ident of a declared type (and not a
            // local binding), this is a static call: `T.method(args)`.
            // Validate the type/method/variant resolution here, mirroring
            // ConstructorCall's resolver path. Otherwise recurse into the
            // receiver as a value expression.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, cx.params, &cx.scopes)
                && let Some(decl) = cx.types.get(&id.name)
            {
                cx.errors.refs.record(id.span, SymbolKind::Type, &id.name);
                let table = cx.methods.get(&id.name).cloned().unwrap_or_default();
                let is_static_method = table.statics.contains_key(&method.name);
                let is_of_constructor = method.name == "of"
                    && matches!(
                        decl.body,
                        TypeBody::Refined { .. } | TypeBody::Opaque { .. }
                    );
                let is_unsafe_constructor =
                    method.name == "unsafe" && matches!(decl.body, TypeBody::Opaque { .. });
                let is_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == method.name),
                    _ => false,
                };
                if !(is_static_method || is_of_constructor || is_unsafe_constructor || is_variant) {
                    cx.errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, method.name
                            ),
                        )
                        // Finding #46: cross-file table lookup — see resolver.rs:1029.
                        .with_note("type declared here"),
                    );
                }
            } else {
                check_expr_references(receiver, cx);
            }
            for a in args {
                check_expr_references(a, cx);
            }
        }
        ExprKind::Match { discriminant, arms } => {
            check_expr_references(discriminant, cx);
            for arm in arms {
                // Pattern bindings introduce names in the arm body. The
                // type checker validates the pattern against the discriminant
                // type. Resolver pushes a scope with those binding names so
                // body references resolve.
                let mut arm_scope = HashMap::new();
                collect_pattern_bindings(&arm.pattern, &mut arm_scope);
                cx.scopes.push(arm_scope);
                match &arm.body {
                    MatchBody::Expr(e) => check_expr_references(e, cx),
                    MatchBody::Block(b) => check_block_references(b, cx),
                }
                cx.scopes.pop();
            }
        }
        ExprKind::Is { value, pattern } => {
            check_expr_references(value, cx);
            // `is` pattern bindings flow through to the truthy branch of
            // an enclosing context; binding scope is handled by the type
            // checker. Resolver doesn't introduce anything here.
            let _ = pattern;
        }
    }
}

/// Walk an expression collecting names introduced by `is` patterns inside
/// it, when applied as a Boolean test. Mirrors the binding-flow rule from
/// v0.2 §3.9 — bindings from `expr is Pat`, `lhs && (expr is Pat)`, or
/// `(expr is Pat)` flow into the surrounding truthy branch.
fn collect_is_binding_names(expr: &Expr, into: &mut HashMap<String, ()>) {
    match &expr.kind {
        ExprKind::Is { pattern, .. } => collect_is_pattern_binding_names(pattern, into),
        ExprKind::BinOp(BinOp::And, l, r) => {
            collect_is_binding_names(l, into);
            collect_is_binding_names(r, into);
        }
        ExprKind::Paren(inner) => collect_is_binding_names(inner, into),
        _ => {}
    }
}

/// The depth-1 names an `is` pattern introduces — a `Variant`'s own flat
/// bindings (`is` supports only flat, depth-1 name bindings, ADR 0169 keeps
/// nesting/guards match-only, matching `gather_pattern_bindings`), or — #474
/// — for an or-pattern, the first alternative's (Rule 2 guarantees every
/// alternative gives a shared name the same type, so any one alternative's
/// names are representative of them all).
fn collect_is_pattern_binding_names(pattern: &Pattern, into: &mut HashMap<String, ()>) {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            for b in bindings {
                if let Pattern::Binding(name) = b.pattern() {
                    into.insert(name.name.clone(), ());
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                collect_is_pattern_binding_names(first, into);
            }
        }
        _ => {}
    }
}

/// Walk a pattern collecting the names it would bind, recursively through
/// nested payload patterns (ADR 0169) — `Some(Ok(x))` binds `x`.
fn collect_pattern_bindings(pattern: &Pattern, into: &mut HashMap<String, ()>) {
    for id in pattern.bound_names() {
        into.insert(id.name.clone(), ());
    }
}

/// Find the unique sum type that owns a given variant name. Returns None
/// if no type owns it; ignores cases of multiple owners (those are
/// reported via `find_ambiguous_variant_owners`).
fn find_unique_variant_owner<'a>(
    name: &str,
    types: &'a HashMap<String, Arc<TypeDecl>>,
) -> Option<&'a TypeDecl> {
    let owners = find_ambiguous_variant_owners(name, types);
    if owners.len() == 1 {
        Some(owners[0])
    } else {
        None
    }
}

fn find_ambiguous_variant_owners<'a>(
    name: &str,
    types: &'a HashMap<String, Arc<TypeDecl>>,
) -> Vec<&'a TypeDecl> {
    let mut out = Vec::new();
    for t in types.values() {
        if let TypeBody::Sum(s) = &t.body
            && s.variants.iter().any(|v| v.name.name == name)
        {
            out.push(t.as_ref());
        }
    }
    out
}
