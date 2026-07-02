//! Expression and pattern checking.
//!
//! Split out of `checker.rs` (v0.29.10) verbatim; the parent module
//! re-exports these via `use expressions::*`.

use super::*;

/// v0.26 (ADR 0054): the deletion span for `given` entry `i`, list-aware so
/// the result never double-commas, leading-commas, or leaves `given ,`:
/// an entry with a successor deletes through the successor's start
/// (`C1, `); a final entry deletes from its predecessor's end (`, C2`); the
/// only entry deletes from the return type's end — the `given` keyword goes
/// with it (no dangling `given`).
pub(crate) fn given_removal_span(
    entries: &[(String, Span)],
    i: usize,
    return_ty_span: Span,
) -> Span {
    if entries.len() == 1 {
        Span::new(return_ty_span.end, entries[0].1.end)
    } else if i + 1 < entries.len() {
        Span::new(entries[i].1.start, entries[i + 1].1.start)
    } else {
        Span::new(entries[i - 1].1.end, entries[i].1.end)
    }
}

/// v0.26 (ADR 0054): the insertion edit that adds `name` to the `given`
/// clause — `, name` after the last entry, or ` given name` synthesised at
/// the anchor (the handler's return type) when the clause is absent. `None`
/// when there is no clause and no sound anchor (provider bodies — their
/// clause lives on the `provides` line, not at the op's return type).
pub(crate) fn given_insertion_edit(
    entries: &[(String, Span)],
    anchor: Option<Span>,
    name: &str,
) -> Option<(Span, String)> {
    if let Some((_, last)) = entries.last() {
        Some((Span::new(last.end, last.end), format!(", {name}")))
    } else {
        anchor.map(|a| (Span::new(a.end, a.end), format!(" given {name}")))
    }
}

// -- type-declaration validation --

pub(crate) fn check_ident(id: &Ident, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(ty) = ctx.lookup(id.name.as_str()) {
        return Some(ty);
    }
    // v0.20a: a named function referenced as a *value* where a function type
    // is expected (the contextual relaxation of `bynk.resolve.fn_without_call`,
    // relocated here from the resolver). A Var-bearing expected (generic
    // instantiation, pass 1) counts as a function-type expectation.
    if let Some(fn_decl) = ctx.input.fns.get(&id.name).cloned() {
        let fn_expected = matches!(expected, Some(Ty::Fn { .. }));
        if fn_expected {
            ctx.refs.record(id.span, SymbolKind::Fn, &id.name);
            if !fn_decl.type_params.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.generics.uninferable_type_arg",
                        id.span,
                        format!(
                            "generic function `{}` cannot be passed as a value in v0.20a — its type parameters cannot be instantiated here",
                            id.name
                        ),
                    )
                    .with_note("wrap it in a lambda, or call it directly"),
                );
                return None;
            }
            let params: Option<Vec<Ty>> = fn_decl
                .params
                .iter()
                .map(|p| resolve_type_ref(&p.type_ref, &ctx.input.types))
                .collect();
            let ret = resolve_type_ref(&fn_decl.return_type, &ctx.input.types)?;
            return Some(Ty::Fn {
                params: params?,
                ret: Box::new(ret),
            });
        }
        // Bare reference outside a function-typed position: the original
        // rule, with the checker's type knowledge behind it.
        ctx.errors.push(
            CompileError::new(
                "bynk.resolve.fn_without_call",
                id.span,
                format!(
                    "`{}` is a function — call it (`{}(…)`), or pass it where a function type is expected",
                    id.name, id.name
                ),
            )
            .with_note(
                "a bare function reference is only a value in a function-typed position (v0.20a)",
            ),
        );
        return None;
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
                        "bynk.types.variant_missing_payload",
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

/// v0.9.4 Part B (slice 1): `Val[T]` / `Val[T](literal)` for refined types,
/// valid only in test bodies. Sum/record/opaque types are not yet supported.
pub(crate) fn check_val(
    type_ref: &TypeRef,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "bynk.val.outside_test",
                span,
                "`Val[T]` is only valid inside a test case body",
            )
            .with_note(
                "fabricated values are test-time construction; use them only inside `case \"...\" { ... }` blocks",
            ),
        );
    }
    let ty = match resolve_type_ref(type_ref, &ctx.input.types) {
        Some(t) => {
            // v0.25: `Val[T]` names the type.
            record_type_refs(type_ref, &ctx.input.types, &HashSet::new(), ctx.refs);
            t
        }
        None => {
            ctx.errors.push(CompileError::new(
                "bynk.val.unknown_type",
                span,
                "`Val[T]` refers to a type that does not resolve",
            ));
            return None;
        }
    };
    match &ty {
        // Refined types: bare (generate a default) or a single literal pin.
        Ty::Named {
            name,
            kind: NamedKind::Refined(base),
        } => {
            let name = name.clone();
            let base = *base;
            let decl = match ctx.input.types.get(&name) {
                Some(d) => d.clone(),
                // Unreachable: the type already resolved above.
                None => return None,
            };
            let refinement = type_decl_refinement(&decl);
            match args {
                [] => {
                    if refinement.is_some_and(refinement_needs_pin) {
                        ctx.errors.push(
                            CompileError::new(
                                "bynk.val.needs_pin",
                                span,
                                format!(
                                    "bare `Val[{name}]` cannot generate a value for a `Matches` refinement"
                                ),
                            )
                            .with_note("provide an explicit value, e.g. `Val[T](\"...\")`"),
                        );
                    }
                }
                [arg] => {
                    type_of(arg, Some(&Ty::Base(base)), ctx);
                    match const_literal(arg) {
                        Some(lit) if literal_matches_base(&lit, base) => {
                            if let Some(r) = refinement
                                && let Some(failed) = first_failed_predicate(r, &lit)
                            {
                                ctx.errors.push(CompileError::new(
                                    "bynk.val.literal_violates",
                                    arg.span,
                                    format!(
                                        "literal {} does not satisfy `{}` required by type `{}`",
                                        lit.display(),
                                        failed.name(),
                                        name
                                    ),
                                ));
                            }
                        }
                        _ => {
                            ctx.errors.push(CompileError::new(
                                "bynk.val.pin_not_literal",
                                arg.span,
                                format!(
                                    "`Val[{name}](...)` requires a literal `{}` value",
                                    base.name()
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    ctx.errors.push(CompileError::new(
                        "bynk.val.arity",
                        span,
                        format!(
                            "`Val[{name}]` takes at most one pin argument, but {} were given",
                            args.len()
                        ),
                    ));
                }
            }
        }
        // v0.9.4 slice 2: opaque / sum / record — bare generation only. Pins for
        // these kinds (variant pins, record overrides) are a later increment.
        Ty::Named {
            name,
            kind: NamedKind::Opaque(_) | NamedKind::Sum | NamedKind::Record,
        } => {
            let name = name.clone();
            if !args.is_empty() {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.val.pin_unsupported",
                        span,
                        format!(
                            "pinned `Val[{name}](...)` is not yet supported for this kind of type — use bare `Val[{name}]`"
                        ),
                    )
                    .with_note("literal pins are currently supported for refined types only"),
                );
            } else if !can_mock_bare(&ty, &ctx.input.types, MOCK_DEPTH) {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.val.needs_pin",
                        span,
                        format!(
                            "bare `Val[{name}]` cannot generate a value — it (transitively) needs a `Matches` refinement or is recursively unbounded"
                        ),
                    )
                    .with_note("provide an explicit value in the test instead"),
                );
            }
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.val.unsupported_kind",
                span,
                format!("`Val` is not a value type: `{}`", ty.display()),
            ));
        }
    }
    Some(ty)
}

/// v0.9.4 slice 2 recursion depth cap for bare `Val` generation — guards
/// against recursively-unbounded types (a sum whose first variant re-enters the
/// type). Beyond it, bare generation is refused.
const MOCK_DEPTH: u32 = 12;

/// Whether a bare `Val[T]` can generate a value for `ty`: refined types must
/// not carry a `Matches` predicate (no default), and sums/records must have
/// every (first-variant / field) component recursively mockable within the
/// depth cap.
fn can_mock_bare(ty: &Ty, types: &HashMap<String, TypeDecl>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match ty {
        Ty::Base(_) => true,
        Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return false;
            };
            match &decl.body {
                TypeBody::Refined { refinement, .. } => {
                    !refinement.as_ref().is_some_and(refinement_needs_pin)
                }
                TypeBody::Opaque { .. } => true,
                TypeBody::Sum(s) => s.variants.first().is_some_and(|v| {
                    v.payload.iter().all(|f| {
                        resolve_type_ref(&f.type_ref, types)
                            .is_some_and(|t| can_mock_bare(&t, types, depth - 1))
                    })
                }),
                TypeBody::Record(r) => r.fields.iter().all(|f| {
                    resolve_type_ref(&f.type_ref, types)
                        .is_some_and(|t| can_mock_bare(&t, types, depth - 1))
                }),
            }
        }
        _ => false,
    }
}

pub(crate) fn check_expect(inner: &Expr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "bynk.expect.outside_case",
                span,
                "`expect` is only valid inside a `case` body",
            )
            .with_note(
                "expectations verify predicates at test runtime; use them only inside `case \"...\" { ... }` blocks",
            ),
        );
    }
    let val_ty = type_of(inner, Some(&Ty::Base(BaseType::Bool)), ctx);
    if let Some(actual) = val_ty
        && !compatible(&actual, &Ty::Base(BaseType::Bool))
    {
        ctx.errors.push(CompileError::new(
            "bynk.expect.not_bool",
            inner.span,
            format!(
                "`expect` predicate has type `{}`, but a `Bool` is required",
                actual.display(),
            ),
        ));
    }
    Some(Ty::Unit)
}

/// Resolve an observation seam `Cap.op` (v0.117) against the capabilities the
/// unit under test consumes. Returns the operation's signature on success; on
/// failure it pushes `bynk.observe.not_a_seam` / `bynk.observe.unknown_op` and
/// returns `None`.
fn resolve_observation_seam(cap: &Ident, op: &Ident, ctx: &mut Ctx) -> Option<CapabilityOpInfo> {
    let Some(cap_info) = ctx.caps.capabilities.get(&cap.name).cloned() else {
        ctx.errors.push(
            CompileError::new(
                "bynk.observe.not_a_seam",
                cap.span,
                format!(
                    "`{}` is not a capability the unit under test consumes; only a consumed \
                     capability's calls can be observed",
                    cap.name
                ),
            )
            .with_note("observe a capability the target `consumes` / has in scope via `given`"),
        );
        return None;
    };
    let Some(op_info) = cap_info.ops.iter().find(|o| o.name == op.name).cloned() else {
        ctx.errors.push(CompileError::new(
            "bynk.observe.unknown_op",
            op.span,
            format!(
                "capability `{}` has no operation named `{}`",
                cap.name, op.name
            ),
        ));
        return None;
    };
    ctx.refs.record(
        op.span,
        SymbolKind::CapabilityOp,
        &format!("{}.{}", cap.name, op.name),
    );
    Some(op_info)
}

/// Type-check an observation (v0.117, testing track slice 5). The subject
/// `Cap.op` must be a consumed capability operation; `with <pred>` is the pure
/// invariant predicate over the operation's parameters (in scope by name); a
/// count must be a non-negative literal; `before Cap.op` resolves a second seam.
/// The observation itself is a `Bool` claim about the recorded trace.
pub(crate) fn check_observation(o: &ObservationExpr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "bynk.observe.outside_case",
                span,
                "an observation is only valid inside a `case` body",
            )
            .with_note("observations assert over calls recorded during a `case`"),
        );
    }
    let op_info = resolve_observation_seam(&o.cap, &o.op, ctx);
    match &o.matcher {
        ObservationMatcher::Called { count, with_pred } => {
            if let Some(c) = count
                && !matches!(&c.kind, ExprKind::IntLit(n) if *n >= 0)
            {
                ctx.errors.push(CompileError::new(
                    "bynk.observe.bad_count",
                    c.span,
                    "a call count must be a non-negative integer literal (`called once` or `called <n> times`)",
                ));
            }
            if let Some(p) = with_pred {
                if let Some(impure) = predicate_impure_construct(p) {
                    ctx.errors.push(
                        CompileError::new(
                            "bynk.observe.impure_with",
                            impure,
                            "a `with` predicate uses an effectful or test-only construct; it must be pure",
                        )
                        .with_note(
                            "a `with` predicate may read the operation's arguments and call pure value methods only",
                        ),
                    );
                }
                // Scope the predicate over the operation's parameters by name.
                let mut scope: HashMap<String, Ty> = HashMap::new();
                if let Some(info) = &op_info {
                    for (name, ty) in info.param_names.iter().zip(info.params.iter()) {
                        scope.insert(name.clone(), ty.clone());
                    }
                }
                ctx.scopes.push(scope);
                let pred_ty = type_of(p, Some(&Ty::Base(BaseType::Bool)), ctx);
                ctx.scopes.pop();
                if let Some(t) = pred_ty
                    && !compatible(&t, &Ty::Base(BaseType::Bool))
                {
                    ctx.errors.push(CompileError::new(
                        "bynk.observe.with_not_bool",
                        p.span,
                        format!(
                            "a `with` predicate has type `{}`, but a `Bool` is required",
                            t.display()
                        ),
                    ));
                }
            }
        }
        ObservationMatcher::NeverCalled => {}
        ObservationMatcher::Before { cap, op } => {
            let _ = resolve_observation_seam(cap, op, ctx);
        }
    }
    Some(Ty::Base(BaseType::Bool))
}

/// Type-check `trace(Cap.op)` (v0.117). Resolves the seam and yields
/// `List[<CallRecord>]`, where `<CallRecord>` is the synthetic per-operation
/// record (registered in the test-body type table) whose fields are the
/// operation's parameters.
pub(crate) fn check_trace(cap: &Ident, op: &Ident, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    if !ctx.in_test_body {
        ctx.errors.push(
            CompileError::new(
                "bynk.observe.trace_outside_test",
                span,
                "`trace` is only valid inside a `case` body",
            )
            .with_note("`trace(Cap.op)` reads the calls recorded during a `case`"),
        );
    }
    resolve_observation_seam(cap, op, ctx)?;
    Some(Ty::List(Box::new(Ty::Named {
        name: call_record_type_name(&cap.name, &op.name),
        kind: NamedKind::Record,
    })))
}

pub(crate) fn check_unary(op: UnaryOp, inner: &Expr, op_span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let t = type_of(inner, None, ctx)?;
    match op {
        UnaryOp::Neg => {
            if t.base() == Some(BaseType::Int) {
                Some(Ty::Base(BaseType::Int))
            } else {
                ctx.errors.push(CompileError::new(
                    "bynk.types.type_mismatch",
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
                    "bynk.types.type_mismatch",
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

/// v0.21: whether one operand is `Int` and the other `Float` — the mix the
/// no-coercion rule (ADR 0041) rejects with its own diagnostic.
pub(crate) fn numeric_mix(a: Option<BaseType>, b: Option<BaseType>) -> bool {
    matches!(
        (a, b),
        (Some(BaseType::Int), Some(BaseType::Float)) | (Some(BaseType::Float), Some(BaseType::Int))
    )
}

/// v0.86 (ADR 0112): a `Duration` operand misused. Shares the
/// `no_numeric_coercion` code (D4) but with a `Duration`-specific message,
/// since the `.toFloat()` advice is wrong here.
fn push_duration_op_error(op: BinOp, span: Span, lt: &Ty, rt: &Ty, ctx: &mut Ctx) {
    ctx.errors.push(
        CompileError::new(
            "bynk.types.no_numeric_coercion",
            span,
            format!(
                "operator `{}` is not defined for operands `{}` and `{}`",
                op.name(),
                lt.display(),
                rt.display()
            ),
        )
        .with_note(
            "`Duration` supports `+`/`-` with another `Duration`, `*` by an `Int`, \
             comparison, and instant math with an `Instant` (`now + 5.minutes`); use \
             `.toMillis()` to compute in raw milliseconds",
        ),
    );
}

/// v0.90 (ADR 0114): an `Instant` operand misused. Shares the
/// `no_numeric_coercion` code with an `Instant`-specific message.
fn push_instant_op_error(op: BinOp, span: Span, lt: &Ty, rt: &Ty, ctx: &mut Ctx) {
    ctx.errors.push(
        CompileError::new(
            "bynk.types.no_numeric_coercion",
            span,
            format!(
                "operator `{}` is not defined for operands `{}` and `{}`",
                op.name(),
                lt.display(),
                rt.display()
            ),
        )
        .with_note(
            "`Instant` supports `+`/`-` with a `Duration` (yielding an `Instant`), \
             `Instant - Instant` (yielding a `Duration`), and comparison; it has no \
             arithmetic with `Int` — use `.toEpochMillis()` for a raw millis count",
        ),
    );
}

fn push_no_numeric_coercion(op: BinOp, span: Span, lt: &Ty, rt: &Ty, ctx: &mut Ctx) {
    ctx.errors.push(
        CompileError::new(
            "bynk.types.no_numeric_coercion",
            span,
            format!(
                "operator `{}` cannot mix `Int` and `Float` operands; got `{}` and `{}`",
                op.name(),
                lt.display(),
                rt.display()
            ),
        )
        .with_note(
            "there is no implicit numeric coercion — convert explicitly with \
             `.toFloat()` on the `Int`, or `.round()`/`.floor()`/`.ceil()`/`.truncate()` \
             on the `Float`",
        ),
    );
}

pub(crate) fn check_binop(op: BinOp, lhs: &Expr, rhs: &Expr, ctx: &mut Ctx) -> Option<Ty> {
    // For `&&` (and v0.80 `implies`), if the lhs is or contains an `is` test,
    // propagate the bindings into the rhs scope (so `r is Ok(n) && n > 0`, and
    // `r is Ok(n) implies n > 0`, both work). `implies` is `!P || Q`, so the rhs
    // is only reached when the lhs holds — the same binding scope as `&&`.
    if matches!(op, BinOp::And | BinOp::Implies) {
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
                "bynk.types.type_mismatch",
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
                "bynk.types.type_mismatch",
                rhs.span,
                format!(
                    "operator `{}` requires `Bool` operands; right operand has type `{}`",
                    op.name(),
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
            // v0.90 (ADR 0114): `Instant` arithmetic. Handled first so an
            // `Instant ± Duration` (either order) routes here, not to the
            // `Duration` block. D3: advance/retreat an instant by a span; the
            // span between two instants.
            use BaseType::{Duration, Instant, Int};
            if lt_base == Some(Instant) || rt_base == Some(Instant) {
                return match (op, lt_base, rt_base) {
                    (BinOp::Add, Some(Instant), Some(Duration))
                    | (BinOp::Add, Some(Duration), Some(Instant))
                    | (BinOp::Sub, Some(Instant), Some(Duration)) => Some(Ty::Base(Instant)),
                    (BinOp::Sub, Some(Instant), Some(Instant)) => Some(Ty::Base(Duration)),
                    // Every other `Instant` combination is rejected (e.g.
                    // `Instant + Instant`, `Instant * Int`, `Instant ± Int`).
                    _ => {
                        push_instant_op_error(op, span, &lt, &rt, ctx);
                        None
                    }
                };
            }
            // v0.86 (ADR 0112): `Duration` arithmetic. Handled before the
            // Int/Float rules so the `Duration`-closed ops (D3) are explicit;
            // anything else involving a `Duration` is a coercion error. The
            // former `Int ± Duration` clock-math mix (0112 D4) is **withdrawn**
            // (ADR 0114 D5): timestamp math now goes through `Instant`.
            if lt_base == Some(Duration) || rt_base == Some(Duration) {
                return match (op, lt_base, rt_base) {
                    // D3: span ± span; span scaled by an integer scalar.
                    (BinOp::Add | BinOp::Sub, Some(Duration), Some(Duration)) => {
                        Some(Ty::Base(Duration))
                    }
                    (BinOp::Mul, Some(Duration), Some(Int))
                    | (BinOp::Mul, Some(Int), Some(Duration)) => Some(Ty::Base(Duration)),
                    // Every other `Duration` combination is rejected (e.g.
                    // `Duration + Int`, `Int + Duration`, `Duration * Duration`).
                    _ => {
                        push_duration_op_error(op, span, &lt, &rt, ctx);
                        None
                    }
                };
            }
            // v0.21: arithmetic is defined on `Int` and `Float`, never mixed
            // — there is no implicit numeric coercion (ADR 0041).
            match (lt_base, rt_base) {
                (Some(BaseType::Int), Some(BaseType::Int)) => Some(Ty::Base(BaseType::Int)),
                (Some(BaseType::Float), Some(BaseType::Float)) => Some(Ty::Base(BaseType::Float)),
                (Some(BaseType::Int), Some(BaseType::Float))
                | (Some(BaseType::Float), Some(BaseType::Int)) => {
                    push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                    None
                }
                _ => {
                    let (side, side_span, ty) =
                        if !matches!(lt_base, Some(BaseType::Int) | Some(BaseType::Float)) {
                            ("left", lhs.span, &lt)
                        } else {
                            ("right", rhs.span, &rt)
                        };
                    ctx.errors.push(CompileError::new(
                        "bynk.types.type_mismatch",
                        side_span,
                        format!(
                            "operator `{}` requires `Int` or `Float` operands; {side} operand has type `{}`",
                            op.name(),
                            ty.display()
                        ),
                    ));
                    None
                }
            }
        }
        BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
            if lt_base != rt_base || lt_base.is_none() {
                if numeric_mix(lt_base, rt_base) {
                    push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                    return None;
                }
                ctx.errors.push(CompileError::new(
                    "bynk.types.type_mismatch",
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
            if !matches!(
                lt_base,
                Some(BaseType::Int)
                    | Some(BaseType::String)
                    | Some(BaseType::Float)
                    | Some(BaseType::Duration)
                    | Some(BaseType::Instant)
            ) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.type_mismatch",
                    span,
                    format!(
                        "operator `{}` is only defined on `Int`, `Float`, `Duration`, `Instant`, and `String`, not `{}`",
                        op.name(),
                        lt.display()
                    ),
                ));
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        BinOp::Eq | BinOp::NotEq => {
            // v0.100: a `Stream[T]` is a live value-over-time source, not a
            // value — it is not equatable. (Assignability makes `Stream`
            // structurally `compatible`, which `==` would otherwise accept;
            // this guard keeps the non-comparable promise the type carries.)
            if matches!(lt, Ty::Stream(_)) || matches!(rt, Ty::Stream(_)) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.stream_not_comparable",
                    span,
                    format!(
                        "operator `{}` cannot compare `Stream` values — a stream is a live value-over-time source, not a comparable value",
                        op.name()
                    ),
                ));
                return None;
            }
            // v0.102: a held value (`Connection[F]`) has identity, not
            // value-equality (§2.9.3), so it is not `==`-comparable — the same
            // guard as `Stream` (assignability would otherwise let `==` accept it).
            if lt.is_held() || rt.is_held() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.held_not_comparable",
                    span,
                    format!(
                        "operator `{}` cannot compare held values — a `{}` has identity, not value-equality",
                        op.name(),
                        if lt.is_held() { lt.display() } else { rt.display() },
                    ),
                ));
                return None;
            }
            if lt_base.is_some() && rt_base.is_some() {
                if lt_base != rt_base {
                    if numeric_mix(lt_base, rt_base) {
                        push_no_numeric_coercion(op, span, &lt, &rt, ctx);
                        return None;
                    }
                    ctx.errors.push(CompileError::new(
                        "bynk.types.type_mismatch",
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
                    "bynk.types.type_mismatch",
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
        // `And` and `Implies` are handled in the early-return block above (for
        // is-binding propagation); listed here only for match exhaustiveness.
        BinOp::And | BinOp::Or | BinOp::Implies => {
            if lt.base() != Some(BaseType::Bool) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.type_mismatch",
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
                    "bynk.types.type_mismatch",
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

/// v0.20a: type-check the application of a function-typed value (`f(x)`
/// where `f` is a param or local of type `A -> B`). Reuses the ordinary
/// argument rules; an effectful result (`ret` is `Effect[_]`) is an effect
/// operation, legal only in an effectful context — the same confinement a
/// capability call obeys.
/// v0.20a: type-check a lambda (`(params) => body`). Two paths:
///
/// - **Expected function type** (ground — guaranteed by the generic
///   instantiation order): params type contextually (an annotation must be
///   compatible with the expected param), the body checks against the
///   expected return with `effectful` derived from it, and the result is the
///   expected type (checking-mode bidirectionality).
/// - **Unconstrained**: every param must be annotated
///   (`bynk.lambda.unannotated_param`); effectfulness is decided by a
///   syntactic pre-scan of the body (`<-`, capability calls, effectful named
///   or value calls), and the result type wraps in `Effect` when it fired.
///
/// The enclosing handler's capability map and `given` tracking stay shared —
/// a lambda may close over and call a `given` capability (ADR 0033). The
/// frame swap forbids `commit` inside a lambda (`agent_state_ty = None` →
/// the existing `bynk.commit.outside_agent`).
pub(crate) fn check_lambda(
    lambda: &LambdaExpr,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let expected_fn = match expected {
        Some(Ty::Fn { params, ret }) => Some((params.clone(), (**ret).clone())),
        _ => None,
    };

    // Establish param types.
    let mut param_tys: Vec<Ty> = Vec::new();
    let mut scope: HashMap<String, Ty> = HashMap::new();
    if let Some((eps, _)) = &expected_fn {
        if eps.len() != lambda.params.len() {
            ctx.errors.push(CompileError::new(
                "bynk.types.lambda_mismatch",
                lambda.span,
                format!(
                    "this lambda takes {} parameter(s), but a function of {} parameter(s) is expected",
                    lambda.params.len(),
                    eps.len()
                ),
            ));
            return None;
        }
        for (p, ep) in lambda.params.iter().zip(eps) {
            let ty = match &p.type_ref {
                Some(tr) => {
                    let annotated = resolve_type_ref(tr, &ctx.input.types)?;
                    if !compatible(ep, &annotated) {
                        ctx.errors.push(CompileError::new(
                            "bynk.types.lambda_mismatch",
                            p.span,
                            format!(
                                "lambda parameter `{}` is annotated `{}`, but `{}` is expected here",
                                p.name.name,
                                annotated.display(),
                                ep.display()
                            ),
                        ));
                    }
                    annotated
                }
                None => {
                    // v0.27 (ADR 0056): a param typed from the expected fn
                    // type gets an inferred-type inlay hint at its name.
                    if p.name.name != "_" {
                        ctx.hints.record(p.name.span, format!(": {}", ep.display()));
                    }
                    ep.clone()
                }
            };
            scope.insert(p.name.name.clone(), ty.clone());
            param_tys.push(ty);
        }
    } else {
        let mut missing = false;
        for p in &lambda.params {
            match &p.type_ref {
                Some(tr) => {
                    let ty = resolve_type_ref(tr, &ctx.input.types)?;
                    scope.insert(p.name.name.clone(), ty.clone());
                    param_tys.push(ty);
                }
                None => {
                    ctx.errors.push(
                        CompileError::new(
                            "bynk.lambda.unannotated_param",
                            p.span,
                            format!(
                                "lambda parameter `{}` needs a type annotation — no function type is expected here to infer it from",
                                p.name.name
                            ),
                        )
                        .with_note("annotate the parameter (e.g. `(x: Int) => …`), or pass the lambda where a function type is expected"),
                    );
                    missing = true;
                }
            }
        }
        if missing {
            return None;
        }
    }

    // v0.31: lambda parameters are in scope over the lambda body.
    for (p, ty) in lambda.params.iter().zip(&param_tys) {
        if p.name.name != "_" {
            ctx.locals.record(
                p.name.name.clone(),
                p.name.span,
                ty.display(),
                lambda.body.span,
            );
        }
    }

    ctx.scopes.push(scope);

    // v0.20a generics: an expected return that still carries a *flexible*
    // type variable (pass 2 of inference — the lambda's result is what binds
    // it) is treated as unconstrained: the body types bottom-up and the
    // caller's unify captures the variable. v0.20b: the enclosing generic
    // fn's own type parameters are *rigid* — an expected return of
    // `Option[A]` inside `find[A]`'s body is fully constrained, and the
    // body's `None`/`[]`/`Map.empty()` may infer from it.
    let ret_constrained = expected_fn
        .as_ref()
        .is_some_and(|(_, er)| !contains_flexible_var(er, &ctx.type_vars));

    // Decide the body's effectfulness BEFORE typing it: the effect gates
    // (`bind_in_pure_context`, `capability_in_pure_context`, the fn-value
    // gate) fire during typing off `ctx.effectful`.
    let body_effectful = match &expected_fn {
        Some((_, er)) if ret_constrained => er.is_effect(),
        _ => body_performs_effects(&lambda.body, ctx),
    };

    // Frame swap (save/restore — the capability map and given-tracking stay
    // shared so closures over capabilities work and count as uses).
    let saved_effectful = ctx.effectful;
    let saved_return_ty = ctx.return_ty.clone();
    let saved_return_ty_span = ctx.return_ty_span;
    let saved_agent_state_ty = ctx.agent_state_ty.take();
    let saved_commit_seen = ctx.commit_seen;
    ctx.effectful = body_effectful;
    ctx.return_ty = match &expected_fn {
        Some((_, er)) if ret_constrained => er.clone(),
        // Placeholder: no diagnostic path can consult it — the pre-scan sets
        // `effectful` whenever a `<-` exists, so `bind_in_pure_context`'s
        // return-type label is unreachable here.
        _ => Ty::Unit,
    };
    ctx.return_ty_span = lambda.span;
    ctx.commit_seen = false;

    let body_expected = match &expected_fn {
        Some((_, er)) if ret_constrained => Some(er.clone()),
        _ => None,
    };
    let body_ty = type_of(&lambda.body, body_expected.as_ref(), ctx);

    ctx.effectful = saved_effectful;
    ctx.return_ty = saved_return_ty;
    ctx.return_ty_span = saved_return_ty_span;
    ctx.agent_state_ty = saved_agent_state_ty;
    ctx.commit_seen = saved_commit_seen;
    ctx.scopes.pop();

    match expected_fn {
        // Var-bearing expected return: report the actual function type and
        // let the caller's unify bind the variable.
        Some((eps, _)) if !ret_constrained => {
            let bt = body_ty?;
            let ret = if body_effectful && !bt.is_effect() {
                Ty::Effect(Box::new(bt))
            } else {
                bt
            };
            Some(Ty::Fn {
                params: eps,
                ret: Box::new(ret),
            })
        }
        Some((eps, er)) => {
            if let Some(bt) = body_ty.as_ref() {
                // A pure body against an effectful expected return auto-lifts
                // (the emitter's async arrow realises the lifted Promise).
                let lifted =
                    maybe_auto_lift(Some(bt.clone()), Some(&er)).unwrap_or_else(|| bt.clone());
                if !compatible(&lifted, &er) {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.lambda_mismatch",
                        lambda.body.span,
                        format!(
                            "lambda body has type `{}`, but `{}` is expected",
                            bt.display(),
                            er.display()
                        ),
                    ));
                    return None;
                }
            }
            Some(Ty::Fn {
                params: eps,
                ret: Box::new(er),
            })
        }
        None => {
            let bt = body_ty?;
            let ret = if body_effectful && !bt.is_effect() {
                Ty::Effect(Box::new(bt))
            } else {
                bt
            };
            Some(Ty::Fn {
                params: param_tys,
                ret: Box::new(ret),
            })
        }
    }
}

/// v0.20a: the syntactic pre-scan deciding a lambda's effectfulness in an
/// unconstrained position, run after the lambda's params are in scope and
/// before typing. True on: an `<-` bind; a capability static-call; a call on
/// a scope binding or named function whose type/signature returns `Effect`;
/// `Effect.pure`. Does **not** descend into nested lambdas — an inner
/// lambda's effects are its own.
fn body_performs_effects(e: &Expr, ctx: &Ctx) -> bool {
    fn block_performs(b: &Block, ctx: &Ctx) -> bool {
        for s in &b.statements {
            match s {
                Statement::EffectLet(_) => return true,
                Statement::Let(l) => {
                    if body_performs_effects(&l.value, ctx) {
                        return true;
                    }
                }
                Statement::Expect(a) => {
                    if body_performs_effects(&a.value, ctx) {
                        return true;
                    }
                }
                Statement::Send(_) => return true,
                Statement::Assign(a) => {
                    if body_performs_effects(&a.value, ctx) {
                        return true;
                    }
                }
            }
        }
        body_performs_effects(&b.tail, ctx)
    }
    match &e.kind {
        ExprKind::Lambda(_) => false,
        // v0.43: an interpolated string is effectful iff one of its holes is.
        ExprKind::InterpStr(parts) => parts
            .iter()
            .any(|part| matches!(part, InterpPart::Hole(hole) if body_performs_effects(hole, ctx))),
        ExprKind::Block(b) => block_performs(b, ctx),
        ExprKind::EffectPure(_) => true,
        // A capability operation call (`Cap.op(…)`) or `Effect.pure` shape.
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if let ExprKind::Ident(id) = &receiver.kind
                && ctx.caps.capabilities.contains_key(&id.name)
            {
                return true;
            }
            // v0.20b: the effectful kernel fold returns `Effect[Acc]`.
            // Detected by name (the pre-scan is syntactic); a false positive
            // only *permits* effect syntax — a pure body still types pure.
            if method.name == FOLD_EFF {
                return true;
            }
            body_performs_effects(receiver, ctx)
                || args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::Call { name, args, .. } => {
            if let Some(Ty::Fn { ret, .. }) = ctx.lookup(&name.name)
                && ret.is_effect()
            {
                return true;
            }
            if let Some(f) = ctx.input.fns.get(&name.name)
                && matches!(f.return_type, TypeRef::Effect(..))
            {
                return true;
            }
            args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::ConstructorCall { args, .. } => {
            args.iter().any(|a| body_performs_effects(a, ctx))
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            body_performs_effects(cond, ctx)
                || block_performs(then_block, ctx)
                || block_performs(else_block, ctx)
        }
        ExprKind::Match { discriminant, arms } => {
            body_performs_effects(discriminant, ctx)
                || arms.iter().any(|a| match &a.body {
                    MatchBody::Expr(e) => body_performs_effects(e, ctx),
                    MatchBody::Block(b) => block_performs(b, ctx),
                })
        }
        ExprKind::BinOp(_, l, r) => body_performs_effects(l, ctx) || body_performs_effects(r, ctx),
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i)
        | ExprKind::Expect(i) => body_performs_effects(i, ctx),
        ExprKind::RecordConstruction { fields, .. } => fields.iter().any(|f| {
            f.value
                .as_ref()
                .is_some_and(|v| body_performs_effects(v, ctx))
        }),
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            body_performs_effects(base, ctx)
                || overrides.iter().any(|f| {
                    f.value
                        .as_ref()
                        .is_some_and(|v| body_performs_effects(v, ctx))
                })
        }
        ExprKind::FieldAccess { receiver, .. } => body_performs_effects(receiver, ctx),
        ExprKind::Is { value, .. } => body_performs_effects(value, ctx),
        ExprKind::Val { args, .. } => args.iter().any(|a| body_performs_effects(a, ctx)),
        ExprKind::ListLit(elems) => elems.iter().any(|e| body_performs_effects(e, ctx)),
        // v0.117: observations and `trace` read the recorded call log — the
        // recording rides the *real* capability calls elsewhere in the body; the
        // observation expression itself performs no effect.
        ExprKind::Observation(_) | ExprKind::Trace { .. } => false,
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => false,
    }
}

pub(crate) fn check_variant_construction(
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
                "bynk.types.variant_arity",
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
                "bynk.types.variant_payload_mismatch",
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

pub(crate) fn check_if(
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
            "bynk.types.if_non_bool_cond",
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
                        "bynk.types.if_branch_mismatch",
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

pub(crate) fn check_ok(
    inner: &Expr,
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
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
                    "bynk.types.ambiguous_constructor",
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
                    "bynk.types.ok_value_mismatch",
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
                        "bynk.types.ok_value_mismatch",
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
                    "bynk.types.cannot_infer_result_type_params",
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

/// v0.44: type-check a `QueueResult` variant reference or construction. `Ack`
/// is nullary; `Retry` carries one `String` reason. Non-generic.
pub(crate) fn check_queue_variant(
    span: Span,
    variant: QueueVariant,
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    match variant.payload {
        QueueVariantPayload::None => {
            if !args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
                    span,
                    format!(
                        "`QueueResult.{}` takes no arguments, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            Some(Ty::QueueResult)
        }
        QueueVariantPayload::Message => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
                    span,
                    format!(
                        "`QueueResult.{}` expects 1 `String` argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], Some(&Ty::Base(BaseType::String)), ctx)?;
            if !compatible(&arg_ty, &Ty::Base(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`QueueResult.{}` expects a `String` reason, but got `{}`",
                        variant.name,
                        arg_ty.display(),
                    ),
                ));
                return None;
            }
            Some(Ty::QueueResult)
        }
    }
}

/// Type-check construction of an `HttpResult[T]` variant (v0.9 §4.3).
///
/// Variants come in six payload shapes:
/// - `Value` (`Ok`, `Created`, `Accepted`) — argument's type is `T`. `T` is
///   taken from the expected type if available; otherwise reported as ambiguous.
/// - `Message` (`BadRequest`, `Conflict`, `TooManyRequests`, `ServerError`, …)
///   — argument must be `String`.
/// - `Location` (`Found`, `SeeOther`, `PermanentRedirect`, …) — argument must
///   be `String`: the redirect target URL, emitted as a `Location` header.
/// - `Streamed` (`Streaming`) — argument must be `Stream[String]`, SSE-framed.
/// - `Raw` (`Raw`) — two arguments, a `Bytes` body then a `String` content-type;
///   the only two-argument shape.
/// - `None` (`NoContent`, `NotFound`, `MethodNotAllowed`, …) — no argument
///   permitted; `T` is taken from the expected type or left inferred.
pub(crate) fn check_http_variant(
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
                    "bynk.types.variant_arity",
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
                            "bynk.types.ok_value_mismatch",
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
                    "bynk.types.variant_arity",
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
                    "bynk.types.argument_mismatch",
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
        HttpVariantPayload::Location => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 `String` location argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], Some(&Ty::Base(BaseType::String)), ctx)?;
            if !compatible(&arg_ty, &Ty::Base(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`HttpResult.{}` expects a `String` location URL, but got `{}`",
                        variant.name,
                        arg_ty.display(),
                    ),
                ));
                return None;
            }
            // A redirect carries no body; inner T is irrelevant, so fall back
            // to the propagated expected type or `()`.
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        // v0.101 (real-time track slice 1): `Streaming(s)` carries a
        // `Stream[String]` body, SSE-framed at the boundary. Like the redirect
        // and message shapes, the JSON body parameter `T` is irrelevant.
        HttpVariantPayload::Streamed => {
            let stream_str = Ty::Stream(Box::new(Ty::Base(BaseType::String)));
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 1 `Stream[String]` argument, but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let arg_ty = type_of(&args[0], Some(&stream_str), ctx)?;
            if !compatible(&arg_ty, &stream_str) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`HttpResult.{}` expects a `Stream[String]` body, but got `{}`",
                        variant.name,
                        arg_ty.display(),
                    ),
                ));
                return None;
            }
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        // v0.111: `Raw(body, contentType)` — the first two-argument shape. The
        // body is a `Bytes` written straight into the response; the content-type
        // is any `String`, unvalidated (opaque, ADR 0143 D3). The JSON body
        // parameter `T` is irrelevant, as for the redirect/message/stream shapes.
        HttpVariantPayload::Raw => {
            if args.len() != 2 {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
                    span,
                    format!(
                        "`HttpResult.{}` expects 2 arguments (a `Bytes` body and a `String` content-type), but {} were given",
                        variant.name,
                        args.len(),
                    ),
                ));
                return None;
            }
            let body_ty = type_of(&args[0], Some(&Ty::Base(BaseType::Bytes)), ctx)?;
            if !compatible(&body_ty, &Ty::Base(BaseType::Bytes)) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[0].span,
                    format!(
                        "`HttpResult.{}` expects a `Bytes` body, but got `{}`",
                        variant.name,
                        body_ty.display(),
                    ),
                ));
                return None;
            }
            let ct_ty = type_of(&args[1], Some(&Ty::Base(BaseType::String)), ctx)?;
            if !compatible(&ct_ty, &Ty::Base(BaseType::String)) {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[1].span,
                    format!(
                        "`HttpResult.{}` expects a `String` content-type, but got `{}`",
                        variant.name,
                        ct_ty.display(),
                    ),
                ));
                return None;
            }
            let t_ty = expected_t.unwrap_or(Ty::Unit);
            Some(Ty::HttpResult(Box::new(t_ty)))
        }
        HttpVariantPayload::None => {
            if !args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.variant_arity",
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

pub(crate) fn check_err(
    inner: &Expr,
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let surrounding = surrounding_result(expected, &ctx.return_ty);
    let expected_e = surrounding.as_ref().map(|(_, e)| e.clone());
    let inner_ty = type_of(inner, expected_e.as_ref(), ctx)?;
    match surrounding {
        Some((t_ty, e_ty)) => {
            if !compatible(&inner_ty, &e_ty) {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.types.err_value_mismatch",
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
                    "bynk.types.cannot_infer_result_type_params",
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

pub(crate) fn check_some(
    inner: &Expr,
    _span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let expected_inner = expected
        .and_then(peel_to_option)
        .or_else(|| peel_to_option(&ctx.return_ty));
    let inner_ty = type_of(inner, expected_inner.as_ref(), ctx)?;
    if let Some(exp) = &expected_inner
        && !compatible(&inner_ty, exp)
    {
        ctx.errors.push(CompileError::new(
            "bynk.types.some_value_mismatch",
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

pub(crate) fn check_none(span: Span, expected: Option<&Ty>, ctx: &mut Ctx) -> Option<Ty> {
    if let Some(t) = expected.and_then(peel_to_option) {
        return Some(Ty::Option(Box::new(t)));
    }
    if let Some(t) = peel_to_option(&ctx.return_ty) {
        return Some(Ty::Option(Box::new(t)));
    }
    ctx.errors.push(
        CompileError::new(
            "bynk.types.cannot_infer_option_type_param",
            span,
            "cannot infer the value type of `None`",
        )
        .with_note(
            "add an annotation (`let x: Option[T] = None`) or use `None` where the context expects an `Option`",
        ),
    );
    None
}

pub(crate) fn check_question(inner: &Expr, span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let inner_ty = type_of(inner, None, ctx)?;
    let Ty::Result(t, e) = &inner_ty else {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.question_on_non_result",
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
                    "bynk.types.question_error_mismatch",
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
                "bynk.types.question_outside_result",
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
            "bynk.types.question_error_mismatch",
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

pub(crate) fn check_record_spread(
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
                "bynk.record_spread.non_record_base",
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
            "bynk.record_spread.type_mismatch",
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
                "bynk.record_spread.unknown_field",
                f.name.span,
                format!(
                    "record type `{}` has no field `{}`",
                    record_name, f.name.name
                ),
            ));
            continue;
        };
        // v0.36 (ADR 0069, slice 2): a spread override label references the field.
        ctx.refs.record(
            f.name.span,
            SymbolKind::Field,
            &format!("{}.{}", record_name, f.name.name),
        );
        let expected_ty = resolve_type_ref(&declared_field.type_ref, &ctx.input.types);
        let value_ty = match &f.value {
            Some(v) => type_of(v, expected_ty.as_ref(), ctx),
            None => ctx.lookup(&f.name.name),
        };
        if let (Some(actual), Some(expected_ty)) = (value_ty, expected_ty)
            && !compatible(&actual, &expected_ty)
        {
            ctx.errors.push(CompileError::new(
                "bynk.record_spread.field_type_mismatch",
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

pub(crate) fn check_record_construction(
    type_name: &Ident,
    fields: &[FieldInit],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let decl = ctx.input.types.get(&type_name.name)?.clone();
    ctx.refs
        .record(type_name.span, SymbolKind::Type, &type_name.name);
    if matches!(decl.body, TypeBody::Opaque { .. }) {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.opaque_record_construction",
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
            // v0.36 (ADR 0069, slice 2): a construction field label is a
            // reference to the record field.
            ctx.refs.record(
                f.name.span,
                SymbolKind::Field,
                &format!("{}.{}", type_name.name, f.name.name),
            );
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
                        "bynk.types.field_value_mismatch",
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

pub(crate) fn check_field_access(receiver: &Expr, field: &Ident, ctx: &mut Ctx) -> Option<Ty> {
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
                    "bynk.types.variant_missing_payload",
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
    // v0.45: a verified actor binding exposes exactly `.identity` — the sealed,
    // boundary-minted identity value. No other member is valid.
    if let Ty::Actor(identity) = &recv_ty {
        if field.name == "identity" {
            return Some((**identity).clone());
        }
        ctx.errors.push(CompileError::new(
            "bynk.types.unknown_field",
            field.span,
            format!(
                "a verified actor exposes only `.identity`, not `.{}`",
                field.name
            ),
        ));
        return None;
    }
    // `.raw` on an opaque value: only available within the defining commons.
    // Returns the base type. The emitter compiles this to a `value as base`
    // type assertion (see emitter::lower_expr for FieldAccess).
    if field.name == RAW
        && let Ty::Named {
            kind: NamedKind::Opaque(base),
            name,
        } = &recv_ty
    {
        if !ctx.input.is_local_type(name) {
            ctx.errors.push(
                CompileError::new(
                    "bynk.types.opaque_raw_outside",
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
    // v0.22b: `JsonError` is a compiler-known record (ADR 0047) — uniform
    // `String` fields so a decode failure is inspectable in Bynk.
    if recv_ty == Ty::JsonError {
        return match field.name.as_str() {
            "kind" | "path" | "message" => Some(Ty::Base(BaseType::String)),
            other => {
                ctx.errors.push(CompileError::new(
                    "bynk.types.unknown_field",
                    field.span,
                    format!(
                        "`JsonError` has no field `{other}` — its fields are `kind`, `path`, `message`"
                    ),
                ));
                None
            }
        };
    }
    let Ty::Named {
        name,
        kind: NamedKind::Record,
    } = &recv_ty
    else {
        let mut err = CompileError::new(
            "bynk.types.field_access_on_non_record",
            field.span,
            format!(
                "field access requires a record type, but the receiver has type `{}`",
                recv_ty.display()
            ),
        );
        // #48: a `.raw` (or any field) on a *refined* value is a common
        // mistake — refined values widen to their base type, so there's
        // nothing to unwrap. Say what's right, and offer the mechanical fix
        // (drop `.raw`) when that's what was written.
        if let Ty::Named {
            kind: NamedKind::Refined(_),
            ..
        } = &recv_ty
        {
            err = err.with_note(
                "a refined value is usable wherever its base type is expected — \
                 pass it directly (`.raw` is for opaque types)",
            );
            if field.name == RAW {
                err = err.with_suggestion(
                    "remove `.raw` — a refined value is already its base type",
                    vec![(
                        bynk_syntax::span::Span::new(receiver.span.end, field.span.end),
                        String::new(),
                    )],
                    Applicability::MachineApplicable,
                );
            }
        }
        ctx.errors.push(err);
        return None;
    };
    let decl = ctx.input.types.get(name)?;
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    let Some(field_decl) = r.fields.iter().find(|f| f.name.name == field.name) else {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.unknown_field",
                field.span,
                format!("record type `{}` has no field `{}`", name, field.name),
            )
            .with_label(decl.name.span, "type declared here"),
        );
        return None;
    };
    // v0.36 (ADR 0069, slice 2): the field is an index symbol, keyed by the
    // compound `"Type.field"` name (read access is a reference site).
    ctx.refs.record(
        field.span,
        SymbolKind::Field,
        &format!("{name}.{}", field.name),
    );
    resolve_type_ref(&field_decl.type_ref, &ctx.input.types)
}

pub(crate) fn check_match(
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
            "bynk.types.match_non_sum_discriminant",
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
                "bynk.types.unreachable_arm",
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
                // v0.25: a qualified `T.Variant` pattern references `T`.
                if let Some(tn) = type_name
                    && ctx.input.types.contains_key(&tn.name)
                {
                    ctx.refs.record(tn.span, SymbolKind::Type, &tn.name);
                }
                // Validate the variant against expected_variants.
                let variant_info = expected_variants.iter().find(|v| v.name == variant.name);
                let Some(variant_info) = variant_info else {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.unknown_variant_in_pattern",
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
                        "bynk.types.pattern_type_mismatch",
                        tn.span,
                        format!(
                            "pattern qualifier `{}` does not match the discriminant type `{}`",
                            tn.name, name
                        ),
                    ));
                }
                if !covered.insert(variant.name.clone()) {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.duplicate_variant_arm",
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
                                                "bynk.types.unknown_pattern_field",
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
                                            "bynk.types.mixed_pattern_bindings",
                                            b.span,
                                            "pattern bindings must be all named (`field: name`) or all positional",
                                        ));
                                    }
                                }
                            }
                        } else if bindings.len() != variant_info.payload.len() {
                            ctx.errors.push(CompileError::new(
                                "bynk.types.pattern_arity",
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
                            "bynk.types.pattern_arity",
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
                        "bynk.types.non_exhaustive_match",
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
                    "bynk.types.match_arm_mismatch",
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

pub(crate) fn check_is(value: &Expr, pattern: &Pattern, _span: Span, ctx: &mut Ctx) -> Option<Ty> {
    let value_ty = type_of(value, None, ctx)?;
    let variants = variants_of(&value_ty, &ctx.input.types);
    match pattern {
        Pattern::Wildcard(_) => {
            // `_` matches anything, but is only meaningful over a sum today.
            if variants.is_none() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.is_non_sum",
                    pattern.span(),
                    format!(
                        "the `is` operator requires a sum, `Result`, or `Option` value, but got `{}`",
                        value_ty.display()
                    ),
                ));
            }
            return Some(Ty::Base(BaseType::Bool));
        }
        Pattern::Variant {
            variant,
            bindings,
            type_name,
            ..
        } => {
            // v0.25: a qualified `T.Variant` pattern references `T`.
            if let Some(tn) = type_name
                && ctx.input.types.contains_key(&tn.name)
            {
                ctx.refs.record(tn.span, SymbolKind::Type, &tn.name);
            }
            // 1. Sum-variant interpretation: the name is a variant of `value`'s
            //    sum type. (Takes priority when `value` is that sum.)
            let info = variants
                .as_ref()
                .and_then(|vs| vs.iter().find(|v| v.name == variant.name));
            let Some(info) = info else {
                // 2. v0.13 refinement narrowing: a bare nullary name that
                //    resolves to a refined type whose base matches `value`.
                if type_name.is_none()
                    && bindings.is_empty()
                    && let Some(decl) = ctx.input.types.get(&variant.name)
                    && let TypeBody::Refined { base, .. } = &decl.body
                {
                    if compatible(&value_ty, &Ty::Base(*base)) {
                        // v0.25: `x is RefinedType` names the type.
                        ctx.refs
                            .record(variant.span, SymbolKind::Type, &variant.name);
                        return Some(Ty::Base(BaseType::Bool));
                    }
                    ctx.errors.push(CompileError::new(
                        "bynk.types.is_base_mismatch",
                        pattern.span(),
                        format!(
                            "`is {}` checks an `{}` value, but got `{}`",
                            variant.name,
                            base.name(),
                            value_ty.display()
                        ),
                    ));
                    return Some(Ty::Base(BaseType::Bool));
                }
                // 3. Neither a variant nor a base-compatible refined type.
                if variants.is_none() {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.is_non_sum",
                        pattern.span(),
                        format!(
                            "the `is` operator requires a sum, `Result`, or `Option` value, but got `{}`",
                            value_ty.display()
                        ),
                    ));
                } else {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.is_unknown_variant",
                        variant.span,
                        format!(
                            "type `{}` has no variant `{}`",
                            value_ty.display(),
                            variant.name
                        ),
                    ));
                }
                return Some(Ty::Base(BaseType::Bool));
            };
            // Just validate bindings shape; binding TYPES introduced via
            // `collect_is_bindings` are handled at the consumer site.
            if !bindings.is_empty() && info.payload.is_empty() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.pattern_arity",
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
                        "bynk.types.pattern_arity",
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
                // v0.13 refinement narrowing: `ident is RefinedType` re-binds the
                // identifier to the refined type in the narrowed branch.
                if let (
                    ExprKind::Ident(id),
                    Pattern::Variant {
                        variant,
                        bindings,
                        type_name: None,
                        ..
                    },
                ) = (&value.kind, pattern)
                    && bindings.is_empty()
                    && variants_of(&value_ty, &ctx.input.types)
                        .is_none_or(|vs| !vs.iter().any(|v| v.name == variant.name))
                    && let Some(decl) = ctx.input.types.get(&variant.name)
                    && let TypeBody::Refined { base, .. } = &decl.body
                    && compatible(&value_ty, &Ty::Base(*base))
                {
                    out.push((
                        id.name.clone(),
                        Ty::Named {
                            name: variant.name.clone(),
                            kind: NamedKind::Refined(*base),
                        },
                    ));
                    return;
                }
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
