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
    /// `ValidationError` — built-in error type.
    ValidationError,
}

/// The shape of a named type — what its declaration looks like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedKind {
    /// Refined-base type: widens to the recorded base.
    Refined(BaseType),
    /// Record type.
    Record,
    /// Sum type.
    Sum,
}

impl Ty {
    /// Display name for diagnostics.
    pub fn display(&self) -> String {
        match self {
            Ty::Base(b) => b.name().to_string(),
            Ty::Named { name, .. } => name.clone(),
            Ty::Result(t, e) => format!("Result[{}, {}]", t.display(), e.display()),
            Ty::Option(t) => format!("Option[{}]", t.display()),
            Ty::ValidationError => "ValidationError".to_string(),
        }
    }

    /// The underlying base type, if this type widens to a base type.
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
struct Ctx<'a> {
    input: &'a ResolvedCommons,
    expr_types: &'a mut HashMap<Span, Ty>,
    errors: &'a mut Vec<CompileError>,
    /// Stack of in-scope name → type frames.
    scopes: Vec<HashMap<String, Ty>>,
    return_ty: Ty,
    return_ty_span: Span,
}

impl<'a> Ctx<'a> {
    fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    fn bind(&mut self, name: String, ty: Ty) {
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
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span: f.return_type.span(),
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
fn type_from_decl(id: &Ident, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    let decl = types.get(&id.name)?;
    Some(named_ty(decl))
}

/// Build a `Ty::Named` for the given declaration.
fn named_ty(decl: &TypeDecl) -> Ty {
    let kind = match &decl.body {
        TypeBody::Refined { base, .. } => NamedKind::Refined(*base),
        TypeBody::Record(_) => NamedKind::Record,
        TypeBody::Sum(_) => NamedKind::Sum,
    };
    Ty::Named {
        name: decl.name.name.clone(),
        kind,
    }
}

fn resolve_type_ref(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
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
        TypeRef::ValidationError(_) => Some(Ty::ValidationError),
    }
}

/// `t` is usable where `u` is expected.
fn compatible(t: &Ty, u: &Ty) -> bool {
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
        (Ty::ValidationError, Ty::ValidationError) => true,
        _ => false,
    }
}

fn type_of_block(block: &Block, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
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
                ctx.bind(l.name.name.clone(), final_ty);
            }
        }
    }
    let ty = type_of(&block.tail, expected, ctx);
    if let Some(ty) = &ty {
        ctx.expr_types.insert(block.span, ty.clone());
    }
    ctx.pop_scope();
    ty
}

fn type_of(expr: &Expr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let ty = match &expr.kind {
        ExprKind::IntLit(_) => Some(Ty::Base(BaseType::Int)),
        ExprKind::StrLit(_) => Some(Ty::Base(BaseType::String)),
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
        ExprKind::Ident(id) => check_ident(id, ctx),
        ExprKind::Paren(inner) => type_of(inner, expected, ctx),
        ExprKind::Call(name, args) => check_call(name, args, expr.span, ctx),
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
        } => check_static_call(type_name, method, args, expr.span, ctx),
        ExprKind::RecordConstruction { type_name, fields } => {
            check_record_construction(type_name, fields, expr.span, ctx)
        }
        ExprKind::FieldAccess { receiver, field } => check_field_access(receiver, field, ctx),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => check_method_call(receiver, method, args, expr.span, ctx),
        ExprKind::Match { discriminant, arms } => {
            check_match(discriminant, arms, expr.span, expected, ctx)
        }
        ExprKind::Is { value, pattern } => check_is(value, pattern, expr.span, ctx),
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
    let surrounding = surrounding_result(expected, &ctx.return_ty);
    let expected_t = surrounding.as_ref().map(|(t, _)| t.clone());
    let inner_ty = type_of(inner, expected_t.as_ref(), ctx)?;
    match surrounding {
        Some((t_ty, e_ty)) => {
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
        None => {
            ctx.errors.push(
                CompileError::new(
                    "karn.types.cannot_infer_result_type_params",
                    span,
                    "cannot infer the error type parameter of `Ok(...)`",
                )
                .with_note(
                    "add a `let` annotation (`let x: Result[T, E] = Ok(...)`) \
                     or declare the enclosing function's return type as `Result[T, E]`",
                ),
            );
            None
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
    let expected_inner = match expected {
        Some(Ty::Option(t)) => Some((**t).clone()),
        _ => match &ctx.return_ty {
            Ty::Option(t) => Some((**t).clone()),
            _ => None,
        },
    };
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
    if let Some(Ty::Option(t)) = expected {
        return Some(Ty::Option(t.clone()));
    }
    if let Ty::Option(t) = &ctx.return_ty {
        return Some(Ty::Option(t.clone()));
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
    if let Some(Ty::Result(t, e)) = expected {
        return Some(((**t).clone(), (**e).clone()));
    }
    if let Ty::Result(t, e) = return_ty {
        return Some(((**t).clone(), (**e).clone()));
    }
    None
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
    let Ty::Result(_ret_t, ret_e) = &ctx.return_ty else {
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

    // 2) Built-in `of` constructor on refined types.
    if method.name == "of"
        && let TypeBody::Refined { base, .. } = &decl.body
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
        let expected = Ty::Base(*base);
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

fn check_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let decl = ctx.input.types.get(&type_name.name)?.clone();
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
            MatchBody::Expr(e) => type_of(e, expected, ctx),
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
        _ => None,
    }
}
