//! Type checker and refinement validator (spec §§5.2–5.4, §6).
//!
//! Operates on a [`ResolvedCommons`]. Walks declarations, validates each
//! refinement against the spec's predicate-base compatibility and combination
//! rules, then type-checks every function body. The output of this pass is
//! a [`TypedCommons`], carrying the AST plus a per-expression type table the
//! emitter consumes.

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
}

impl Ty {
    pub fn base(&self) -> BaseType {
        match self {
            Ty::Base(b) => *b,
            Ty::Named { base, .. } => *base,
        }
    }

    /// Display name for diagnostics.
    pub fn display(&self) -> String {
        match self {
            Ty::Base(b) => b.name().to_string(),
            Ty::Named { name, .. } => name.clone(),
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

fn check_fn(
    f: &FnDecl,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) {
    // Resolve parameter types.
    let mut env: HashMap<&str, Ty> = HashMap::new();
    for p in &f.params {
        let ty = match resolve_type_ref(&p.type_ref, &input.types) {
            Some(t) => t,
            None => continue,
        };
        env.insert(p.name.name.as_str(), ty);
    }
    let return_ty = match resolve_type_ref(&f.return_type, &input.types) {
        Some(t) => t,
        None => return,
    };

    let Some(body_ty) = type_of(&f.body, &env, input, expr_types, errors) else {
        return;
    };

    if !compatible(&body_ty, &return_ty) {
        errors.push(
            CompileError::new(
                "karn.types.return_mismatch",
                f.body.span,
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
    }
}

fn type_of(
    expr: &Expr,
    env: &HashMap<&str, Ty>,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) -> Option<Ty> {
    let ty = match &expr.kind {
        ExprKind::IntLit(_) => Some(Ty::Base(BaseType::Int)),
        ExprKind::StrLit(_) => Some(Ty::Base(BaseType::String)),
        ExprKind::BoolLit(_) => Some(Ty::Base(BaseType::Bool)),
        ExprKind::Ident(id) => env.get(id.name.as_str()).cloned(),
        ExprKind::Paren(inner) => type_of(inner, env, input, expr_types, errors),
        ExprKind::Call(name, args) => check_call(name, args, env, input, expr_types, errors),
        ExprKind::UnaryOp(op, inner) => {
            check_unary(*op, inner, expr.span, env, input, expr_types, errors)
        }
        ExprKind::BinOp(op, lhs, rhs) => check_binop(*op, lhs, rhs, env, input, expr_types, errors),
    };
    if let Some(ty) = &ty {
        expr_types.insert(expr.span, ty.clone());
    }
    ty
}

fn check_unary(
    op: UnaryOp,
    inner: &Expr,
    op_span: Span,
    env: &HashMap<&str, Ty>,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) -> Option<Ty> {
    let t = type_of(inner, env, input, expr_types, errors)?;
    match op {
        UnaryOp::Neg => {
            if t.base() == BaseType::Int {
                Some(Ty::Base(BaseType::Int))
            } else {
                errors.push(CompileError::new(
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
            if t.base() == BaseType::Bool {
                Some(Ty::Base(BaseType::Bool))
            } else {
                errors.push(CompileError::new(
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

fn check_binop(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    env: &HashMap<&str, Ty>,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) -> Option<Ty> {
    let lt = type_of(lhs, env, input, expr_types, errors);
    let rt = type_of(rhs, env, input, expr_types, errors);
    let (lt, rt) = (lt?, rt?);
    let span = lhs.span.merge(rhs.span);

    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
            if lt.base() != BaseType::Int {
                errors.push(CompileError::new(
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
            if rt.base() != BaseType::Int {
                errors.push(CompileError::new(
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
            if lt.base() != rt.base() {
                errors.push(CompileError::new(
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
            if !matches!(lt.base(), BaseType::Int | BaseType::String) {
                errors.push(CompileError::new(
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
            if lt.base() != rt.base() {
                errors.push(CompileError::new(
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
            if lt.base() != BaseType::Bool {
                errors.push(CompileError::new(
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
            if rt.base() != BaseType::Bool {
                errors.push(CompileError::new(
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

fn check_call(
    name: &Ident,
    args: &[Expr],
    env: &HashMap<&str, Ty>,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
) -> Option<Ty> {
    let fn_decl = input.fns.get(&name.name)?;

    if fn_decl.params.len() != args.len() {
        // Already reported in resolver; just bail.
        for a in args {
            let _ = type_of(a, env, input, expr_types, errors);
        }
        return None;
    }

    let mut ok = true;
    for (i, (param, arg)) in fn_decl.params.iter().zip(args.iter()).enumerate() {
        let Some(arg_ty) = type_of(arg, env, input, expr_types, errors) else {
            ok = false;
            continue;
        };
        let Some(param_ty) = resolve_type_ref(&param.type_ref, &input.types) else {
            ok = false;
            continue;
        };
        if !compatible(&arg_ty, &param_ty) {
            errors.push(
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

    resolve_type_ref(&fn_decl.return_type, &input.types)
}
