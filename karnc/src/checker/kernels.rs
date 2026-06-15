//! Kernel methods and collection / JSON statics.
//!
//! Split out of `checker.rs` (v0.29.10) verbatim; the parent module
//! re-exports these via `use kernels::*`.

use super::*;

/// v0.20b: `List.empty()` / `Map.empty()` — the built-in collection statics.
/// Their element/key/value types are exactly as uninferable as an empty
/// `[]`, so they share `karn.types.uninferable_element_type`. The resolver
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
            "karn.types.method_arity",
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
                "karn.types.uninferable_element_type",
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
                "karn.types.method_arity",
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
                        "karn.effect.fn_value_in_pure_context",
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
        _ => {
            ctx.errors.push(CompileError::new(
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `List[{}]` type has no method `{}` — the kernel is `length`, `get`, `prepend`, `fold`, `foldEff`",
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
            "karn.types.method_not_found",
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
            "karn.types.method_arity",
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
            "karn.types.method_not_found",
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
            "karn.types.method_arity",
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
        "karn.types.argument_mismatch",
        arg.span,
        format!(
            "{label} expects a function over the receiver's value, but got `{}`",
            actual.display()
        ),
    ));
    None
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
                "karn.types.method_arity",
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
                        "karn.types.argument_mismatch",
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
                "karn.types.method_not_found",
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
                "karn.types.method_arity",
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
                            "karn.types.argument_mismatch",
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
                        "karn.types.argument_mismatch",
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
                "karn.types.method_not_found",
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
        | Ty::HttpResult(_)
        | Ty::ValidationError
        | Ty::JsonError
        | Ty::Var(_) => false,
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
                "karn.types.method_arity",
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
                    "karn.generics.type_arg_mismatch",
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
                        "karn.types.json_uncodable",
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
                                "karn.generics.uninferable_type_arg",
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
                        "karn.generics.type_arg_mismatch",
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
                        "karn.types.json_uncodable",
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
            "karn.types.method_arity",
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
                "karn.types.method_arity",
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
                "karn.types.method_not_found",
                method.span,
                format!(
                    "the built-in `Map[{}, {}]` type has no method `{}` — the kernel is `length`, `keys`, `get`, `insert`",
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
