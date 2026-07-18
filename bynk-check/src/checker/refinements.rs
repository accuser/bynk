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
                            "bynk.types.field_refinement_not_base",
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
        TypeBody::Sum(s) => {
            check_embeds(&t.name.name, s, types, errors);
        }
    }
}

/// v0.154 (ADR 0178): validate a sum's declared error embeddings. Each
/// `embeds E as V` requires the named variant `V` to exist in this sum and to
/// have **exactly one payload field whose type is `E`** — that is the shape a
/// value of `E` auto-wraps into. A source type may be embedded by at most one
/// variant, so `?`'s conversion is unambiguous.
pub(crate) fn check_embeds(
    sum_name: &str,
    s: &SumBody,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    let mut seen_sources: Vec<Ty> = Vec::new();
    for clause in &s.embeds {
        let Some(source_ty) = resolve_type_ref(&clause.source_type, types) else {
            // An unresolvable type ref is already reported by reference
            // resolution; skip to avoid a duplicate error.
            continue;
        };
        // The named variant must exist in this sum.
        let Some(variant) = s
            .variants
            .iter()
            .find(|v| v.name.name == clause.variant.name)
        else {
            errors.push(CompileError::new(
                "bynk.types.embeds_unknown_variant",
                clause.variant.span,
                format!(
                    "`embeds … as {}` names no variant of `{}`",
                    clause.variant.name, sum_name
                ),
            ));
            continue;
        };
        // The variant must be a single-payload wrapper of the embedded type.
        if variant.payload.len() != 1 {
            errors.push(
                CompileError::new(
                    "bynk.types.embeds_variant_shape",
                    clause.span,
                    format!(
                        "`embeds … as {}` requires `{}` to have exactly one payload field, but it has {}",
                        clause.variant.name,
                        clause.variant.name,
                        variant.payload.len()
                    ),
                )
                .with_note("a value of the embedded type is wrapped into that single field"),
            );
            continue;
        }
        let field_ty = resolve_type_ref(&variant.payload[0].type_ref, types);
        if let Some(field_ty) = &field_ty
            && !compatible(&source_ty, field_ty)
        {
            errors.push(CompileError::new(
                "bynk.types.embeds_variant_shape",
                clause.span,
                format!(
                    "`embeds {} as {}` — but `{}`'s payload field has type `{}`, not `{}`",
                    source_ty.display(),
                    clause.variant.name,
                    clause.variant.name,
                    field_ty.display(),
                    source_ty.display()
                ),
            ));
            continue;
        }
        // The same source type may be embedded once at most (unambiguous `?`).
        if seen_sources.iter().any(|t| compatible(t, &source_ty)) {
            errors.push(CompileError::new(
                "bynk.types.embeds_ambiguous",
                clause.span,
                format!(
                    "`{}` is embedded more than once by `{}` — the conversion would be ambiguous",
                    source_ty.display(),
                    sum_name
                ),
            ));
            continue;
        }
        seen_sources.push(source_ty);
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
        ExprKind::IntLit { value: n, .. } => Some(ConstLit::Int(*n)),
        ExprKind::FloatLit { value, .. } => Some(ConstLit::Float(*value)),
        ExprKind::StrLit(s) => Some(ConstLit::Str(s.clone())),
        ExprKind::BoolLit(b) => Some(ConstLit::Bool(*b)),
        ExprKind::UnitLit => Some(ConstLit::Unit),
        ExprKind::UnaryOp(UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::IntLit { value: n, .. } => Some(ConstLit::Int(n.checked_neg()?)),
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
        (PredKind::Matches(pat), ConstLit::Str(s)) => {
            // Evaluated with the same engine semantics the emitted
            // `new RegExp(...)` runs under (ECMAScript, no flags).
            regress::Regex::new(&format!("^(?:{pat})$"))
                .map(|re| re.find(s).is_some())
                .unwrap_or(false)
        }
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
/// literal takes the type `T` directly (the emitter lowers it to an inline brand
/// cast, `(lit as T)` — ADR 0182); a literal that violates the refinement is a
/// compile error. Returns `None` when no refined type is expected (so the caller
/// keeps the literal's base type) — `.of` remains the only constructor for
/// runtime values.
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
        ..
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
            "bynk.refine.literal_violates",
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
                        "bynk.types.no_numeric_coercion",
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
                    "bynk.types.predicate_base_mismatch",
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
                // Validate with ECMAScript semantics (the `regress` engine):
                // the emitted check runs the pattern under JS `RegExp`, so a
                // pattern the Rust `regex` crate accepts but JS rejects
                // (`(?P<name>…)`, inline flags) would otherwise compile
                // cleanly and then throw at runtime — a 500 on the request
                // path instead of a compile error.
                if let Err(e) = regress::Regex::new(pat) {
                    errors.push(
                        CompileError::new(
                            "bynk.types.invalid_regex",
                            pred.span,
                            format!("invalid regular expression in `Matches(\"{pat}\")`"),
                        )
                        .with_note(format!("regex parse error (JS `RegExp` semantics): {e}")),
                    );
                } else if has_nested_unbounded_quantifier(pat) {
                    // The emitted boundary check runs this pattern under JS
                    // `RegExp`, a backtracking engine. A repeated group that
                    // itself contains an unbounded quantifier (`(a+)+`) makes
                    // matching take exponential time on a crafted near-miss
                    // input — a refined `String` on an HTTP boundary would let
                    // an unauthenticated client stall the Worker (ReDoS, #724).
                    // Reject the pattern at compile time rather than ship the
                    // hazard.
                    errors.push(
                        CompileError::new(
                            "bynk.types.catastrophic_regex",
                            pred.span,
                            format!(
                                "the pattern in `Matches(\"{pat}\")` nests unbounded quantifiers, \
                                 which can cause catastrophic backtracking (ReDoS)"
                            ),
                        )
                        .with_note(
                            "a repeated group that itself contains `*`, `+`, or `{n,}` makes \
                             matching take exponential time on crafted input; restructure the \
                             pattern so no unbounded quantifier is nested inside another",
                        ),
                    );
                }
            }
            PredKind::InRange(lo, hi) => {
                if lo.value > hi.value {
                    errors.push(
                        CompileError::new(
                            "bynk.types.inverted_range",
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
                            "bynk.types.inverted_range",
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
                        "bynk.types.negative_length",
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
        // v0.86/v0.90/v0.110: no refinement predicate applies to `Duration`,
        // `Instant`, or `Bytes` (none is in any `pred_applies_to` row), so a
        // refined one is rejected upstream and there is nothing to
        // consistency-check here.
        BaseType::Duration | BaseType::Instant | BaseType::Bytes => {}
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
                "bynk.types.empty_refinement",
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
                "bynk.types.empty_refinement",
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
                            "bynk.types.empty_refinement",
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
                "bynk.types.empty_refinement",
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
/// it contains a `Matches` predicate, where bare `Val[T]` must be given an
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
    zero_value_ts_inner(type_ref, inline, types, &mut Vec::new())
}

fn zero_value_ts_inner(
    type_ref: &TypeRef,
    inline: Option<&Refinement>,
    types: &HashMap<String, TypeDecl>,
    visiting: &mut Vec<String>,
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
                TypeBody::Record(rec) => {
                    // A record cycle (`A = { b: B }`, `B = { a: A }`) has no
                    // finite zero value; without this guard the recursion is
                    // unbounded and overflows the stack. The resolver rejects
                    // such cycles, but this walk must terminate regardless of
                    // what reaches it.
                    if visiting.iter().any(|n| n == &id.name) {
                        return None;
                    }
                    visiting.push(id.name.clone());
                    let z = agent_state_zero_record(&rec.fields, types, visiting);
                    visiting.pop();
                    z
                }
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
fn agent_state_zero_record(
    fields: &[RecordField],
    types: &HashMap<String, TypeDecl>,
    visiting: &mut Vec<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    for f in fields {
        let z = zero_value_ts_inner(&f.type_ref, f.refinement.as_ref(), types, visiting)?;
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
            // v0.86/v0.90: a `Duration` is milliseconds and an `Instant` is
            // epoch milliseconds; the zero of each is `0` (the Unix epoch).
            BaseType::Duration | BaseType::Instant => "0",
            // v0.110 (ADR 0142): the zero of `Bytes` is the empty octet
            // sequence (`""` in base64), erased to an empty `Uint8Array`.
            BaseType::Bytes => "new Uint8Array()",
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
        // No refinement predicate applies to `Duration`, `Instant`, or
        // `Bytes`, so the question is vacuous — admit it (mirrors `Bool`).
        BaseType::Duration | BaseType::Instant | BaseType::Bytes => true,
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
/// emitted refined-type constructor anchors it (`^(?:pattern)$`), and
/// evaluated with the same engine semantics (ECMAScript, no flags).
fn regex_matches_empty(pattern: &str) -> bool {
    match regress::Regex::new(&format!("^(?:{pattern})$")) {
        Ok(re) => re.find("").is_some(),
        Err(_) => false,
    }
}

/// #724 — detect one catastrophic-backtracking (ReDoS) signature: an unbounded
/// quantifier applied to a group that itself contains an unbounded quantifier
/// ("star height ≥ 2", e.g. `(a+)+`, `(a*)*b`, `((ab)+)+`). Under the JS
/// backtracking `RegExp` the emitted boundary check runs, this class takes
/// exponential time on a crafted near-miss input; the conservative structural
/// rule rejects it at compile time.
///
/// "Unbounded" means `*`, `+`, or `{n,}` (open upper bound); `?` and `{n,m}`
/// (finite) cannot explode. The scan is purely structural — the pattern is
/// already known valid (`regress` accepted it) — so it need not model match
/// semantics, only quantifier nesting through groups. Inner unbounded
/// quantifiers propagate up through *bounded* quantifiers too, so `((a+)?)+`
/// is still caught. The check is conservative in the safe direction: it can
/// reject a star-height-2 pattern whose sub-expressions provably never overlap,
/// but every *nested-quantifier* blowup is flagged.
///
/// This does **not** cover the whole exponential class. Ambiguous alternation
/// under a single quantifier — `(a|a)+`, `(\d|\d\d)+`, `(foo|foobar)+` — is
/// exponential too (two distinct paths spell the same string, so a backtracker
/// explores `2ⁿ` labelings of `aⁿ`), yet it is star height 1 and is *not*
/// flagged here. Detecting it needs branch-overlap analysis and is a deferred
/// follow-up (#724). Nor does this target the polynomial class (`\d*\d*`,
/// quadratic). The guard closes the common nested-quantifier subclass, not
/// catastrophic backtracking in general.
fn has_nested_unbounded_quantifier(pat: &str) -> bool {
    // Precondition: `pat` is a valid regex (`regress` accepted it), so its
    // parentheses are balanced. The `stack.last_mut().unwrap()` arms below rely
    // on that — a `)` never pops the root frame — which the sole caller ensures
    // by running this only after the validity check. The `)` arm keeps a
    // defensive `unwrap_or` regardless.
    let chars: Vec<char> = pat.chars().collect();
    // One boolean per open group (index 0 = top level): does this group contain
    // an unbounded quantifier anywhere within it?
    let mut stack: Vec<bool> = vec![false];
    // The atom a following quantifier would apply to: `None` if none is pending,
    // else `Some(inner_unbounded)` where `inner_unbounded` is true when that atom
    // is a group carrying an unbounded quantifier inside it.
    let mut pending: Option<bool> = None;
    let mut i = 0;

    // Fold a pending atom that turned out to be unquantified into the current
    // frame: if it was a group with inner unbounded content, that content still
    // lives in the enclosing group.
    fn fold(pending: &mut Option<bool>, stack: &mut [bool]) {
        if pending.take() == Some(true) {
            *stack.last_mut().unwrap() = true;
        }
    }

    while i < chars.len() {
        match chars[i] {
            // An escape is a single atom; skip the escaped char.
            '\\' => {
                fold(&mut pending, &mut stack);
                i += 2;
                pending = Some(false);
            }
            // A character class is one atom; `*`/`+`/`{` inside it are literal.
            '[' => {
                fold(&mut pending, &mut stack);
                i += 1;
                if i < chars.len() && chars[i] == '^' {
                    i += 1;
                }
                // A leading `]` is a literal member, not the class terminator.
                if i < chars.len() && chars[i] == ']' {
                    i += 1;
                }
                while i < chars.len() && chars[i] != ']' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1; // consume the closing `]`
                pending = Some(false);
            }
            '(' => {
                fold(&mut pending, &mut stack);
                stack.push(false);
                i += 1;
                // Skip a group-type prefix so its punctuation is not mistaken for
                // an atom: `(?:`, `(?=`, `(?!`, `(?<=`, `(?<!`, `(?<name>`.
                if i < chars.len() && chars[i] == '?' {
                    i += 1;
                    if i < chars.len() && matches!(chars[i], ':' | '=' | '!') {
                        i += 1;
                    } else if i < chars.len() && chars[i] == '<' {
                        i += 1;
                        if i < chars.len() && matches!(chars[i], '=' | '!') {
                            i += 1;
                        } else {
                            while i < chars.len() && chars[i] != '>' {
                                i += 1;
                            }
                            if i < chars.len() {
                                i += 1; // consume `>`
                            }
                        }
                    }
                }
            }
            ')' => {
                fold(&mut pending, &mut stack);
                let closed = stack.pop().unwrap_or(false);
                i += 1;
                // The whole group is now an atom in its parent frame.
                pending = Some(closed);
            }
            // Alternation ends the current atom; an unbounded one folds in.
            '|' => {
                fold(&mut pending, &mut stack);
                i += 1;
            }
            // Unbounded quantifier.
            '*' | '+' => {
                let atom_unbounded = pending.take().unwrap_or(false);
                if atom_unbounded {
                    return true; // unbounded nested inside unbounded
                }
                *stack.last_mut().unwrap() = true;
                i += 1;
                if i < chars.len() && chars[i] == '?' {
                    i += 1; // lazy marker
                }
            }
            // Optional: bounded, but inner unbounded content still propagates.
            '?' => {
                if let Some(atom_unbounded) = pending.take() {
                    if atom_unbounded {
                        *stack.last_mut().unwrap() = true;
                    }
                    i += 1;
                    if i < chars.len() && chars[i] == '?' {
                        i += 1; // lazy marker
                    }
                } else {
                    // Stray `?` (unreachable for a valid pattern) — treat as atom.
                    i += 1;
                    pending = Some(false);
                }
            }
            '{' => {
                if let Some((unbounded_q, next)) = parse_brace_quantifier(&chars, i) {
                    let atom_unbounded = pending.take().unwrap_or(false);
                    if unbounded_q && atom_unbounded {
                        return true;
                    }
                    if unbounded_q || atom_unbounded {
                        *stack.last_mut().unwrap() = true;
                    }
                    i = next;
                    if i < chars.len() && chars[i] == '?' {
                        i += 1; // lazy marker
                    }
                } else {
                    // A `{` that is not a quantifier is a literal atom.
                    fold(&mut pending, &mut stack);
                    i += 1;
                    pending = Some(false);
                }
            }
            // Any other char (`.`, literal, `^`, `$`) is an ordinary atom.
            _ => {
                fold(&mut pending, &mut stack);
                i += 1;
                pending = Some(false);
            }
        }
    }
    false
}

/// Parse a `{m}`, `{m,}`, or `{m,n}` quantifier starting at `chars[start] == '{'`.
/// Returns `(is_unbounded, index_after_close)` when it is a well-formed
/// quantifier — `is_unbounded` is true only for `{m,}` (open upper bound) — or
/// `None` when the braces are not a quantifier (then they are literal text, as
/// JS `RegExp` treats them).
fn parse_brace_quantifier(chars: &[char], start: usize) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let lo_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == lo_start {
        return None; // `{m` requires at least one digit
    }
    let mut unbounded = false;
    if i < chars.len() && chars[i] == ',' {
        i += 1;
        let hi_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == hi_start {
            unbounded = true; // `{m,}` — no upper bound
        }
    }
    if i < chars.len() && chars[i] == '}' {
        Some((unbounded, i + 1))
    } else {
        None
    }
}

#[cfg(test)]
mod redos_tests {
    use super::has_nested_unbounded_quantifier as redos;

    #[test]
    fn flags_nested_unbounded_quantifiers() {
        // The classic exponential shapes: an unbounded quantifier over a group
        // that itself repeats unboundedly.
        assert!(redos("(a+)+"));
        assert!(redos("(a+)+$"));
        assert!(redos("(a*)*"));
        assert!(redos("(a+)*"));
        assert!(redos("(a*)+"));
        assert!(redos("((ab)+)+"));
        assert!(redos("(a{1,})+")); // `{1,}` is unbounded
        assert!(redos("(a+){2,}")); // outer open bound over inner `+`
        assert!(redos("x(y(z+)+w)+")); // nested a level deep
        assert!(redos("(\\d+)+"));
        assert!(redos("(?:a+)+")); // non-capturing group
        assert!(redos("([a-z]+)*")); // class inside the repeated group
        assert!(redos("((a+)?)+")); // inner unbounded propagates through `?`
        assert!(redos("(a+|b)+")); // alternation does not launder the nesting
    }

    #[test]
    fn allows_safe_patterns() {
        // A single quantifier level is fine, however placed.
        assert!(!redos("a+"));
        assert!(!redos("(a+)")); // repeated once, not nested
        assert!(!redos("(a+)(b+)")); // siblings, not nested
        assert!(!redos("(ab)+")); // repeated group with no inner quantifier
        assert!(!redos("(a+)?")); // bounded outer quantifier
        assert!(!redos("(a+){2,3}")); // finite outer bound
        assert!(!redos("(a{2,3})+")); // finite *inner* bound cannot explode
        assert!(!redos("[a-z]+")); // `+` binds the class, not a group
        assert!(!redos("a{2,}b{2,}")); // two unbounded, neither nested
    }

    #[test]
    fn does_not_flag_known_deferred_exponential_cases() {
        // Ambiguous alternation under a single quantifier is exponential (EDA)
        // but star height 1, so the nested-quantifier detector does *not* flag
        // it — a knowingly-deferred follow-up (branch-overlap analysis, #724).
        // Pinned here so the scope boundary is explicit and nobody later assumes
        // these are covered.
        assert!(!redos("(a|a)+"));
        assert!(!redos("(\\d|\\d\\d)+"));
        assert!(!redos("(foo|foobar)+"));
    }

    #[test]
    fn allows_every_pattern_used_in_the_repo() {
        // Guard against a false positive on the `Matches` patterns the fixtures,
        // docs, and examples ship — none nests unbounded quantifiers.
        for pat in [
            "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            "[a-z][a-z0-9-]*",
            "[A-Z]{3}-[0-9]{4}",
            "[A-Z]{3}",
            "[a-z]+",
            "[A-Z]+",
            "[a-z]+(?<=ing)",
            "[a-z0-9_]+",
            "[a-z0-9]{1,16}",
            "[a-z0-9]{1,8}",
            "[A-Z0-9]{3,16}",
            "[a-z0-9]{3,8}",
            "[a-zA-Z0-9]{6,8}",
            "ab|cd",
            "AUTH-[0-9]{8}",
            "AUTH-[0-9]+",
            "CUST-[0-9]+",
            "https?://.+",
            "ORD-[0-9]{6}",
            "ORD-[0-9]+",
            "SHP-[0-9]{8}",
            "T-[0-9]+",
        ] {
            assert!(!redos(pat), "false positive on safe pattern `{pat}`");
        }
    }
}
