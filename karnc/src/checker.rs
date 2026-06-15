//! Type checker and refinement validator (spec §§5–6, v0.1 §4.2, v0.2 §4.2).
//!
//! Operates on a [`ResolvedCommons`]. Walks declarations, validates each
//! refinement against the spec's predicate-base compatibility and combination
//! rules, then type-checks every function and method body.
//!
//! v0.2 extensions:
//! - Record types (compatibility, field access, construction).
//! - Sum types and variant construction (qualified and unqualified).
//! - Methods (instance and static) with UFCS-style call resolution.
//! - Pattern matching with exhaustiveness checking.
//! - The `is` operator with binding flow into truthy contexts.
//! - The built-in generic `Option[T]`.

use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::ast::*;
use crate::builtin_names::methods::*;
use crate::builtin_names::types::*;
use crate::error::{Applicability, CompileError};
use crate::hints::HintSink;
use crate::index::{RefSink, SymbolKind};
use crate::locals::LocalsSink;
use crate::resolver::{MethodTable, ResolvedCommons};
use crate::span::Span;

mod calls;
mod expressions;
mod kernels;
mod refinements;

use calls::*;
use expressions::*;
use kernels::*;
use refinements::*;

pub use calls::check_state_initialiser;
pub use refinements::zero_value_ts;

// ==== Type representation ====

/// A resolved type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A base type (`Int`, `String`, `Bool`).
    Base(BaseType),
    /// A user-declared named type. `kind` records the declaration's shape
    /// for compatibility / dispatch decisions.
    Named { name: String, kind: NamedKind },
    /// `Result[T, E]`.
    Result(Box<Ty>, Box<Ty>),
    /// `Option[T]`.
    Option(Box<Ty>),
    /// `Effect[T]` (v0.5).
    Effect(Box<Ty>),
    /// `HttpResult[T]` (v0.9).
    HttpResult(Box<Ty>),
    /// `List[T]` — built-in immutable list (v0.20b).
    List(Box<Ty>),
    /// `Map[K, V]` — built-in immutable map (v0.20b). The key type is
    /// confined to value-keyable types at TypeRef resolution.
    Map(Box<Ty>, Box<Ty>),
    /// `ValidationError` — built-in error type.
    ValidationError,
    /// `JsonError` — built-in JSON-decode error type (v0.22b). A uniform
    /// record: `kind`/`path`/`message`, all `String`.
    JsonError,
    /// `()` — the unit type (v0.5).
    Unit,
    /// `A -> B` — a function type (v0.20a). Effectful iff `ret` is
    /// `Effect[_]` (the structural rule); no separate flag, so there is a
    /// single source of truth.
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    /// A function type parameter (v0.20a). Two lives: *rigid* while checking
    /// a generic function's own body (name-equality in `compatible`), and
    /// *flexible* during call-site instantiation, where it is matched by
    /// `unify` and fully eliminated by `substitute` before any `compatible`
    /// runs against argument types. Vars never escape call checking into the
    /// caller's expression types.
    Var(String),
}

/// The shape of a named type — what its declaration looks like.
///
/// `Refined` widens to its base type when used in arithmetic, comparisons,
/// and other operations on the base. `Opaque` does NOT widen — its identity
/// is nominal and the base type is hidden outside the defining commons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedKind {
    /// Refined-base type: widens to the recorded base.
    Refined(BaseType),
    /// Record type.
    Record,
    /// Sum type.
    Sum,
    /// Opaque base type. The base is hidden; identity is purely nominal.
    /// The recorded base is used by the type checker (for `.raw`, `.of`,
    /// `.unsafe`) and by the emitter, but not for compatibility widening.
    Opaque(BaseType),
}

impl Ty {
    /// Display name for diagnostics.
    pub fn display(&self) -> String {
        match self {
            Ty::Base(b) => b.name().to_string(),
            Ty::Named { name, .. } => name.clone(),
            Ty::Result(t, e) => format!("Result[{}, {}]", t.display(), e.display()),
            Ty::Option(t) => format!("Option[{}]", t.display()),
            Ty::Effect(t) => format!("Effect[{}]", t.display()),
            Ty::HttpResult(t) => format!("HttpResult[{}]", t.display()),
            Ty::List(t) => format!("List[{}]", t.display()),
            Ty::Map(k, v) => format!("Map[{}, {}]", k.display(), v.display()),
            Ty::ValidationError => "ValidationError".to_string(),
            Ty::JsonError => "JsonError".to_string(),
            Ty::Unit => "()".to_string(),
            Ty::Fn { params, ret } => {
                let params = match params.len() {
                    0 => "()".to_string(),
                    // A single Fn-typed param needs parens to stay readable
                    // under right-associativity.
                    1 if !matches!(params[0], Ty::Fn { .. }) => params[0].display(),
                    _ => format!(
                        "({})",
                        params
                            .iter()
                            .map(|p| p.display())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                format!("{params} -> {}", ret.display())
            }
            Ty::Var(name) => name.clone(),
        }
    }

    /// True if this type is `Effect[_]`.
    pub fn is_effect(&self) -> bool {
        matches!(self, Ty::Effect(_))
    }

    /// The underlying base type, if this type widens to a base type.
    /// Opaque types deliberately do NOT widen — that's the whole point of
    /// the opacity — so `Ty::Named { kind: Opaque(_), .. }` returns None.
    pub(crate) fn base(&self) -> Option<BaseType> {
        match self {
            Ty::Base(b) => Some(*b),
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            } => Some(*b),
            _ => None,
        }
    }
}

/// Output of type checking.
pub struct TypedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub methods: HashMap<String, MethodTable>,
    pub expr_types: HashMap<Span, Ty>,
}

// ==== Entry points ====

pub fn check(input: ResolvedCommons) -> Result<TypedCommons, Vec<CompileError>> {
    check_record(
        input,
        &mut RefSink::new(),
        &mut HintSink::new(),
        &mut LocalsSink::new(),
    )
}

/// [`check`], recording binding edges into `refs` at the checker's
/// resolution sites (v0.25). A fresh sink records nothing.
pub fn check_record(
    input: ResolvedCommons,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
) -> Result<TypedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut expr_types: HashMap<Span, Ty> = HashMap::new();

    // 1. Validate each type declaration.
    for item in &input.commons.items {
        if let CommonsItem::Type(t) = item {
            check_type_decl(t, &input.types, &mut errors);
        }
    }

    // 2. Type-check each function and method body.
    for item in &input.commons.items {
        if let CommonsItem::Fn(f) = item {
            refs.set_owner(f.name.display());
            check_fn(f, &input, &mut expr_types, &mut errors, refs, hints, locals);
            refs.clear_owner();
        }
    }

    if errors.is_empty() {
        Ok(TypedCommons {
            commons: input.commons,
            types: input.types,
            fns: input.fns,
            methods: input.methods,
            expr_types,
        })
    } else {
        Err(errors)
    }
}

/// Check a single handler body (used for service and agent handlers).
///
/// `capabilities_in_scope` is the set of capabilities the handler may
/// reference. `agent_state_ty` is set when checking an agent handler — it
/// determines the type the `commit` statement must produce.
#[allow(clippy::too_many_arguments)]
pub fn check_handler_body(
    body: &Block,
    return_type: &TypeRef,
    return_ty_span: Span,
    params: &[Param],
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    capabilities: HashMap<String, CapabilityInfo>,
    declared_capabilities: HashMap<String, CapabilityInfo>,
    agent_state_ty: Option<Ty>,
    agent_self_scope: Option<HashMap<String, Ty>>,
    given: &[CapRef],
    given_anchor: Option<Span>,
    report_unused: bool,
) {
    let Some(return_ty) = resolve_type_ref(return_type, &input.types) else {
        return;
    };
    let no_vars = HashSet::new();
    record_type_refs(return_type, &input.types, &no_vars, refs);
    // Build the parameter scope.
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    for p in params {
        if let Some(t) = resolve_type_ref(&p.type_ref, &input.types) {
            record_type_refs(&p.type_ref, &input.types, &no_vars, refs);
            // v0.31: a handler/op parameter is in scope over the whole body.
            if p.name.name != "_" {
                locals.record(p.name.name.clone(), p.name.span, t.display(), body.span);
            }
            param_scope.insert(p.name.name.clone(), t);
        }
    }
    if let Some(self_scope) = agent_self_scope {
        param_scope.extend(self_scope);
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let given_entries: Vec<(String, Span)> = given
        .iter()
        .map(|c| (c.key().to_string(), c.span))
        .collect();
    let given_remaining: HashSet<String> = given_entries.iter().map(|(k, _)| k.clone()).collect();
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        refs,
        hints,
        locals,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty,
        commit_seen: false,
        caps: CapabilityCtx {
            capabilities,
            declared_capabilities,
            given_remaining,
            given_used: HashSet::new(),
            given_entries: given_entries.clone(),
            given_anchor,
        },
        in_test_body: false,
        test_services: HashSet::new(),
        type_vars: HashSet::new(),
    };
    // Check the body and validate it matches the return type.
    let Some(body_ty) = type_of_block(body, Some(&return_ty), &mut ctx) else {
        return;
    };
    if !compatible(&body_ty, &return_ty) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.return_mismatch",
                body.tail.span,
                format!(
                    "handler body has type `{}`, but the declared return type is `{}`",
                    body_ty.display(),
                    return_ty.display()
                ),
            )
            .with_label(return_ty_span, "declared return type"),
        );
    }
    // Bidirectional `given` check.
    // 1) Every used capability is declared. (Handled in capability-call site.)
    // 2) Every declared capability is used — anything left in given_remaining
    //    minus given_used is unused. Emit as a warning-category error so the
    //    test harness can match it. Entries are walked in declaration order
    //    (deduplicated by key) so diagnostics and their fixes are stable.
    let mut reported: HashSet<&str> = HashSet::new();
    for (i, (c, _)) in given_entries.iter().enumerate() {
        if !report_unused {
            break;
        }
        if ctx.caps.given_used.contains(c) || !reported.insert(c) {
            continue;
        }
        ctx.errors.push(
            CompileError::new(
                "karn.given.unused_capability",
                return_ty_span,
                format!("capability `{c}` is declared in `given` but never used in the body"),
            )
            .with_note(
                "remove the capability from the `given` clause, or use it in the handler body",
            )
            // v0.26 (ADR 0054): the removal is list-aware — only `report_unused`
            // sites are handlers, where the clause follows the return type, so
            // `return_ty_span` anchors the only-entry case.
            .with_suggestion(
                format!("remove `{c}` from the `given` clause"),
                vec![(
                    given_removal_span(&given_entries, i, return_ty_span),
                    String::new(),
                )],
                Applicability::MachineApplicable,
            ),
        );
    }
}

// ==== Checking context and capability metadata ====

/// v0.9.4: a compile-time-constant literal usable for static refinement
/// discharge during `T.of(...)` construction.
enum ConstLit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
}

impl ConstLit {
    fn display(&self) -> String {
        match self {
            ConstLit::Int(n) => n.to_string(),
            ConstLit::Float(v) => v.to_string(),
            ConstLit::Str(s) => format!("{s:?}"),
            ConstLit::Bool(b) => b.to_string(),
            ConstLit::Unit => "()".to_string(),
        }
    }
}

/// Mutable per-function context.
/// Capability bookkeeping for the checker — the `given`-clause lifecycle and
/// capability dispatch, grouped out of [`Ctx`] (v0.29.10). Empty (`Default`)
/// for pure functions / non-context code.
#[derive(Default)]
pub struct CapabilityCtx {
    /// Capabilities in scope for the current handler, as a name → CapabilityInfo
    /// map. Empty for pure functions and non-context code.
    pub capabilities: HashMap<String, CapabilityInfo>,
    /// All capabilities declared in the surrounding context (for diagnostic
    /// purposes — used to detect `<Cap>.op(...)` calls where the capability is
    /// declared in the context but not listed in `given`).
    pub declared_capabilities: HashMap<String, CapabilityInfo>,
    /// Names of capabilities the user listed in `given`, but haven't yet
    /// observed used. After checking the body, anything left here is
    /// unused — a warning.
    pub given_remaining: HashSet<String>,
    /// Names of capabilities actually used in the body so far.
    pub given_used: HashSet<String>,
    /// v0.26 (ADR 0054): the `given` clause's entries in declaration order —
    /// (deps key, source span) — so the `given` quick-fixes can author
    /// list-aware edits at the diagnosis site. Empty where no `given` clause
    /// applies (fns, mock ops, state initialisers).
    pub given_entries: Vec<(String, Span)>,
    /// v0.26: where the add-capability fix synthesises an *absent* `given`
    /// clause — the handler's return type (the clause follows it). `None`
    /// where the clause lives elsewhere (a provider's `provides … given`
    /// line); the fix is then offered only when entries already exist.
    pub given_anchor: Option<Span>,
}

pub struct Ctx<'a> {
    pub input: &'a ResolvedCommons,
    pub expr_types: &'a mut HashMap<Span, Ty>,
    pub errors: &'a mut Vec<CompileError>,
    /// v0.25 (ADR 0053): binding edges recorded at the checker's own
    /// resolution sites — capability/service dispatch, typed call dispatch,
    /// annotation resolution. Handler/test/provider bodies never pass
    /// through the resolver's reference walk, so the checker is their only
    /// recording point.
    pub refs: &'a mut RefSink,
    /// v0.27 (ADR 0056): inferred-type inlay hints recorded at the
    /// annotation-absent binding sites (`let` / `let <-` / lambda params)
    /// as the binding's final type is computed.
    pub hints: &'a mut HintSink,
    /// v0.31 (ADR 0064): local bindings recorded with their scope ranges at
    /// every binding site (`let`/`let <-`, params, match patterns), for the
    /// LSP's scope-at-offset query.
    pub locals: &'a mut LocalsSink,
    /// Stack of in-scope name → type frames.
    pub scopes: Vec<HashMap<String, Ty>>,
    pub return_ty: Ty,
    pub return_ty_span: Span,
    /// True if the enclosing function/handler returns `Effect[T]` (v0.5).
    /// Determines whether `<-` and capability calls are permitted.
    pub effectful: bool,
    /// If inside an agent handler, the agent's state type and the agent's
    /// name. Used to validate `commit` statements.
    pub agent_state_ty: Option<Ty>,
    /// True if a `commit` has been seen on the current control-flow path.
    /// Used to detect "two reachable commits".
    pub commit_seen: bool,
    /// Capability bookkeeping — the `given`-clause lifecycle + dispatch,
    /// grouped (v0.29.10). Empty for pure functions / non-context code.
    pub caps: CapabilityCtx,
    /// True when the body being checked is a test case body. Permits
    /// `assert` statements (v0.7).
    pub in_test_body: bool,
    /// The target unit's service names, populated for test case bodies
    /// (v0.25). `svc.call(args)` in a test invokes the target's service —
    /// the emitter wires it from the same set; the checker records the
    /// binding edge here so test-file references index.
    pub test_services: HashSet<String>,
    /// v0.20a: the enclosing function's type parameters (rigid vars), so
    /// nested explicit type arguments (`identity[A](x)` inside a generic
    /// body) resolve. Empty outside generic fn bodies.
    pub type_vars: HashSet<String>,
}

/// Per-capability info for checker dispatch within a handler body.
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    pub name: String,
    pub ops: Vec<CapabilityOpInfo>,
}

#[derive(Debug, Clone)]
pub struct CapabilityOpInfo {
    pub name: String,
    pub params: Vec<Ty>,
    pub return_ty: Ty,
}

impl<'a> Ctx<'a> {
    pub fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    /// Returns the type of an expression's "root identifier" — for `a.b.c`
    /// that's `a`; for a bare `a` it's `a`. Used to detect whether a chain's
    /// outermost name shadows an alias / consumed-context prefix.
    pub fn lookup_root_ident(&self, expr: &Expr) -> Option<Ty> {
        match &expr.kind {
            ExprKind::Ident(id) => self.lookup(&id.name),
            ExprKind::FieldAccess { receiver, .. } => self.lookup_root_ident(receiver),
            ExprKind::MethodCall { receiver, .. } => self.lookup_root_ident(receiver),
            _ => None,
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    pub fn bind(&mut self, name: String, ty: Ty) {
        self.scopes.last_mut().unwrap().insert(name, ty);
    }
}

// ==== Type-system core (resolution, unification, compatibility, inference) ====

/// Build a `Ty` from a TypeDecl name reference.
pub fn type_from_decl(id: &Ident, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    let decl = types.get(&id.name)?;
    Some(named_ty(decl))
}

/// Build a `Ty::Named` for the given declaration.
pub fn named_ty(decl: &TypeDecl) -> Ty {
    let kind = match &decl.body {
        TypeBody::Refined { base, .. } => NamedKind::Refined(*base),
        TypeBody::Record(_) => NamedKind::Record,
        TypeBody::Sum(_) => NamedKind::Sum,
        TypeBody::Opaque { base, .. } => NamedKind::Opaque(*base),
    };
    Ty::Named {
        name: decl.name.name.clone(),
        kind,
    }
}

/// v0.20a: like [`resolve_type_ref`], with a set of in-scope **type
/// parameters**: a `Named` reference matching one resolves to [`Ty::Var`]
/// (checked before the type-table lookup — a type parameter shadows a
/// same-named declaration; the collision is diagnosed at the declaration).
pub fn resolve_type_ref_in(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    vars: &HashSet<String>,
) -> Option<Ty> {
    match r {
        TypeRef::Named(id) if vars.contains(&id.name) => Some(Ty::Var(id.name.clone())),
        TypeRef::Result(t, e, _) => Some(Ty::Result(
            Box::new(resolve_type_ref_in(t, types, vars)?),
            Box::new(resolve_type_ref_in(e, types, vars)?),
        )),
        TypeRef::Option(t, _) => Some(Ty::Option(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::Effect(t, _) => Some(Ty::Effect(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::HttpResult(t, _) => Some(Ty::HttpResult(Box::new(resolve_type_ref_in(
            t, types, vars,
        )?))),
        TypeRef::List(t, _) => Some(Ty::List(Box::new(resolve_type_ref_in(t, types, vars)?))),
        TypeRef::Map(k, v, _) => Some(Ty::Map(
            Box::new(resolve_type_ref_in(k, types, vars)?),
            Box::new(resolve_type_ref_in(v, types, vars)?),
        )),
        TypeRef::Fn(params, ret, _) => {
            let params: Option<Vec<Ty>> = params
                .iter()
                .map(|p| resolve_type_ref_in(p, types, vars))
                .collect();
            Some(Ty::Fn {
                params: params?,
                ret: Box::new(resolve_type_ref_in(ret, types, vars)?),
            })
        }
        _ => resolve_type_ref(r, types),
    }
}

/// v0.20a: substitute type variables in `t` per `subst`. Must be total when
/// instantiating a call (the uninferable check runs first); an unbound Var
/// passes through unchanged for partial substitution during inference.
fn substitute(t: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match t {
        Ty::Var(n) => subst.get(n).cloned().unwrap_or_else(|| t.clone()),
        Ty::Result(a, b) => Ty::Result(
            Box::new(substitute(a, subst)),
            Box::new(substitute(b, subst)),
        ),
        Ty::Option(a) => Ty::Option(Box::new(substitute(a, subst))),
        Ty::Effect(a) => Ty::Effect(Box::new(substitute(a, subst))),
        Ty::HttpResult(a) => Ty::HttpResult(Box::new(substitute(a, subst))),
        Ty::List(a) => Ty::List(Box::new(substitute(a, subst))),
        Ty::Map(k, v) => Ty::Map(
            Box::new(substitute(k, subst)),
            Box::new(substitute(v, subst)),
        ),
        Ty::Fn { params, ret } => Ty::Fn {
            params: params.iter().map(|p| substitute(p, subst)).collect(),
            ret: Box::new(substitute(ret, subst)),
        },
        _ => t.clone(),
    }
}

/// v0.20a: does `t` still contain a type variable?
fn contains_var(t: &Ty) -> bool {
    match t {
        Ty::Var(_) => true,
        Ty::Result(a, b) | Ty::Map(a, b) => contains_var(a) || contains_var(b),
        Ty::Option(a) | Ty::Effect(a) | Ty::HttpResult(a) | Ty::List(a) => contains_var(a),
        Ty::Fn { params, ret } => params.iter().any(contains_var) || contains_var(ret),
        _ => false,
    }
}

/// v0.20b: does `t` contain a type variable that is NOT one of the enclosing
/// function's rigid type parameters? Rigid vars are fully constrained inside
/// the body; only flexible (call-site instantiation) vars mean "still being
/// inferred".
fn contains_flexible_var(t: &Ty, rigid: &HashSet<String>) -> bool {
    match t {
        Ty::Var(n) => !rigid.contains(n),
        Ty::Result(a, b) | Ty::Map(a, b) => {
            contains_flexible_var(a, rigid) || contains_flexible_var(b, rigid)
        }
        Ty::Option(a) | Ty::Effect(a) | Ty::HttpResult(a) | Ty::List(a) => {
            contains_flexible_var(a, rigid)
        }
        Ty::Fn { params, ret } => {
            params.iter().any(|p| contains_flexible_var(p, rigid))
                || contains_flexible_var(ret, rigid)
        }
        _ => false,
    }
}

/// v0.20a: argument-directed unification. Walks `pattern` (possibly
/// Var-bearing) against the ground `actual`; a Var binds on first sight and
/// must match its prior binding **exactly** afterwards (keep inference dumb
/// and predictable — the explicit `name[T](…)` form is the pressure valve).
/// Returns false on a conflict; structural mismatches are NOT reported here —
/// the post-substitution `compatible` check owns those diagnostics.
fn unify(pattern: &Ty, actual: &Ty, subst: &mut HashMap<String, Ty>) -> bool {
    match (pattern, actual) {
        (Ty::Var(n), a) => match subst.get(n) {
            Some(bound) => bound == a,
            None => {
                subst.insert(n.clone(), a.clone());
                true
            }
        },
        (Ty::Result(a1, b1), Ty::Result(a2, b2)) | (Ty::Map(a1, b1), Ty::Map(a2, b2)) => {
            unify(a1, a2, subst) && unify(b1, b2, subst)
        }
        (Ty::Option(a1), Ty::Option(a2))
        | (Ty::Effect(a1), Ty::Effect(a2))
        | (Ty::HttpResult(a1), Ty::HttpResult(a2))
        | (Ty::List(a1), Ty::List(a2)) => unify(a1, a2, subst),
        (
            Ty::Fn {
                params: p1,
                ret: r1,
            },
            Ty::Fn {
                params: p2,
                ret: r2,
            },
        ) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(a, b)| unify(a, b, subst))
                && unify(r1, r2, subst)
        }
        // Ground-vs-ground: any pair is fine here; `compatible` owns the
        // real check after substitution.
        _ => true,
    }
}

/// v0.25 (ADR 0053): record a binding edge for every `Named` reference
/// inside a type-ref that resolved. Called alongside the `resolve_type_ref*`
/// annotation sites; `skip` holds the enclosing fn's type parameters (rigid
/// vars are not type symbols). Handler signatures and body annotations never
/// pass through the resolver's reference walk, so these sites are their only
/// recording point; where both passes run, assembly dedupes.
pub(crate) fn record_type_refs(
    r: &TypeRef,
    types: &HashMap<String, TypeDecl>,
    skip: &HashSet<String>,
    refs: &mut RefSink,
) {
    match r {
        TypeRef::Named(id) => {
            if types.contains_key(&id.name) && !skip.contains(&id.name) {
                refs.record(id.span, SymbolKind::Type, &id.name);
            }
        }
        TypeRef::Fn(params, ret, _) => {
            for p in params {
                record_type_refs(p, types, skip, refs);
            }
            record_type_refs(ret, types, skip, refs);
        }
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            record_type_refs(a, types, skip, refs);
            record_type_refs(b, types, skip, refs);
        }
        TypeRef::Option(t, _)
        | TypeRef::Effect(t, _)
        | TypeRef::HttpResult(t, _)
        | TypeRef::List(t, _) => record_type_refs(t, types, skip, refs),
        TypeRef::Base(..)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
    }
}

pub fn resolve_type_ref(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    match r {
        TypeRef::Base(b, _) => Some(Ty::Base(*b)),
        TypeRef::Named(id) => type_from_decl(id, types),
        // v0.20a: a function type. Effectfulness is structural (ret is
        // Effect[_]); nothing extra to record.
        TypeRef::Fn(params, ret, _) => {
            let params: Option<Vec<Ty>> =
                params.iter().map(|p| resolve_type_ref(p, types)).collect();
            Some(Ty::Fn {
                params: params?,
                ret: Box::new(resolve_type_ref(ret, types)?),
            })
        }
        TypeRef::Result(t, e, _) => {
            let t = resolve_type_ref(t, types)?;
            let e = resolve_type_ref(e, types)?;
            Some(Ty::Result(Box::new(t), Box::new(e)))
        }
        TypeRef::Option(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::Option(Box::new(t)))
        }
        TypeRef::Effect(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::Effect(Box::new(t)))
        }
        TypeRef::HttpResult(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::HttpResult(Box::new(t)))
        }
        TypeRef::List(t, _) => {
            let t = resolve_type_ref(t, types)?;
            Some(Ty::List(Box::new(t)))
        }
        TypeRef::Map(k, v, _) => {
            let k = resolve_type_ref(k, types)?;
            let v = resolve_type_ref(v, types)?;
            Some(Ty::Map(Box::new(k), Box::new(v)))
        }
        TypeRef::ValidationError(_) => Some(Ty::ValidationError),
        TypeRef::JsonError(_) => Some(Ty::JsonError),
        TypeRef::Unit(_) => Some(Ty::Unit),
    }
}

/// `t` is usable where `u` is expected.
pub fn compatible(t: &Ty, u: &Ty) -> bool {
    match (t, u) {
        (Ty::Base(a), Ty::Base(b)) => a == b,
        (Ty::Named { name: a, kind: ka }, Ty::Named { name: b, kind: kb }) => a == b && ka == kb,
        // Refined → base (widening).
        (
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            },
            Ty::Base(target),
        ) => b == target,
        (Ty::Base(_), Ty::Named { .. }) => false,
        (Ty::Result(t1, e1), Ty::Result(t2, e2)) => compatible(t1, t2) && compatible(e1, e2),
        (Ty::Option(a), Ty::Option(b)) => compatible(a, b),
        (Ty::Effect(a), Ty::Effect(b)) => compatible(a, b),
        (Ty::HttpResult(a), Ty::HttpResult(b)) => compatible(a, b),
        // v0.20b: collections are covariant in their element/value types;
        // Map keys must match exactly — key-position widening would split a
        // map's keys across refined/base identities at lookup time.
        (Ty::List(a), Ty::List(b)) => compatible(a, b),
        (Ty::Map(k1, v1), Ty::Map(k2, v2)) => k1 == k2 && compatible(v1, v2),
        (Ty::ValidationError, Ty::ValidationError) => true,
        (Ty::JsonError, Ty::JsonError) => true,
        (Ty::Unit, Ty::Unit) => true,
        // v0.20a: function types — **contravariant** in parameters, covariant
        // in the return type. `compatible(t, u)` is "t usable where u is
        // expected" and is already asymmetric (refined → base widening), so
        // the per-position argument order flips for params: a function
        // expecting the *wider* param type is usable where one expecting the
        // narrower is required — and crucially, the covariant direction would
        // let unvalidated base values flow into a refined-typed body.
        (Ty::Fn { params: p, ret: r }, Ty::Fn { params: q, ret: s }) => {
            p.len() == q.len() && p.iter().zip(q).all(|(a, b)| compatible(b, a)) && compatible(r, s)
        }
        // v0.20a: rigid type variables (a generic fn's own body) match by
        // name. Flexible vars never reach `compatible` — they are eliminated
        // by substitution during call-site instantiation.
        (Ty::Var(a), Ty::Var(b)) => a == b,
        _ => false,
    }
}

pub fn type_of_block(block: &Block, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    ctx.push_scope();
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) => {
                let annot_ty = l.type_annot.as_ref().and_then(|a| {
                    // v0.20b: the enclosing fn's type parameters are legal
                    // in body annotations (`let init: List[B] = …`).
                    let r = resolve_type_ref_in(a, &ctx.input.types, &ctx.type_vars);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
                    } else {
                        record_type_refs(a, &ctx.input.types, &ctx.type_vars, ctx.refs);
                    }
                    r
                });
                let rhs_ty = type_of(&l.value, annot_ty.as_ref(), ctx);
                let final_ty = match (annot_ty, rhs_ty) {
                    (Some(annot), Some(rhs)) => {
                        if !compatible(&rhs, &annot) {
                            ctx.errors.push(
                                CompileError::new(
                                    "karn.types.let_annotation_mismatch",
                                    l.value.span,
                                    format!(
                                        "let binding's value has type `{}`, but the annotation declares `{}`",
                                        rhs.display(),
                                        annot.display()
                                    ),
                                )
                                .with_label(
                                    l.type_annot.as_ref().unwrap().span(),
                                    "declared type annotation",
                                ),
                            );
                        }
                        annot
                    }
                    (Some(annot), None) => annot,
                    (None, Some(rhs)) => rhs,
                    (None, None) => continue,
                };
                if l.name.name != "_" {
                    // v0.27 (ADR 0056): an annotation-absent binding gets an
                    // inferred-type inlay hint at the binding name.
                    if l.type_annot.is_none() {
                        ctx.hints
                            .record(l.name.span, format!(": {}", final_ty.display()));
                    }
                    // v0.31: in scope from after this statement to block end.
                    ctx.locals.record(
                        l.name.name.clone(),
                        l.name.span,
                        final_ty.display(),
                        Span {
                            start: l.span.end,
                            end: block.span.end,
                        },
                    );
                    ctx.bind(l.name.name.clone(), final_ty);
                }
            }
            Statement::EffectLet(l) => {
                if !ctx.effectful {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.effect.bind_in_pure_context",
                            l.span,
                            "the `<-` operator can only be used inside an effectful body (one returning `Effect[T]`)",
                        )
                        .with_label(
                            ctx.return_ty_span,
                            format!("enclosing return type is `{}`", ctx.return_ty.display()),
                        )
                        .with_note(
                            "change the enclosing function/handler's return type to `Effect[...]`, or use `let ... =` for a pure binding",
                        ),
                    );
                }
                // Determine the inner Effect[T] payload type for the binding.
                let annot_ty = l.type_annot.as_ref().and_then(|a| {
                    // v0.20b: the enclosing fn's type parameters are legal
                    // in body annotations (`let init: List[B] = …`).
                    let r = resolve_type_ref_in(a, &ctx.input.types, &ctx.type_vars);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
                    } else {
                        record_type_refs(a, &ctx.input.types, &ctx.type_vars, ctx.refs);
                    }
                    r
                });
                // The expected type for the RHS is `Effect[annot]` if annot present.
                let rhs_expected = annot_ty.as_ref().map(|t| Ty::Effect(Box::new(t.clone())));
                let rhs_ty = type_of(&l.value, rhs_expected.as_ref(), ctx);
                let inner_ty = match rhs_ty {
                    Some(Ty::Effect(t)) => Some((*t).clone()),
                    Some(other) => {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.effect.bind_on_non_effect",
                                l.value.span,
                                format!(
                                    "the `<-` operator requires an `Effect[T]` value, but got `{}`",
                                    other.display()
                                ),
                            )
                            .with_note(
                                "use `let ... =` for a pure binding, or wrap the value with `Effect.pure(...)`",
                            ),
                        );
                        None
                    }
                    None => None,
                };
                let final_ty = match (annot_ty, inner_ty) {
                    (Some(annot), Some(rhs)) => {
                        if !compatible(&rhs, &annot) {
                            ctx.errors.push(CompileError::new(
                                "karn.types.let_annotation_mismatch",
                                l.value.span,
                                format!(
                                    "let-binding's value has type `Effect[{}]`, but the annotation declares `Effect[{}]`",
                                    rhs.display(),
                                    annot.display()
                                ),
                            ));
                        }
                        annot
                    }
                    (Some(annot), None) => annot,
                    (None, Some(rhs)) => rhs,
                    (None, None) => continue,
                };
                if l.name.name != "_" {
                    // v0.27 (ADR 0056): as for `let =`, but `final_ty` here
                    // is the peeled `Effect[T]` payload — the binding's
                    // actual type, which is what the hint must show.
                    if l.type_annot.is_none() {
                        ctx.hints
                            .record(l.name.span, format!(": {}", final_ty.display()));
                    }
                    ctx.locals.record(
                        l.name.name.clone(),
                        l.name.span,
                        final_ty.display(),
                        Span {
                            start: l.span.end,
                            end: block.span.end,
                        },
                    );
                    ctx.bind(l.name.name.clone(), final_ty);
                }
            }
            Statement::Commit(c) => {
                let Some(state_ty) = ctx.agent_state_ty.clone() else {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.commit.outside_agent",
                            c.span,
                            "`commit` is only valid inside an agent handler",
                        )
                        .with_note(
                            "agent handlers update persistent state via `commit newState`; \
                             services and free functions do not have state",
                        ),
                    );
                    let _ = type_of(&c.value, None, ctx);
                    continue;
                };
                if ctx.commit_seen {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.commit.two_reachable_commits",
                            c.span,
                            "two `commit` statements are reachable on the same execution path",
                        )
                        .with_note(
                            "an agent handler may commit at most once per invocation; use branches to commit different values conditionally",
                        ),
                    );
                }
                let val_ty = type_of(&c.value, Some(&state_ty), ctx);
                if let Some(actual) = val_ty
                    && !compatible(&actual, &state_ty)
                {
                    ctx.errors.push(CompileError::new(
                        "karn.commit.wrong_state_type",
                        c.value.span,
                        format!(
                            "`commit` expression has type `{}`, but the agent's state type is `{}`",
                            actual.display(),
                            state_ty.display()
                        ),
                    ));
                }
                ctx.commit_seen = true;
            }
            Statement::Assert(a) => {
                if !ctx.in_test_body {
                    ctx.errors.push(
                        CompileError::new(
                            "karn.assert.outside_test",
                            a.span,
                            "`assert` is only valid inside a test case body",
                        )
                        .with_note(
                            "assertion statements verify conditions at test runtime; use them only inside `test \"...\" { ... }` blocks",
                        ),
                    );
                }
                let val_ty = type_of(&a.value, Some(&Ty::Base(BaseType::Bool)), ctx);
                if let Some(actual) = val_ty
                    && !compatible(&actual, &Ty::Base(BaseType::Bool))
                {
                    ctx.errors.push(CompileError::new(
                        "karn.assert.non_bool",
                        a.value.span,
                        format!(
                            "`assert` expression has type `{}`, but a `Bool` is required",
                            actual.display(),
                        ),
                    ));
                }
            }
        }
    }
    let ty = type_of(&block.tail, expected, ctx);
    let ty = maybe_auto_lift(ty, expected);
    if let Some(ty) = &ty {
        ctx.expr_types.insert(block.span, ty.clone());
    }
    ctx.pop_scope();
    ty
}

/// v0.7.1 tail-position auto-lift. If the expected type is `Effect[T]` and
/// the computed type is `T` (not itself an `Effect[_]`), lift it to
/// `Effect[T]`. Otherwise leave the type alone — the surrounding compatibility
/// check will report any genuine mismatch.
fn maybe_auto_lift(ty: Option<Ty>, expected: Option<&Ty>) -> Option<Ty> {
    if let Some(actual) = &ty
        && let Some(Ty::Effect(et)) = expected
        && !actual.is_effect()
        && compatible(actual, et)
    {
        return Some(Ty::Effect(Box::new(actual.clone())));
    }
    ty
}

pub fn type_of(expr: &Expr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let ty = match &expr.kind {
        // v0.9.4: a literal in a refined-expected position takes the refined
        // type (validated now); otherwise it keeps its base type.
        // v0.20a: a lambda. With an expected function type, params type
        // contextually and the body checks against the expected return; in an
        // unconstrained position, every param must be annotated and
        // effectfulness is inferred bottom-up by a syntactic pre-scan.
        ExprKind::Lambda(lambda) => check_lambda(lambda, expected, ctx),
        ExprKind::IntLit(_) => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::Int)))
        }
        ExprKind::FloatLit { .. } => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::Float)))
        }
        ExprKind::StrLit(_) => {
            admit_refined_literal(expr, expected, ctx).or(Some(Ty::Base(BaseType::String)))
        }
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
        // v0.20b: a list literal. Elements check against the expected
        // element type when one is supplied (so refined literals admit,
        // v0.9.4); an empty `[]` has no inferable element type without one.
        ExprKind::ListLit(elems) => {
            let expected_elem = expected.and_then(peel_to_list);
            if elems.is_empty() {
                match expected_elem {
                    Some(t) => Some(Ty::List(Box::new(t))),
                    None => {
                        ctx.errors.push(
                            CompileError::new(
                                "karn.types.uninferable_element_type",
                                expr.span,
                                "an empty `[]` has no inferable element type",
                            )
                            .with_note(
                                "annotate the binding (`let xs: List[T] = []`) or use the empty list where a `List[T]` is expected",
                            ),
                        );
                        None
                    }
                }
            } else {
                let mut elem_ty: Option<Ty> = expected_elem;
                for e in elems {
                    let Some(t) = type_of(e, elem_ty.as_ref(), ctx) else {
                        continue;
                    };
                    match &elem_ty {
                        Some(et) => {
                            if !compatible(&t, et) {
                                ctx.errors.push(CompileError::new(
                                    "karn.types.list_element_mismatch",
                                    e.span,
                                    format!(
                                        "list element has type `{}`, but the list's element type is `{}`",
                                        t.display(),
                                        et.display()
                                    ),
                                ));
                            }
                        }
                        None => elem_ty = Some(t),
                    }
                }
                elem_ty.map(|t| Ty::List(Box::new(t)))
            }
        }
        ExprKind::Ident(id) => {
            // v0.9: a bare ident may name an HttpResult variant. Resolve to
            // HttpResult only when (a) the surrounding type implies it, or
            // (b) no user sum-type variant of the same name exists. This
            // keeps `NotFound` resolving to a user `StockError` variant
            // when the caller expects a domain Result.
            if ctx.lookup(id.name.as_str()).is_none()
                && let Some(v) = http_variant(&id.name)
            {
                let user_owns = ctx.input.types.values().any(|t| {
                    matches!(&t.body, TypeBody::Sum(s)
                        if s.variants.iter().any(|var| var.name.name == id.name))
                });
                let http_implied = expected
                    .map(|t| peel_to_http_result(t).is_some())
                    .unwrap_or(false)
                    || peel_to_http_result(&ctx.return_ty).is_some();
                if http_implied || !user_owns {
                    check_http_variant(id.span, v, &[], expected, ctx)
                } else {
                    check_ident(id, expected, ctx)
                }
            } else {
                check_ident(id, expected, ctx)
            }
        }
        ExprKind::Paren(inner) => type_of(inner, expected, ctx),
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            // v0.9: HttpResult variant call. Prefer HttpResult when the
            // surrounding type implies it; otherwise defer to fn/user-variant
            // resolution and only fall back to HttpResult when nothing else
            // owns the name.
            let user_owners: usize = ctx
                .input
                .types
                .values()
                .filter(|t| {
                    matches!(&t.body, TypeBody::Sum(s)
                        if s.variants.iter().any(|v| v.name.name == name.name))
                })
                .count();
            let http_implied = expected
                .map(|t| peel_to_http_result(t).is_some())
                .unwrap_or(false)
                || peel_to_http_result(&ctx.return_ty).is_some();
            let unowned = !ctx.input.fns.contains_key(&name.name) && user_owners == 0;
            if let Some(v) = http_variant(&name.name)
                && (http_implied || unowned)
            {
                check_http_variant(expr.span, v, args, expected, ctx)
            } else {
                check_call(name, type_args, args, expr.span, ctx)
            }
        }
        ExprKind::UnaryOp(op, inner) => check_unary(*op, inner, expr.span, ctx),
        ExprKind::BinOp(op, lhs, rhs) => check_binop(*op, lhs, rhs, ctx),
        ExprKind::Block(b) => type_of_block(b, expected, ctx),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => check_if(cond, then_block, else_block, expr.span, expected, ctx),
        ExprKind::Ok(inner) => check_ok(inner, expr.span, expected, ctx),
        ExprKind::Err(inner) => check_err(inner, expr.span, expected, ctx),
        ExprKind::Some(inner) => check_some(inner, expr.span, expected, ctx),
        ExprKind::None => check_none(expr.span, expected, ctx),
        ExprKind::Question(inner) => check_question(inner, expr.span, ctx),
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            if type_name.name == HTTP_RESULT {
                if let Some(v) = http_variant(&method.name) {
                    check_http_variant(expr.span, v, args, expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                    None
                }
            } else {
                check_static_call(type_name, method, args, expr.span, ctx)
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            check_record_construction(type_name, fields, expr.span, ctx)
        }
        ExprKind::FieldAccess { receiver, field } => {
            // v0.9: `HttpResult.Variant` qualified nullary variant access.
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.lookup(id.name.as_str()).is_none()
                && id.name == HTTP_RESULT
            {
                if let Some(v) = http_variant(&field.name) {
                    if !matches!(v.payload, HttpVariantPayload::None) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.variant_missing_payload",
                            field.span,
                            format!(
                                "`HttpResult.{}` has a payload — call it with an argument",
                                v.name
                            ),
                        ));
                        return None;
                    }
                    check_http_variant(field.span, v, &[], expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        field.span,
                        format!("`HttpResult` has no variant named `{}`", field.name),
                    ));
                    None
                }
            } else {
                check_field_access(receiver, field, ctx)
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            type_args,
            args,
        } => {
            // v0.9: `HttpResult.Variant(args)` — explicit HttpResult construction.
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.lookup(id.name.as_str()).is_none()
                && id.name == HTTP_RESULT
            {
                if let Some(v) = http_variant(&method.name) {
                    check_http_variant(expr.span, v, args, expected, ctx)
                } else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_static_member",
                        method.span,
                        format!("`HttpResult` has no variant named `{}`", method.name),
                    ));
                    None
                }
            } else {
                check_method_call(receiver, method, type_args, args, expr.span, expected, ctx)
            }
        }
        ExprKind::Match { discriminant, arms } => {
            check_match(discriminant, arms, expr.span, expected, ctx)
        }
        ExprKind::Is { value, pattern } => check_is(value, pattern, expr.span, ctx),
        ExprKind::UnitLit => Some(Ty::Unit),
        ExprKind::EffectPure(inner) => {
            let expected_inner = match expected {
                Some(Ty::Effect(t)) => Some((**t).clone()),
                _ => None,
            };
            let inner_ty = type_of(inner, expected_inner.as_ref(), ctx)?;
            Some(Ty::Effect(Box::new(inner_ty)))
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => check_record_spread(
            type_name.as_ref(),
            base,
            overrides,
            expr.span,
            expected,
            ctx,
        ),
        ExprKind::Assert(inner) => check_assert(inner, expr.span, ctx),
        ExprKind::Mock { type_ref, args } => check_mock(type_ref, args, expr.span, ctx),
    };
    if let Some(ty) = &ty {
        ctx.expr_types.insert(expr.span, ty.clone());
    }
    ty
}

// ==== Peel helpers (unwrap Effect / Result / Option / List / Map) ====

/// Peel one optional `Effect[_]` wrapper to expose an underlying `HttpResult[T]`.
fn peel_to_http_result(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::HttpResult(inner) => Some((**inner).clone()),
        Ty::Effect(inner) => peel_to_http_result(inner),
        _ => None,
    }
}

fn surrounding_result(expected: Option<&Ty>, return_ty: &Ty) -> Option<(Ty, Ty)> {
    if let Some(t) = expected
        && let Some(pair) = peel_to_result(t)
    {
        return Some(pair);
    }
    peel_to_result(return_ty)
}

/// Peel one optional `Effect[_]` wrapper to expose an underlying `Result[T, E]`.
/// Used by `Ok` / `Err` checking in v0.7.1 so that bare constructors in
/// `Effect[Result[T, E]]` tail positions can pick up the surrounding type's
/// parameters via the auto-lift propagation.
fn peel_to_result(ty: &Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::Result(t, e) => Some(((**t).clone(), (**e).clone())),
        Ty::Effect(inner) => peel_to_result(inner),
        _ => None,
    }
}

/// Companion to `peel_to_result` for `Option[T]`.
fn peel_to_option(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Option(t) => Some((**t).clone()),
        Ty::Effect(inner) => peel_to_option(inner),
        _ => None,
    }
}

/// Companion to `peel_to_result` for `List[T]` (v0.20b) — the expected
/// element type of a list literal, looking through `Effect[_]` so tail
/// auto-lift positions still propagate it.
fn peel_to_list(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::List(t) => Some((**t).clone()),
        Ty::Effect(inner) => peel_to_list(inner),
        _ => None,
    }
}

/// Companion to `peel_to_list` for `Map[K, V]` (v0.20b).
fn peel_to_map(ty: &Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::Map(k, v) => Some(((**k).clone(), (**v).clone())),
        Ty::Effect(inner) => peel_to_map(inner),
        _ => None,
    }
}

// ==== Structural compatibility and variant introspection ====

/// A flattened view of a type's variants (name + payload types).
struct VariantInfo {
    name: String,
    payload: Vec<(String, Ty)>,
}

/// Project a return type produced in the consumed context's namespace into
/// the caller's namespace by re-resolving named types that exist on both
/// sides. The structural shape stays the same; the brand changes.
fn rebrand_return_type(t: &Ty, caller_types: &HashMap<String, TypeDecl>) -> Ty {
    match t {
        Ty::Named { name, kind } => {
            // If the caller's namespace has the same name, prefer the caller's
            // view (it carries the caller's brand at emission time). Otherwise
            // keep the consumed-context name; the caller can hold it opaquely.
            if let Some(decl) = caller_types.get(name) {
                named_ty(decl)
            } else {
                Ty::Named {
                    name: name.clone(),
                    kind: kind.clone(),
                }
            }
        }
        Ty::Result(t, e) => Ty::Result(
            Box::new(rebrand_return_type(t, caller_types)),
            Box::new(rebrand_return_type(e, caller_types)),
        ),
        Ty::Option(t) => Ty::Option(Box::new(rebrand_return_type(t, caller_types))),
        Ty::Effect(t) => Ty::Effect(Box::new(rebrand_return_type(t, caller_types))),
        Ty::HttpResult(t) => Ty::HttpResult(Box::new(rebrand_return_type(t, caller_types))),
        Ty::List(t) => Ty::List(Box::new(rebrand_return_type(t, caller_types))),
        Ty::Map(k, v) => Ty::Map(
            Box::new(rebrand_return_type(k, caller_types)),
            Box::new(rebrand_return_type(v, caller_types)),
        ),
        Ty::Base(_) | Ty::ValidationError | Ty::JsonError | Ty::Unit => t.clone(),
        // v0.20a: function types are confined to non-boundary positions
        // (`karn.types.function_at_boundary`), so a cross-context return can
        // never carry one; Vars never escape call checking.
        Ty::Fn { .. } | Ty::Var(_) => t.clone(),
    }
}

/// Structural compatibility check for values crossing a context boundary
/// (v0.6 §4.3). The two types may be expressed in different namespaces
/// (caller-side / callee-side type tables), so we walk them in parallel
/// against their respective tables.
fn structurally_compatible(
    arg: &Ty,
    param: &Ty,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
) -> bool {
    structurally_compatible_inner(arg, param, arg_types, param_types, &mut HashSet::new())
}

fn structurally_compatible_inner(
    arg: &Ty,
    param: &Ty,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    match (arg, param) {
        (Ty::Base(a), Ty::Base(b)) => a == b,
        (Ty::ValidationError, Ty::ValidationError) => true,
        (Ty::JsonError, Ty::JsonError) => true,
        (Ty::Unit, Ty::Unit) => true,
        (Ty::Result(t1, e1), Ty::Result(t2, e2)) => {
            structurally_compatible_inner(t1, t2, arg_types, param_types, visited)
                && structurally_compatible_inner(e1, e2, arg_types, param_types, visited)
        }
        (Ty::Option(a), Ty::Option(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::Effect(a), Ty::Effect(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::HttpResult(a), Ty::HttpResult(b)) => {
            structurally_compatible_inner(a, b, arg_types, param_types, visited)
        }
        (Ty::Named { name: an, .. }, Ty::Named { name: bn, .. }) => {
            // Cycle break: once we've started comparing (an, bn) we trust
            // the recursive case to succeed.
            let key = (an.clone(), bn.clone());
            if !visited.insert(key.clone()) {
                return true;
            }
            let ok = structural_compare_named(an, bn, arg_types, param_types, visited);
            visited.remove(&key);
            ok
        }
        // Refined-named widens to its base; tolerate one-sided widening only
        // when comparing within the same nominal name (handled above) or when
        // the param accepts a plain base.
        (
            Ty::Named {
                kind: NamedKind::Refined(b),
                ..
            },
            Ty::Base(target),
        ) => b == target,
        _ => false,
    }
}

fn structural_compare_named(
    arg_name: &str,
    param_name: &str,
    arg_types: &HashMap<String, TypeDecl>,
    param_types: &HashMap<String, TypeDecl>,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    // The "same nominal name" case is the most common: both sides derive
    // the same commons type. Compare their structural shapes.
    let Some(arg_decl) = arg_types.get(arg_name) else {
        return false;
    };
    let Some(param_decl) = param_types.get(param_name) else {
        return false;
    };
    match (&arg_decl.body, &param_decl.body) {
        (
            TypeBody::Refined {
                base: ab,
                refinement: ar,
                ..
            },
            TypeBody::Refined {
                base: bb,
                refinement: br,
                ..
            },
        ) => {
            if ab != bb {
                return false;
            }
            refinements_match(ar.as_ref(), br.as_ref())
        }
        (
            TypeBody::Opaque {
                base: ab,
                refinement: ar,
                ..
            },
            TypeBody::Opaque {
                base: bb,
                refinement: br,
                ..
            },
        ) => {
            // Opaque types must share a name to be compatible (a context's
            // opaque cannot be reinterpreted as a different context's opaque).
            if arg_name != param_name {
                return false;
            }
            if ab != bb {
                return false;
            }
            refinements_match(ar.as_ref(), br.as_ref())
        }
        (TypeBody::Record(a), TypeBody::Record(b)) => {
            if a.fields.len() != b.fields.len() {
                return false;
            }
            for af in &a.fields {
                let Some(bf) = b.fields.iter().find(|f| f.name.name == af.name.name) else {
                    return false;
                };
                let at = resolve_type_ref(&af.type_ref, arg_types);
                let bt = resolve_type_ref(&bf.type_ref, param_types);
                let (Some(at), Some(bt)) = (at, bt) else {
                    return false;
                };
                if !structurally_compatible_inner(&at, &bt, arg_types, param_types, visited) {
                    return false;
                }
            }
            true
        }
        (TypeBody::Sum(a), TypeBody::Sum(b)) => {
            if a.variants.len() != b.variants.len() {
                return false;
            }
            for av in &a.variants {
                let Some(bv) = b.variants.iter().find(|v| v.name.name == av.name.name) else {
                    return false;
                };
                if av.payload.len() != bv.payload.len() {
                    return false;
                }
                for (af, bf) in av.payload.iter().zip(bv.payload.iter()) {
                    if af.name.name != bf.name.name {
                        return false;
                    }
                    let at = resolve_type_ref(&af.type_ref, arg_types);
                    let bt = resolve_type_ref(&bf.type_ref, param_types);
                    let (Some(at), Some(bt)) = (at, bt) else {
                        return false;
                    };
                    if !structurally_compatible_inner(&at, &bt, arg_types, param_types, visited) {
                        return false;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

fn refinements_match(a: Option<&Refinement>, b: Option<&Refinement>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(_), None) => true, // sending side is more restrictive — receiving is more permissive
        (None, Some(_)) => false,
        (Some(a), Some(b)) => {
            if a.predicates.len() != b.predicates.len() {
                return false;
            }
            // Exact match required (per spec §4.3 conservative rule).
            // Predicate order matters here; refinements are conventionally
            // written in a fixed order.
            for (pa, pb) in a.predicates.iter().zip(b.predicates.iter()) {
                if !predicate_eq(&pa.kind, &pb.kind) {
                    return false;
                }
            }
            true
        }
    }
}

fn predicate_eq(a: &PredKind, b: &PredKind) -> bool {
    match (a, b) {
        (PredKind::Matches(x), PredKind::Matches(y)) => x == y,
        (PredKind::InRange(a1, a2), PredKind::InRange(b1, b2)) => {
            a1.value == b1.value && a2.value == b2.value
        }
        (PredKind::InRangeF(a1, a2), PredKind::InRangeF(b1, b2)) => {
            a1.value == b1.value && a2.value == b2.value
        }
        (PredKind::MinLength(a), PredKind::MinLength(b)) => a == b,
        (PredKind::MaxLength(a), PredKind::MaxLength(b)) => a == b,
        (PredKind::Length(a), PredKind::Length(b)) => a == b,
        (PredKind::NonNegative, PredKind::NonNegative) => true,
        (PredKind::Positive, PredKind::Positive) => true,
        (PredKind::NonEmpty, PredKind::NonEmpty) => true,
        _ => false,
    }
}

fn variants_of(ty: &Ty, types: &HashMap<String, TypeDecl>) -> Option<Vec<VariantInfo>> {
    match ty {
        Ty::Named {
            kind: NamedKind::Sum,
            name,
        } => {
            let decl = types.get(name)?;
            if let TypeBody::Sum(s) = &decl.body {
                Some(
                    s.variants
                        .iter()
                        .map(|v| VariantInfo {
                            name: v.name.name.clone(),
                            payload: v
                                .payload
                                .iter()
                                .map(|f| {
                                    let t = resolve_type_ref(&f.type_ref, types)
                                        .unwrap_or(Ty::Base(BaseType::Int));
                                    (f.name.name.clone(), t)
                                })
                                .collect(),
                        })
                        .collect(),
                )
            } else {
                None
            }
        }
        Ty::Result(t, e) => Some(vec![
            VariantInfo {
                name: "Ok".to_string(),
                payload: vec![("value".to_string(), (**t).clone())],
            },
            VariantInfo {
                name: "Err".to_string(),
                payload: vec![("error".to_string(), (**e).clone())],
            },
        ]),
        Ty::Option(t) => Some(vec![
            VariantInfo {
                name: "Some".to_string(),
                payload: vec![("value".to_string(), (**t).clone())],
            },
            VariantInfo {
                name: "None".to_string(),
                payload: vec![],
            },
        ]),
        Ty::HttpResult(t) => Some(
            HTTP_VARIANTS
                .iter()
                .map(|v| VariantInfo {
                    name: v.name.to_string(),
                    payload: match v.payload {
                        HttpVariantPayload::None => vec![],
                        HttpVariantPayload::Value => vec![("value".to_string(), (**t).clone())],
                        HttpVariantPayload::Message => {
                            vec![("message".to_string(), Ty::Base(BaseType::String))]
                        }
                    },
                })
                .collect(),
        ),
        _ => None,
    }
}

// ── v0.9.2: agent state-field zeroability ──────────────────────────────────
//
// Fresh agent state is the zero-value record (finding #10): a never-seen key
// reads `0` / `false` / `""` / `None` rather than `undefined`. A type is
// *zeroable* when it has a defined zero; agent state fields must be zeroable,
// since a fresh key has no committed value to load. Non-zeroable fields (a
// non-Option sum, an opaque type, or a refined type whose refinement excludes
// the underlying zero) are a compile error until explicit-initialiser syntax
// lands.

#[cfg(test)]
mod generics_tests {
    use super::*;

    fn var(n: &str) -> Ty {
        Ty::Var(n.to_string())
    }
    fn int() -> Ty {
        Ty::Base(BaseType::Int)
    }

    #[test]
    fn unify_binds_and_holds() {
        let mut s = HashMap::new();
        assert!(unify(&var("A"), &int(), &mut s));
        assert_eq!(s.get("A"), Some(&int()));
        // Same binding again: fine. A different one: conflict.
        assert!(unify(&var("A"), &int(), &mut s));
        assert!(!unify(&var("A"), &Ty::Base(BaseType::String), &mut s));
    }

    #[test]
    fn unify_walks_structure() {
        let mut s = HashMap::new();
        let pattern = Ty::Fn {
            params: vec![var("A")],
            ret: Box::new(Ty::Effect(Box::new(var("B")))),
        };
        let actual = Ty::Fn {
            params: vec![int()],
            ret: Box::new(Ty::Effect(Box::new(Ty::Base(BaseType::String)))),
        };
        assert!(unify(&pattern, &actual, &mut s));
        assert_eq!(s.get("A"), Some(&int()));
        assert_eq!(s.get("B"), Some(&Ty::Base(BaseType::String)));
    }

    #[test]
    fn substitute_grounds_fully() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), int());
        let t = Ty::Option(Box::new(Ty::Fn {
            params: vec![var("A")],
            ret: Box::new(var("A")),
        }));
        let g = substitute(&t, &s);
        assert!(!contains_var(&g));
    }

    /// The §2 invariant (pinned per the plan): every expected-driven feature
    /// in `type_of` matches *concrete* `Ty` variants, so a Var-bearing
    /// expected imposes no constraint — `compatible` must simply reject
    /// Var-vs-ground pairs rather than panic or accept.
    #[test]
    fn var_bearing_expected_is_benign() {
        assert!(!compatible(&int(), &var("A")));
        assert!(!compatible(&var("A"), &int()));
        // Rigid vars: name equality only.
        assert!(compatible(&var("A"), &var("A")));
        assert!(!compatible(&var("A"), &var("B")));
    }
}

/// Characterization pins for `checker.rs`'s pure free functions (v0.29.10
/// slice 0). These pin *current* behaviour ahead of the upcoming module split
/// so the verbatim moves are verifiable. Any surprising behaviour is pinned
/// as-is, flagged with a comment — these are not specifications.
#[cfg(test)]
mod pure_helper_pins {
    use super::*;
    use crate::ast::{FloatBound, RefinementPred};

    // -- small constructors ------------------------------------------------

    fn sp() -> Span {
        Span::new(0, 0)
    }
    fn ident(n: &str) -> Ident {
        Ident {
            name: n.to_string(),
            span: sp(),
        }
    }
    fn var(n: &str) -> Ty {
        Ty::Var(n.to_string())
    }
    fn int() -> Ty {
        Ty::Base(BaseType::Int)
    }
    fn string() -> Ty {
        Ty::Base(BaseType::String)
    }
    fn expr(kind: ExprKind) -> Expr {
        Expr { kind, span: sp() }
    }
    fn pred(kind: PredKind) -> RefinementPred {
        RefinementPred { kind, span: sp() }
    }
    fn refinement(preds: Vec<PredKind>) -> Refinement {
        Refinement {
            predicates: preds.into_iter().map(pred).collect(),
            span: sp(),
        }
    }
    fn fbound(value: f64) -> FloatBound {
        FloatBound {
            value,
            lexeme: value.to_string(),
            span: Span::new(0, 0),
        }
    }
    fn ibound(value: i64) -> IntBound {
        IntBound {
            value,
            span: Span::new(0, 0),
        }
    }
    /// An `InRange` predicate from two int values (test convenience).
    fn in_range(lo: i64, hi: i64) -> PredKind {
        PredKind::InRange(ibound(lo), ibound(hi))
    }
    fn refined_decl(name: &str, base: BaseType, refinement: Option<Refinement>) -> TypeDecl {
        TypeDecl {
            name: ident(name),
            body: TypeBody::Refined {
                base,
                base_span: sp(),
                refinement,
            },
            documentation: None,
            span: sp(),
            trivia: crate::ast::Trivia::default(),
        }
    }
    fn record_decl(name: &str) -> TypeDecl {
        TypeDecl {
            name: ident(name),
            body: TypeBody::Record(crate::ast::RecordBody {
                fields: vec![],
                span: sp(),
            }),
            documentation: None,
            span: sp(),
            trivia: crate::ast::Trivia::default(),
        }
    }

    // -- unify -------------------------------------------------------------

    #[test]
    fn unify_identical_concrete_types() {
        let mut s = HashMap::new();
        assert!(unify(&int(), &int(), &mut s));
        assert!(s.is_empty());
    }

    #[test]
    fn unify_var_binds_in_subst() {
        let mut s = HashMap::new();
        assert!(unify(&var("A"), &string(), &mut s));
        assert_eq!(s.get("A"), Some(&string()));
    }

    #[test]
    fn unify_nested_generic_binds() {
        // List[A] vs List[Int] binds A := Int.
        let mut s = HashMap::new();
        let pat = Ty::List(Box::new(var("A")));
        let act = Ty::List(Box::new(int()));
        assert!(unify(&pat, &act, &mut s));
        assert_eq!(s.get("A"), Some(&int()));
    }

    #[test]
    fn unify_surprise_concrete_mismatch_returns_true() {
        // SURPRISING (pinned as-is): `unify`'s catch-all is `_ => true`, so a
        // ground-vs-ground mismatch (Int vs String) and a constructor mismatch
        // (List vs Option) both *succeed* here — `compatible` owns those
        // diagnostics post-substitution, not `unify`.
        let mut s = HashMap::new();
        assert!(unify(&int(), &string(), &mut s));
        assert!(unify(
            &Ty::List(Box::new(int())),
            &Ty::Option(Box::new(int())),
            &mut s,
        ));
        // The only false paths: a Var rebind conflict and an Fn arity mismatch.
        let mut s2 = HashMap::new();
        assert!(unify(&var("A"), &int(), &mut s2));
        assert!(!unify(&var("A"), &string(), &mut s2));
        let mut s3 = HashMap::new();
        let f1 = Ty::Fn {
            params: vec![int()],
            ret: Box::new(int()),
        };
        let f2 = Ty::Fn {
            params: vec![int(), int()],
            ret: Box::new(int()),
        };
        assert!(!unify(&f1, &f2, &mut s3));
    }

    // -- substitute --------------------------------------------------------

    #[test]
    fn substitute_replaces_bound_var() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), int());
        assert_eq!(substitute(&var("A"), &s), int());
    }

    #[test]
    fn substitute_recurses_into_nested() {
        let mut s = HashMap::new();
        s.insert("A".to_string(), string());
        let t = Ty::Map(Box::new(var("A")), Box::new(int()));
        assert_eq!(
            substitute(&t, &s),
            Ty::Map(Box::new(string()), Box::new(int())),
        );
    }

    #[test]
    fn substitute_leaves_unbound_var_alone() {
        let s = HashMap::new();
        assert_eq!(substitute(&var("Z"), &s), var("Z"));
    }

    // -- contains_var / contains_flexible_var ------------------------------

    #[test]
    fn contains_var_positive_and_negative() {
        assert!(contains_var(&Ty::Option(Box::new(var("A")))));
        assert!(!contains_var(&Ty::Option(Box::new(int()))));
        assert!(!contains_var(&int()));
    }

    #[test]
    fn contains_flexible_var_respects_rigid_set() {
        let mut rigid = HashSet::new();
        rigid.insert("A".to_string());
        // A is rigid → not flexible.
        assert!(!contains_flexible_var(&var("A"), &rigid));
        // B is not rigid → flexible.
        assert!(contains_flexible_var(&var("B"), &rigid));
        // No vars at all → not flexible.
        assert!(!contains_flexible_var(&int(), &rigid));
    }

    // -- peel_to_* ---------------------------------------------------------

    #[test]
    fn peel_to_result_matches_and_misses() {
        let r = Ty::Result(Box::new(int()), Box::new(string()));
        assert_eq!(peel_to_result(&r), Some((int(), string())));
        assert_eq!(peel_to_result(&int()), None);
        // Pinned: peels through Effect[_].
        assert_eq!(
            peel_to_result(&Ty::Effect(Box::new(r))),
            Some((int(), string()))
        );
    }

    #[test]
    fn peel_to_option_matches_and_misses() {
        assert_eq!(peel_to_option(&Ty::Option(Box::new(int()))), Some(int()));
        assert_eq!(peel_to_option(&int()), None);
    }

    #[test]
    fn peel_to_list_matches_and_misses() {
        assert_eq!(peel_to_list(&Ty::List(Box::new(string()))), Some(string()));
        assert_eq!(peel_to_list(&int()), None);
    }

    #[test]
    fn peel_to_map_matches_and_misses() {
        let m = Ty::Map(Box::new(string()), Box::new(int()));
        assert_eq!(peel_to_map(&m), Some((string(), int())));
        assert_eq!(peel_to_map(&int()), None);
    }

    #[test]
    fn peel_to_http_result_matches_and_misses() {
        assert_eq!(
            peel_to_http_result(&Ty::HttpResult(Box::new(int()))),
            Some(int()),
        );
        assert_eq!(peel_to_http_result(&int()), None);
    }

    // -- maybe_auto_lift ---------------------------------------------------

    #[test]
    fn maybe_auto_lift_lifts_into_expected_effect() {
        // T lifts to Effect[T] when expected is Effect[T] and T is not effectful.
        let expected = Ty::Effect(Box::new(int()));
        let lifted = maybe_auto_lift(Some(int()), Some(&expected));
        assert_eq!(lifted, Some(Ty::Effect(Box::new(int()))));
    }

    #[test]
    fn maybe_auto_lift_leaves_non_matching_alone() {
        // Already Effect[_]: untouched.
        let expected = Ty::Effect(Box::new(int()));
        assert_eq!(
            maybe_auto_lift(Some(Ty::Effect(Box::new(int()))), Some(&expected)),
            Some(Ty::Effect(Box::new(int()))),
        );
        // Expected not an Effect: untouched.
        assert_eq!(maybe_auto_lift(Some(int()), Some(&int())), Some(int()));
        // None type: untouched.
        assert_eq!(maybe_auto_lift(None, Some(&expected)), None);
    }

    // -- const_literal -----------------------------------------------------

    #[test]
    fn const_literal_extracts_literals() {
        assert!(matches!(
            const_literal(&expr(ExprKind::IntLit(7))),
            Some(ConstLit::Int(7)),
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::BoolLit(true))),
            Some(ConstLit::Bool(true)),
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::StrLit("hi".into()))),
            Some(ConstLit::Str(s)) if s == "hi",
        ));
        assert!(matches!(
            const_literal(&expr(ExprKind::FloatLit {
                value: 1.5,
                lexeme: "1.5".into(),
            })),
            Some(ConstLit::Float(_)),
        ));
        // Unary-neg on an int literal folds.
        let neg = expr(ExprKind::UnaryOp(
            UnaryOp::Neg,
            Box::new(expr(ExprKind::IntLit(3))),
        ));
        assert!(matches!(const_literal(&neg), Some(ConstLit::Int(-3))));
    }

    #[test]
    fn const_literal_rejects_non_literals() {
        assert!(const_literal(&expr(ExprKind::Ident(ident("x")))).is_none());
    }

    // -- eval_predicate ----------------------------------------------------

    #[test]
    fn eval_predicate_int_and_float() {
        assert!(eval_predicate(&PredKind::NonNegative, &ConstLit::Int(0)));
        assert!(!eval_predicate(&PredKind::NonNegative, &ConstLit::Int(-1)));
        assert!(eval_predicate(&PredKind::Positive, &ConstLit::Int(1)));
        assert!(!eval_predicate(&PredKind::Positive, &ConstLit::Int(0)));
        assert!(eval_predicate(&in_range(1, 10), &ConstLit::Int(5),));
        assert!(!eval_predicate(&in_range(1, 10), &ConstLit::Int(11),));
    }

    #[test]
    fn eval_predicate_string() {
        assert!(eval_predicate(
            &PredKind::MinLength(2),
            &ConstLit::Str("ab".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::MinLength(3),
            &ConstLit::Str("ab".into()),
        ));
        assert!(eval_predicate(
            &PredKind::NonEmpty,
            &ConstLit::Str("x".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::NonEmpty,
            &ConstLit::Str(String::new()),
        ));
        assert!(eval_predicate(
            &PredKind::Matches("[a-z]+".into()),
            &ConstLit::Str("abc".into()),
        ));
        assert!(!eval_predicate(
            &PredKind::Matches("[a-z]+".into()),
            &ConstLit::Str("ABC".into()),
        ));
    }

    #[test]
    fn eval_predicate_base_mismatch_is_vacuously_true() {
        // SURPRISING (pinned as-is): a predicate/literal base mismatch returns
        // `true` — base/predicate mismatch is a declaration-time error reported
        // elsewhere, not by construction-time eval.
        assert!(eval_predicate(&PredKind::MinLength(5), &ConstLit::Int(0),));
    }

    // -- literal_matches_base ----------------------------------------------

    #[test]
    fn literal_matches_base_pairs() {
        assert!(literal_matches_base(&ConstLit::Int(1), BaseType::Int));
        assert!(literal_matches_base(
            &ConstLit::Str("x".into()),
            BaseType::String,
        ));
        assert!(!literal_matches_base(&ConstLit::Int(1), BaseType::String));
        assert!(!literal_matches_base(&ConstLit::Unit, BaseType::Int));
    }

    // -- type_decl_base / type_decl_refinement -----------------------------

    #[test]
    fn type_decl_base_refined_vs_record() {
        let refined = refined_decl("Age", BaseType::Int, None);
        assert_eq!(type_decl_base(&refined), Some(BaseType::Int));
        assert_eq!(type_decl_base(&record_decl("Pt")), None);
    }

    #[test]
    fn type_decl_refinement_present_vs_absent() {
        let with = refined_decl(
            "Age",
            BaseType::Int,
            Some(refinement(vec![PredKind::Positive])),
        );
        assert!(type_decl_refinement(&with).is_some());
        let without = refined_decl("Raw", BaseType::Int, None);
        assert!(type_decl_refinement(&without).is_none());
        assert!(type_decl_refinement(&record_decl("Pt")).is_none());
    }

    // -- check_*_refinement_consistency ------------------------------------

    #[test]
    fn int_refinement_consistency() {
        // Consistent: 1..=10 with Positive — no error.
        let mut errs = vec![];
        check_int_refinement_consistency(
            &refinement(vec![PredKind::Positive, in_range(1, 10)]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // Inconsistent: InRange(10, 1) is empty → exactly one error.
        let mut errs = vec![];
        check_int_refinement_consistency(&refinement(vec![in_range(10, 1)]), &mut errs);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
    }

    #[test]
    fn float_refinement_consistency() {
        // Consistent range.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![PredKind::InRangeF(fbound(0.0), fbound(1.0))]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // Empty: 5.0..=1.0 → one error.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![PredKind::InRangeF(fbound(5.0), fbound(1.0))]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
        // Degenerate-but-exclusive: Positive with InRangeF(0.0, 0.0) → lo==hi
        // and lo_exclusive → one error.
        let mut errs = vec![];
        check_float_refinement_consistency(
            &refinement(vec![
                PredKind::Positive,
                PredKind::InRangeF(fbound(0.0), fbound(0.0)),
            ]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn string_refinement_consistency() {
        // Consistent: MinLength(1), MaxLength(10).
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::MinLength(1), PredKind::MaxLength(10)]),
            &mut errs,
        );
        assert!(errs.is_empty());
        // min > max → one error.
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::MinLength(10), PredKind::MaxLength(2)]),
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].category, "karn.types.empty_refinement");
        // Conflicting exact lengths → TWO errors (pinned as-is): the explicit
        // `Length(3)`/`Length(5)` conflict push, *plus* the subsequent
        // min_len(5) > max_len(3) empty-range push (each `Length` clamps both
        // bounds to itself).
        let mut errs = vec![];
        check_string_refinement_consistency(
            &refinement(vec![PredKind::Length(3), PredKind::Length(5)]),
            &mut errs,
        );
        assert_eq!(errs.len(), 2);
        assert!(
            errs.iter()
                .all(|e| e.category == "karn.types.empty_refinement")
        );
    }

    // -- numeric_mix -------------------------------------------------------

    #[test]
    fn numeric_mix_int_float_pairs() {
        assert!(numeric_mix(Some(BaseType::Int), Some(BaseType::Float)));
        assert!(numeric_mix(Some(BaseType::Float), Some(BaseType::Int)));
        assert!(!numeric_mix(Some(BaseType::Int), Some(BaseType::Int)));
        assert!(!numeric_mix(Some(BaseType::Float), Some(BaseType::Float)));
        assert!(!numeric_mix(None, Some(BaseType::Int)));
    }
}
