//! Call / application dispatch.
//!
//! Split out of `checker.rs` (v0.29.10) verbatim; the parent module
//! re-exports these via `use calls::*`.

use super::*;

/// v0.39 (ADR 0072): record a parameter-name inlay hint for one call argument,
/// unless it would be noise — the `_`/`self` placeholders, or an argument that
/// is already the identically-named identifier (`f(count)` for parameter
/// `count`, matching rust-analyzer's suppression).
fn record_param_hint(hints: &mut HintSink, param_name: &str, arg: &Expr) {
    if param_name == "_" || param_name == "self" {
        return;
    }
    if let ExprKind::Ident(id) = &arg.kind
        && id.name == param_name
    {
        return;
    }
    hints.record_param(arg.span, format!("{param_name}:"));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_fn(
    f: &FnDecl,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
) {
    // v0.20a: the fn's type parameters are *rigid* type variables while
    // checking its own body. A type param shadowing a declared type is
    // confusing — diagnose the collision.
    let vars: HashSet<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    for tp in &f.type_params {
        if input.types.contains_key(&tp.name.name) {
            errors.push(
                CompileError::new(
                    "bynk.generics.type_arg_mismatch",
                    tp.span,
                    format!(
                        "type parameter `{}` shadows the declared type of the same name",
                        tp.name.name
                    ),
                )
                .with_note("rename the type parameter"),
            );
        }
    }
    let return_ty = match resolve_type_ref_in(&f.return_type, &input.types, &vars) {
        Some(t) => t,
        None => return,
    };
    record_type_refs(&f.return_type, &input.types, &vars, refs);
    let mut param_scope: HashMap<String, Ty> = HashMap::new();
    // For methods, the implicit `self` parameter has the attached type.
    if let FnName::Method { type_name, .. } = &f.name
        && f.has_self
        && let Some(self_ty) = type_from_decl(type_name, &input.types)
    {
        param_scope.insert("self".to_string(), self_ty);
    }
    for p in &f.params {
        if let Some(ty) = resolve_type_ref_in(&p.type_ref, &input.types, &vars) {
            record_type_refs(&p.type_ref, &input.types, &vars, refs);
            // v0.31: a fn parameter is in scope over the whole body.
            if p.name.name != "_" {
                locals.record(p.name.name.clone(), p.name.span, ty.display(), f.body.span);
            }
            param_scope.insert(p.name.name.clone(), ty);
        }
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        refs,
        hints,
        locals,
        scopes: vec![param_scope],
        return_ty: return_ty.clone(),
        return_ty_span: f.return_type.span(),
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        caps: CapabilityCtx::default(),
        in_test_body: false,
        test_services: HashSet::new(),
        type_vars: vars.clone(),
    };
    let Some(body_ty) = type_of_block(&f.body, Some(&return_ty), &mut ctx) else {
        return;
    };
    if !compatible(&body_ty, &return_ty) {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.return_mismatch",
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

/// v0.11: type-check an agent state-field initialiser (`field: T = init`). The
/// initialiser must be a *static* value of the field type — it is checked in an
/// empty, pure scope (so `self`, parameters, capabilities, and effects are all
/// out of reach) with the field type as the expected type, so refined literals
/// admit (v0.9.4) and sum variants resolve. The init's expression types are
/// recorded into `expr_types` for emission; a single
/// `bynk.agents.bad_state_initialiser` is pushed on any failure.
#[allow(clippy::too_many_arguments)]
pub fn check_state_initialiser(
    init: &Expr,
    field_type: &TypeRef,
    input: &ResolvedCommons,
    expr_types: &mut HashMap<Span, Ty>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
) {
    let Some(field_ty) = resolve_type_ref(field_type, &input.types) else {
        return; // an unresolved field type is reported elsewhere
    };
    let mut local_errors: Vec<CompileError> = Vec::new();
    let result = {
        let mut ctx = Ctx {
            input,
            expr_types,
            errors: &mut local_errors,
            refs,
            hints,
            locals,
            scopes: vec![HashMap::new()],
            return_ty: field_ty.clone(),
            return_ty_span: init.span,
            effectful: false,
            agent_state_ty: None,
            commit_seen: false,
            caps: CapabilityCtx::default(),
            in_test_body: false,
            test_services: HashSet::new(),
            type_vars: HashSet::new(),
        };
        type_of(init, Some(&field_ty), &mut ctx)
    };
    let compatible_result = matches!(&result, Some(t) if compatible(t, &field_ty));
    if !compatible_result || !local_errors.is_empty() {
        let got = result
            .as_ref()
            .map(|t| t.display())
            .unwrap_or_else(|| "an invalid expression".to_string());
        errors.push(
            CompileError::new(
                "bynk.agents.bad_state_initialiser",
                init.span,
                format!(
                    "state field initialiser must be a static value of type `{}` (got `{got}`)",
                    field_ty.display(),
                ),
            )
            .with_note(
                "an initialiser is a compile-time value — a literal, a sum variant, \
                 `Some`/`None`/`Ok`/`Err`, a record, or `T.unsafe(lit)` — with no reference to \
                 `self`, parameters, or capabilities",
            ),
        );
    }
}

pub(crate) fn check_call(
    name: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if let Some(fn_decl) = ctx.input.fns.get(&name.name).cloned() {
        ctx.refs.record(name.span, SymbolKind::Fn, &name.name);
        return check_call_against_fn(name, &fn_decl, type_args, args, ctx);
    }
    // v0.20a: explicit type arguments only apply to (generic) functions.
    if !type_args.is_empty() {
        ctx.errors.push(CompileError::new(
            "bynk.generics.type_arg_mismatch",
            span,
            format!(
                "`{}` is not a generic function — it takes no type arguments",
                name.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
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
    // Agent instantiation: `AgentName(key)` constructs an instance keyed by
    // `key`. The result type carries the agent's name so subsequent
    // `agent_instance.method(args)` lookups can find the agent's handler set.
    if let Some(agent) = ctx.input.agents.get(&name.name).cloned() {
        ctx.refs.record(name.span, SymbolKind::Agent, &name.name);
        let key_ty = resolve_type_ref(&agent.key_type, &ctx.input.types);
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "bynk.agent.construction_arity",
                span,
                format!(
                    "agent `{}` is constructed with one key argument, but {} were given",
                    name.name,
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        let arg_ty = type_of(&args[0], key_ty.as_ref(), ctx);
        if let (Some(a), Some(k)) = (arg_ty.as_ref(), key_ty.as_ref())
            && !compatible(a, k)
        {
            ctx.errors.push(CompileError::new(
                "bynk.agent.key_mismatch",
                args[0].span,
                format!(
                    "agent `{}` key is `{}`, but a value of type `{}` was given",
                    name.name,
                    k.display(),
                    a.display()
                ),
            ));
        }
        return Some(Ty::Named {
            name: name.name.clone(),
            kind: NamedKind::Record,
        });
    }
    // v0.20a: value application — calling a scope binding (param/local) of
    // function type. Placed AFTER fns/variants/agents: putting scope first
    // would change the meaning of currently-passing programs (the additive
    // guard); the resulting ident/call precedence asymmetry is pre-existing
    // and documented in §5.
    if let Some(ty) = ctx.lookup(&name.name) {
        return match ty {
            Ty::Fn { params, ret } => check_value_application(name, &params, &ret, args, span, ctx),
            other => {
                // Relocated from the resolver (which has no type info): a
                // non-function-typed value called as a function.
                ctx.errors.push(
                    CompileError::new(
                        "bynk.resolve.param_as_function",
                        span,
                        format!(
                            "`{}` has type `{}` and is not callable",
                            name.name,
                            other.display()
                        ),
                    )
                    .with_note("only values of function type can be applied"),
                );
                for a in args {
                    let _ = type_of(a, None, ctx);
                }
                None
            }
        };
    }
    let _ = span;
    None
}

fn check_value_application(
    name: &Ident,
    params: &[Ty],
    ret: &Ty,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    if ret.is_effect() && !ctx.effectful {
        ctx.errors.push(
            CompileError::new(
                "bynk.effect.fn_value_in_pure_context",
                span,
                format!(
                    "`{}` is an effectful function (`{}`) and cannot be called in a pure context",
                    name.name,
                    Ty::Fn {
                        params: params.to_vec(),
                        ret: Box::new(ret.clone())
                    }
                    .display()
                ),
            )
            .with_note(
                "effectful function values may only be called where the enclosing body is effectful (its return type is an Effect)",
            ),
        );
    }
    if params.len() != args.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.call_arity",
            span,
            format!(
                "`{}` takes {} argument(s), but {} were given",
                name.name,
                params.len(),
                args.len()
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    for (arg, param_ty) in args.iter().zip(params) {
        let arg_ty = type_of(arg, Some(param_ty), ctx);
        if let Some(a) = arg_ty.as_ref()
            && !compatible(a, param_ty)
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                arg.span,
                format!(
                    "argument has type `{}`, but `{}` expects `{}`",
                    a.display(),
                    name.name,
                    param_ty.display()
                ),
            ));
        }
    }
    Some(ret.clone())
}

/// v0.20a: instantiate and check a call to a generic function. Two-pass
/// argument-directed inference: pass 1 types every **non-lambda** argument
/// left-to-right against the (possibly still Var-bearing) expected — safe
/// because every expected-driven feature in `type_of` matches concrete
/// variants, so a Var falls through benignly (pinned by a unit test) — and
/// unifies; pass 2 types **lambda** arguments against the now-substituted
/// expecteds (a lambda whose expected params are still Var-bearing is
/// uninferable) and unifies the result, capturing return-position variables.
/// Conflicts demand exact equality (`bynk.generics.type_arg_mismatch`); the
/// explicit `name[T](…)` form builds the substitution directly.
fn check_generic_call(
    name: &Ident,
    fn_decl: &FnDecl,
    type_args: &[TypeRef],
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    let vars: HashSet<String> = fn_decl
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    if fn_decl.params.len() != args.len() {
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    let var_params: Vec<Option<Ty>> = fn_decl
        .params
        .iter()
        .map(|p| resolve_type_ref_in(&p.type_ref, &ctx.input.types, &vars))
        .collect();
    let ret_pattern = resolve_type_ref_in(&fn_decl.return_type, &ctx.input.types, &vars)?;

    let mut subst: HashMap<String, Ty> = HashMap::new();
    if !type_args.is_empty() {
        if type_args.len() != fn_decl.type_params.len() {
            ctx.errors.push(CompileError::new(
                "bynk.generics.type_arg_mismatch",
                name.span,
                format!(
                    "`{}` takes {} type argument(s), but {} were given",
                    name.name,
                    fn_decl.type_params.len(),
                    type_args.len()
                ),
            ));
            return None;
        }
        for (tp, ta) in fn_decl.type_params.iter().zip(type_args) {
            // Resolve explicit type args with the *enclosing* fn's type
            // params in scope, so `identity[A](x)` inside a generic body
            // works.
            let ty = resolve_type_ref_in(ta, &ctx.input.types, &ctx.type_vars)?;
            subst.insert(tp.name.name.clone(), ty);
        }
    }

    let mut arg_tys: Vec<Option<Ty>> = vec![None; args.len()];
    // Pass 1 — non-lambda arguments.
    for (i, arg) in args.iter().enumerate() {
        if matches!(arg.kind, ExprKind::Lambda(_)) {
            continue;
        }
        let expected = var_params[i].as_ref().map(|p| substitute(p, &subst));
        let ty = type_of(arg, expected.as_ref(), ctx);
        if let (Some(pattern), Some(actual)) = (var_params[i].as_ref(), ty.as_ref())
            && !unify(pattern, actual, &mut subst)
        {
            ctx.errors.push(CompileError::new(
                "bynk.generics.type_arg_mismatch",
                arg.span,
                format!(
                    "argument {} infers a type for `{}`'s type parameter that conflicts with an earlier argument — annotate with `{}[T](…)`",
                    i + 1,
                    name.name,
                    name.name
                ),
            ));
            return None;
        }
        arg_tys[i] = ty;
    }
    // Pass 2 — lambda arguments, against substituted expecteds.
    for (i, arg) in args.iter().enumerate() {
        if !matches!(arg.kind, ExprKind::Lambda(_)) {
            continue;
        }
        let expected = var_params[i].as_ref().map(|p| substitute(p, &subst));
        let params_unconstrained = matches!(
            expected.as_ref(),
            Some(Ty::Fn { params, .. }) if params.iter().any(contains_var)
        );
        let fully_annotated = matches!(
            &arg.kind,
            ExprKind::Lambda(l) if l.params.iter().all(|p| p.type_ref.is_some())
        );
        if params_unconstrained && !fully_annotated {
            ctx.errors.push(
                CompileError::new(
                    "bynk.generics.uninferable_type_arg",
                    arg.span,
                    format!(
                        "the lambda's parameter types depend on `{}`'s type parameters, which the other arguments do not determine",
                        name.name
                    ),
                )
                .with_note("annotate the lambda's parameters, or give explicit type arguments: `name[T](…)`"),
            );
            return None;
        }
        // A fully-annotated lambda grounds the variables itself: type it
        // bottom-up and let unify capture them.
        let ty = if params_unconstrained {
            type_of(arg, None, ctx)
        } else {
            type_of(arg, expected.as_ref(), ctx)
        };
        if let (Some(pattern), Some(actual)) = (var_params[i].as_ref(), ty.as_ref())
            && !unify(pattern, actual, &mut subst)
        {
            ctx.errors.push(CompileError::new(
                "bynk.generics.type_arg_mismatch",
                arg.span,
                format!(
                    "the lambda's type conflicts with `{}`'s inferred type arguments",
                    name.name
                ),
            ));
            return None;
        }
        arg_tys[i] = ty;
    }
    // Every type parameter must now be determined.
    for tp in &fn_decl.type_params {
        if !subst.contains_key(&tp.name.name) {
            ctx.errors.push(
                CompileError::new(
                    "bynk.generics.uninferable_type_arg",
                    name.span,
                    format!(
                        "type parameter `{}` of `{}` is neither inferable from the arguments nor given explicitly",
                        tp.name.name, name.name
                    ),
                )
                .with_label(tp.span, "declared here")
                .with_note("give explicit type arguments: `name[T](…)`"),
            );
            return None;
        }
    }
    // Final compatibility over the fully-ground parameter types.
    let mut ok = true;
    for (i, (pattern, arg)) in var_params.iter().zip(args).enumerate() {
        record_param_hint(ctx.hints, &fn_decl.params[i].name.name, arg);
        let (Some(pattern), Some(arg_ty)) = (pattern.as_ref(), arg_tys[i].as_ref()) else {
            continue;
        };
        let ground = substitute(pattern, &subst);
        if !compatible(arg_ty, &ground) {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                arg.span,
                format!(
                    "argument {} to `{}` has type `{}`, but `{}` is expected",
                    i + 1,
                    name.name,
                    arg_ty.display(),
                    ground.display()
                ),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    // v0.39 (ADR 0072): when the user omitted the type arguments, show the
    // inferred ones as a `Type`-kind hint after the function name —
    // `identity` ⟨`[Int]`⟩ `(5)`. Declaration order; skipped if any var stayed
    // unresolved (defensive — the arg loop above already grounds them).
    if type_args.is_empty() && !fn_decl.type_params.is_empty() {
        let rendered: Option<Vec<String>> = fn_decl
            .type_params
            .iter()
            .map(|tp| subst.get(&tp.name.name).map(|t| t.display()))
            .collect();
        if let Some(parts) = rendered {
            ctx.hints
                .record(name.span, format!("[{}]", parts.join(", ")));
        }
    }
    let ret = substitute(&ret_pattern, &subst);
    // v0.20b: the return is ground *up to the caller's rigid type
    // parameters* — a generic fn calling another generic fn (bynk.list's
    // `map` calling `reverse`) legitimately instantiates the callee at its
    // own rigid vars, which flow through `compatible` by name-equality.
    Some(ret)
}

fn check_call_against_fn(
    name: &Ident,
    fn_decl: &FnDecl,
    type_args: &[TypeRef],
    args: &[Expr],
    ctx: &mut Ctx,
) -> Option<Ty> {
    // v0.20a: generic functions take the instantiation path; the
    // non-generic path below runs byte-identically to v0.19 (the additive
    // guard). Explicit type args on a non-generic fn are rejected.
    if !fn_decl.type_params.is_empty() {
        return check_generic_call(name, fn_decl, type_args, args, ctx);
    }
    if !type_args.is_empty() {
        ctx.errors.push(CompileError::new(
            "bynk.generics.type_arg_mismatch",
            name.span,
            format!(
                "`{}` is not a generic function — it takes no type arguments",
                name.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
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
        record_param_hint(ctx.hints, &param.name.name, arg);
        let arg_ty = type_of(arg, param_ty.as_ref(), ctx);
        let (Some(arg_ty), Some(param_ty)) = (arg_ty, param_ty.as_ref()) else {
            ok = false;
            continue;
        };
        if !compatible(&arg_ty, param_ty) {
            ctx.errors.push(
                CompileError::new(
                    "bynk.types.argument_mismatch",
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

/// Type-check a kernel-method argument against its expected type, with the
/// expected type propagated in (so lambdas and literals type contextually).
pub(crate) fn check_arg(arg: &Expr, expected: &Ty, what: &str, ctx: &mut Ctx) {
    let Some(actual) = type_of(arg, Some(expected), ctx) else {
        return;
    };
    if !compatible(&actual, expected) {
        ctx.errors.push(CompileError::new(
            "bynk.types.type_mismatch",
            arg.span,
            format!(
                "{what} has type `{}`, but `{}` is required",
                actual.display(),
                expected.display()
            ),
        ));
    }
}

/// Record a capability reference's binding edge, qualifying flattened bare
/// names (`consumes U { Cap }`) to their providing unit (v0.25). A bare
/// non-flattened name is the consuming unit's own declaration — qualified
/// at assembly.
fn record_capability_ref(span: Span, name: &str, ctx: &mut Ctx) {
    if let Some(unit) = ctx.input.cross_context.flattened_caps.get(name) {
        ctx.refs
            .record_in_unit(span, SymbolKind::Capability, name, unit);
    } else {
        ctx.refs.record(span, SymbolKind::Capability, name);
    }
}

pub(crate) fn check_static_call(
    type_name: &Ident,
    method: &Ident,
    args: &[Expr],
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Capability dispatch (v0.5): if `type_name` names a capability declared
    // in the context, dispatch via the capability table. If the capability is
    // declared but not in `given`, error specifically.
    if ctx.caps.declared_capabilities.contains_key(&type_name.name)
        && !ctx.caps.capabilities.contains_key(&type_name.name)
    {
        record_capability_ref(type_name.span, &type_name.name, ctx);
        let mut err = CompileError::new(
            "bynk.given.undeclared_capability",
            type_name.span,
            format!(
                "capability `{}` is used but not listed in the handler's `given` clause",
                type_name.name
            ),
        )
        .with_note(format!(
            "add `{}` to the handler's `given` clause so the dependency surface is visible at the declaration site",
            type_name.name
        ));
        // v0.26 (ADR 0054): the one-click counterpart of the note.
        if let Some((span, insert)) = given_insertion_edit(
            &ctx.caps.given_entries,
            ctx.caps.given_anchor,
            &type_name.name,
        ) {
            err = err.with_suggestion(
                format!("add `{}` to the `given` clause", type_name.name),
                vec![(span, insert)],
                Applicability::MachineApplicable,
            );
        }
        ctx.errors.push(err);
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    if let Some(cap) = ctx.caps.capabilities.get(&type_name.name).cloned() {
        record_capability_ref(type_name.span, &type_name.name, ctx);
        if !ctx.effectful {
            ctx.errors.push(
                CompileError::new(
                    "bynk.effect.capability_in_pure_context",
                    span,
                    format!(
                        "capability `{}` can only be called inside an effectful body (one returning `Effect[T]`)",
                        type_name.name
                    ),
                ),
            );
        }
        ctx.caps.given_used.insert(type_name.name.clone());
        let Some(op) = cap.ops.iter().find(|o| o.name == method.name) else {
            ctx.errors.push(CompileError::new(
                "bynk.capability.unknown_operation",
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
        // v0.36 (ADR 0069, slice 2): the op is an index symbol keyed by the
        // compound `"Cap.op"` name; this local call is a reference.
        ctx.refs.record(
            method.span,
            SymbolKind::CapabilityOp,
            &format!("{}.{}", type_name.name, method.name),
        );
        if op.params.len() != args.len() {
            ctx.errors.push(CompileError::new(
                "bynk.capability.op_arity",
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
                    "bynk.types.argument_mismatch",
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
    ctx.refs
        .record(type_name.span, SymbolKind::Type, &type_name.name);
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
    if method.name == OF
        && let Some(base) = type_decl_base(&decl)
    {
        if args.len() != 1 {
            ctx.errors.push(CompileError::new(
                "bynk.types.constructor_arity",
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
                "bynk.types.constructor_base_mismatch",
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
        // `.of` is always the runtime constructor: it returns
        // `Result[T, ValidationError]`. Compile-time literal admission (v0.9.4)
        // happens instead wherever an expected refined type is known — see
        // `admit_refined_literal`, used by `type_of` — so `.of`'s type never
        // depends on the form of its argument.
        return Some(Ty::Result(
            Box::new(named_ty(&decl)),
            Box::new(Ty::ValidationError),
        ));
    }

    // 2b) Built-in `unsafe` constructor on opaque types — only available
    // inside the defining commons.
    if method.name == UNSAFE
        && let TypeBody::Opaque { base, .. } = &decl.body
    {
        if !ctx.input.is_local_type(&decl.name.name) {
            ctx.errors.push(
                CompileError::new(
                    "bynk.types.opaque_unsafe_outside",
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
                "bynk.types.constructor_arity",
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
                "bynk.types.constructor_base_mismatch",
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
            "bynk.types.unknown_static_member",
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
                "bynk.types.method_arity",
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
        record_param_hint(ctx.hints, &param.name.name, arg);
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
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

pub(crate) fn check_method_call(
    receiver: &Expr,
    method: &Ident,
    type_args: &[TypeRef],
    args: &[Expr],
    span: Span,
    expected: Option<&Ty>,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // v0.22b: explicit type arguments apply only to the `Json.decode[T]`
    // static — every other method/static takes none (the 0039/0045 rule;
    // generic *user* methods remain deferred). A user-declared type named
    // `Json` shadows the codec module and takes no type arguments.
    if !type_args.is_empty()
        && !matches!(&receiver.kind, ExprKind::Ident(id) if id.name == JSON
            && !ctx.input.types.contains_key(JSON))
    {
        ctx.errors.push(CompileError::new(
            "bynk.generics.type_arg_mismatch",
            span,
            format!(
                "`{}` is not a generic method — it takes no type arguments",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    // v0.25: a test body invokes the target's service as `svc.call(args)`.
    // The emitter wires it from the same service set; the checker types it
    // loosely (the runner recovers outcomes at runtime), but the binding
    // edge is real — record it so test-file references index.
    if let ExprKind::Ident(id) = &receiver.kind
        && method.name == "call"
        && ctx.lookup(id.name.as_str()).is_none()
        && ctx.test_services.contains(&id.name)
        && let Some(unit) = ctx.input.cross_context.self_context.clone()
    {
        ctx.refs
            .record_in_unit(id.span, SymbolKind::Service, &id.name, &unit);
    }
    // v0.6: cross-context service call. Two shapes:
    //   - `Alias.service(args)`           where Alias is from `consumes X as Alias`
    //   - `prefix.tail.service(args)`     where `prefix.tail` is a consumed context's
    //                                     qualified name (parsed as nested FieldAccess).
    // The full-qualified-name form must be checked before the bare-ident form
    // (the prefix's first segment doesn't resolve as anything local).
    if ctx.lookup_root_ident(receiver).is_none() {
        // v0.15: cross-context capability call — `B.Cap.op(args)` /
        // `Alias.Cap.op(args)`. Checked before the service-call shape because
        // the receiver carries an extra (capability) segment.
        if let Some(chain) = flatten_ident_chain(receiver)
            && let Some((consumed, cap)) = ctx.input.cross_context.resolve_cross_capability(&chain)
        {
            // v0.25: the capability name-segment (`Cap` in `B.Cap` /
            // `Alias.Cap`) is the outermost field of the receiver chain.
            if let ExprKind::FieldAccess { field, .. } = &receiver.kind {
                ctx.refs
                    .record_in_unit(field.span, SymbolKind::Capability, &cap, &consumed);
            }
            return check_cross_context_capability_call(
                receiver, &consumed, &cap, method, args, span, ctx,
            );
        }
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
                        "bynk.resolve.unconsumed_context",
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
        && (ctx.caps.capabilities.contains_key(&id.name)
            || ctx.caps.declared_capabilities.contains_key(&id.name))
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
    // v0.20b: qualified statics on the built-in collection types —
    // `List.empty()` / `Map.empty()`. Like an empty `[]`, they need an
    // expected type to pin their element/key/value types.
    if let ExprKind::Ident(id) = &receiver.kind
        && ctx.lookup(id.name.as_str()).is_none()
        && !ctx.input.types.contains_key(&id.name)
        && (id.name == LIST || id.name == MAP)
    {
        return check_collection_static(id, method, args, span, expected, ctx);
    }
    // v0.22a: the numeric parse statics — `Int.parse(s)` / `Float.parse(s)`.
    // The parser only admits these keywords in receiver position when
    // followed by `.`, so the Ident shape here is exactly the static form.
    if let ExprKind::Ident(id) = &receiver.kind
        && (id.name == INT || id.name == FLOAT)
    {
        return check_numeric_parse_static(id, method, args, span, ctx);
    }
    // v0.22b: the typed JSON codec statics (ADR 0045).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == JSON
        && ctx.lookup(JSON).is_none()
        && !ctx.input.types.contains_key(JSON)
    {
        return check_json_static(method, type_args, args, span, expected, ctx);
    }
    // v0.20b: `insert`/`prepend` return their receiver's collection type —
    // propagate an expected collection type down the chain so
    // `let m: Map[String, Int] = Map.empty().insert("a", 1)` infers.
    let recv_expected = match (expected, method.name.as_str()) {
        (Some(t), "insert") => peel_to_map(t).map(|(k, v)| Ty::Map(Box::new(k), Box::new(v))),
        (Some(t), "prepend") => peel_to_list(t).map(|e| Ty::List(Box::new(e))),
        _ => None,
    };
    let recv_ty = type_of(receiver, recv_expected.as_ref(), ctx)?;
    // v0.20b: built-in kernel methods on the collection types. These are
    // compiler-known special forms typed directly here — generic in their
    // accumulator without the (deferred) declared-generic-methods feature;
    // the deferral bites only on declared methods (ADR 0037).
    match recv_ty.clone() {
        Ty::List(elem) => {
            return check_list_kernel_method(method, args, &elem, span, ctx);
        }
        Ty::Map(key, val) => {
            return check_map_kernel_method(method, args, &key, &val, span, ctx);
        }
        // v0.21: the numeric kernel — conversions as value methods on the
        // bare base types (a refined value reaches them via `.raw`).
        Ty::Base(base @ (BaseType::Int | BaseType::Float)) => {
            return check_numeric_kernel_method(method, args, base, span, ctx);
        }
        // v0.22a: the string kernel (ADR 0046).
        Ty::Base(BaseType::String) => {
            return check_string_kernel_method(method, args, span, ctx);
        }
        // v0.22a: the Option/Result combinators as kernel methods (ADR 0048).
        Ty::Option(inner) => {
            return check_option_kernel_method(method, args, &inner, span, ctx);
        }
        Ty::Result(ok, err) => {
            return check_result_kernel_method(method, args, &ok, &err, span, ctx);
        }
        _ => {}
    }
    // Find a named type for the receiver, then look up its instance methods.
    let type_name = match &recv_ty {
        Ty::Named { name, .. } => name.clone(),
        _ => {
            ctx.errors.push(CompileError::new(
                "bynk.types.method_on_non_named_type",
                method.span,
                format!(
                    "type `{}` has no methods — only user-declared types support method calls",
                    recv_ty.display()
                ),
            ));
            return None;
        }
    };
    // Agent handler dispatch: when the receiver is an agent instance, look
    // up the method against the agent's declared `on call` handlers and
    // resolve to the handler's return type.
    if let Some(agent) = ctx.input.agents.get(&type_name).cloned() {
        let Some(handler) = agent.handlers.iter().find(|h| {
            h.method_name
                .as_ref()
                .is_some_and(|n| n.name == method.name)
        }) else {
            ctx.errors.push(CompileError::new(
                "bynk.agent.handler_not_found",
                method.span,
                format!(
                    "agent `{}` has no handler named `{}`",
                    type_name, method.name
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        };
        if handler.params.len() != args.len() {
            ctx.errors.push(CompileError::new(
                "bynk.agent.handler_arity",
                method.span,
                format!(
                    "agent handler `{}.{}` expects {} argument(s), but {} were given",
                    type_name,
                    method.name,
                    handler.params.len(),
                    args.len()
                ),
            ));
            for a in args {
                let _ = type_of(a, None, ctx);
            }
            return None;
        }
        for (p, arg) in handler.params.iter().zip(args.iter()) {
            let pty = resolve_type_ref(&p.type_ref, &ctx.input.types);
            let _ = type_of(arg, pty.as_ref(), ctx);
        }
        return resolve_type_ref(&handler.return_type, &ctx.input.types);
    }
    let table = ctx
        .input
        .methods
        .get(&type_name)
        .cloned()
        .unwrap_or_default();
    let Some(method_decl) = table.instance.get(&method.name).cloned() else {
        ctx.errors.push(CompileError::new(
            "bynk.types.method_not_found",
            method.span,
            format!(
                "type `{}` has no instance method named `{}`",
                type_name, method.name
            ),
        ));
        return None;
    };
    // v0.36 (ADR 0069): the method is a first-class index symbol, keyed by the
    // compound `"Type.method"` name. Recorded already-spelled from the resolved
    // receiver type; the bare edge resolves through the same `uses`/`consumes`
    // qualification as cross-file type references.
    ctx.refs.record(
        method.span,
        SymbolKind::Method,
        &format!("{type_name}.{}", method.name),
    );
    // Param count excludes the implicit `self`.
    if method_decl.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "bynk.types.method_arity",
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
        record_param_hint(ctx.hints, &param.name.name, arg);
        let expected = resolve_type_ref(&param.type_ref, &ctx.input.types);
        let actual = type_of(arg, expected.as_ref(), ctx);
        let (Some(actual), Some(expected)) = (actual, expected) else {
            ok = false;
            continue;
        };
        if !compatible(&actual, &expected) {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
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
    if ctx.caps.capabilities.contains_key(head) || ctx.caps.declared_capabilities.contains_key(head)
    {
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
/// v0.15: type-check a cross-context capability call `B.Cap.op(args)` /
/// `Alias.Cap.op(args)`. The capability operation signatures are carried in
/// `consumed_capabilities` (in the providing context's namespace); the
/// capability must be listed in the handler/provider's `given` clause.
fn check_cross_context_capability_call(
    receiver: &Expr,
    consumed: &str,
    cap: &str,
    method: &Ident,
    args: &[Expr],
    _span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    // Capability calls require an effectful body (same rule as local ones).
    if !ctx.effectful {
        ctx.errors.push(CompileError::new(
            "bynk.effect.capability_in_pure_context",
            method.span,
            format!(
                "capability `{consumed}.{cap}` can only be called inside an effectful body (one returning `Effect[T]`)"
            ),
        ));
    }
    // The capability must be declared in this handler/provider's `given`.
    // The local deps key is the capability's simple name.
    if !ctx.caps.given_remaining.contains(cap) {
        let mut err = CompileError::new(
            "bynk.given.undeclared_capability",
            receiver.span,
            format!("capability `{consumed}.{cap}` is used but not listed in the `given` clause"),
        )
        .with_note(format!(
            "add `{consumed}.{cap}` to the handler's `given` clause so the dependency surface is visible at the declaration site"
        ));
        // v0.26 (ADR 0054): the one-click counterpart of the note — the
        // clause entry is the qualified form the user writes (`B.Cap`).
        if let Some((span, insert)) = given_insertion_edit(
            &ctx.caps.given_entries,
            ctx.caps.given_anchor,
            &format!("{consumed}.{cap}"),
        ) {
            err = err.with_suggestion(
                format!("add `{consumed}.{cap}` to the `given` clause"),
                vec![(span, insert)],
                Applicability::MachineApplicable,
            );
        }
        ctx.errors.push(err);
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    }
    ctx.caps.given_used.insert(cap.to_string());

    let info = &ctx.input.cross_context;
    let op = info
        .consumed_capabilities
        .get(consumed)
        .and_then(|caps| caps.get(cap))
        .and_then(|c| c.ops.iter().find(|o| o.name == method.name))
        .cloned();
    let Some(op) = op else {
        ctx.errors.push(CompileError::new(
            "bynk.capability.unknown_operation",
            method.span,
            format!(
                "capability `{consumed}.{cap}` has no operation named `{}`",
                method.name
            ),
        ));
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
    };
    // v0.36 (ADR 0069, slice 2): a cross-context op call references the op,
    // recorded already-qualified into the providing unit (where the op is
    // declared), mirroring the cross-context capability reference.
    ctx.refs.record_in_unit(
        method.span,
        SymbolKind::CapabilityOp,
        &format!("{cap}.{}", method.name),
        consumed,
    );
    if op.params.len() != args.len() {
        ctx.errors.push(CompileError::new(
            "bynk.capability.op_arity",
            method.span,
            format!(
                "capability operation `{consumed}.{cap}.{}` expects {} argument(s), but {} were given",
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

    // Resolve parameter / return types in the consumed context's namespace.
    let consumed_types = info
        .consumed_types
        .get(consumed)
        .cloned()
        .unwrap_or_default();
    let mut all_ok = true;
    for (i, ((pname, ptype_ref), arg)) in op.params.iter().zip(args.iter()).enumerate() {
        record_param_hint(ctx.hints, pname, arg);
        let param_ty = resolve_type_ref(ptype_ref, &consumed_types).unwrap_or(Ty::Unit);
        let Some(arg_ty) = type_of(arg, None, ctx) else {
            all_ok = false;
            continue;
        };
        if !structurally_compatible(&arg_ty, &param_ty, &ctx.input.types, &consumed_types) {
            ctx.errors.push(CompileError::new(
                "bynk.boundary.structural_mismatch",
                arg.span,
                format!(
                    "cross-context argument {} to `{consumed}.{cap}.{}` has type `{}`, but parameter `{pname}` expects `{}`",
                    i + 1,
                    method.name,
                    arg_ty.display(),
                    param_ty.display(),
                ),
            ));
            all_ok = false;
        }
    }
    if !all_ok {
        return None;
    }
    let raw_ret = resolve_type_ref(&op.return_type, &consumed_types).unwrap_or(Ty::Unit);
    Some(rebrand_return_type(&raw_ret, &ctx.input.types))
}

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
                "bynk.effect.cross_context_in_pure_context",
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
                "bynk.consumes.unknown_context",
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
                "bynk.consumes.unknown_service",
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
    ctx.refs
        .record_in_unit(method.span, SymbolKind::Service, &method.name, consumed);

    if service.params.len() != args.len() {
        ctx.errors.push(
            CompileError::new(
                "bynk.consumes.service_arity",
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
        record_param_hint(ctx.hints, pname, arg);
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
                    "bynk.boundary.structural_mismatch",
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
