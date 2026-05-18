//! Type checker and refinement validator (spec §§5.2–5.4, §6; v0.1 §4.2).
//!
//! Operates on a [`ResolvedCommons`]. Walks declarations, validates each
//! refinement against the spec's predicate-base compatibility and combination
//! rules, then type-checks every function body.
//!
//! v0.1 extensions:
//! - The built-in generic `Result[T, E]` type and the `ValidationError` type.
//! - Let bindings (with optional annotation; type inferred from the RHS).
//! - `if`/`else` expressions whose branches must have a common type.
//! - The `?` postfix operator for `Result.Err` propagation.
//! - `Ok(v)` / `Err(v)` constructor expressions with bidirectional inference.
//! - Qualified constructor calls (`TypeName.of(value)`).
//!
//! The output of this pass is a [`TypedCommons`], carrying the AST plus a
//! per-expression type table the emitter consumes.

use std::collections::HashMap;

use regex::Regex;

use crate::ast::*;
use crate::error::CompileError;
use crate::resolver::ResolvedCommons;
use crate::span::Span;

/// A resolved type — what an identifier or expression denotes after checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A base type (`Int`, `String`, `Bool`).
    Base(BaseType),
    /// A named refined type. The base is recorded for widening checks.
    Named { name: String, base: BaseType },
    /// `Result[T, E]` — built-in generic Result type (v0.1).
    Result(Box<Ty>, Box<Ty>),
    /// `ValidationError` — built-in error type (v0.1).
    ValidationError,
}

impl Ty {
    /// Display name for diagnostics.
    pub fn display(&self) -> String {
        match self {
            Ty::Base(b) => b.name().to_string(),
            Ty::Named { name, .. } => name.clone(),
            Ty::Result(t, e) => format!("Result[{}, {}]", t.display(), e.display()),
            Ty::ValidationError => "ValidationError".to_string(),
        }
    }

    /// The underlying base type, if this type widens to a base type.
    /// Only meaningful for `Base` and `Named` variants.
    fn base(&self) -> Option<BaseType> {
        match self {
            Ty::Base(b) => Some(*b),
            Ty::Named { base, .. } => Some(*base),
            _ => None,
        }
    }
}

/// Output of type checking. The expression-type map is keyed by the
/// expression's span (which uniquely identifies a node within a single
/// compilation).
pub struct TypedCommons {
    pub commons: Commons,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub expr_types: HashMap<Span, Ty>,
}

pub fn check(input: ResolvedCommons) -> Result<TypedCommons, Vec<CompileError>> {
    let mut errors = Vec::new();
    let mut expr_types: HashMap<Span, Ty> = HashMap::new();

    // 1. Validate each type declaration's refinement.
    for item in &input.commons.items {
        if let CommonsItem::Type(t) = item {
            check_type_decl(t, &mut errors);
        }
    }

    // 2. Type-check each function body.
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
            expr_types,
        })
    } else {
        Err(errors)
    }
}

fn check_type_decl(t: &TypeDecl, errors: &mut Vec<CompileError>) {
    let Some(refinement) = &t.refinement else {
        return;
    };

    // Per-predicate well-formedness and base compatibility.
    for pred in &refinement.predicates {
        // Predicate-base compatibility.
        if !pred_applies_to(&pred.kind, t.base) {
            errors.push(
                CompileError::new(
                    "karn.types.predicate_base_mismatch",
                    pred.span,
                    format!(
                        "predicate `{}` cannot be applied to base type `{}`",
                        pred.kind.name(),
                        t.base.name()
                    ),
                )
                .with_label(
                    t.base_span,
                    format!("base type `{}` declared here", t.base.name()),
                )
                .with_note(predicate_base_help(pred.kind.name())),
            );
        }

        // Per-predicate argument validity.
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
                            format!("`InRange({lo}, {hi})` has its bounds inverted (`min` must be ≤ `max`)"),
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

    // Combination consistency (only meaningful if base compatibility passed for all preds).
    let all_compatible = refinement
        .predicates
        .iter()
        .all(|p| pred_applies_to(&p.kind, t.base));
    if !all_compatible {
        return;
    }

    match t.base {
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
    // Compute the effective inclusive range.
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
                        errors.push(
                            CompileError::new(
                                "karn.types.empty_refinement",
                                refinement.span,
                                format!(
                                    "conflicting exact lengths: `Length({prev})` and `Length({n})` cannot both hold"
                                ),
                            ),
                        );
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

/// Mutable per-function context threaded through expression type-checking.
struct Ctx<'a> {
    input: &'a ResolvedCommons,
    expr_types: &'a mut HashMap<Span, Ty>,
    errors: &'a mut Vec<CompileError>,
    /// Stack of in-scope name → type frames. The innermost frame is the
    /// current block; deeper frames cover enclosing blocks; the bottom
    /// frame holds the function parameters.
    scopes: Vec<HashMap<String, Ty>>,
    /// The enclosing function's declared return type. Used to infer Ok/Err
    /// type parameters and to validate `?`.
    return_ty: Ty,
    /// Source span for the return-type annotation (for diagnostic labels).
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

fn resolve_type_ref(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> Option<Ty> {
    match r {
        TypeRef::Base(b, _) => Some(Ty::Base(*b)),
        TypeRef::Named(id) => {
            let decl = types.get(&id.name)?;
            Some(Ty::Named {
                name: id.name.clone(),
                base: decl.base,
            })
        }
        TypeRef::Result(t, e, _) => {
            let t = resolve_type_ref(t, types)?;
            let e = resolve_type_ref(e, types)?;
            Some(Ty::Result(Box::new(t), Box::new(e)))
        }
        TypeRef::ValidationError(_) => Some(Ty::ValidationError),
    }
}

/// `t` is usable where `u` is expected.
fn compatible(t: &Ty, u: &Ty) -> bool {
    match (t, u) {
        (Ty::Base(a), Ty::Base(b)) => a == b,
        (Ty::Named { name: a, .. }, Ty::Named { name: b, .. }) => a == b,
        // Refined → base (widening).
        (Ty::Named { base, .. }, Ty::Base(b)) => base == b,
        // Base → refined is rejected (no narrowing).
        (Ty::Base(_), Ty::Named { .. }) => false,
        (Ty::Result(t1, e1), Ty::Result(t2, e2)) => compatible(t1, t2) && compatible(e1, e2),
        (Ty::ValidationError, Ty::ValidationError) => true,
        _ => false,
    }
}

/// Type-check a block. The block's type is the type of its tail expression.
/// `expected` flows into the tail expression for bidirectional inference.
fn type_of_block(block: &Block, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    ctx.scopes.push(HashMap::new());
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
                ctx.scopes
                    .last_mut()
                    .unwrap()
                    .insert(l.name.name.clone(), final_ty);
            }
        }
    }
    let ty = type_of(&block.tail, expected, ctx);
    if let Some(ty) = &ty {
        ctx.expr_types.insert(block.span, ty.clone());
    }
    ctx.scopes.pop();
    ty
}

fn type_of(expr: &Expr, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    let ty = match &expr.kind {
        ExprKind::IntLit(_) => Some(Ty::Base(BaseType::Int)),
        ExprKind::StrLit(_) => Some(Ty::Base(BaseType::String)),
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
        ExprKind::Ident(id) => ctx.lookup(id.name.as_str()),
        ExprKind::Paren(inner) => type_of(inner, expected, ctx),
        ExprKind::Call(name, args) => check_call(name, args, ctx),
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
        ExprKind::Question(inner) => check_question(inner, expr.span, ctx),
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => check_constructor_call(type_name, method, args, expr.span, ctx),
    };
    if let Some(ty) = &ty {
        ctx.expr_types.insert(expr.span, ty.clone());
    }
    ty
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
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::And | BinOp::Or => {
            if lt_base != Some(BaseType::Bool) {
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
            if rt_base != Some(BaseType::Bool) {
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

fn check_call(name: &Ident, args: &[Expr], ctx: &mut Ctx) -> Option<Ty> {
    let fn_decl = ctx.input.fns.get(&name.name)?.clone();

    if fn_decl.params.len() != args.len() {
        // Already reported in resolver; still walk args to surface their errors.
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }

    let mut ok = true;
    let resolved_params: Vec<(Option<Ty>, &Param)> = fn_decl
        .params
        .iter()
        .map(|p| (resolve_type_ref(&p.type_ref, &ctx.input.types), p))
        .collect();
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
    let then_ty = type_of_block(then_block, expected, ctx);
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
    // Identify the surrounding Result[T, E]: prefer the explicit expected
    // type, otherwise fall back to the enclosing function's return type.
    let surrounding = surrounding_result(expected, &ctx.return_ty);
    let expected_t = surrounding.as_ref().map(|(t, _)| t.clone());
    let inner_ty = type_of(inner, expected_t.as_ref(), ctx)?;
    match surrounding {
        Some((t_ty, e_ty)) => {
            // Check the inner value matches the expected T.
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
                    "add a `let` annotation (`let x: Result[T, E] = Err(...)`) \
                     or declare the enclosing function's return type as `Result[T, E]`",
                ),
            );
            None
        }
    }
}

/// Choose the surrounding `Result[T, E]` that bounds an `Ok`/`Err` expression.
/// Prefers an explicit `expected` over the function's return type.
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
            .with_label(span, "this `?` requires a Result")
            .with_note("`?` can only be applied to `Result` values"),
        );
        return None;
    };
    // Enclosing function must return Result[U, E] with matching error type.
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
            )
            .with_note("change the function's return type to `Result[U, E]`"),
        );
        return None;
    };
    if !compatible(e, ret_e) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.question_error_mismatch",
                span,
                format!(
                    "the `?` operator propagates an error of type `{}`, but the enclosing function returns `Result[_, {}]`",
                    e.display(),
                    ret_e.display()
                ),
            )
            .with_label(
                ctx.return_ty_span,
                format!("function returns `{}`", ctx.return_ty.display()),
            )
            .with_note("the inner error type must match the enclosing function's error type"),
        );
        return None;
    }
    Some((**t).clone())
}

fn check_constructor_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Resolver already validated that the type name resolves and the
    // method name is `of`. Be defensive in case earlier errors got past.
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    if method.name != "of" {
        return None;
    }
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
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let arg = &args[0];
    let expected = Ty::Base(decl.base);
    let arg_ty = type_of(arg, Some(&expected), ctx)?;
    if !compatible(&arg_ty, &expected) {
        ctx.errors.push(
            CompileError::new(
                "karn.types.constructor_base_mismatch",
                arg.span,
                format!(
                    "constructor `{}.of` expects a `{}` argument, but got `{}`",
                    type_name.name,
                    decl.base.name(),
                    arg_ty.display()
                ),
            )
            .with_label(decl.base_span, "type's base declared here"),
        );
        return None;
    }
    Some(Ty::Result(
        Box::new(Ty::Named {
            name: type_name.name.clone(),
            base: decl.base,
        }),
        Box::new(Ty::ValidationError),
    ))
}
