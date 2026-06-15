//! Refinement, literal, and zero-value logic.
//!
//! Split out of `checker.rs` (v0.29.10) verbatim; the parent module
//! re-exports these via `use refinements::*`.

use super::*;

pub(crate) fn check_type_decl(
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
pub(crate) fn type_decl_base(decl: &TypeDecl) -> Option<BaseType> {
    match &decl.body {
        TypeBody::Refined { base, .. } => Some(*base),
        TypeBody::Opaque { base, .. } => Some(*base),
        _ => None,
    }
}

/// The refinement attached to a refined or opaque type declaration, if any.
pub(crate) fn type_decl_refinement(decl: &TypeDecl) -> Option<&Refinement> {
    match &decl.body {
        TypeBody::Refined { refinement, .. } | TypeBody::Opaque { refinement, .. } => {
            refinement.as_ref()
        }
        _ => None,
    }
}

/// Extract a compile-time literal from an expression, if it is one v0.9.4's
/// static refinement check accepts: an int/string/bool/unit literal, or a unary
/// minus applied directly to an int literal. Anything else (arithmetic, idents,
/// calls) is not statically evaluated and keeps the runtime `Result` path.
pub(crate) fn const_literal(e: &Expr) -> Option<ConstLit> {
    match &e.kind {
        ExprKind::IntLit(n) => Some(ConstLit::Int(*n)),
        ExprKind::FloatLit { value, .. } => Some(ConstLit::Float(*value)),
        ExprKind::StrLit(s) => Some(ConstLit::Str(s.clone())),
        ExprKind::BoolLit(b) => Some(ConstLit::Bool(*b)),
        ExprKind::UnitLit => Some(ConstLit::Unit),
        ExprKind::UnaryOp(UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::IntLit(n) => Some(ConstLit::Int(n.checked_neg()?)),
            ExprKind::FloatLit { value, .. } => Some(ConstLit::Float(-*value)),
            _ => None,
        },
        _ => None,
    }
}

/// Evaluate a single predicate against a constant literal. A predicate whose
/// expected base type doesn't match the literal (e.g. a length predicate on an
/// int) returns `true` here — the base/predicate mismatch is a declaration-time
/// error reported by `check_refinement`, not a construction concern. String
/// length is measured in Unicode scalar values, which agrees with JS `.length`
/// for the BMP (the range fixtures use ASCII).
pub(crate) fn eval_predicate(pred: &PredKind, lit: &ConstLit) -> bool {
    match (pred, lit) {
        (PredKind::NonNegative, ConstLit::Int(n)) => *n >= 0,
        (PredKind::Positive, ConstLit::Int(n)) => *n > 0,
        (PredKind::InRange(lo, hi), ConstLit::Int(n)) => lo.value <= *n && *n <= hi.value,
        (PredKind::NonNegative, ConstLit::Float(v)) => *v >= 0.0,
        (PredKind::Positive, ConstLit::Float(v)) => *v > 0.0,
        (PredKind::InRangeF(lo, hi), ConstLit::Float(v)) => lo.value <= *v && *v <= hi.value,
        (PredKind::MinLength(k), ConstLit::Str(s)) => s.chars().count() as i64 >= *k,
        (PredKind::MaxLength(k), ConstLit::Str(s)) => (s.chars().count() as i64) <= *k,
        (PredKind::Length(k), ConstLit::Str(s)) => s.chars().count() as i64 == *k,
        (PredKind::NonEmpty, ConstLit::Str(s)) => !s.is_empty(),
        (PredKind::Matches(pat), ConstLit::Str(s)) => Regex::new(&format!("^(?:{pat})$"))
            .map(|re| re.is_match(s))
            .unwrap_or(false),
        _ => true,
    }
}

/// The first predicate the literal fails, or `None` if it satisfies them all.
pub(crate) fn first_failed_predicate<'a>(
    refinement: &'a Refinement,
    lit: &ConstLit,
) -> Option<&'a PredKind> {
    for p in &refinement.predicates {
        if !eval_predicate(&p.kind, lit) {
            return Some(&p.kind);
        }
    }
    None
}

pub(crate) fn literal_matches_base(lit: &ConstLit, base: BaseType) -> bool {
    matches!(
        (lit, base),
        (ConstLit::Int(_), BaseType::Int)
            | (ConstLit::Str(_), BaseType::String)
            | (ConstLit::Bool(_), BaseType::Bool)
            | (ConstLit::Float(_), BaseType::Float)
    )
}

/// v0.9.4: expected-type-directed literal admission. When a position expects a
/// **refined** type `T` and `expr` is a compile-time literal of `T`'s base, the
/// literal takes the type `T` directly (the emitter lowers it to
/// `T.unsafe(...)`); a literal that violates the refinement is a compile error.
/// Returns `None` when no refined type is expected (so the caller keeps the
/// literal's base type) — `.of` remains the only constructor for runtime values.
/// Opaque types are intentionally excluded: their representation is hidden, so
/// they are still built via `T.of(...)`.
pub(crate) fn admit_refined_literal(
    expr: &Expr,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let Some(Ty::Named {
        name,
        kind: NamedKind::Refined(base),
    }) = expected
    else {
        return None;
    };
    let lit = const_literal(expr)?;
    if !literal_matches_base(&lit, *base) {
        return None;
    }
    let decl = ctx.input.types.get(name)?.clone();
    if let Some(refinement) = type_decl_refinement(&decl)
        && let Some(failed) = first_failed_predicate(refinement, &lit)
    {
        ctx.errors.push(CompileError::new(
            "karn.refine.literal_violates",
            expr.span,
            format!(
                "literal {} does not satisfy `{}` required by type `{}`",
                lit.display(),
                failed.name(),
                name
            ),
        ));
    }
    Some(named_ty(&decl))
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
            // v0.21: `InRange` bounds must match the numeric base type —
            // `Float where InRange(0, 1)` is the no-coercion rule applied
            // to refinement bounds, not a predicate/base mismatch.
            let numeric_bound_mismatch = matches!(
                (&pred.kind, base),
                (PredKind::InRange(_, _), BaseType::Float)
                    | (PredKind::InRangeF(_, _), BaseType::Int)
            );
            if numeric_bound_mismatch {
                let (bounds, want) = if base == BaseType::Float {
                    ("`Int`", "`InRange(0.0, 1.0)`")
                } else {
                    ("`Float`", "`InRange(0, 1)`")
                };
                errors.push(
                    CompileError::new(
                        "karn.types.no_numeric_coercion",
                        pred.span,
                        format!(
                            "`InRange` bounds are {bounds} literals, but the base type is `{}`",
                            base.name()
                        ),
                    )
                    .with_label(
                        base_span,
                        format!("base type `{}` declared here", base.name()),
                    )
                    .with_note(format!(
                        "refinement bounds must match the base type — e.g. {want}"
                    )),
                );
                continue;
            }
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
                if lo.value > hi.value {
                    errors.push(
                        CompileError::new(
                            "karn.types.inverted_range",
                            pred.span,
                            format!(
                                "`InRange({}, {})` has its bounds inverted (`min` must be ≤ `max`)",
                                lo.value, hi.value
                            ),
                        )
                        .with_note("swap the arguments, e.g. `InRange(min, max)`")
                        // v0.40 (ADR 0073): a machine-applicable swap — replace
                        // each bound's text with the other's, in place.
                        .with_suggestion(
                            "swap the bounds",
                            vec![
                                (lo.span, hi.value.to_string()),
                                (hi.span, lo.value.to_string()),
                            ],
                            Applicability::MachineApplicable,
                        ),
                    );
                }
            }
            PredKind::InRangeF(lo, hi) => {
                if lo.value > hi.value {
                    errors.push(
                        CompileError::new(
                            "karn.types.inverted_range",
                            pred.span,
                            format!(
                                "`InRange({}, {})` has its bounds inverted (`min` must be ≤ `max`)",
                                lo.lexeme, hi.lexeme
                            ),
                        )
                        .with_note("swap the arguments, e.g. `InRange(min, max)`")
                        .with_suggestion(
                            "swap the bounds",
                            vec![(lo.span, hi.lexeme.clone()), (hi.span, lo.lexeme.clone())],
                            Applicability::MachineApplicable,
                        ),
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
        BaseType::Float => check_float_refinement_consistency(refinement, errors),
    }
}

fn pred_applies_to(pred: &PredKind, base: BaseType) -> bool {
    matches!(
        (pred, base),
        (PredKind::Matches(_), BaseType::String)
            | (PredKind::InRange(_, _), BaseType::Int)
            | (PredKind::InRangeF(_, _), BaseType::Float)
            | (PredKind::MinLength(_), BaseType::String)
            | (PredKind::MaxLength(_), BaseType::String)
            | (PredKind::Length(_), BaseType::String)
            | (PredKind::NonNegative, BaseType::Int | BaseType::Float)
            | (PredKind::Positive, BaseType::Int | BaseType::Float)
            | (PredKind::NonEmpty, BaseType::String)
    )
}

fn predicate_base_help(name: &str) -> &'static str {
    match name {
        "Matches" | "MinLength" | "MaxLength" | "Length" | "NonEmpty" => {
            "this predicate applies to `String` only"
        }
        "NonNegative" | "Positive" => "this predicate applies to `Int` and `Float` only",
        "InRange" => {
            "this predicate applies to `Int` and `Float` only, with bounds matching the base"
        }
        _ => "see the documentation for valid predicate-base combinations",
    }
}

pub(crate) fn check_int_refinement_consistency(
    refinement: &Refinement,
    errors: &mut Vec<CompileError>,
) {
    let mut lo: i64 = i64::MIN;
    let mut hi: i64 = i64::MAX;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::Positive => lo = lo.max(1),
            PredKind::NonNegative => lo = lo.max(0),
            PredKind::InRange(a, b) => {
                lo = lo.max(a.value);
                hi = hi.min(b.value);
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

pub(crate) fn check_float_refinement_consistency(
    refinement: &Refinement,
    errors: &mut Vec<CompileError>,
) {
    let mut lo = f64::NEG_INFINITY;
    let mut hi = f64::INFINITY;
    // `Positive` excludes the lower endpoint (0.0 itself is not positive).
    let mut lo_exclusive = false;
    for p in &refinement.predicates {
        match &p.kind {
            PredKind::Positive if 0.0 >= lo => {
                lo = 0.0;
                lo_exclusive = true;
            }
            PredKind::NonNegative if 0.0 > lo => {
                lo = 0.0;
                lo_exclusive = false;
            }
            PredKind::InRangeF(a, b) => {
                if a.value > lo {
                    lo = a.value;
                    lo_exclusive = false;
                }
                hi = hi.min(b.value);
            }
            _ => {}
        }
    }
    if lo > hi || (lo == hi && lo_exclusive) {
        errors.push(
            CompileError::new(
                "karn.types.empty_refinement",
                refinement.span,
                "this refinement has no valid values — the predicates contradict each other",
            )
            .with_note(format!(
                "the effective range is `{lo}..={hi}`{}, which is empty",
                if lo_exclusive {
                    " (lower bound exclusive)"
                } else {
                    ""
                }
            )),
        );
    }
}

pub(crate) fn check_string_refinement_consistency(
    refinement: &Refinement,
    errors: &mut Vec<CompileError>,
) {
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

/// v0.9.1: `assert e` as an expression. Test-privileged. Requires `e : Bool`.
/// Always yields type `()`.
/// True if a refinement cannot be satisfied by a generated default value — i.e.
/// it contains a `Matches` predicate, where bare `Mock[T]` must be given an
/// explicit pin instead.
pub(crate) fn refinement_needs_pin(refinement: &Refinement) -> bool {
    refinement
        .predicates
        .iter()
        .any(|p| matches!(p.kind, PredKind::Matches(_)))
}

/// The TypeScript zero-value expression for `type_ref` (with an optional
/// inline field refinement), or `None` if the type is not zeroable.
pub fn zero_value_ts(
    type_ref: &TypeRef,
    inline: Option<&Refinement>,
    types: &HashMap<String, TypeDecl>,
) -> Option<String> {
    match type_ref {
        TypeRef::Base(b, _) => {
            if refinement_admits_zero(*b, inline) {
                zero_of_base(*b)
            } else {
                None
            }
        }
        // Option's zero is None, regardless of the inner type.
        TypeRef::Option(_, _) => Some("None".to_string()),
        TypeRef::Named(id) => {
            let decl = types.get(&id.name)?;
            match &decl.body {
                TypeBody::Refined {
                    base, refinement, ..
                } => {
                    if refinement_admits_zero(*base, refinement.as_ref()) {
                        zero_of_base(*base)
                    } else {
                        None
                    }
                }
                TypeBody::Record(rec) => agent_state_zero_record(&rec.fields, types),
                // Non-Option sum types and opaque types have no defined zero.
                TypeBody::Sum(_) | TypeBody::Opaque { .. } => None,
            }
        }
        // Result / Effect / HttpResult / ValidationError / Unit are not
        // admissible state-field types and have no zero.
        _ => None,
    }
}

/// The zero record `{ f₁: z₁, …, fₙ: zₙ }` for a set of fields, or `None` if
/// any field is not zeroable.
pub fn agent_state_zero_record(
    fields: &[RecordField],
    types: &HashMap<String, TypeDecl>,
) -> Option<String> {
    let mut parts = Vec::new();
    for f in fields {
        let z = zero_value_ts(&f.type_ref, f.refinement.as_ref(), types)?;
        parts.push(format!("{}: {}", f.name.name, z));
    }
    Some(format!("{{ {} }}", parts.join(", ")))
}

fn zero_of_base(b: BaseType) -> Option<String> {
    Some(
        match b {
            BaseType::Int => "0",
            BaseType::Bool => "false",
            BaseType::String => "\"\"",
            BaseType::Float => "0",
        }
        .to_string(),
    )
}

/// Whether the zero value of `base` satisfies every predicate in `refinement`.
/// Conservative: any predicate we cannot prove admits the zero returns false,
/// surfacing the `non_zeroable_state_field` diagnostic rather than risking an
/// invalid fresh state.
fn refinement_admits_zero(base: BaseType, refinement: Option<&Refinement>) -> bool {
    let Some(r) = refinement else {
        return true;
    };
    r.predicates.iter().all(|p| pred_admits_zero(base, &p.kind))
}

fn pred_admits_zero(base: BaseType, k: &PredKind) -> bool {
    match base {
        BaseType::Int => match k {
            PredKind::NonNegative => true,
            PredKind::Positive => false,
            PredKind::InRange(lo, hi) => lo.value <= 0 && 0 <= hi.value,
            // Length/Matches predicates don't apply to Int; reject conservatively.
            _ => false,
        },
        BaseType::String => match k {
            PredKind::Matches(p) => regex_matches_empty(p),
            PredKind::MinLength(n) => *n <= 0,
            PredKind::MaxLength(n) => *n >= 0,
            PredKind::Length(n) => *n == 0,
            PredKind::NonEmpty => false,
            // Numeric predicates don't apply to String; reject conservatively.
            _ => false,
        },
        // The only Bool zero is `false`; no Bool refinement predicates exist.
        BaseType::Bool => true,
        BaseType::Float => match k {
            PredKind::NonNegative => true,
            PredKind::Positive => false,
            PredKind::InRangeF(lo, hi) => lo.value <= 0.0 && 0.0 <= hi.value,
            // Other predicates don't apply to Float; reject conservatively.
            _ => false,
        },
    }
}

/// Does the refinement pattern match the empty string? Anchored exactly as the
/// emitted refined-type constructor anchors it (`^(?:pattern)$`).
fn regex_matches_empty(pattern: &str) -> bool {
    match Regex::new(&format!("^(?:{pattern})$")) {
        Ok(re) => re.is_match(""),
        Err(_) => false,
    }
}
