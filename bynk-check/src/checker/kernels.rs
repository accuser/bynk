//! Kernel methods and collection / JSON statics.
//!
//! Split out of `checker.rs` (v0.29.10) verbatim; the parent module
//! re-exports these via `use kernels::*`.

use super::*;

/// v0.20b: `List.empty()` / `Map.empty()` — the built-in collection statics.
/// Their element/key/value types are exactly as uninferable as an empty
/// `[]`, so they share `bynk.types.uninferable_element_type`. The resolver
/// has already rejected any static other than `empty`.
pub(crate) fn check_collection_static(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    for a in args {
        let _ = type_of(a, None, ctx);
    }
    if method.name != "empty" {
        // The resolver owns the unknown-static diagnostic; don't double up.
        return None;
    }
    if !args.is_empty() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!("`{}.empty` takes no arguments", type_name.name),
        ));
        return None;
    }
    let inferred = match type_name.name.as_str() {
        LIST => expected
            .and_then(peel_to_list)
            .map(|t| Ty::List(Box::new(t))),
        _ => expected
            .and_then(peel_to_map)
            .map(|(k, v)| Ty::Map(Box::new(k), Box::new(v))),
    };
    if inferred.is_none() {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.uninferable_element_type",
                span,
                format!(
                    "`{}.empty()` has no expected type to infer its type arguments from",
                    type_name.name
                ),
            )
            .with_note(
                "annotate the binding (`let xs: List[T] = List.empty()`) or use it where the collection type is expected",
            ),
        );
    }
    inferred
}

/// v0.88 (ADR 0116 D2): the closed orderable base set for `sortBy`/`min`/`max`.
/// A key widens through `Ty::base()` (Refined → base; Opaque does not widen,
/// so an opaque key — whose base is hidden — is not orderable). `Instant`
/// joins this set with slice 1b.
fn is_orderable(t: &Ty) -> bool {
    matches!(
        t.base(),
        Some(
            BaseType::Int
                | BaseType::Float
                | BaseType::String
                | BaseType::Duration
                | BaseType::Instant
        )
    )
}

/// v0.88 (ADR 0116 D3): the numeric keys `sum`/`average` accept.
fn is_numeric(t: &Ty) -> bool {
    matches!(
        t.base(),
        Some(BaseType::Int | BaseType::Float | BaseType::Duration)
    )
}

/// v0.88: value-keyable for `distinct`/`distinctBy` — the Map-key rule
/// (ADR 0110 D5): `Int`/`String`, including Refined *and* Opaque over them
/// (dedup is by value *equality*, which an opaque id supports even though it
/// does not widen for ordering).
fn is_keyable(t: &Ty) -> bool {
    match t {
        Ty::Base(BaseType::Int | BaseType::String) => true,
        Ty::Named {
            kind: NamedKind::Refined(b) | NamedKind::Opaque(b),
            ..
        } => matches!(b, BaseType::Int | BaseType::String),
        _ => false,
    }
}

fn require_orderable(key: &Ty, method: &str, span: Span, ctx: &mut Ctx) {
    if !is_orderable(key) {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.key_not_orderable",
                span,
                format!(
                    "`{method}` needs an orderable key, but the key function returns `{}`",
                    key.display()
                ),
            )
            .with_note(
                "orderable keys are `Int`, `Float`, `String`, `Duration`, `Instant` (and refined types over them)",
            ),
        );
    }
}

fn require_numeric(key: &Ty, method: &str, span: Span, ctx: &mut Ctx) {
    if !is_numeric(key) {
        ctx.errors.push(
            CompileError::new(
                "bynk.query.sum_needs_numeric",
                span,
                format!(
                    "`{method}` needs a numeric key, but the key function returns `{}`",
                    key.display()
                ),
            )
            .with_note("numeric keys are `Int`, `Float`, `Duration` (and refined types over them)"),
        );
    }
}

fn require_keyable(key: &Ty, method: &str, span: Span, ctx: &mut Ctx) {
    if !is_keyable(key) {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.unkeyable_distinct",
                span,
                format!(
                    "`{method}` needs a value-keyable element/key, but got `{}`",
                    key.display()
                ),
            )
            .with_note(
                "value-keyable types are `Int`, `String`, or a refined/opaque type over them",
            ),
        );
    }
}

/// v0.94 (ADR 0120): the element type `U` of a join's `other` collection — it
/// MUST match the receiver's shape (`List` joins `List`, `Query` joins `Query`).
fn join_other_elem(other: &Ty, shape_is_query: bool) -> Option<Ty> {
    match (shape_is_query, other) {
        (false, Ty::List(e)) => Some((**e).clone()),
        (true, Ty::Query(e)) => Some((**e).clone()),
        _ => None,
    }
}

/// v0.94 (ADR 0116/0120): the wrong join-method arity, with the args still typed
/// for error recovery.
fn join_arity_err(kind: &str, name: &str, want: usize, args: &[Expr], span: Span, ctx: &mut Ctx) {
    ctx.errors.push(CompileError::new(
        "bynk.types.method_arity",
        span,
        format!("`{kind}.{name}` takes {want} arguments, got {}", args.len()),
    ));
    for a in args {
        let _ = type_of(a, None, ctx);
    }
}

/// v0.94 (ADR 0120): the shared checker for `joinOn`/`leftJoin` — an `other` of
/// the receiver's shape, `left: T -> K` / `right: U -> K` (value-keyable and
/// matching), and a combiner `into: (T, U) -> V` (or `(T, Option[U]) -> V` for
/// `leftJoin`). Returns the result element `V`; the caller wraps it (`List[V]`
/// eager / `Query[V]` lazy). Arguments are positional (labelled call arguments
/// are a deferred general feature — ADR 0120).
fn check_equi_join(
    args: &[Expr],
    elem: &Ty,
    span: Span,
    shape_is_query: bool,
    is_left: bool,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let kind = if shape_is_query { "Query" } else { "List" };
    let name = if is_left { "leftJoin" } else { "joinOn" };
    if args.len() != 4 {
        join_arity_err(kind, name, 4, args, span, ctx);
        return None;
    }
    let other = type_of(&args[0], None, ctx)?;
    let Some(u) = join_other_elem(&other, shape_is_query) else {
        ctx.errors.push(CompileError::new(
            "bynk.types.argument_mismatch",
            args[0].span,
            format!(
                "`{kind}.{name}` joins another `{kind}`, but got `{}`",
                other.display()
            ),
        ));
        return None;
    };
    let left = check_kernel_fn_arg(
        &args[1],
        vec![elem.clone()],
        &format!("the `{kind}.{name}` left key"),
        ctx,
    )?;
    require_keyable(&left, &format!("{kind}.{name}"), args[1].span, ctx);
    let right = check_kernel_fn_arg(
        &args[2],
        vec![u.clone()],
        &format!("the `{kind}.{name}` right key"),
        ctx,
    )?;
    if !compatible(&left, &right) {
        ctx.errors.push(CompileError::new(
            "bynk.query.join_key_mismatch",
            args[2].span,
            format!(
                "`{kind}.{name}` join keys must have the same type — the left key is `{}` but the right key is `{}`",
                left.display(),
                right.display()
            ),
        ));
    }
    let other_param = if is_left {
        Ty::Option(Box::new(u.clone()))
    } else {
        u.clone()
    };
    let v = check_kernel_fn_arg(
        &args[3],
        vec![elem.clone(), other_param],
        &format!("the `{kind}.{name}` combiner"),
        ctx,
    )?;
    Some(v)
}

/// v0.94 (ADR 0120): `join(other, on: (T, U) -> Bool, into: (T, U) -> V)` — a
/// predicate (nested-loop) join. Returns `V`.
fn check_pred_join(
    args: &[Expr],
    elem: &Ty,
    span: Span,
    shape_is_query: bool,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let kind = if shape_is_query { "Query" } else { "List" };
    if args.len() != 3 {
        join_arity_err(kind, "join", 3, args, span, ctx);
        return None;
    }
    let other = type_of(&args[0], None, ctx)?;
    let Some(u) = join_other_elem(&other, shape_is_query) else {
        ctx.errors.push(CompileError::new(
            "bynk.types.argument_mismatch",
            args[0].span,
            format!(
                "`{kind}.join` joins another `{kind}`, but got `{}`",
                other.display()
            ),
        ));
        return None;
    };
    let on = Ty::Fn {
        params: vec![elem.clone(), u.clone()],
        ret: Box::new(Ty::Base(BaseType::Bool)),
    };
    check_arg(&args[1], &on, &format!("the `{kind}.join` predicate"), ctx);
    let v = check_kernel_fn_arg(
        &args[2],
        vec![elem.clone(), u.clone()],
        &format!("the `{kind}.join` combiner"),
        ctx,
    )?;
    Some(v)
}

/// v0.94 (ADR 0116 D7 / 0120): `groupBy(key: T -> K, into: (K, List[T]) -> V)` —
/// partition by a value-keyable key, projecting each group through `into`.
/// Returns `V`.
fn check_group_by(
    args: &[Expr],
    elem: &Ty,
    span: Span,
    shape_is_query: bool,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let kind = if shape_is_query { "Query" } else { "List" };
    if args.len() != 2 {
        join_arity_err(kind, "groupBy", 2, args, span, ctx);
        return None;
    }
    let key = check_kernel_fn_arg(
        &args[0],
        vec![elem.clone()],
        &format!("the `{kind}.groupBy` key"),
        ctx,
    )?;
    require_keyable(&key, &format!("{kind}.groupBy"), args[0].span, ctx);
    let v = check_kernel_fn_arg(
        &args[1],
        vec![key.clone(), Ty::List(Box::new(elem.clone()))],
        &format!("the `{kind}.groupBy` combiner"),
        ctx,
    )?;
    Some(v)
}

/// v0.20b: type a built-in `List[T]` kernel method. The fold accumulator is
/// inferred from the `init` argument, then the step function checks against
/// the fully-instantiated function type (params type contextually, v0.20a).
pub(crate) fn check_list_kernel_method(
    method: &Ident,
    args: &[Expr],
    elem: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`List.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "length" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        "get" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                "the `List.get` index",
                ctx,
            );
            Some(Ty::Option(Box::new(elem.clone())))
        }
        "prepend" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], elem, "the `List.prepend` element", ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "fold" => {
            if !arity(2, ctx) {
                return None;
            }
            let acc = type_of(&args[0], None, ctx)?;
            let step = Ty::Fn {
                params: vec![acc.clone(), elem.clone()],
                ret: Box::new(acc.clone()),
            };
            check_arg(&args[1], &step, "the `List.fold` step function", ctx);
            Some(acc)
        }
        FOLD_EFF => {
            if !arity(2, ctx) {
                return None;
            }
            let acc = type_of(&args[0], None, ctx)?;
            let step = Ty::Fn {
                params: vec![acc.clone(), elem.clone()],
                ret: Box::new(Ty::Effect(Box::new(acc.clone()))),
            };
            check_arg(&args[1], &step, "the `List.foldEff` step function", ctx);
            // `foldEff` runs its effectful step function — like any
            // effectful function-value call, it is confined to effectful
            // contexts (0031).
            if !ctx.effectful {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.effect.fn_value_in_pure_context",
                        span,
                        "`List.foldEff` runs an effectful step function and cannot be called in a pure context",
                    )
                    .with_note(
                        "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
                    ),
                );
            }
            Some(Ty::Effect(Box::new(acc)))
        }
        // v0.146 (ADR 0170): `forEach(f: T -> Effect[()]) -> Effect[()]` runs an
        // effectful step for each element **in turn**; v0.147 (ADR 0171):
        // `parTraverse` runs them **concurrently** (the fan-out form) and awaits
        // them together. Same type — the difference is sequential-vs-parallel
        // lowering. Both are the `List` analogue of the `Query` terminals; like
        // `foldEff`, each runs an effectful function value and so is confined to
        // effectful contexts.
        FOR_EACH | PAR_TRAVERSE => {
            if !arity(1, ctx) {
                return None;
            }
            let f = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Effect(Box::new(Ty::Unit))),
            };
            check_arg(
                &args[0],
                &f,
                &format!("the `List.{}` function", method.name),
                ctx,
            );
            if !ctx.effectful {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.effect.fn_value_in_pure_context",
                        span,
                        format!(
                            "`List.{}` runs an effectful function and cannot be called in a pure context",
                            method.name
                        ),
                    )
                    .with_note(
                        "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
                    ),
                );
            }
            Some(Ty::Effect(Box::new(Ty::Unit)))
        }
        // v0.148 (ADR 0172): the collect-all effectful iterators.
        // `traverseAll(f: A -> Effect[Result[B, E]]) -> Effect[List[Result[B, E]]]`
        // runs `f` over each element **in order**; `parTraverseAll` runs them
        // **concurrently**. Neither short-circuits: every element's outcome
        // (`Ok` or `Err`) is collected into the result list — a `Result` `Err` is
        // a value, not a fault, so nothing is discarded. Like `forEach`, each
        // runs an effectful function value and is confined to effectful contexts.
        TRAVERSE_ALL | PAR_TRAVERSE_ALL => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                &format!("the `List.{}` function", method.name),
                ctx,
            )?;
            // The function must return `Effect[Result[B, E]]`; peel to the
            // `Result[B, E]` element the result list collects.
            let result_ty = match &ret {
                Ty::Effect(inner) => match &**inner {
                    r @ Ty::Result(_, _) => r.clone(),
                    other => {
                        ctx.errors.push(CompileError::new(
                            "bynk.types.argument_mismatch",
                            args[0].span,
                            format!(
                                "the `List.{}` function must return `Effect[Result[B, E]]`, but returns `Effect[{}]`",
                                method.name,
                                other.display()
                            ),
                        ));
                        return None;
                    }
                },
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `List.{}` function must return `Effect[Result[B, E]]`, but returns `{}`",
                            method.name,
                            other.display()
                        ),
                    ));
                    return None;
                }
            };
            if !ctx.effectful {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.effect.fn_value_in_pure_context",
                        span,
                        format!(
                            "`List.{}` runs an effectful function and cannot be called in a pure context",
                            method.name
                        ),
                    )
                    .with_note(
                        "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
                    ),
                );
            }
            Some(Ty::Effect(Box::new(Ty::List(Box::new(result_ty)))))
        }
        // v0.150 (ADR 0174): the **short-circuit** collect iterators.
        // `traverseTry(f: A -> Effect[Result[B, E]]) -> Effect[Result[List[B], E]]`
        // runs `f` over each element **in order**, stopping at the first `Err` and
        // returning it; `parTraverseTry` issues all concurrently, awaits them, and
        // returns the first `Err` in input order (or `Ok` of the collected values).
        // The fault-propagating counterpart to `traverseAll` (which gathers every
        // outcome). Like `forEach`, each runs an effectful function value and is
        // confined to effectful contexts.
        TRAVERSE_TRY | PAR_TRAVERSE_TRY => {
            if !arity(1, ctx) {
                return None;
            }
            let (ok_ty, err_ty) = check_try_fn_arg(&args[0], elem, &method.name, "List", ctx)?;
            if !ctx.effectful {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.effect.fn_value_in_pure_context",
                        span,
                        format!(
                            "`List.{}` runs an effectful function and cannot be called in a pure context",
                            method.name
                        ),
                    )
                    .with_note(
                        "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
                    ),
                );
            }
            Some(Ty::Effect(Box::new(Ty::Result(
                Box::new(Ty::List(Box::new(ok_ty))),
                Box::new(err_ty),
            ))))
        }
        // v0.88 (ADR 0116, query-algebra slice 1): the eager in-memory builder
        // and terminal vocabulary as kernel methods. Lazy storage queries reuse
        // these names over a `Query[T]` receiver (slice 2); here every chain is
        // eager — builders return the collection, terminals return `T`.
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `List.map` function", ctx)?;
            Some(Ty::List(Box::new(ret)))
        }
        "filter" => {
            if !arity(1, ctx) {
                return None;
            }
            let p = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Base(BaseType::Bool)),
            };
            check_arg(&args[0], &p, "the `List.filter` predicate", ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "flatMap" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `List.flatMap` function",
                ctx,
            )?;
            match ret {
                Ty::List(_) => Some(ret),
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `List.flatMap` function must return a `List`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "take" | "skip" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                &format!("the `List.{}` count", method.name),
                ctx,
            );
            Some(Ty::List(Box::new(elem.clone())))
        }
        "count" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        "any" | "all" => {
            if !arity(1, ctx) {
                return None;
            }
            let p = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Base(BaseType::Bool)),
            };
            check_arg(
                &args[0],
                &p,
                &format!("the `List.{}` predicate", method.name),
                ctx,
            );
            Some(Ty::Base(BaseType::Bool))
        }
        "first" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Option(Box::new(elem.clone())))
        }
        "firstOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], elem, "the `List.firstOrElse` fallback", ctx);
            Some(elem.clone())
        }
        // v0.88 (ADR 0116 D2/D3/D4): ordering and aggregate vocabulary. The
        // key is the projection's return type; orderable/numeric widen through
        // `Ty::base()` (so Opaque keys, whose base is hidden, are rejected —
        // ordering an opaque id is meaningless). Empty aggregates are total:
        // min/max/average -> Option, sum -> the zero (D4).
        "sortBy" => {
            if !arity(1, ctx) {
                return None;
            }
            let key =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `List.sortBy` key", ctx)?;
            require_orderable(&key, "List.sortBy", args[0].span, ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "distinct" => {
            if !arity(0, ctx) {
                return None;
            }
            require_keyable(elem, "List.distinct", span, ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "distinctBy" => {
            if !arity(1, ctx) {
                return None;
            }
            let key = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `List.distinctBy` key",
                ctx,
            )?;
            require_keyable(&key, "List.distinctBy", args[0].span, ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        "sum" => {
            if !arity(1, ctx) {
                return None;
            }
            let key = check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `List.sum` key", ctx)?;
            require_numeric(&key, "List.sum", args[0].span, ctx);
            Some(key)
        }
        "min" | "max" => {
            if !arity(1, ctx) {
                return None;
            }
            let key = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                &format!("the `List.{}` key", method.name),
                ctx,
            )?;
            require_orderable(&key, &format!("List.{}", method.name), args[0].span, ctx);
            Some(Ty::Option(Box::new(key)))
        }
        "average" => {
            if !arity(1, ctx) {
                return None;
            }
            let key =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `List.average` key", ctx)?;
            require_numeric(&key, "List.average", args[0].span, ctx);
            // D3: average of a Duration is a Duration (integer-rounded millis);
            // average of Int/Float is a Float (no truncation).
            let result = match key.base() {
                Some(BaseType::Duration) => Ty::Base(BaseType::Duration),
                _ => Ty::Base(BaseType::Float),
            };
            Some(Ty::Option(Box::new(result)))
        }
        // v0.94 (ADR 0116/0120): joins & grouping, combiner form. Each projects
        // its result through `into` (no pair type); the result is `List[V]`.
        "joinOn" => Some(Ty::List(Box::new(check_equi_join(
            args, elem, span, false, false, ctx,
        )?))),
        "leftJoin" => Some(Ty::List(Box::new(check_equi_join(
            args, elem, span, false, true, ctx,
        )?))),
        "join" => Some(Ty::List(Box::new(check_pred_join(
            args, elem, span, false, ctx,
        )?))),
        "groupBy" => Some(Ty::List(Box::new(check_group_by(
            args, elem, span, false, ctx,
        )?))),
        // v0.119 (ADR 0155, DECISION C-a): `run.upTo(step)` — the history strictly
        // before `step`. A history-scoped accessor: admitted only when the element
        // is a synthesised history `Step`, so it never leaks onto ordinary lists
        // (it collapses into a general `List` slice if one ever lands). Returns the
        // prefix as an ordinary `List[Step]`.
        "upTo"
            if matches!(&elem, Ty::Named { name, kind: NamedKind::Record }
                if name.starts_with("__History_") && name.ends_with("_Step")) =>
        {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], elem, "the `History.upTo` step", ctx);
            Some(Ty::List(Box::new(elem.clone())))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `List[{}]` type has no method `{}` — the kernel is `length`, `get`, `prepend`, `fold`, `foldEff`, `forEach`, `parTraverse`, `traverseAll`, `parTraverseAll`, `traverseTry`, `parTraverseTry`, `map`, `filter`, `flatMap`, `sortBy`, `take`, `skip`, `distinct`, `distinctBy`, `joinOn`, `leftJoin`, `join`, `groupBy`, `count`, `any`, `all`, `first`, `firstOrElse`, `sum`, `min`, `max`, `average`",
                    elem.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.91 (ADR 0115): the lazy query vocabulary names — the builders/terminals a
/// `store` map receiver lifts into a `Query` (vs the entry ops `put`/`get`/…).
/// `count` is the query terminal; the map's entry-count op is `size`.
pub(crate) fn is_query_op(name: &str) -> bool {
    matches!(
        name,
        "map"
            | "filter"
            | "flatMap"
            | "sortBy"
            | "take"
            | "skip"
            | "distinct"
            | "distinctBy"
            | "joinOn"
            | "leftJoin"
            | "join"
            | "groupBy"
            | "collect"
            | "first"
            | "firstOrElse"
            | "count"
            | "fold"
            | "any"
            | "all"
            | "sum"
            | "min"
            | "max"
            | "average"
            | "forEach"
            | "parTraverse"
            | "traverseAll"
            | "parTraverseAll"
            | "traverseTry"
            | "parTraverseTry"
    )
}

/// v0.91 (ADR 0115/0119, query-algebra slice 2): type a **lazy** storage-query
/// method on a `Query[T]` receiver (or a `store` field lifted into one). The
/// vocabulary mirrors the eager `List` one (ADR 0116), but **builders return
/// `Query[U]`** (still lazy, chainable) and **terminals return `Effect[T]`** (the
/// storage read folds into the agent's storage capability, ADR 0115 D5). `elem`
/// is the query's element type. Joins and `groupBy` arrive with slice 4.
pub(crate) fn check_query_kernel_method(
    method: &Ident,
    args: &[Expr],
    elem: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Query.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    let query = |t: Ty| Ty::Query(Box::new(t));
    let eff = |t: Ty| Ty::Effect(Box::new(t));
    match method.name.as_str() {
        // -- builders (return Query) --
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `Query.map` function",
                ctx,
            )?;
            Some(query(ret))
        }
        "filter" => {
            if !arity(1, ctx) {
                return None;
            }
            let p = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Base(BaseType::Bool)),
            };
            check_arg(&args[0], &p, "the `Query.filter` predicate", ctx);
            Some(query(elem.clone()))
        }
        "flatMap" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `Query.flatMap` function",
                ctx,
            )?;
            match ret {
                Ty::Query(_) => Some(ret),
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Query.flatMap` function must return a `Query`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "sortBy" => {
            if !arity(1, ctx) {
                return None;
            }
            let key =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `Query.sortBy` key", ctx)?;
            require_orderable(&key, "Query.sortBy", args[0].span, ctx);
            Some(query(elem.clone()))
        }
        "take" | "skip" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                &format!("the `Query.{}` count", method.name),
                ctx,
            );
            Some(query(elem.clone()))
        }
        "distinct" => {
            if !arity(0, ctx) {
                return None;
            }
            require_keyable(elem, "Query.distinct", span, ctx);
            Some(query(elem.clone()))
        }
        "distinctBy" => {
            if !arity(1, ctx) {
                return None;
            }
            let key = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `Query.distinctBy` key",
                ctx,
            )?;
            require_keyable(&key, "Query.distinctBy", args[0].span, ctx);
            Some(query(elem.clone()))
        }
        // -- terminals (return Effect) --
        "collect" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(eff(Ty::List(Box::new(elem.clone()))))
        }
        "first" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(eff(Ty::Option(Box::new(elem.clone()))))
        }
        "firstOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], elem, "the `Query.firstOrElse` fallback", ctx);
            Some(eff(elem.clone()))
        }
        "count" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(eff(Ty::Base(BaseType::Int)))
        }
        "fold" => {
            if !arity(2, ctx) {
                return None;
            }
            let acc = type_of(&args[0], None, ctx)?;
            let step = Ty::Fn {
                params: vec![acc.clone(), elem.clone()],
                ret: Box::new(acc.clone()),
            };
            check_arg(&args[1], &step, "the `Query.fold` step function", ctx);
            Some(eff(acc))
        }
        "any" | "all" => {
            if !arity(1, ctx) {
                return None;
            }
            let p = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Base(BaseType::Bool)),
            };
            check_arg(
                &args[0],
                &p,
                &format!("the `Query.{}` predicate", method.name),
                ctx,
            );
            Some(eff(Ty::Base(BaseType::Bool)))
        }
        "sum" => {
            if !arity(1, ctx) {
                return None;
            }
            let key =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `Query.sum` key", ctx)?;
            require_numeric(&key, "Query.sum", args[0].span, ctx);
            Some(eff(key))
        }
        "min" | "max" => {
            if !arity(1, ctx) {
                return None;
            }
            let key = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                &format!("the `Query.{}` key", method.name),
                ctx,
            )?;
            require_orderable(&key, &format!("Query.{}", method.name), args[0].span, ctx);
            Some(eff(Ty::Option(Box::new(key))))
        }
        "average" => {
            if !arity(1, ctx) {
                return None;
            }
            let key =
                check_kernel_fn_arg(&args[0], vec![elem.clone()], "the `Query.average` key", ctx)?;
            require_numeric(&key, "Query.average", args[0].span, ctx);
            let result = match key.base() {
                Some(BaseType::Duration) => Ty::Base(BaseType::Duration),
                _ => Ty::Base(BaseType::Float),
            };
            Some(eff(Ty::Option(Box::new(result))))
        }
        // v0.107 (slice 4): `forEach` runs the effectful function over each element
        // **in turn**; `parTraverse` runs them **concurrently** (the broadcast form —
        // a slow/half-dead connection must not head-of-line-block the rest). Same
        // type; the difference is sequential-vs-parallel lowering.
        "forEach" | "parTraverse" => {
            if !arity(1, ctx) {
                return None;
            }
            let f = Ty::Fn {
                params: vec![elem.clone()],
                ret: Box::new(Ty::Effect(Box::new(Ty::Unit))),
            };
            check_arg(
                &args[0],
                &f,
                &format!("the `Query.{}` function", method.name),
                ctx,
            );
            Some(eff(Ty::Unit))
        }
        // v0.149 (ADR 0173): the collect-all terminals over a lazy `Query`/a
        // lifted storage collection (the broadcast form for a held
        // `Map[K, Connection]`). `f: T -> Effect[Result[U, E]]`; every outcome is
        // gathered (no short-circuit) into `Effect[List[Result[U, E]]]`. The
        // `List.traverseAll`/`parTraverseAll` shape (ADR 0172) over the query
        // vocabulary, so the same fan-out reaches live connections.
        TRAVERSE_ALL | PAR_TRAVERSE_ALL => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                &format!("the `Query.{}` function", method.name),
                ctx,
            )?;
            let result_ty = match &ret {
                Ty::Effect(inner) => match &**inner {
                    r @ Ty::Result(_, _) => r.clone(),
                    other => {
                        ctx.errors.push(CompileError::new(
                            "bynk.types.argument_mismatch",
                            args[0].span,
                            format!(
                                "the `Query.{}` function must return `Effect[Result[U, E]]`, but returns `Effect[{}]`",
                                method.name,
                                other.display()
                            ),
                        ));
                        return None;
                    }
                },
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Query.{}` function must return `Effect[Result[U, E]]`, but returns `{}`",
                            method.name,
                            other.display()
                        ),
                    ));
                    return None;
                }
            };
            Some(eff(Ty::List(Box::new(result_ty))))
        }
        // v0.150 (ADR 0174): the short-circuit collect terminals over a lazy
        // `Query`/a lifted storage collection — stop at the first `Err`, returning
        // `Effect[Result[List[U], E]]`. The `List.traverseTry`/`parTraverseTry`
        // shape over the query broadcast, so the fan-out reaches live connections.
        TRAVERSE_TRY | PAR_TRAVERSE_TRY => {
            if !arity(1, ctx) {
                return None;
            }
            let (ok_ty, err_ty) = check_try_fn_arg(&args[0], elem, &method.name, "Query", ctx)?;
            Some(eff(Ty::Result(
                Box::new(Ty::List(Box::new(ok_ty))),
                Box::new(err_ty),
            )))
        }
        // v0.94 (ADR 0116/0120): joins & grouping are lazy builders — they project
        // through `into` and stay chainable as `Query[V]`.
        "joinOn" => Some(query(check_equi_join(args, elem, span, true, false, ctx)?)),
        "leftJoin" => Some(query(check_equi_join(args, elem, span, true, true, ctx)?)),
        "join" => Some(query(check_pred_join(args, elem, span, true, ctx)?)),
        "groupBy" => Some(query(check_group_by(args, elem, span, true, ctx)?)),
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Query[{}]` type has no method `{}` — builders are `map`/`filter`/`flatMap`/`sortBy`/`take`/`skip`/`distinct`/`distinctBy`/`joinOn`/`leftJoin`/`join`/`groupBy`, terminals `collect`/`first`/`firstOrElse`/`count`/`fold`/`any`/`all`/`sum`/`min`/`max`/`average`/`forEach`/`parTraverse`/`traverseAll`/`parTraverseAll`/`traverseTry`/`parTraverseTry`",
                    elem.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.100: type a built-in `Stream[T]` kernel method (real-time track slice 0).
/// The vocabulary is deliberately minimal at v1 — the lazy builders `map` and a
/// bounded `take` return `Stream[U]`/`Stream[T]`, and the single terminal
/// `collect` drains the stream into `Effect[List[T]]` (the observation point a
/// test asserts on). A fuller algebra (`filter`/`scan`/fan-in) earns its own
/// slice + ADR, as the query algebra did. `elem` is the stream's element type.
pub(crate) fn check_stream_kernel_method(
    method: &Ident,
    args: &[Expr],
    elem: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Stream.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    let stream = |t: Ty| Ty::Stream(Box::new(t));
    let eff = |t: Ty| Ty::Effect(Box::new(t));
    match method.name.as_str() {
        // -- builders (return Stream) --
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![elem.clone()],
                "the `Stream.map` function",
                ctx,
            )?;
            Some(stream(ret))
        }
        "take" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                "the `Stream.take` count",
                ctx,
            );
            Some(stream(elem.clone()))
        }
        // -- terminal (returns Effect) --
        "collect" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(eff(Ty::List(Box::new(elem.clone()))))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Stream[{}]` type has no method `{}` — builders are `map`/`take`, the terminal is `collect`",
                    elem.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.100: the `Stream.of(xs)` static constructor — builds a deterministic
/// `Stream[T]` from a `List[T]` (the in-memory source v1 ships; live runtime
/// sources arrive with their consumers, e.g. the streaming-HTTP terminal). The
/// element type is inferred from the argument list. Mirrors the
/// `Duration.millis` / `Instant.fromEpochMillis` static-constructor shape.
pub(crate) fn check_stream_static(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if method.name != OF {
        // The resolver owns the unknown-static diagnostic; don't double up.
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if args.len() != 1 {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!("`Stream.of` takes 1 argument, got {}", args.len()),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    match type_of(&args[0], None, ctx)? {
        Ty::List(elem) => Some(Ty::Stream(elem)),
        other => {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                args[0].span,
                format!(
                    "`Stream.of` expects a `List[T]`, but the argument is `{}`",
                    other.display()
                ),
            ));
            None
        }
    }
}

/// v0.102: type a held-resource operation on a `Connection[F]` (real-time track
/// slice 2). `send(f: F)` writes a frame and is **non-consuming** (the binding
/// stays owned); `close()` is **consuming** (ends the connection's life). Both
/// are effectful (`Effect[()]`). The *ownership* transitions these imply are
/// enforced by the linearity pass (§3 step 11); this only types the call.
/// `frame` is the connection's `F` (the server→client frame type).
pub(crate) fn check_connection_method(
    method: &Ident,
    args: &[Expr],
    frame: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let eff_unit = || Ty::Effect(Box::new(Ty::Unit));
    match method.name.as_str() {
        "send" => {
            if args.len() != 1 {
                ctx.errors.push(CompileError::new(
                    "bynk.types.method_arity",
                    span,
                    format!("`Connection.send` takes 1 argument, got {}", args.len()),
                ));
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                return None;
            }
            // The channel is typed: the frame must match `F` (a wrong-shaped
            // frame is a compile error at the `.send` site, §2.9.1).
            check_arg(&args[0], frame, "the `Connection.send` frame", ctx);
            Some(eff_unit())
        }
        "close" => {
            if !args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "bynk.types.method_arity",
                    span,
                    format!("`Connection.close` takes no arguments, got {}", args.len()),
                ));
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                return None;
            }
            Some(eff_unit())
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Connection[{}]` type has no method `{}` — the operations are `send` and `close`",
                    frame.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.21: type a built-in numeric kernel method (ADR 0041). Conversions are
/// value methods on the bare base types: `Int -> Float` is total
/// (`toFloat`); `Float -> Int` is one of four named, lossy roundings — there
/// is deliberately no ambiguous `toInt`. v0.22a (ADR 0048) extends the
/// kernel with `abs`/`min`/`max`/`clamp` on both numeric types and
/// `isNaN`/`isFinite` on `Float`.
pub(crate) fn check_numeric_kernel_method(
    method: &Ident,
    args: &[Expr],
    base: BaseType,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let b = Ty::Base(base);
    // (parameter types, return type)
    let sig: Option<(Vec<Ty>, Ty)> = match (base, method.name.as_str()) {
        (BaseType::Int, "toFloat") => Some((vec![], Ty::Base(BaseType::Float))),
        (BaseType::Float, "round" | "floor" | "ceil" | "truncate") => {
            Some((vec![], Ty::Base(BaseType::Int)))
        }
        (_, "abs") => Some((vec![], b.clone())),
        (_, "min" | "max") => Some((vec![b.clone()], b.clone())),
        (_, "clamp") => Some((vec![b.clone(), b.clone()], b.clone())),
        // v0.42 (ADR 0074): render a number as text — the missing direction
        // (`Int.parse` covers parsing). Total on both `Int` and `Float`.
        (_, "toString") => Some((vec![], Ty::Base(BaseType::String))),
        (BaseType::Float, "isNaN" | "isFinite") => Some((vec![], Ty::Base(BaseType::Bool))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        let kernel = match base {
            BaseType::Int => "`toFloat`, `toString`, `abs`, `min`, `max`, `clamp`",
            _ => {
                "`round`, `floor`, `ceil`, `truncate`, `toString`, `abs`, `min`, `max`, \
                 `clamp`, `isNaN`, `isFinite`"
            }
        };
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "the built-in `{}` type has no method `{}` — the kernel is {kernel}",
                base.name(),
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`{}.{}` takes {} argument{}, got {}",
                base.name(),
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (a, p) in args.iter().zip(&params) {
        check_arg(
            a,
            p,
            &format!("the `{}.{}` argument", base.name(), method.name),
            ctx,
        );
    }
    Some(ret)
}

/// v0.86 (ADR 0112): type a `Duration` kernel method. `Duration` arithmetic and
/// comparison are operators (D3/D4); the kernel is the explicit escape to raw
/// milliseconds (`toMillis`) plus `toString`.
pub(crate) fn check_duration_kernel_method(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "toMillis" => Some((vec![], Ty::Base(BaseType::Int))),
        "toString" => Some((vec![], Ty::Base(BaseType::String))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "the built-in `Duration` type has no method `{}` — the kernel is `toMillis`, `toString`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`Duration.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    Some(ret)
}

/// v0.90 (ADR 0114 D6): the `Instant` kernel — the explicit escape to raw epoch
/// milliseconds. Comparison/arithmetic are operators (D3); the static
/// `Instant.fromEpochMillis(n)` (the build-from-`Int` direction) is resolved
/// separately as a type static.
pub(crate) fn check_instant_kernel_method(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "toEpochMillis" => Some((vec![], Ty::Base(BaseType::Int))),
        "toString" => Some((vec![], Ty::Base(BaseType::String))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "the built-in `Instant` type has no method `{}` — the kernel is `toEpochMillis`, `toString`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`Instant.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    Some(ret)
}

/// v0.110 (ADR 0142 D3/D4): the `Bytes` kernel — `length` (octet count) plus
/// the String-interop bridge. `toBase64` is total; `decodeUtf8` is partial
/// (`None` on an invalid UTF-8 sequence). Equality is an operator (D4, content
/// compare); the statics `Bytes.fromUtf8`/`fromBase64`/`empty` (the
/// build-from-`String` direction) are resolved separately as type statics.
pub(crate) fn check_bytes_kernel_method(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "length" => Some((vec![], Ty::Base(BaseType::Int))),
        "toBase64" => Some((vec![], Ty::Base(BaseType::String))),
        "decodeUtf8" => Some((vec![], Ty::Option(Box::new(Ty::Base(BaseType::String))))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "the built-in `Bytes` type has no method `{}` — the kernel is `length`, `toBase64`, `decodeUtf8`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`Bytes.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    Some(ret)
}

/// v0.22a: type a built-in `String` kernel method (ADR 0046). `String` is
/// opaque (no char access), so its operations are compiler built-ins
/// lowering to TS string methods — the 0034/0037 hybrid posture. Semantics
/// are UTF-16 code units, except `chars()` (code points, normatively).
pub(crate) fn check_string_kernel_method(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let s = Ty::Base(BaseType::String);
    let int = Ty::Base(BaseType::Int);
    let boolean = Ty::Base(BaseType::Bool);
    let strings = Ty::List(Box::new(s.clone()));
    // (parameter types, return type)
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "length" => Some((vec![], int.clone())),
        "split" => Some((vec![s.clone()], strings.clone())),
        "trim" | "toUpper" | "toLower" => Some((vec![], s.clone())),
        "contains" | "startsWith" | "endsWith" => Some((vec![s.clone()], boolean)),
        "replace" => Some((vec![s.clone(), s.clone()], s.clone())),
        "slice" => Some((vec![int.clone(), int.clone()], s.clone())),
        "indexOf" => Some((vec![s.clone()], Ty::Option(Box::new(int)))),
        "chars" => Some((vec![], strings)),
        "concat" => Some((vec![s.clone()], s.clone())),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "the built-in `String` type has no method `{}` — the kernel is \
                 `length`, `split`, `trim`, `contains`, `startsWith`, `endsWith`, \
                 `replace`, `slice`, `indexOf`, `toUpper`, `toLower`, `chars`, `concat`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`String.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (a, p) in args.iter().zip(&params) {
        check_arg(a, p, &format!("the `String.{}` argument", method.name), ctx);
    }
    Some(ret)
}

/// v0.22a: type a function-valued kernel argument (the 0048 combinators —
/// `map`/`andThen`/`mapErr`). Parameter types are known from the receiver;
/// the return is read from the actual: an expected return carrying a
/// flexible variable lets a lambda type bottom-up (the v0.20a pass-2 rule),
/// and `unify` captures it here. Returns the function's return type.
fn check_kernel_fn_arg(arg: &Expr, params: Vec<Ty>, label: &str, ctx: &mut Ctx) -> Option<Ty> {
    const RET_VAR: &str = "__kernel_ret";
    let expected = Ty::Fn {
        params: params.clone(),
        ret: Box::new(Ty::Var(RET_VAR.to_string())),
    };
    let actual = type_of(arg, Some(&expected), ctx)?;
    let mut subst: HashMap<String, Ty> = HashMap::new();
    if unify(&expected, &actual, &mut subst)
        && let Some(ret) = subst.get(RET_VAR).cloned()
    {
        // `unify` is permissive ground-vs-ground; re-check the whole shape.
        let want = Ty::Fn {
            params,
            ret: Box::new(ret.clone()),
        };
        if compatible(&actual, &want) {
            return Some(ret);
        }
    }
    ctx.errors.push(CompileError::new(
        "bynk.types.argument_mismatch",
        arg.span,
        format!(
            "{label} expects a function over the receiver's value, but got `{}`",
            actual.display()
        ),
    ));
    None
}

/// v0.150 (ADR 0174): infer a `traverseTry`/`parTraverseTry` function argument's
/// return type, which MUST be `Effect[Result[U, E]]`, and peel it to `(U, E)`.
/// `recv` is the collection's element type; `kind` is `"List"` or `"Query"` (for
/// the diagnostic). A non-`Result` effect is `bynk.types.argument_mismatch`.
fn check_try_fn_arg(
    arg: &Expr,
    elem: &Ty,
    method: &str,
    kind: &str,
    ctx: &mut Ctx,
) -> Option<(Ty, Ty)> {
    let ret = check_kernel_fn_arg(
        arg,
        vec![elem.clone()],
        &format!("the `{kind}.{method}` function"),
        ctx,
    )?;
    match &ret {
        Ty::Effect(inner) => match &**inner {
            Ty::Result(ok, err) => Some(((**ok).clone(), (**err).clone())),
            other => {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    arg.span,
                    format!(
                        "the `{kind}.{method}` function must return `Effect[Result[U, E]]`, but returns `Effect[{}]`",
                        other.display()
                    ),
                ));
                None
            }
        },
        other => {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                arg.span,
                format!(
                    "the `{kind}.{method}` function must return `Effect[Result[U, E]]`, but returns `{}`",
                    other.display()
                ),
            ));
            None
        }
    }
}

/// v0.22a: type a built-in `Option[T]` kernel method (ADR 0048) — the
/// combinators as value methods on the compiler-known receiver (collision-
/// free, unlike free functions imported by bare name).
pub(crate) fn check_option_kernel_method(
    method: &Ident,
    args: &[Expr],
    inner: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Option.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![inner.clone()],
                "the `Option.map` function",
                ctx,
            )?;
            Some(Ty::Option(Box::new(ret)))
        }
        "andThen" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![inner.clone()],
                "the `Option.andThen` function",
                ctx,
            )?;
            match ret {
                Ty::Option(_) => Some(ret),
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Option.andThen` function must return an `Option`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "getOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], inner, "the `Option.getOrElse` fallback", ctx);
            Some(inner.clone())
        }
        "isSome" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        "okOr" => {
            if !arity(1, ctx) {
                return None;
            }
            let err = type_of(&args[0], None, ctx)?;
            Some(Ty::Result(Box::new(inner.clone()), Box::new(err)))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Option[{}]` type has no method `{}` — the kernel is \
                     `map`, `andThen`, `getOrElse`, `isSome`, `okOr`",
                    inner.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22a: type a built-in `Result[T, E]` kernel method (ADR 0048).
pub(crate) fn check_result_kernel_method(
    method: &Ident,
    args: &[Expr],
    ok: &Ty,
    err: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Result.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "map" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret =
                check_kernel_fn_arg(&args[0], vec![ok.clone()], "the `Result.map` function", ctx)?;
            Some(Ty::Result(Box::new(ret), Box::new(err.clone())))
        }
        "andThen" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![ok.clone()],
                "the `Result.andThen` function",
                ctx,
            )?;
            match ret {
                Ty::Result(b, e2) => {
                    if !compatible(&e2, err) && !compatible(err, &e2) {
                        ctx.errors.push(CompileError::new(
                            "bynk.types.argument_mismatch",
                            args[0].span,
                            format!(
                                "the `Result.andThen` function's error type `{}` does not match the receiver's `{}`",
                                e2.display(),
                                err.display()
                            ),
                        ));
                        return None;
                    }
                    Some(Ty::Result(b, Box::new(err.clone())))
                }
                other => {
                    ctx.errors.push(CompileError::new(
                        "bynk.types.argument_mismatch",
                        args[0].span,
                        format!(
                            "the `Result.andThen` function must return a `Result`, but returns `{}`",
                            other.display()
                        ),
                    ));
                    None
                }
            }
        }
        "mapErr" => {
            if !arity(1, ctx) {
                return None;
            }
            let ret = check_kernel_fn_arg(
                &args[0],
                vec![err.clone()],
                "the `Result.mapErr` function",
                ctx,
            )?;
            Some(Ty::Result(Box::new(ok.clone()), Box::new(ret)))
        }
        "getOrElse" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], ok, "the `Result.getOrElse` fallback", ctx);
            Some(ok.clone())
        }
        "isOk" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Bool))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Result[{}, {}]` type has no method `{}` — the kernel is \
                     `map`, `andThen`, `mapErr`, `getOrElse`, `isOk`",
                    ok.display(),
                    err.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22b: whether a type can pass through the typed JSON codec — every
/// boundary-serialisable shape: bases, named types, and the built-in
/// generic containers over them. Functions, effects, `HttpResult`, the
/// error builtins, and type variables cannot.
fn json_codable(t: &Ty) -> bool {
    match t {
        Ty::Base(_) | Ty::Named { .. } | Ty::Unit => true,
        Ty::Result(a, b) => json_codable(a) && json_codable(b),
        Ty::Option(a) | Ty::List(a) => json_codable(a),
        Ty::Map(k, v) => json_codable(k) && json_codable(v),
        Ty::Fn { .. }
        | Ty::Effect(_)
        | Ty::Query(_)
        | Ty::Stream(_)
        | Ty::Connection(_)
        | Ty::HttpResult(_)
        | Ty::QueueResult
        | Ty::ValidationError
        | Ty::JsonError
        | Ty::Var(_)
        | Ty::Actor(_)
        | Ty::ActorSum(_) => false,
    }
}

/// v0.22b: type the `Json` codec statics (ADR 0045). `encode(v) -> String`
/// over any codable value (it throws on a non-finite `Float`, per 0040 —
/// documented, not typed); `decode[T](s) -> Result[T, JsonError]` with `T`
/// explicit (`Json.decode[Order](s)`) or inferred from an expected
/// `Result[T, JsonError]`.
pub(crate) fn check_json_static(
    method: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity1 = |ctx: &mut Ctx| {
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Json.{}` takes 1 argument, got {}",
                    method.name,
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "encode" => {
            if !type_args.is_empty() {
                ctx.errors.push(CompileError::new(
                    "bynk.generics.type_arg_mismatch",
                    span,
                    "`Json.encode` takes no type arguments — its type comes from the value",
                ));
            }
            if !arity1(ctx) {
                return None;
            }
            let t = type_of(&args[0], None, ctx)?;
            if !json_codable(&t) {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.types.json_uncodable",
                        args[0].span,
                        format!("`{}` cannot be encoded as JSON", t.display()),
                    )
                    .with_note(
                        "the codec covers base types, named types, and the built-in \
                         containers over them — not functions, effects, or error builtins",
                    ),
                );
                return None;
            }
            Some(Ty::Base(BaseType::String))
        }
        "decode" => {
            if !arity1(ctx) {
                return None;
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::String),
                "the `Json.decode` input",
                ctx,
            );
            let t = match type_args {
                [one] => resolve_type_ref(one, &ctx.input.types)?,
                [] => match expected {
                    Some(Ty::Result(t, e)) if **e == Ty::JsonError => (**t).clone(),
                    _ => {
                        ctx.errors.push(
                            CompileError::new(
                                "bynk.generics.uninferable_type_arg",
                                span,
                                "cannot infer the target type of `Json.decode`",
                            )
                            .with_note(
                                "give it explicitly (`Json.decode[Order](s)`) or use the \
                                 result where a `Result[T, JsonError]` is expected",
                            ),
                        );
                        return None;
                    }
                },
                _ => {
                    ctx.errors.push(CompileError::new(
                        "bynk.generics.type_arg_mismatch",
                        span,
                        format!(
                            "`Json.decode` takes exactly one type argument, got {}",
                            type_args.len()
                        ),
                    ));
                    return None;
                }
            };
            if !json_codable(&t) || t == Ty::Unit {
                ctx.errors.push(
                    CompileError::new(
                        "bynk.types.json_uncodable",
                        span,
                        format!("`{}` cannot be decoded from JSON", t.display()),
                    )
                    .with_note(
                        "the codec covers base types, named types, and the built-in \
                         containers over them — not functions, effects, or error builtins",
                    ),
                );
                return None;
            }
            Some(Ty::Result(Box::new(t), Box::new(Ty::JsonError)))
        }
        _ => {
            // The resolver owns the unknown-static diagnostic.
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.22a: type the numeric parse statics — `Int.parse(s)` /
/// `Float.parse(s)` (`-> Option[T]`, ADR 0048). Parsing is full-string
/// (trailing garbage is `None`, unlike `parseFloat`); an out-of-safe-range
/// `Int` or non-finite `Float` is `None`.
pub(crate) fn check_numeric_parse_static(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if method.name != "parse" {
        // The resolver owns the unknown-static diagnostic; don't double up.
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if args.len() != 1 {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`{}.parse` takes 1 argument, got {}",
                type_name.name,
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    check_arg(
        &args[0],
        &Ty::Base(BaseType::String),
        &format!("the `{}.parse` argument", type_name.name),
        ctx,
    );
    let inner = if type_name.name == INT {
        BaseType::Int
    } else {
        BaseType::Float
    };
    Some(Ty::Option(Box::new(Ty::Base(inner))))
}

/// v0.86 (ADR 0112): type the `Duration.millis(n: Int) -> Duration` static
/// constructor — building a `Duration` from a runtime `Int` of milliseconds.
pub(crate) fn check_duration_static(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if method.name != "millis" {
        // The resolver owns the unknown-static diagnostic; don't double up.
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if args.len() != 1 {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!("`Duration.millis` takes 1 argument, got {}", args.len()),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    check_arg(
        &args[0],
        &Ty::Base(BaseType::Int),
        "the `Duration.millis` argument",
        ctx,
    );
    Some(Ty::Base(BaseType::Duration))
}

/// v0.90 (ADR 0114 D6): the `Instant.fromEpochMillis(n)` static constructor —
/// the way to build an `Instant` from a runtime `Int` (a wire/stored epoch
/// value); instants are otherwise minted only by `Clock.now()`.
pub(crate) fn check_instant_static(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if method.name != "fromEpochMillis" {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if args.len() != 1 {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`Instant.fromEpochMillis` takes 1 argument, got {}",
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    check_arg(
        &args[0],
        &Ty::Base(BaseType::Int),
        "the `Instant.fromEpochMillis` argument",
        ctx,
    );
    Some(Ty::Base(BaseType::Instant))
}

/// v0.110 (ADR 0142 D2): the `Bytes` static constructors — the only way to
/// build a `Bytes` (no source literal). `fromUtf8(s)` is total (the UTF-8
/// encoding of a string); `fromBase64(s)` is partial (`None` on invalid
/// base64); `empty()` is the zero value (the empty octet sequence).
pub(crate) fn check_bytes_static(
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // (params, return) keyed on the static name; `None` = unknown (the
    // resolver owns the unknown-static diagnostic; don't double up).
    let sig: Option<(Vec<Ty>, Ty)> = match method.name.as_str() {
        "fromUtf8" => Some((vec![Ty::Base(BaseType::String)], Ty::Base(BaseType::Bytes))),
        "fromBase64" => Some((
            vec![Ty::Base(BaseType::String)],
            Ty::Option(Box::new(Ty::Base(BaseType::Bytes))),
        )),
        "empty" => Some((vec![], Ty::Base(BaseType::Bytes))),
        _ => None,
    };
    let Some((params, ret)) = sig else {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    if args.len() != params.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_arity",
            span,
            format!(
                "`Bytes.{}` takes {} argument{}, got {}",
                method.name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (a, p) in args.iter().zip(params.iter()) {
        check_arg(a, p, &format!("the `Bytes.{}` argument", method.name), ctx);
    }
    Some(ret)
}

/// v0.20b: type a built-in `Map[K, V]` kernel method.
pub(crate) fn check_map_kernel_method(
    method: &Ident,
    args: &[Expr],
    key: &Ty,
    val: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_arity",
                span,
                format!(
                    "`Map.{}` takes {n} argument{}, got {}",
                    method.name,
                    if n == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    match method.name.as_str() {
        "length" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::Base(BaseType::Int))
        }
        "keys" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::List(Box::new(key.clone())))
        }
        // v0.149 (ADR 0173): the `keys` sibling — the values in key order.
        "values" => {
            if !arity(0, ctx) {
                return None;
            }
            Some(Ty::List(Box::new(val.clone())))
        }
        "get" => {
            if !arity(1, ctx) {
                return None;
            }
            check_arg(&args[0], key, "the `Map.get` key", ctx);
            Some(Ty::Option(Box::new(val.clone())))
        }
        "insert" => {
            if !arity(2, ctx) {
                return None;
            }
            check_arg(&args[0], key, "the `Map.insert` key", ctx);
            check_arg(&args[1], val, "the `Map.insert` value", ctx);
            Some(Ty::Map(Box::new(key.clone()), Box::new(val.clone())))
        }
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Map[{}, {}]` type has no method `{}` — the kernel is `length`, `keys`, `values`, `get`, `insert`",
                    key.display(),
                    val.display(),
                    method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            None
        }
    }
}
