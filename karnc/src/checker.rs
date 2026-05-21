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
use crate::error::CompileError;
use crate::resolver::{MethodTable, ResolvedCommons};
use crate::span::Span;

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
    /// `ValidationError` — built-in error type.
    ValidationError,
    /// `()` — the unit type (v0.5).
    Unit,
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
            Ty::ValidationError => "ValidationError".to_string(),
            Ty::Unit => "()".to_string(),
        }
    }

    /// True if this type is `Effect[_]`.
    pub fn is_effect(&self) -> bool {
        matches!(self, Ty::Effect(_))
    }

    /// The underlying base type, if this type widens to a base type.
    /// Opaque types deliberately do NOT widen — that's the whole point of
    /// the opacity — so `Ty::Named { kind: Opaque(_), .. }` returns None.
    fn base(&self) -> Option<BaseType> {
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

pub fn check(input: ResolvedCommons) -> Result<TypedCommons, Vec<CompileError>> {
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
            check_fn(f, &input, &mut expr_types, &mut errors);
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
    capabilities: HashMap<String, CapabilityInfo>,
    declared_capabilities: HashMap<String, CapabilityInfo>,
    agent_state_ty: Option<Ty>,
    agent_self_scope: Option<HashMap<String, Ty>>,
    given_declared: Vec<String>,
) {
    let Some(return_ty) = resolve_type_ref(return_type, &input.types) else {
        return;
    };
    // Build the parameter scope.
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    for p in params {
        if let Some(t) = resolve_type_ref(&p.type_ref, &input.types) {
            param_scope.insert(p.name.name.clone(), t);
        }
    }
    if let Some(self_scope) = agent_self_scope {
        param_scope.extend(self_scope);
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let given_remaining: HashSet<String> = given_declared.iter().cloned().collect();
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty,
        commit_seen: false,
        capabilities,
        declared_capabilities,
        given_remaining,
        given_used: HashSet::new(),
        in_test_body: false,
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
    //    test harness can match it.
    let declared: HashSet<String> = given_declared.iter().cloned().collect();
    for c in &declared {
        if !ctx.given_used.contains(c) {
            ctx.errors.push(
                CompileError::new(
                    "karn.given.unused_capability",
                    return_ty_span,
                    format!("capability `{c}` is declared in `given` but never used in the body"),
                )
                .with_note(
                    "remove the capability from the `given` clause, or use it in the handler body",
                ),
            );
        }
    }
}

// -- type-declaration validation --

fn check_type_decl(
    t: &TypeDecl,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    match &t.body {
        TypeBody::Refined {
            base,
            base_span,
            refinement,
        } => {
            check_refinement(*base, *base_span, refinement.as_ref(), errors);
        }
        TypeBody::Opaque {
            base,
            base_span,
            refinement,
        } => {
            // Opaque types share refinement-validity rules with refined types.
            check_refinement(*base, *base_span, refinement.as_ref(), errors);
        }
        TypeBody::Record(r) => {
            for f in &r.fields {
                if let Some(ref_r) = &f.refinement {
                    // Inline refinements on fields must apply to the field's base type.
                    if let Some(b) = field_base_type(&f.type_ref, types) {
                        check_refinement(b, f.type_ref.span(), Some(ref_r), errors);
                    } else {
                        errors.push(CompileError::new(
                            "karn.types.field_refinement_not_base",
                            ref_r.span,
                            format!(
                                "inline refinement on field `{}` requires a base or refined type",
                                f.name.name
                            ),
                        ));
                    }
                }
            }
        }
        TypeBody::Sum(_) => {
            // No further per-variant checks at the type level.
        }
    }
}

/// The base type of a field's type-ref (chasing through named refined types).
fn field_base_type(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<BaseType> {
    match r {
        TypeRef::Base(b, _) => Some(*b),
        TypeRef::Named(id) => match types.get(&id.name).map(|t| &t.body) {
            Some(TypeBody::Refined { base, .. }) => Some(*base),
            _ => None,
        },
        _ => None,
    }
}

/// The implicit base type of a TypeDecl whose constructor would be `T.of`:
/// Refined and Opaque types alike share the `of(base) -> Result[T, _]` shape.
/// Returns None for record / sum types.
fn type_decl_base(decl: &TypeDecl) -> Option<BaseType> {
    match &decl.body {
        TypeBody::Refined { base, .. } => Some(*base),
        TypeBody::Opaque { base, .. } => Some(*base),
        _ => None,
    }
}

fn check_refinement(
    base: BaseType,
    base_span: Span,
    refinement: Option<&Refinement>,
    errors: &mut Vec<CompileError>,
) {
    let Some(refinement) = refinement else {
        return;
    };

    for pred in &refinement.predicates {
        if !pred_applies_to(&pred.kind, base) {
            errors.push(
                CompileError::new(
                    "karn.types.predicate_base_mismatch",
                    pred.span,
                    format!(
                        "predicate `{}` cannot be applied to base type `{}`",
                        pred.kind.name(),
                        base.name()
                    ),
                )
                .with_label(
                    base_span,
                    format!("base type `{}` declared here", base.name()),
                )
                .with_note(predicate_base_help(pred.kind.name())),
            );
        }
        match &pred.kind {
            PredKind::Matches(pat) => {
                if let Err(e) = Regex::new(pat) {
                    errors.push(
                        CompileError::new(
                            "karn.types.invalid_regex",
                            pred.span,
                            format!("invalid regular expression in `Matches(\"{pat}\")`"),
                        )
                        .with_note(format!("regex parse error: {e}")),
                    );
                }
            }
            PredKind::InRange(lo, hi) => {
                if lo > hi {
                    errors.push(
                        CompileError::new(
                            "karn.types.inverted_range",
                            pred.span,
                            format!(
                                "`InRange({lo}, {hi})` has its bounds inverted (`min` must be ≤ `max`)"
                            ),
                        )
                        .with_note("swap the arguments, e.g. `InRange(min, max)`"),
                    );
                }
            }
            PredKind::MinLength(n) | PredKind::MaxLength(n) | PredKind::Length(n) => {
                if *n < 0 {
                    errors.push(CompileError::new(
                        "karn.types.negative_length",
                        pred.span,
                        format!("length argument must be non-negative, got {n}"),
                    ));
                }
            }
            PredKind::NonNegative | PredKind::Positive | PredKind::NonEmpty => {}
        }
    }

    let all_compatible = refinement
        .predicates
        .iter()
        .all(|p| pred_applies_to(&p.kind, base));
    if !all_compatible {
        return;
    }
    match base {
        BaseType::Int => check_int_refinement_consistency(refinement, errors),
        BaseType::String => check_string_refinement_consistency(refinement, errors),
        BaseType::Bool => {}
    }
}

fn pred_applies_to(pred: &PredKind, base: BaseType) -> bool {
    matches!(
        (pred, base),
        (PredKind::Matches(_), BaseType::String)
            | (PredKind::InRange(_, _), BaseType::Int)
            | (PredKind::MinLength(_), BaseType::String)
            | (PredKind::MaxLength(_), BaseType::String)
            | (PredKind::Length(_), BaseType::String)
            | (PredKind::NonNegative, BaseType::Int)
            | (PredKind::Positive, BaseType::Int)
            | (PredKind::NonEmpty, BaseType::String)
    )
}

fn predicate_base_help(name: &str) -> &'static str {
    match name {
        "Matches" | "MinLength" | "MaxLength" | "Length" | "NonEmpty" => {
            "this predicate applies to `String` only"
        }
        "NonNegative" | "Positive" | "InRange" => "this predicate applies to `Int` only",
        _ => "see the documentation for valid predicate-base combinations",
    }
}

fn check_int_refinement_consistency(refinement: &Refinement, errors: &mut Vec<CompileError>) {
    let mut lo: i64 = i64::MIN;
    let mut hi: i64 = i64::MAX;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::Positive => lo = lo.max(1),
            PredKind::NonNegative => lo = lo.max(0),
            PredKind::InRange(a, b) => {
                lo = lo.max(*a);
                hi = hi.min(*b);
            }
            _ => {}
        }
    }
    if lo > hi {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — the predicates contradict each other",
            )
            .with_note(format!(
                "the effective range is `{lo}..={hi}`, which is empty"
            )),
        );
    }
}

fn check_string_refinement_consistency(refinement: &Refinement, errors: &mut Vec<CompileError>) {
    let mut min_len: i64 = 0;
    let mut max_len: i64 = i64::MAX;
    let mut exact_len: Option<i64> = None;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::MinLength(n) => min_len = min_len.max(*n),
            PredKind::MaxLength(n) => max_len = max_len.min(*n),
            PredKind::NonEmpty => min_len = min_len.max(1),
            PredKind::Length(n) => {
                if let Some(prev) = exact_len {
                    if prev != *n {
                        errors.push(CompileError::new(
                            "karn.types.empty_refinement",
                            refinement.span,
                            format!(
                                "conflicting exact lengths: `Length({prev})` and `Length({n})` cannot both hold"
                            ),
                        ));
                    }
                } else {
                    exact_len = Some(*n);
                }
                min_len = min_len.max(*n);
                max_len = max_len.min(*n);
            }
            _ => {}
        }
    }
    if min_len > max_len {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — minimum length exceeds maximum length",
            )
            .with_note(format!(
                "the effective length range is `{min_len}..={max_len}`, which is empty"
            )),
        );
    }
}

// -- function body type checking --

/// Mutable per-function context.
pub struct Ctx<'a> {
    pub input: &'a ResolvedCommons,
    pub expr_types: &'a mut HashMap<Span, Ty>,
    pub errors: &'a mut Vec<CompileError>,
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
    /// True when the body being checked is a test case body. Permits
    /// `assert` statements (v0.7).
    pub in_test_body: bool,
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

fn check_fn(
    f: &FnDecl,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) {
    let return_ty = match resolve_type_ref(&f.return_type, &input.types) {
        Some(t) => t,
        None => return,
    };
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    // For methods, the implicit `self` parameter has the attached type.
    if let FnName::Method { type_name, .. } = &f.name
        && f.has_self
        && let Some(self_ty) = type_from_decl(type_name, &input.types)
    {
        param_scope.insert("self".to_string(), self_ty);
    }
    for p in &f.params {
        if let Some(ty) = resolve_type_ref(&p.type_ref, &input.types) {
            param_scope.insert(p.name.name.clone(), ty);
        }
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span: f.return_type.span(),
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        capabilities: HashMap::new(),
        declared_capabilities: HashMap::new(),
        given_remaining: HashSet::new(),
        given_used: HashSet::new(),
        in_test_body: false,
    };
    let Some(body_ty) = type_of_block(&f.body, Some(&return_ty), &mut ctx) else {
        return;
    };
    if !compatible(&body_ty, &return_ty) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.return_mismatch",
                f.body.tail.span,
                format!(
                    "function body has type `{}`, but the declared return type is `{}`",
                    body_ty.display(),
                    return_ty.display()
                ),
            )
            .with_label(f.return_type.span(), "declared return type"),
        );
    }
}

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

pub fn resolve_type_ref(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    match r {
        TypeRef::Base(b, _) => Some(Ty::Base(*b)),
        TypeRef::Named(id) => type_from_decl(id, types),
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
        TypeRef::ValidationError(_) => Some(Ty::ValidationError),
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
        (Ty::ValidationError, Ty::ValidationError) => true,
        (Ty::Unit, Ty::Unit) => true,
        _ => false,
    }
}

pub fn type_of_block(block: &Block, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    ctx.push_scope();
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) => {
                let annot_ty = l.type_annot.as_ref().and_then(|a| {
                    let r = resolve_type_ref(a, &ctx.input.types);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
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
                    let r = resolve_type_ref(a, &ctx.input.types);
                    if r.is_none() {
                        ctx.errors.push(CompileError::new(
                            "karn.resolve.unknown_type",
                            a.span(),
                            "type in `let` annotation does not resolve",
                        ));
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
        ExprKind::IntLit(_) => Some(Ty::Base(BaseType::Int)),
        ExprKind::StrLit(_) => Some(Ty::Base(BaseType::String)),
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
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
                    check_ident(id, ctx)
                }
            } else {
                check_ident(id, ctx)
            }
        }
        ExprKind::Paren(inner) => type_of(inner, expected, ctx),
        ExprKind::Call(name, args) => {
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
                check_call(name, args, expr.span, ctx)
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
            if type_name.name == "HttpResult" {
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
                && id.name == "HttpResult"
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
            args,
        } => {
            // v0.9: `HttpResult.Variant(args)` — explicit HttpResult construction.
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.lookup(id.name.as_str()).is_none()
                && id.name == "HttpResult"
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
                check_method_call(receiver, method, args, expr.span, ctx)
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
    };
    if let Some(ty) = &ty {
        ctx.expr_types.insert(expr.span, ty.clone());
    }
    ty
}

fn check_ident(id: &Ident, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(ty) = ctx.lookup(id.name.as_str()) {
        return Some(ty);
    }
    // Bare variant of a unique-owner sum type (nullary variants).
    let owners: Vec<&TypeDecl> = ctx
        .input
        .types
        .values()
        .filter(|t| matches!(&t.body, TypeBody::Sum(s) if s.variants.iter().any(|v| v.name.name == id.name)))
        .collect();
    if owners.len() == 1 {
        let owner = owners[0];
        if let TypeBody::Sum(s) = &owner.body
            && let Some(variant) = s.variants.iter().find(|v| v.name.name == id.name)
        {
            if !variant.payload.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.variant_missing_payload",
                        id.span,
                        format!(
                            "variant `{}` of `{}` has a payload — call it with arguments: `{}(...)`",
                            id.name, owner.name.name, id.name
                        ),
                    )
                    .with_label(variant.span, "variant declared here"),
                );
                return None;
            }
            return Some(named_ty(owner));
        }
    }
    None
}

fn check_unary(op: UnaryOp, inner: &Expr, op_span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let t = type_of(inner, None, ctx)?;
    match op {
        UnaryOp::Neg => {
            if t.base() == Some(BaseType::Int) {
                Some(Ty::Base(BaseType::Int))
            } else {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    op_span,
                    format!(
                        "unary `-` requires `Int`, but the operand has type `{}`",
                        t.display()
                    ),
                ));
                None
            }
        }
        UnaryOp::Not => {
            if t.base() == Some(BaseType::Bool) {
                Some(Ty::Base(BaseType::Bool))
            } else {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    op_span,
                    format!(
                        "unary `!` requires `Bool`, but the operand has type `{}`",
                        t.display()
                    ),
                ));
                None
            }
        }
    }
}

fn check_binop(op: BinOp, lhs: &Expr, rhs: &Expr, ctx: &mut Ctx) -> Option<Ty> {
    // For `&&`, if the lhs is or contains an `is` test, propagate the
    // bindings into the rhs scope (so `r is Ok(n) && n > 0` works).
    if op == BinOp::And {
        let lt = type_of(lhs, Some(&Ty::Base(BaseType::Bool)), ctx);
        let bindings = collect_is_bindings(lhs, ctx);
        ctx.push_scope();
        for (name, ty) in bindings {
            ctx.bind(name, ty);
        }
        let rt = type_of(rhs, Some(&Ty::Base(BaseType::Bool)), ctx);
        ctx.pop_scope();
        let (lt, rt) = (lt?, rt?);
        if lt.base() != Some(BaseType::Bool) {
            ctx.errors.push(CompileError::new(
                "karn.types.type_mismatch",
                lhs.span,
                format!(
                    "operator `&&` requires `Bool` operands; left operand has type `{}`",
                    lt.display()
                ),
            ));
            return None;
        }
        if rt.base() != Some(BaseType::Bool) {
            ctx.errors.push(CompileError::new(
                "karn.types.type_mismatch",
                rhs.span,
                format!(
                    "operator `&&` requires `Bool` operands; right operand has type `{}`",
                    rt.display()
                ),
            ));
            return None;
        }
        return Some(Ty::Base(BaseType::Bool));
    }

    let lt = type_of(lhs, None, ctx);
    let rt = type_of(rhs, None, ctx);
    let (lt, rt) = (lt?, rt?);
    let span = lhs.span.merge(rhs.span);
    let lt_base = lt.base();
    let rt_base = rt.base();
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
            if lt_base != Some(BaseType::Int) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    lhs.span,
                    format!(
                        "operator `{}` requires `Int` operands; left operand has type `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            if rt_base != Some(BaseType::Int) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    rhs.span,
                    format!(
                        "operator `{}` requires `Int` operands; right operand has type `{}`",
                        op.name(),
                        rt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
            if lt_base != rt_base || lt_base.is_none() {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` requires both operands to have the same base type; got `{}` and `{}`",
                        op.name(),
                        lt.display(),
                        rt.display()
                    ),
                ));
                return None;
            }
            if !matches!(lt_base, Some(BaseType::Int) | Some(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` is only defined on `Int` and `String`, not `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::Eq | BinOp::NotEq => {
            if lt_base.is_some() && rt_base.is_some() {
                if lt_base != rt_base {
                    ctx.errors.push(CompileError::new(
                        "karn.types.type_mismatch",
                        span,
                        format!(
                            "operator `{}` requires both operands to have the same base type; got `{}` and `{}`",
                            op.name(),
                            lt.display(),
                            rt.display()
                        ),
                    ));
                    return None;
                }
            } else if !compatible(&lt, &rt) && !compatible(&rt, &lt) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` requires both operands to have the same type; got `{}` and `{}`",
                        op.name(),
                        lt.display(),
                        rt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::And | BinOp::Or => {
            if lt.base() != Some(BaseType::Bool) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    lhs.span,
                    format!(
                        "operator `{}` requires `Bool` operands; left operand has type `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            if rt.base() != Some(BaseType::Bool) {
                ctx.errors.push(CompileError::new(
                    "karn.types.type_mismatch",
                    rhs.span,
                    format!(
                        "operator `{}` requires `Bool` operands; right operand has type `{}`",
                        op.name(),
                        rt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
    }
}

fn check_call(name: &Ident, args: &[Expr], span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(fn_decl) = ctx.input.fns.get(&name.name).cloned() {
        return check_call_against_fn(name, &fn_decl, args, ctx);
    }
    // Could be a bare variant constructor with payload.
    let owners: Vec<TypeDecl> = ctx
        .input
        .types
        .values()
        .filter(|t| matches!(&t.body, TypeBody::Sum(s) if s.variants.iter().any(|v| v.name.name == name.name)))
        .cloned()
        .collect();
    if owners.len() == 1 {
        let owner = owners.into_iter().next().unwrap();
        return check_variant_construction(&owner, &name.name, args, span, ctx);
    }
    let _ = span;
    None
}

fn check_call_against_fn(
    name: &Ident,
    fn_decl: &FnDecl,
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    if fn_decl.params.len() != args.len() {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let resolved_params: Vec<(Option<Ty>, &Param)> = fn_decl
        .params
        .iter()
        .map(|p| (resolve_type_ref(&p.type_ref, &ctx.input.types), p))
        .collect();
    let mut ok = true;
    for (i, ((param_ty, param), arg)) in resolved_params.iter().zip(args.iter()).enumerate() {
        let arg_ty = type_of(arg, param_ty.as_ref(), ctx);
        let (Some(arg_ty), Some(param_ty)) = (arg_ty, param_ty.as_ref()) else {
            ok = false;
            continue;
        };
        if !compatible(&arg_ty, param_ty) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.argument_mismatch",
                    arg.span,
                    format!(
                        "argument {} to `{}` has type `{}`, but parameter `{}` expects `{}`",
                        i + 1,
                        name.name,
                        arg_ty.display(),
                        param.name.name,
                        param_ty.display()
                    ),
                )
                .with_label(param.span, "parameter declared here"),
            );
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    resolve_type_ref(&fn_decl.return_type, &ctx.input.types)
}

fn check_variant_construction(
    owner: &TypeDecl,
    variant_name: &str,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let TypeBody::Sum(s) = &owner.body else {
        return None;
    };
    let variant = s.variants.iter().find(|v| v.name.name == variant_name)?;
    if variant.payload.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.variant_arity",
                span,
                format!(
                    "variant `{}` of `{}` expects {} argument(s), but {} were given",
                    variant_name,
                    owner.name.name,
                    variant.payload.len(),
                    args.len()
                ),
            )
            .with_label(variant.span, "variant declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (field, arg)) in variant.payload.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&field.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.variant_payload_mismatch",
                arg.span,
                format!(
                    "argument {} to variant `{}` has type `{}`, but field `{}` expects `{}`",
                    i + 1,
                    variant_name,
                    actual.display(),
                    field.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    Some(named_ty(owner))
}

fn check_if(
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    if_span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let cond_ty = type_of(cond, Some(&Ty::Base(BaseType::Bool)), ctx);
    if let Some(c) = &cond_ty
        && c.base() != Some(BaseType::Bool)
    {
        ctx.errors.push(CompileError::new(
            "karn.types.if_non_bool_cond",
            cond.span,
            format!(
                "`if` condition must have type `Bool`, but has type `{}`",
                c.display()
            ),
        ));
    }
    // `is` bindings in the condition flow into the then-branch.
    let bindings = collect_is_bindings(cond, ctx);
    ctx.push_scope();
    for (name, ty) in bindings {
        ctx.bind(name, ty);
    }
    let then_ty = type_of_block(then_block, expected, ctx);
    ctx.pop_scope();
    let else_ty = type_of_block(else_block, expected, ctx);
    match (then_ty, else_ty) {
        (Some(t), Some(e)) => {
            if t == e {
                Some(t)
            } else {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.if_branch_mismatch",
                        if_span,
                        format!(
                            "`if` branches produce different types: `{}` and `{}`",
                            t.display(),
                            e.display()
                        ),
                    )
                    .with_label(
                        then_block.tail.span,
                        format!("then-branch has type `{}`", t.display()),
                    )
                    .with_label(
                        else_block.tail.span,
                        format!("else-branch has type `{}`", e.display()),
                    )
                    .with_note("both branches of an `if` expression must produce the same type"),
                );
                None
            }
        }
        _ => None,
    }
}

fn check_ok(inner: &Expr, span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    // v0.9: `Ok` is now overloaded between `Result.Ok` and `HttpResult.Ok`.
    // First consult the expected type (propagated from let-annotations, match
    // arms, and the enclosing return type via tail-position auto-lift).
    let in_result = surrounding_result(expected, &ctx.return_ty);
    let in_http = expected
        .and_then(peel_to_http_result)
        .or_else(|| peel_to_http_result(&ctx.return_ty));
    match (in_result.clone(), in_http.clone()) {
        (Some(_), Some(_)) => {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.ambiguous_constructor",
                    span,
                    "ambiguous constructor `Ok`: could be `Result.Ok` or `HttpResult.Ok`",
                )
                .with_note("qualify it as `Result.Ok(...)` or `HttpResult.Ok(...)`"),
            );
            // Best-effort: still type the inner.
            let _ = type_of(inner, None, ctx);
            None
        }
        (None, Some(t_ty)) => {
            let inner_ty = type_of(inner, Some(&t_ty), ctx)?;
            if !compatible(&inner_ty, &t_ty) {
                ctx.errors.push(CompileError::new(
                    "karn.types.ok_value_mismatch",
                    inner.span,
                    format!(
                        "`Ok(...)` value has type `{}`, but the surrounding context expects `HttpResult[{}]`",
                        inner_ty.display(),
                        t_ty.display(),
                    ),
                ));
                return None;
            }
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        (Some((t_ty, e_ty)), None) => {
            let inner_ty = type_of(inner, Some(&t_ty), ctx)?;
            if !compatible(&inner_ty, &t_ty) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.ok_value_mismatch",
                        inner.span,
                        format!(
                            "`Ok(...)` value has type `{}`, but the surrounding context expects `Result[{}, {}]`",
                            inner_ty.display(),
                            t_ty.display(),
                            e_ty.display()
                        ),
                    )
                    .with_label(ctx.return_ty_span, "context's expected `Result` type"),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t_ty), Box::new(e_ty)))
        }
        (None, None) => {
            let _ = type_of(inner, None, ctx);
            ctx.errors.push(
                CompileError::new(
                    "karn.types.cannot_infer_result_type_params",
                    span,
                    "cannot infer the type parameter of `Ok(...)`",
                )
                .with_note(
                    "add a `let` annotation (`let x: Result[T, E] = Ok(...)`) \
                     or declare the enclosing function's return type as `Result[T, E]` or `HttpResult[T]`",
                ),
            );
            None
        }
    }
}

/// Peel one optional `Effect[_]` wrapper to expose an underlying `HttpResult[T]`.
fn peel_to_http_result(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::HttpResult(inner) => Some((**inner).clone()),
        Ty::Effect(inner) => peel_to_http_result(inner),
        _ => None,
    }
}

/// Type-check construction of an `HttpResult[T]` variant (v0.9 §4.3).
///
/// Variants come in three payload shapes:
/// - `Value` (`Ok`, `Created`) — argument's type is `T`. `T` is taken from
///   the expected type if available; otherwise reported as ambiguous.
/// - `Message` (`BadRequest`, `Conflict`, `UnprocessableEntity`,
///   `ServerError`) — argument must be `String`.
/// - `None` (`NoContent`, `Unauthorized`, `Forbidden`, `NotFound`) — no
///   argument permitted; `T` is taken from the expected type or left
///   inferred.
fn check_http_variant(
    span: Span,
    variant: HttpVariant,
    args: &[Expr],
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let expected_t = expected
        .and_then(peel_to_http_result)
        .or_else(|| peel_to_http_result(&ctx.return_ty));
    match variant.payload {
        HttpVariantPayload::Value => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], expected_t.as_ref(), ctx)?;
            let t_ty = match (expected_t, arg_ty.clone()) {
                (Some(t), _) => {
                    if !compatible(&arg_ty, &t) {
                        ctx.errors.push(CompileError::new(
                            "karn.types.ok_value_mismatch",
                            args[0].span,
                            format!(
                                "`HttpResult.{}` value has type `{}`, but the surrounding context expects `HttpResult[{}]`",
                                variant.name,
                                arg_ty.display(),
                                t.display(),
                            ),
                        ));
                        return None;
                    }
                    t
                }
                (None, t) => t,
            };
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        HttpVariantPayload::Message => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 `String` argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], Some(&Ty::Base(BaseType::String)), ctx)?;
            if !compatible(&arg_ty, &Ty::Base(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "karn.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`HttpResult.{}` expects a `String` message, but got `{}`",
                        variant.name,
                        arg_ty.display(),
                    ),
                ));
                return None;
            }
            // Inner T is irrelevant for message variants but the type needs
            // a concrete payload. Pick `()` when nothing is known; otherwise
            // use the propagated expected type.
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        HttpVariantPayload::None => {
            if !args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "karn.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` takes no arguments, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
    }
}

fn check_err(inner: &Expr, span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let surrounding = surrounding_result(expected, &ctx.return_ty);
    let expected_e = surrounding.as_ref().map(|(_, e)| e.clone());
    let inner_ty = type_of(inner, expected_e.as_ref(), ctx)?;
    match surrounding {
        Some((t_ty, e_ty)) => {
            if !compatible(&inner_ty, &e_ty) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.err_value_mismatch",
                        inner.span,
                        format!(
                            "`Err(...)` value has type `{}`, but the surrounding context expects `Result[{}, {}]`",
                            inner_ty.display(),
                            t_ty.display(),
                            e_ty.display()
                        ),
                    )
                    .with_label(ctx.return_ty_span, "context's expected `Result` type"),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t_ty), Box::new(e_ty)))
        }
        None => {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.cannot_infer_result_type_params",
                    span,
                    "cannot infer the value type parameter of `Err(...)`",
                )
                .with_note(
                    "add a `let` annotation or declare the enclosing function's return type as `Result[T, E]`",
                ),
            );
            None
        }
    }
}

fn check_some(inner: &Expr, _span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let expected_inner = expected
        .and_then(peel_to_option)
        .or_else(|| peel_to_option(&ctx.return_ty));
    let inner_ty = type_of(inner, expected_inner.as_ref(), ctx)?;
    if let Some(exp) = &expected_inner
        && !compatible(&inner_ty, exp)
    {
        ctx.errors.push(CompileError::new(
            "karn.types.some_value_mismatch",
            inner.span,
            format!(
                "`Some(...)` value has type `{}`, but the surrounding context expects `Option[{}]`",
                inner_ty.display(),
                exp.display()
            ),
        ));
        return None;
    }
    Some(Ty::Option(Box::new(inner_ty)))
}

fn check_none(span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(t) = expected.and_then(peel_to_option) {
        return Some(Ty::Option(Box::new(t)));
    }
    if let Some(t) = peel_to_option(&ctx.return_ty) {
        return Some(Ty::Option(Box::new(t)));
    }
    ctx.errors.push(
        CompileError::new(
            "karn.types.cannot_infer_option_type_param",
            span,
            "cannot infer the value type of `None`",
        )
        .with_note(
            "add an annotation (`let x: Option[T] = None`) or use `None` where the context expects an `Option`",
        ),
    );
    None
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

fn check_question(inner: &Expr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let inner_ty = type_of(inner, None, ctx)?;
    let Ty::Result(t, e) = &inner_ty else {
        ctx.errors.push(
            CompileError::new(
                "karn.types.question_on_non_result",
                inner.span,
                format!(
                    "the `?` operator requires a `Result[T, E]` value, but got `{}`",
                    inner_ty.display()
                ),
            )
            .with_label(span, "this `?` requires a Result"),
        );
        return None;
    };
    // v0.5: `?` is also valid inside `Effect[Result[T, E]]` — the `Err` is
    // propagated as `Effect.pure(Err(e))`.
    let effect_result = if let Ty::Effect(inner_eff) = &ctx.return_ty
        && let Ty::Result(_, eff_e) = inner_eff.as_ref()
    {
        Some(eff_e.as_ref().clone())
    } else {
        None
    };
    let Ty::Result(_ret_t, ret_e) = &ctx.return_ty else {
        if let Some(eff_e) = effect_result {
            if !compatible(e, &eff_e) {
                ctx.errors.push(CompileError::new(
                    "karn.types.question_error_mismatch",
                    span,
                    format!(
                        "the `?` operator propagates an error of type `{}`, but the enclosing function returns `Effect[Result[_, {}]]`",
                        e.display(),
                        eff_e.display()
                    ),
                ));
                return None;
            }
            return Some((**t).clone());
        }
        ctx.errors.push(
            CompileError::new(
                "karn.types.question_outside_result",
                span,
                "the `?` operator can only be used inside a function returning `Result`",
            )
            .with_label(
                ctx.return_ty_span,
                format!("function returns `{}`", ctx.return_ty.display()),
            ),
        );
        return None;
    };
    if !compatible(e, ret_e) {
        ctx.errors.push(CompileError::new(
            "karn.types.question_error_mismatch",
            span,
            format!(
                "the `?` operator propagates an error of type `{}`, but the enclosing function returns `Result[_, {}]`",
                e.display(),
                ret_e.display()
            ),
        ));
        return None;
    }
    Some((**t).clone())
}

fn check_static_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Capability dispatch (v0.5): if `type_name` names a capability declared
    // in the context, dispatch via the capability table. If the capability is
    // declared but not in `given`, error specifically.
    if ctx.declared_capabilities.contains_key(&type_name.name)
        && !ctx.capabilities.contains_key(&type_name.name)
    {
        ctx.errors.push(
            CompileError::new(
                "karn.given.undeclared_capability",
                type_name.span,
                format!(
                    "capability `{}` is used but not listed in the handler's `given` clause",
                    type_name.name
                ),
            )
            .with_note(
                "add `{name}` to the handler's `given` clause so the dependency surface is visible at the declaration site",
            ),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if let Some(cap) = ctx.capabilities.get(&type_name.name).cloned() {
        if !ctx.effectful {
            ctx.errors.push(
                CompileError::new(
                    "karn.effect.capability_in_pure_context",
                    span,
                    format!(
                        "capability `{}` can only be called inside an effectful body (one returning `Effect[T]`)",
                        type_name.name
                    ),
                ),
            );
        }
        ctx.given_used.insert(type_name.name.clone());
        let Some(op) = cap.ops.iter().find(|o| o.name == method.name) else {
            ctx.errors.push(CompileError::new(
                "karn.capability.unknown_operation",
                method.span,
                format!(
                    "capability `{}` has no operation named `{}`",
                    type_name.name, method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        };
        if op.params.len() != args.len() {
            ctx.errors.push(CompileError::new(
                "karn.capability.op_arity",
                span,
                format!(
                    "capability operation `{}.{}` expects {} argument(s), but {} were given",
                    type_name.name,
                    method.name,
                    op.params.len(),
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        let op_clone = op.clone();
        for (i, (param_ty, arg)) in op_clone.params.iter().zip(args.iter()).enumerate() {
            let arg_ty = type_of(arg, Some(param_ty), ctx);
            if let Some(actual) = arg_ty
                && !compatible(&actual, param_ty)
            {
                ctx.errors.push(CompileError::new(
                    "karn.types.argument_mismatch",
                    arg.span,
                    format!(
                        "argument {} to capability `{}.{}` has type `{}`, but parameter expects `{}`",
                        i + 1,
                        type_name.name,
                        method.name,
                        actual.display(),
                        param_ty.display()
                    ),
                ));
            }
        }
        return Some(op_clone.return_ty);
    }
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    let table = ctx
        .input
        .methods
        .get(&type_name.name)
        .cloned()
        .unwrap_or_default();

    // 1) User-declared static method.
    if let Some(method_decl) = table.statics.get(&method.name).cloned() {
        return check_method_args(&method_decl, args, ctx, type_name, method);
    }

    // 2) Built-in `of` constructor on refined or opaque types.
    if method.name == "of"
        && let Some(base) = type_decl_base(&decl)
    {
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_arity",
                span,
                format!(
                    "constructor `{}.of` expects 1 argument, but {} were given",
                    type_name.name,
                    args.len()
                ),
            ));
            return None;
        }
        let arg = &args[0];
        let expected = Ty::Base(base);
        let arg_ty = type_of(arg, Some(&expected), ctx)?;
        if !compatible(&arg_ty, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_base_mismatch",
                arg.span,
                format!(
                    "constructor `{}.of` expects a `{}` argument, but got `{}`",
                    type_name.name,
                    base.name(),
                    arg_ty.display()
                ),
            ));
            return None;
        }
        return Some(Ty::Result(
            Box::new(named_ty(&decl)),
            Box::new(Ty::ValidationError),
        ));
    }

    // 2b) Built-in `unsafe` constructor on opaque types — only available
    // inside the defining commons.
    if method.name == "unsafe"
        && let TypeBody::Opaque { base, .. } = &decl.body
    {
        if !ctx.input.is_local_type(&decl.name.name) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.opaque_unsafe_outside",
                    method.span,
                    format!(
                        "`{}.unsafe(...)` is only available within the commons that defines the opaque type `{}`",
                        type_name.name, type_name.name
                    ),
                )
                .with_note(
                    "outside the defining commons, opaque values are constructed via `T.of(value)`",
                ),
            );
            return None;
        }
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_arity",
                span,
                format!(
                    "`{}.unsafe` expects 1 argument, but {} were given",
                    type_name.name,
                    args.len()
                ),
            ));
            return None;
        }
        let arg = &args[0];
        let expected = Ty::Base(*base);
        let arg_ty = type_of(arg, Some(&expected), ctx)?;
        if !compatible(&arg_ty, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.constructor_base_mismatch",
                arg.span,
                format!(
                    "`{}.unsafe` expects a `{}` argument, but got `{}`",
                    type_name.name,
                    base.name(),
                    arg_ty.display()
                ),
            ));
            return None;
        }
        return Some(named_ty(&decl));
    }

    // 3) Qualified variant construction `TypeName.Variant(args)`.
    if let TypeBody::Sum(_) = &decl.body {
        return check_variant_construction(&decl, &method.name, args, span, ctx);
    }

    ctx.errors.push(
        CompileError::new(
            "karn.types.unknown_static_member",
            method.span,
            format!(
                "type `{}` has no static method or variant named `{}`",
                type_name.name, method.name
            ),
        )
        .with_label(decl.name.span, "type declared here"),
    );
    None
}

fn check_method_args(
    method_decl: &FnDecl,
    args: &[Expr],
    ctx: &mut Ctx,
    type_name: &Ident,
    method: &Ident,
) -> Option<Ty> {
    if method_decl.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.method_arity",
                method.span,
                format!(
                    "static method `{}.{}` expects {} argument(s), but {} were given",
                    type_name.name,
                    method.name,
                    method_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(method_decl.name.ident().span, "method declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (param, arg)) in method_decl.params.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}.{}` has type `{}`, but parameter `{}` expects `{}`",
                    i + 1,
                    type_name.name,
                    method.name,
                    actual.display(),
                    param.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    resolve_type_ref(&method_decl.return_type, &ctx.input.types)
}

fn check_record_spread(
    type_name: Option<&Ident>,
    base: &Expr,
    overrides: &[FieldInit],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // 1) Determine the record type.
    let base_ty = type_of(base, expected, ctx)?;
    let record_name = match &base_ty {
        Ty::Named {
            name,
            kind: NamedKind::Record,
        } => name.clone(),
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.non_record_base",
                base.span,
                format!(
                    "record spread requires a record-typed base, but got `{}`",
                    base_ty.display()
                ),
            ));
            return None;
        }
    };
    if let Some(tn) = type_name
        && tn.name != record_name
    {
        ctx.errors.push(CompileError::new(
            "karn.record_spread.type_mismatch",
            tn.span,
            format!(
                "spread type prefix `{}` does not match the base's type `{}`",
                tn.name, record_name
            ),
        ));
    }
    let decl = ctx.input.types.get(&record_name)?.clone();
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    let declared: HashMap<&str, &RecordField> =
        r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
    let _ = span;
    for f in overrides {
        let Some(declared_field) = declared.get(f.name.name.as_str()) else {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.unknown_field",
                f.name.span,
                format!(
                    "record type `{}` has no field `{}`",
                    record_name, f.name.name
                ),
            ));
            continue;
        };
        let expected_ty = resolve_type_ref(&declared_field.type_ref, &ctx.input.types);
        let value_ty = match &f.value {
            Some(v) => type_of(v, expected_ty.as_ref(), ctx),
            None => ctx.lookup(&f.name.name),
        };
        if let (Some(actual), Some(expected_ty)) = (value_ty, expected_ty)
            && !compatible(&actual, &expected_ty)
        {
            ctx.errors.push(CompileError::new(
                "karn.record_spread.field_type_mismatch",
                f.value.as_ref().map(|v| v.span).unwrap_or(f.name.span),
                format!(
                    "spread override of field `{}` has type `{}`, but the declared type is `{}`",
                    f.name.name,
                    actual.display(),
                    expected_ty.display()
                ),
            ));
        }
    }
    Some(base_ty)
}

fn check_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    if matches!(decl.body, TypeBody::Opaque { .. }) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.opaque_record_construction",
                type_name.span,
                format!(
                    "opaque type `{}` cannot be constructed with record-literal syntax",
                    type_name.name
                ),
            )
            .with_note(
                "construct opaque values via `T.of(value)` (validated) or `T.unsafe(value)` (inside the defining commons)",
            ),
        );
        return None;
    }
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    // Collect declared fields.
    let declared: HashMap<&str, &RecordField> =
        r.fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
    let _ = span;
    for f in fields {
        if let Some(declared_field) = declared.get(f.name.name.as_str()) {
            let expected = resolve_type_ref(&declared_field.type_ref, &ctx.input.types);
            let value_ty = match &f.value {
                Some(v) => type_of(v, expected.as_ref(), ctx),
                None => ctx.lookup(&f.name.name),
            };
            if let (Some(actual), Some(expected)) = (value_ty, expected)
                && !compatible(&actual, &expected)
            {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.field_value_mismatch",
                        f.value.as_ref().map(|v| v.span).unwrap_or(f.name.span),
                        format!(
                            "field `{}` expects `{}`, but the value has type `{}`",
                            f.name.name,
                            expected.display(),
                            actual.display()
                        ),
                    )
                    .with_label(declared_field.name.span, "field declared here"),
                );
            }
        }
    }
    Some(named_ty(&decl))
}

fn check_field_access(receiver: &Expr, field: &Ident, ctx: &mut Ctx) -> Option<Ty> {
    // Qualified nullary variant: `TypeName.Variant` where TypeName is a
    // declared sum type and Variant is one of its payload-less variants.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && let Some(decl) = ctx.input.types.get(&id.name)
        && let TypeBody::Sum(s) = &decl.body
        && let Some(variant) = s.variants.iter().find(|v| v.name.name == field.name)
    {
        if !variant.payload.is_empty() {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.variant_missing_payload",
                    field.span,
                    format!(
                        "variant `{}.{}` has a payload — call it with arguments",
                        id.name, field.name
                    ),
                )
                .with_label(variant.span, "variant declared here"),
            );
            return None;
        }
        return Some(named_ty(decl));
    }
    let recv_ty = type_of(receiver, None, ctx)?;
    // `.raw` on an opaque value: only available within the defining commons.
    // Returns the base type. The emitter compiles this to a `value as base`
    // type assertion (see emitter::lower_expr for FieldAccess).
    if field.name == "raw"
        && let Ty::Named {
            kind: NamedKind::Opaque(base),
            name,
        } = &recv_ty
    {
        if !ctx.input.is_local_type(name) {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.opaque_raw_outside",
                    field.span,
                    format!(
                        "`.raw` on opaque type `{}` is only available within its defining commons",
                        name
                    ),
                )
                .with_note(
                    "the base representation of an opaque type is hidden from importers; \
                     define a method on the type or use a public accessor",
                ),
            );
            return None;
        }
        return Some(Ty::Base(*base));
    }
    let Ty::Named {
        name,
        kind: NamedKind::Record,
    } = &recv_ty
    else {
        ctx.errors.push(CompileError::new(
            "karn.types.field_access_on_non_record",
            field.span,
            format!(
                "field access requires a record type, but the receiver has type `{}`",
                recv_ty.display()
            ),
        ));
        return None;
    };
    let decl = ctx.input.types.get(name)?;
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    let Some(field_decl) = r.fields.iter().find(|f| f.name.name == field.name) else {
        ctx.errors.push(
            CompileError::new(
                "karn.types.unknown_field",
                field.span,
                format!("record type `{}` has no field `{}`", name, field.name),
            )
            .with_label(decl.name.span, "type declared here"),
        );
        return None;
    };
    resolve_type_ref(&field_decl.type_ref, &ctx.input.types)
}

fn check_method_call(
    receiver: &Expr,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // v0.6: cross-context service call. Two shapes:
    //   - `Alias.service(args)`           where Alias is from `consumes X as Alias`
    //   - `prefix.tail.service(args)`     where `prefix.tail` is a consumed context's
    //                                     qualified name (parsed as nested FieldAccess).
    // The full-qualified-name form must be checked before the bare-ident form
    // (the prefix's first segment doesn't resolve as anything local).
    if ctx.lookup_root_ident(receiver).is_none() {
        if let Some(consumed) = cross_context_prefix(receiver, ctx) {
            return check_cross_context_call(receiver, &consumed, method, args, span, ctx);
        }
        // Looks like a dotted prefix (no local binding for the root). If the
        // chain matches the shape of a consumed-context call but the prefix
        // isn't actually consumed, surface an explicit diagnostic so the user
        // can fix the missing `consumes` clause rather than seeing a silent
        // "no methods" error later.
        if let ExprKind::FieldAccess { .. } = &receiver.kind
            && let Some(chain) = flatten_ident_chain(receiver)
            && chain.contains('.')
        {
            let info = &ctx.input.cross_context;
            let in_context = info.self_context.is_some();
            if in_context && info.resolve_prefix(&chain).is_none() {
                ctx.errors.push(
                    CompileError::new(
                        "karn.resolve.unconsumed_context",
                        receiver.span,
                        format!(
                            "`{chain}.{}` looks like a cross-context service call, but `{chain}` is not in this context's `consumes` clauses",
                            method.name
                        ),
                    )
                    .with_note(
                        "add a `consumes {chain}` clause at the top of the context, or use an alias and call it through the alias",
                    ),
                );
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                return None;
            }
        }
    }
    // Detect capability call (v0.5): receiver is a bare Ident naming a
    // capability declared in the context (in scope via `given`, or declared
    // but undeclared in `given` — the static-call path emits the error).
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && (ctx.capabilities.contains_key(&id.name)
            || ctx.declared_capabilities.contains_key(&id.name))
    {
        return check_static_call(id, method, args, span, ctx);
    }
    // Detect static-call shape: receiver is a bare Ident naming a declared
    // type (not a local/param). Dispatch to check_static_call.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && ctx.input.types.contains_key(&id.name)
    {
        return check_static_call(id, method, args, span, ctx);
    }
    let recv_ty = type_of(receiver, None, ctx)?;
    // Find a named type for the receiver, then look up its instance methods.
    let type_name = match &recv_ty {
        Ty::Named { name, .. } => name.clone(),
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_on_non_named_type",
                method.span,
                format!(
                    "type `{}` has no methods — only user-declared types support method calls",
                    recv_ty.display()
                ),
            ));
            return None;
        }
    };
    let table = ctx
        .input
        .methods
        .get(&type_name)
        .cloned()
        .unwrap_or_default();
    let Some(method_decl) = table.instance.get(&method.name).cloned() else {
        ctx.errors.push(CompileError::new(
            "karn.types.method_not_found",
            method.span,
            format!(
                "type `{}` has no instance method named `{}`",
                type_name, method.name
            ),
        ));
        return None;
    };
    // Param count excludes the implicit `self`.
    if method_decl.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.types.method_arity",
                method.span,
                format!(
                    "method `{}.{}` expects {} argument(s), but {} were given",
                    type_name,
                    method.name,
                    method_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(method_decl.name.ident().span, "method declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let mut ok = true;
    for (i, (param, arg)) in method_decl.params.iter().zip(args.iter()).enumerate() {
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "karn.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}.{}` has type `{}`, but parameter `{}` expects `{}`",
                    i + 1,
                    type_name,
                    method.name,
                    actual.display(),
                    param.name.name,
                    expected.display()
                ),
            ));
            ok = false;
        }
    }
    let _ = span;
    if !ok {
        return None;
    }
    resolve_type_ref(&method_decl.return_type, &ctx.input.types)
}

fn check_match(
    discriminant: &Expr,
    arms: &[MatchArm],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let disc_ty = type_of(discriminant, None, ctx)?;
    let expected_variants = variants_of(&disc_ty, &ctx.input.types);
    let Some(expected_variants) = expected_variants else {
        ctx.errors.push(CompileError::new(
            "karn.types.match_non_sum_discriminant",
            discriminant.span,
            format!(
                "cannot match on a value of type `{}` — `match` requires a sum, `Result`, or `Option`",
                disc_ty.display()
            ),
        ));
        return None;
    };
    let mut arm_types: Vec<(Ty, Span)> = Vec::new();
    let mut covered: HashSet<String> = HashSet::new();
    let mut saw_wildcard = false;
    let mut unreachable_reported = false;
    for arm in arms {
        if saw_wildcard && !unreachable_reported {
            ctx.errors.push(CompileError::new(
                "karn.types.unreachable_arm",
                arm.span,
                "this match arm is unreachable because a wildcard arm precedes it",
            ));
            unreachable_reported = true;
        }
        ctx.push_scope();
        match &arm.pattern {
            Pattern::Wildcard(_) => {
                saw_wildcard = true;
            }
            Pattern::Variant {
                type_name,
                variant,
                bindings,
                span: pat_span,
            } => {
                // Validate the variant against expected_variants.
                let variant_info = expected_variants.iter().find(|v| v.name == variant.name);
                let Some(variant_info) = variant_info else {
                    ctx.errors.push(CompileError::new(
                        "karn.types.unknown_variant_in_pattern",
                        *pat_span,
                        format!(
                            "type `{}` has no variant `{}`",
                            disc_ty.display(),
                            variant.name
                        ),
                    ));
                    ctx.pop_scope();
                    continue;
                };
                // Optional qualifier must match the discriminant type's name.
                if let Some(tn) = type_name
                    && let Ty::Named { name, .. } = &disc_ty
                    && &tn.name != name
                {
                    ctx.errors.push(CompileError::new(
                        "karn.types.pattern_type_mismatch",
                        tn.span,
                        format!(
                            "pattern qualifier `{}` does not match the discriminant type `{}`",
                            tn.name, name
                        ),
                    ));
                }
                if !covered.insert(variant.name.clone()) {
                    ctx.errors.push(CompileError::new(
                        "karn.types.duplicate_variant_arm",
                        *pat_span,
                        format!("variant `{}` is matched more than once", variant.name),
                    ));
                }
                if bindings.is_empty() && !variant_info.payload.is_empty() {
                    // Variant has payload but pattern has no bindings — allowed,
                    // means "don't bind".
                } else if !bindings.is_empty() {
                    // Resolve each binding to a payload field's type.
                    if !variant_info.payload.is_empty() {
                        let payload_map: HashMap<&str, (usize, &Ty)> = variant_info
                            .payload
                            .iter()
                            .enumerate()
                            .map(|(i, (name, ty))| (name.as_str(), (i, ty)))
                            .collect();
                        // Allow positional or named bindings, but not both.
                        let any_named = bindings
                            .iter()
                            .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
                        if any_named {
                            for b in bindings {
                                match &b.kind {
                                    PatternBindingKind::Named { field, name } => {
                                        let Some((_, ty)) = payload_map.get(field.name.as_str())
                                        else {
                                            ctx.errors.push(CompileError::new(
                                                "karn.types.unknown_pattern_field",
                                                field.span,
                                                format!(
                                                    "variant `{}` has no payload field `{}`",
                                                    variant.name, field.name
                                                ),
                                            ));
                                            continue;
                                        };
                                        if !b.is_wildcard() {
                                            ctx.bind(name.name.clone(), (*ty).clone());
                                        }
                                    }
                                    PatternBindingKind::Positional { .. } => {
                                        ctx.errors.push(CompileError::new(
                                            "karn.types.mixed_pattern_bindings",
                                            b.span,
                                            "pattern bindings must be all named (`field: name`) or all positional",
                                        ));
                                    }
                                }
                            }
                        } else if bindings.len() != variant_info.payload.len() {
                            ctx.errors.push(CompileError::new(
                                "karn.types.pattern_arity",
                                *pat_span,
                                format!(
                                    "variant `{}` has {} payload field(s), but the pattern has {} binding(s)",
                                    variant.name,
                                    variant_info.payload.len(),
                                    bindings.len()
                                ),
                            ));
                        } else {
                            for (b, (_, ty)) in bindings
                                .iter()
                                .zip(variant_info.payload.iter().map(|p| (&p.0, &p.1)))
                            {
                                if !b.is_wildcard() {
                                    ctx.bind(b.local_name().name.clone(), ty.clone());
                                }
                            }
                        }
                    } else {
                        ctx.errors.push(CompileError::new(
                            "karn.types.pattern_arity",
                            *pat_span,
                            format!(
                                "variant `{}` has no payload, but the pattern binds fields",
                                variant.name
                            ),
                        ));
                    }
                }
            }
        }
        let body_ty = match &arm.body {
            MatchBody::Expr(e) => maybe_auto_lift(type_of(e, expected, ctx), expected),
            MatchBody::Block(b) => type_of_block(b, expected, ctx),
        };
        ctx.pop_scope();
        if let Some(t) = body_ty {
            arm_types.push((t, arm.body.span()));
        }
    }
    // Exhaustiveness.
    if !saw_wildcard {
        for v in &expected_variants {
            if !covered.contains(&v.name) {
                ctx.errors.push(
                    CompileError::new(
                        "karn.types.non_exhaustive_match",
                        span,
                        format!(
                            "non-exhaustive `match` — variant `{}` of `{}` is not covered",
                            v.name,
                            disc_ty.display()
                        ),
                    )
                    .with_note("add a match arm for this variant, or use a wildcard `_` arm"),
                );
            }
        }
    }
    // All arm bodies must agree.
    if arm_types.is_empty() {
        return None;
    }
    let first = arm_types[0].0.clone();
    for (t, span) in arm_types.iter().skip(1) {
        if *t != first {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.match_arm_mismatch",
                    *span,
                    format!(
                        "match-arm body has type `{}`, but earlier arms have type `{}`",
                        t.display(),
                        first.display()
                    ),
                )
                .with_note("every arm of a `match` must produce the same type"),
            );
            return None;
        }
    }
    Some(first)
}

fn check_is(value: &Expr, pattern: &Pattern, _span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let value_ty = type_of(value, None, ctx)?;
    let Some(variants) = variants_of(&value_ty, &ctx.input.types) else {
        ctx.errors.push(CompileError::new(
            "karn.types.is_non_sum",
            pattern.span(),
            format!(
                "the `is` operator requires a sum, `Result`, or `Option` value, but got `{}`",
                value_ty.display()
            ),
        ));
        return Some(Ty::Base(BaseType::Bool));
    };
    match pattern {
        Pattern::Wildcard(_) => {
            // Always true; trivially typed.
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let info = variants.iter().find(|v| v.name == variant.name);
            let Some(info) = info else {
                ctx.errors.push(CompileError::new(
                    "karn.types.is_unknown_variant",
                    variant.span,
                    format!(
                        "type `{}` has no variant `{}`",
                        value_ty.display(),
                        variant.name
                    ),
                ));
                return Some(Ty::Base(BaseType::Bool));
            };
            // Just validate bindings shape; binding TYPES introduced via
            // `collect_is_bindings` are handled at the consumer site.
            if !bindings.is_empty() && info.payload.is_empty() {
                ctx.errors.push(CompileError::new(
                    "karn.types.pattern_arity",
                    pattern.span(),
                    format!(
                        "variant `{}` has no payload, but the pattern binds fields",
                        variant.name
                    ),
                ));
            } else if !bindings.is_empty() {
                let any_named = bindings
                    .iter()
                    .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
                if !any_named && bindings.len() != info.payload.len() {
                    ctx.errors.push(CompileError::new(
                        "karn.types.pattern_arity",
                        pattern.span(),
                        format!(
                            "variant `{}` has {} payload field(s), but the pattern has {} binding(s)",
                            variant.name,
                            info.payload.len(),
                            bindings.len()
                        ),
                    ));
                }
            }
        }
    }
    Some(Ty::Base(BaseType::Bool))
}

/// Collect the bindings introduced by `is` patterns inside a condition
/// expression. Currently we recognise:
///  - `expr is Pat`
///  - `lhs && rhs`        (recursive into both sides; later wins on collision)
///  - `(expr)` parens
fn collect_is_bindings(expr: &Expr, ctx: &mut Ctx) -> Vec<(String, Ty)> {
    let mut out = Vec::new();
    collect_is_bindings_into(expr, ctx, &mut out);
    out
}

fn collect_is_bindings_into(expr: &Expr, ctx: &mut Ctx, out: &mut Vec<(String, Ty)>) {
    match &expr.kind {
        ExprKind::Is { value, pattern } => {
            // Recompute value type from the expr_types side-table; this avoids
            // mutating type-checking state. If we don't have it, fall back to
            // recomputing.
            let value_ty = ctx.expr_types.get(&value.span).cloned();
            if let Some(value_ty) = value_ty {
                gather_pattern_bindings(&value_ty, pattern, &ctx.input.types, out);
            }
        }
        ExprKind::BinOp(BinOp::And, lhs, rhs) => {
            collect_is_bindings_into(lhs, ctx, out);
            collect_is_bindings_into(rhs, ctx, out);
        }
        ExprKind::Paren(inner) => collect_is_bindings_into(inner, ctx, out),
        _ => {}
    }
}

fn gather_pattern_bindings(
    value_ty: &Ty,
    pattern: &Pattern,
    types: &HashMap<String, TypeDecl>,
    out: &mut Vec<(String, Ty)>,
) {
    let Pattern::Variant {
        variant, bindings, ..
    } = pattern
    else {
        return;
    };
    let Some(variants) = variants_of(value_ty, types) else {
        return;
    };
    let Some(info) = variants.iter().find(|v| v.name == variant.name) else {
        return;
    };
    let any_named = bindings
        .iter()
        .any(|b| matches!(b.kind, PatternBindingKind::Named { .. }));
    if any_named {
        let payload_map: HashMap<&str, &Ty> =
            info.payload.iter().map(|(n, t)| (n.as_str(), t)).collect();
        for b in bindings {
            if let PatternBindingKind::Named { field, name } = &b.kind
                && let Some(ty) = payload_map.get(field.name.as_str())
                && name.name != "_"
            {
                out.push((name.name.clone(), (*ty).clone()));
            }
        }
    } else {
        for (b, (_, ty)) in bindings.iter().zip(info.payload.iter()) {
            if !b.is_wildcard() {
                out.push((b.local_name().name.clone(), ty.clone()));
            }
        }
    }
}

/// A flattened view of a type's variants (name + payload types).
struct VariantInfo {
    name: String,
    payload: Vec<(String, Ty)>,
}

/// If `receiver` resolves to a consumed-context prefix (an alias or a
/// dotted qualified name appearing in `consumes`), return the consumed
/// context's qualified name. Otherwise None. Local bindings, types, and
/// capabilities take precedence — those are checked at the call site.
fn cross_context_prefix(receiver: &Expr, ctx: &Ctx) -> Option<String> {
    let info = &ctx.input.cross_context;
    if info.consumed_contexts.is_empty() && info.aliases.is_empty() {
        return None;
    }
    // Walk the receiver to assemble a candidate dotted name. Supports:
    //   Ident(X)                                 -> "X"
    //   FieldAccess { Ident(A), B }              -> "A.B"
    //   FieldAccess { FieldAccess { Ident(A), B }, C } -> "A.B.C"
    let candidate = flatten_ident_chain(receiver)?;
    let head = candidate.split('.').next().unwrap_or("");
    // The head must not shadow a local binding / capability / declared type.
    if ctx.lookup(head).is_some() {
        return None;
    }
    if ctx.capabilities.contains_key(head) || ctx.declared_capabilities.contains_key(head) {
        return None;
    }
    // If the head is a known local type, only an alias whose name happens to
    // collide could redirect this; aliases conflicting with types are an
    // error in project.rs, so a clash here is impossible at this point.
    info.resolve_prefix(candidate.as_str())
}

/// Flatten an `Ident`/`FieldAccess` chain into its dotted name, or None if
/// any segment isn't a bare identifier.
fn flatten_ident_chain(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(id) => Some(id.name.clone()),
        ExprKind::FieldAccess { receiver, field } => {
            let head = flatten_ident_chain(receiver)?;
            Some(format!("{head}.{}", field.name))
        }
        _ => None,
    }
}

/// Type-check a cross-context service call (v0.6 §4.2). `receiver` carries
/// the prefix's source span for diagnostics. `consumed` is the resolved
/// qualified name of the consumed context.
fn check_cross_context_call(
    receiver: &Expr,
    consumed: &str,
    method: &Ident,
    args: &[Expr],
    _span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // The consuming context must be effectful at this call site (services
    // and agent handlers are; pure free fns are not).
    if !ctx.effectful {
        ctx.errors.push(
            CompileError::new(
                "karn.effect.cross_context_in_pure_context",
                method.span,
                format!(
                    "cross-context service call `{}.{}` can only be made inside an effectful body (one returning `Effect[T]`)",
                    consumed, method.name
                ),
            )
            .with_label(receiver.span, "consumed context prefix"),
        );
    }
    let info = &ctx.input.cross_context;
    let Some(svcs) = info.consumed_services.get(consumed) else {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.unknown_context",
                receiver.span,
                format!("context `{consumed}` is not in scope here"),
            )
            .with_note(
                "add a `consumes` clause for the target context at the top of the consuming context",
            ),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    let Some(service) = svcs.get(&method.name).cloned() else {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.unknown_service",
                method.span,
                format!(
                    "context `{consumed}` has no service named `{}`",
                    method.name
                ),
            )
            .with_note(
                "cross-context calls require an `on call` service handler in the consumed context",
            ),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };

    if service.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "karn.consumes.service_arity",
                method.span,
                format!(
                    "cross-context service `{consumed}.{}` expects {} argument(s), but {} were given",
                    method.name,
                    service.params.len(),
                    args.len()
                ),
            )
            .with_label(service.span, "service declared here"),
        );
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }

    // Resolve the consumed-context types so we can describe parameter shapes.
    let consumed_types = info
        .consumed_types
        .get(consumed)
        .cloned()
        .unwrap_or_default();

    // Walk each argument, checking structural compatibility (Phase 4).
    let mut all_ok = true;
    for (i, ((pname, ptype_ref), arg)) in service.params.iter().zip(args.iter()).enumerate() {
        let param_ty = resolve_type_ref(ptype_ref, &consumed_types).unwrap_or(Ty::Unit);
        // Type-check the argument in the caller's context.
        let arg_ty = type_of(arg, None, ctx);
        let Some(arg_ty) = arg_ty else {
            all_ok = false;
            continue;
        };
        if !structurally_compatible(&arg_ty, &param_ty, &ctx.input.types, &consumed_types) {
            ctx.errors.push(
                CompileError::new(
                    "karn.boundary.structural_mismatch",
                    arg.span,
                    format!(
                        "cross-context argument {} to `{consumed}.{}` has type `{}` in `{}`, but parameter `{pname}` expects `{}` in `{}`",
                        i + 1,
                        method.name,
                        arg_ty.display(),
                        ctx.input
                            .cross_context
                            .self_context
                            .as_deref()
                            .unwrap_or("?"),
                        param_ty.display(),
                        consumed,
                    ),
                )
                .with_label(service.span, "service declared here")
                .with_note(
                    "values crossing a context boundary must have structurally compatible types (same commons-derived type, or identical record/sum shape)",
                ),
            );
            all_ok = false;
        }
    }
    if !all_ok {
        return None;
    }

    // Return type rebrand: project the consumed context's return type into
    // the calling context's namespace by renaming named types whose unqualified
    // name appears in the caller's type table (v0.6 §4.5).
    let raw_ret = resolve_type_ref(&service.return_type, &consumed_types).unwrap_or(Ty::Unit);
    let rebranded = rebrand_return_type(&raw_ret, &ctx.input.types);
    Some(rebranded)
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
        Ty::Base(_) | Ty::ValidationError | Ty::Unit => t.clone(),
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
        (PredKind::InRange(a1, a2), PredKind::InRange(b1, b2)) => a1 == b1 && a2 == b2,
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
