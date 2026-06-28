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

use crate::index::{RefSink, SymbolKind};
use bynk_syntax::ast::*;
use bynk_syntax::error::CompileError;

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
    pub instance: HashMap<String, FnDecl>,
    pub statics: HashMap<String, FnDecl>,
}

/// Output of resolution: the AST plus the symbol tables the checker needs.
pub struct ResolvedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    /// Per-type method tables (instance + static).
    pub methods: HashMap<String, MethodTable>,
    /// Names of types declared in *this* commons (as opposed to imported via
    /// `uses`). Used by the checker to gate access to `.raw` and `.unsafe()`
    /// on opaque types.
    pub local_type_names: std::collections::HashSet<String>,
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
    pub consumed_types: HashMap<String, HashMap<String, TypeDecl>>,
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
}

/// Resolve names in a single-file (or already-merged) commons. Use this
/// entry point only for self-contained Bynk programs. For multi-file
/// projects and `uses`-resolving commons, use [`resolve_file`] against a
/// pre-built combined symbol table.
pub fn resolve(commons: Commons) -> Result<ResolvedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
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
            | CommonsItem::Actor(_) => {}
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
                    types.insert(t.name.name.clone(), t.clone());
                    methods.insert(t.name.name.clone(), MethodTable::default());
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
                        fns.insert(id.name.clone(), f.clone());
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
                        bucket.insert(method_name.name.clone(), f.clone());
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
            CommonsItem::Fn(f) => {
                check_fn_refs(f, &types, &fns, &methods, &mut sinks);
            }
            // v0.5 items are resolved via a separate context-level pass.
            CommonsItem::Capability(_)
            | CommonsItem::Provider(_)
            | CommonsItem::Service(_)
            | CommonsItem::Agent(_)
            | CommonsItem::Actor(_) => {}
        }
    }

    if errors.is_empty() {
        let local_type_names = types.keys().cloned().collect();
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
            | CommonsItem::Actor(_) => {}
        }
        sinks.refs.clear_owner();
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Recursively walk a type declaration to check that every type reference
/// inside it resolves.
fn check_type_decl_refs(t: &TypeDecl, types: &HashMap<String, TypeDecl>, errors: &mut Sinks) {
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
                // Detect direct self-reference: `type A = { f: A }`.
                // v0.2 has no indirection (no `Option[A]`, no records-of-records
                // wrapping the same type) for this check to defeat; a direct
                // `Named(A)` field where A is the enclosing type is forbidden.
                if let TypeRef::Named(id) = &f.type_ref
                    && id.name == t.name.name
                {
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
                }
                check_type_ref_resolves(&f.type_ref, types, errors);
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
                    check_type_ref_resolves(&f.type_ref, types, errors);
                }
            }
        }
    }
}

fn check_fn_refs(
    f: &FnDecl,
    types: &HashMap<String, TypeDecl>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Sinks,
) {
    // Parameter types resolve.
    // v0.20a: the fn's type parameters are legal named references in its
    // own signature and body annotations.
    let type_params: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
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
    let mut scopes: Vec<HashMap<String, ()>> = Vec::new();
    check_block_references(
        &f.body,
        &params,
        in_method,
        &mut scopes,
        types,
        &type_params,
        fns,
        methods,
        errors,
    );
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

/// Recursively check that every type reference resolves.
fn check_type_ref_resolves(r: &TypeRef, types: &HashMap<String, TypeDecl>, errors: &mut Sinks) {
    check_type_ref_resolves_in(r, types, &HashSet::new(), errors)
}

/// v0.20a: like [`check_type_ref_resolves`], with the enclosing function's
/// type parameters in scope — a `Named` reference matching one is a type
/// variable, not an unknown type.
fn check_type_ref_resolves_in(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
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
            if types.contains_key(&id.name) {
                errors.refs.record(id.span, SymbolKind::Type, &id.name);
            } else if !type_params.contains(&id.name) {
                errors.push(unknown_type_error(id));
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
    types: &HashMap<String, TypeDecl>,
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

#[allow(clippy::too_many_arguments)]
fn check_block_references(
    block: &Block,
    params: &HashMap<String, ()>,
    in_method: bool,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    type_params: &HashSet<String>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Sinks,
) {
    scopes.push(HashMap::new());
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                check_expr_references(
                    &l.value,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
                if let Some(annot) = &l.type_annot {
                    check_type_ref_resolves_in(annot, types, type_params, errors);
                }
                if let Some(prev) = types.get(&l.name.name) {
                    errors.push(
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
                } else if let Some(prev) = fns.get(&l.name.name) {
                    errors.push(
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
                    scopes.last_mut().unwrap().insert(l.name.name.clone(), ());
                }
            }
            Statement::Assert(a) => {
                check_expr_references(
                    &a.value,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
            Statement::Send(s) => {
                check_expr_references(
                    &s.value,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
            Statement::Assign(a) => {
                // v0.81: walk the RHS for references; the target resolves to a
                // `store` field, handled in the storage-track checker slice.
                check_expr_references(
                    &a.value,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
    }
    check_expr_references(
        &block.tail,
        params,
        in_method,
        scopes,
        types,
        type_params,
        fns,
        methods,
        errors,
    );
    scopes.pop();
}

#[allow(clippy::too_many_arguments)]
fn check_expr_references(
    expr: &Expr,
    params: &HashMap<String, ()>,
    in_method: bool,
    scopes: &mut Vec<HashMap<String, ()>>,
    types: &HashMap<String, TypeDecl>,
    type_params: &HashSet<String>,
    fns: &HashMap<String, FnDecl>,
    methods: &HashMap<String, MethodTable>,
    errors: &mut Sinks,
) {
    match &expr.kind {
        // v0.43: resolve names referenced inside each interpolation hole.
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(hole) = part {
                    check_expr_references(
                        hole,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    );
                }
            }
        }
        ExprKind::IntLit(_)
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        // v0.20b: a list literal — each element resolves as a value.
        ExprKind::ListLit(elems) => {
            for el in elems {
                check_expr_references(
                    el,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
        // v0.20a: a lambda introduces a scope frame holding its params; the
        // body walks with the frame in place. Annotated param types resolve
        // through the ordinary type-ref check.
        ExprKind::Lambda(lambda) => {
            for p in &lambda.params {
                if let Some(tr) = &p.type_ref {
                    check_type_ref_resolves_in(tr, types, type_params, errors);
                }
            }
            let mut frame: HashMap<String, ()> = HashMap::new();
            for p in &lambda.params {
                frame.insert(p.name.name.clone(), ());
            }
            scopes.push(frame);
            check_expr_references(
                &lambda.body,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            scopes.pop();
        }
        ExprKind::EffectPure(inner) => {
            check_expr_references(
                inner,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
        }
        ExprKind::Assert(inner) => {
            check_expr_references(
                inner,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
        }
        ExprKind::Mock { args, .. } => {
            // v0.9.4: the mocked type is validated by the checker; resolve any
            // pin-argument references here.
            for a in args {
                check_expr_references(
                    a,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => {
            if let Some(tn) = type_name
                && !types.contains_key(&tn.name)
            {
                errors.push(unknown_type_error(tn));
            }
            check_expr_references(
                base,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            for f in overrides {
                if let Some(v) = &f.value {
                    check_expr_references(
                        v,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    );
                }
            }
        }
        ExprKind::Ident(id) => {
            if id.name == "self" {
                if !in_method {
                    errors.push(
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
            if name_in_scope(&id.name, params, scopes) {
                // OK.
            } else if http_variant(&id.name).is_some() {
                // v0.9: predeclared HttpResult variant (e.g. `NoContent`,
                // `Unauthorized`). The checker validates payload arity and
                // expected-type disambiguation.
            } else if let Some(sum_owner) = find_unique_variant_owner(&id.name, types) {
                // It's a bare variant reference. We treat it as a valid
                // expression in resolver — the type checker will assign
                // the correct sum type. Mark with no error.
                let _ = sum_owner;
            } else if types.contains_key(&id.name) {
                errors.push(
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
            } else if fns.contains_key(&id.name) {
                // v0.20a: a bare named-function reference may be a function
                // VALUE where a function type is expected. The resolver has
                // no type information, so the judgment (and the
                // `bynk.resolve.fn_without_call` diagnostic for non-function
                // positions) now lives in the checker's ident rule. Silent
                // pass here keeps `unknown_name` from misfiring.
                errors.refs.record(id.span, SymbolKind::Fn, &id.name);
            } else if find_ambiguous_variant_owners(&id.name, types).len() > 1 {
                errors.push(
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
                errors.push(
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
        ExprKind::Call { name, args, .. } => {
            match fns.get(&name.name) {
                Some(decl) => {
                    errors.refs.record(name.span, SymbolKind::Fn, &name.name);
                    if decl.params.len() != args.len() {
                        errors.push(
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
                            .with_label(decl.name.ident().span, "function declared here"),
                        );
                    }
                }
                None => {
                    // Maybe it's a variant constructor with a payload (e.g., `Placed(at, total)`).
                    let owners = find_ambiguous_variant_owners(&name.name, types);
                    if http_variant(&name.name).is_some() {
                        // v0.9: predeclared HttpResult variant constructor.
                    } else if owners.len() == 1 {
                        // Single owner — treat as variant construction. Type
                        // checker validates arg count and types.
                    } else if owners.len() > 1 {
                        errors.push(CompileError::new(
                            "bynk.resolve.ambiguous_variant",
                            name.span,
                            format!(
                                "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}(...)`",
                                name.name, name.name
                            ),
                        ));
                    } else if types.contains_key(&name.name) {
                        errors.push(CompileError::new(
                            "bynk.resolve.type_as_function",
                            name.span,
                            format!(
                                "`{}` is a type, not a function — use `{}.of(value)` or `{} {{ ... }}` instead",
                                name.name, name.name, name.name
                            ),
                        ));
                    } else if name_in_scope(&name.name, params, scopes) {
                        // v0.20a: an in-scope value being called may be a
                        // legal value application if its type is a function
                        // type. The resolver has no type information, so the
                        // judgment (and `bynk.resolve.param_as_function` for
                        // non-function-typed values) lives in the checker's
                        // call dispatch. Silent pass.
                    } else {
                        errors.push(
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
                check_expr_references(
                    a,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
        ExprKind::BinOp(_, lhs, rhs) => {
            check_expr_references(
                lhs,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            check_expr_references(
                rhs,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
        }
        ExprKind::UnaryOp(_, e) => check_expr_references(
            e,
            params,
            in_method,
            scopes,
            types,
            type_params,
            fns,
            methods,
            errors,
        ),
        ExprKind::Paren(e) => check_expr_references(
            e,
            params,
            in_method,
            scopes,
            types,
            type_params,
            fns,
            methods,
            errors,
        ),
        ExprKind::Block(b) => check_block_references(
            b,
            params,
            in_method,
            scopes,
            types,
            type_params,
            fns,
            methods,
            errors,
        ),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            check_expr_references(
                cond,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            // `is`-pattern bindings inside the condition flow into the
            // then-branch's scope (v0.2 §3.9).
            let mut then_extra: HashMap<String, ()> = HashMap::new();
            collect_is_binding_names(cond, &mut then_extra);
            scopes.push(then_extra);
            check_block_references(
                then_block,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            scopes.pop();
            check_block_references(
                else_block,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
        }
        ExprKind::Ok(inner) | ExprKind::Err(inner) | ExprKind::Question(inner) => {
            check_expr_references(
                inner,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
        }
        ExprKind::Some(inner) => {
            check_expr_references(
                inner,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
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
                    errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                }
                for a in args {
                    check_expr_references(
                        a,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    );
                }
                return;
            }
            if let Some(decl) = types.get(&type_name.name) {
                errors
                    .refs
                    .record(type_name.span, SymbolKind::Type, &type_name.name);
                let table = methods.get(&type_name.name).cloned().unwrap_or_default();
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
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                type_name.name, method.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                errors.push(unknown_type_error(type_name));
            }
            for a in args {
                check_expr_references(
                    a,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            match types.get(&type_name.name) {
                Some(decl) => {
                    errors
                        .refs
                        .record(type_name.span, SymbolKind::Type, &type_name.name);
                    match &decl.body {
                        TypeBody::Record(r) => {
                            let declared: HashMap<&str, &RecordField> =
                                r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
                            let mut provided: HashMap<&str, &Ident> = HashMap::new();
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
                                        .with_label(decl.name.span, "type declared here"),
                                    );
                                }
                                if let Some(prev) = provided.get(f.name.name.as_str()) {
                                    errors.push(
                                        CompileError::new(
                                            "bynk.resolve.duplicate_field_init",
                                            f.name.span,
                                            format!(
                                                "field `{}` is initialised more than once",
                                                f.name.name
                                            ),
                                        )
                                        .with_label(prev.span, "previously initialised here"),
                                    );
                                } else {
                                    provided.insert(f.name.name.as_str(), &f.name);
                                }
                                // Shorthand `name` — must be in scope.
                                match &f.value {
                                    Some(v) => check_expr_references(
                                        v,
                                        params,
                                        in_method,
                                        scopes,
                                        types,
                                        type_params,
                                        fns,
                                        methods,
                                        errors,
                                    ),
                                    None => {
                                        if !name_in_scope(&f.name.name, params, scopes) {
                                            errors.push(
                                            CompileError::new(
                                                "bynk.resolve.unknown_name",
                                                f.name.span,
                                                format!(
                                                    "shorthand field initialiser `{}` requires a binding of that name in scope",
                                                    f.name.name
                                                ),
                                            )
                                            .with_note(
                                                "either bring `{name}` into scope or use the full `field: value` form",
                                            ),
                                        );
                                        }
                                    }
                                }
                            }
                            for decl_field in &r.fields {
                                if !provided.contains_key(decl_field.name.name.as_str()) {
                                    errors.push(
                                        CompileError::new(
                                            "bynk.resolve.missing_field",
                                            type_name.span,
                                            format!(
                                                "missing required field `{}` for record `{}`",
                                                decl_field.name.name, type_name.name
                                            ),
                                        )
                                        .with_label(decl_field.name.span, "field declared here"),
                                    );
                                }
                            }
                        }
                        TypeBody::Opaque { .. } => {
                            errors.push(
                            CompileError::new(
                                "bynk.resolve.opaque_record_construction",
                                type_name.span,
                                format!(
                                    "opaque type `{}` cannot be constructed with record-literal syntax",
                                    type_name.name
                                ),
                            )
                            .with_label(decl.name.span, "type declared here")
                            .with_note(
                                "construct opaque values via `T.of(value)` (validated) or `T.unsafe(value)` (inside the defining commons)",
                            ),
                        );
                        }
                        _ => {
                            errors.push(
                            CompileError::new(
                                "bynk.resolve.not_a_record_type",
                                type_name.span,
                                format!(
                                    "`{}` is not a record type — only record types can be constructed with `{{ ... }}`",
                                    type_name.name
                                ),
                            )
                            .with_label(decl.name.span, "type declared here"),
                        );
                        }
                    }
                }
                None => errors.push(unknown_type_error(type_name)),
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // v0.9: `HttpResult.Variant` qualified nullary variant.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && id.name == "HttpResult"
            {
                if http_variant(&field.name).is_none() {
                    errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        field.span,
                        format!("`HttpResult` has no variant named `{}`", field.name),
                    ));
                }
                return;
            }
            // `TypeName.Variant` — qualified nullary variant reference.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && let Some(decl) = types.get(&id.name)
            {
                errors.refs.record(id.span, SymbolKind::Type, &id.name);
                let known_variant = match &decl.body {
                    TypeBody::Sum(s) => s.variants.iter().any(|v| v.name.name == field.name),
                    _ => false,
                };
                if !known_variant {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            field.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, field.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                check_expr_references(
                    receiver,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
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
                && !name_in_scope(&id.name, params, scopes)
                && id.name == "HttpResult"
            {
                if http_variant(&method.name).is_none() {
                    errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                }
                for a in args {
                    check_expr_references(
                        a,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    );
                }
                return;
            }
            // v0.20b: `List.empty()` / `Map.empty()` — qualified statics on
            // the built-in collection types (no user declaration to resolve
            // against; the checker owns their typing). v0.22a adds the
            // numeric parse statics, `Int.parse(…)` / `Float.parse(…)`.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && matches!(
                    id.name.as_str(),
                    "List" | "Map" | "Int" | "Float" | "Json" | "Duration" | "Instant" | "Stream"
                )
                && !types.contains_key(&id.name)
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
                    _ => &["parse"],
                };
                let only = allowed.join("`/`");
                if !allowed.contains(&method.name.as_str()) {
                    errors.push(CompileError::new(
                        "bynk.resolve.unknown_static_member",
                        method.span,
                        format!(
                            "the built-in `{}` type has no static method named `{}` — the statics are `{only}`",
                            id.name, method.name
                        ),
                    ));
                }
                for a in args {
                    check_expr_references(
                        a,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    );
                }
                return;
            }
            // If the receiver is a bare ident of a declared type (and not a
            // local binding), this is a static call: `T.method(args)`.
            // Validate the type/method/variant resolution here, mirroring
            // ConstructorCall's resolver path. Otherwise recurse into the
            // receiver as a value expression.
            if let ExprKind::Ident(id) = &receiver.kind
                && !name_in_scope(&id.name, params, scopes)
                && let Some(decl) = types.get(&id.name)
            {
                errors.refs.record(id.span, SymbolKind::Type, &id.name);
                let table = methods.get(&id.name).cloned().unwrap_or_default();
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
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.unknown_static_member",
                            method.span,
                            format!(
                                "type `{}` has no static method or variant named `{}`",
                                id.name, method.name
                            ),
                        )
                        .with_label(decl.name.span, "type declared here"),
                    );
                }
            } else {
                check_expr_references(
                    receiver,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
            for a in args {
                check_expr_references(
                    a,
                    params,
                    in_method,
                    scopes,
                    types,
                    type_params,
                    fns,
                    methods,
                    errors,
                );
            }
        }
        ExprKind::Match { discriminant, arms } => {
            check_expr_references(
                discriminant,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
            for arm in arms {
                // Pattern bindings introduce names in the arm body. The
                // type checker validates the pattern against the discriminant
                // type. Resolver pushes a scope with those binding names so
                // body references resolve.
                let mut arm_scope = HashMap::new();
                collect_pattern_bindings(&arm.pattern, &mut arm_scope);
                scopes.push(arm_scope);
                match &arm.body {
                    MatchBody::Expr(e) => check_expr_references(
                        e,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    ),
                    MatchBody::Block(b) => check_block_references(
                        b,
                        params,
                        in_method,
                        scopes,
                        types,
                        type_params,
                        fns,
                        methods,
                        errors,
                    ),
                }
                scopes.pop();
            }
        }
        ExprKind::Is { value, pattern } => {
            check_expr_references(
                value,
                params,
                in_method,
                scopes,
                types,
                type_params,
                fns,
                methods,
                errors,
            );
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
        ExprKind::Is {
            pattern: Pattern::Variant { bindings, .. },
            ..
        } => {
            for b in bindings {
                if !b.is_wildcard() {
                    into.insert(b.local_name().name.clone(), ());
                }
            }
        }
        ExprKind::BinOp(BinOp::And, l, r) => {
            collect_is_binding_names(l, into);
            collect_is_binding_names(r, into);
        }
        ExprKind::Paren(inner) => collect_is_binding_names(inner, into),
        _ => {}
    }
}

/// Walk a pattern collecting the names it would bind.
fn collect_pattern_bindings(pattern: &Pattern, into: &mut HashMap<String, ()>) {
    match pattern {
        Pattern::Wildcard(_) => {}
        Pattern::Variant { bindings, .. } => {
            for b in bindings {
                if !b.is_wildcard() {
                    into.insert(b.local_name().name.clone(), ());
                }
            }
        }
    }
}

/// Find the unique sum type that owns a given variant name. Returns None
/// if no type owns it; ignores cases of multiple owners (those are
/// reported via `find_ambiguous_variant_owners`).
fn find_unique_variant_owner<'a>(
    name: &str,
    types: &'a HashMap<String, TypeDecl>,
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
    types: &'a HashMap<String, TypeDecl>,
) -> Vec<&'a TypeDecl> {
    let mut out = Vec::new();
    for t in types.values() {
        if let TypeBody::Sum(s) = &t.body
            && s.variants.iter().any(|v| v.name.name == name)
        {
            out.push(t);
        }
    }
    out
}
