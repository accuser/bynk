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
    requirements: &mut RequirementSink,
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
                locals.record(
                    p.name.name.clone(),
                    p.name.span,
                    crate::locals::LocalKind::Param,
                    ty.display(),
                    f.body.span,
                );
            }
            param_scope.insert(p.name.name.clone(), ty);
        }
    }
    // v0.115: check the function's contract clauses (`requires`/`ensures`) in
    // the parameter scope before the body — a contract is the invariant
    // predicate attached to a function (ADR 0144). `result` in an `ensures` is
    // the return value, awaited for an `Effect`.
    if !f.requires.is_empty() || !f.ensures.is_empty() {
        let result_ty = match &return_ty {
            Ty::Effect(inner) => (**inner).clone(),
            t => t.clone(),
        };
        let has_result_param = f.params.iter().any(|p| p.name.name == "result");
        let fn_label = format!("function `{}`", f.name.display());
        check_contracts(
            &f.requires,
            &f.ensures,
            &param_scope,
            &result_ty,
            has_result_param,
            &fn_label,
            input,
            expr_types,
            errors,
            refs,
            hints,
            locals,
            requirements,
            &vars,
        );
    }
    let effectful = matches!(&return_ty, Ty::Effect(_));
    let mut ctx = Ctx {
        input,
        expr_types,
        errors,
        refs,
        hints,
        locals,
        requirements,
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
        store_cells: HashMap::new(),
        store_maps: HashMap::new(),
        store_sets: HashMap::new(),
        store_caches: HashMap::new(),
        store_logs: HashMap::new(),
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
    // A state initialiser is a static, pure value — no capability calls reach
    // here — so requirements are discarded into a throwaway sink.
    let mut init_requirements = RequirementSink::new();
    let result = {
        let mut ctx = Ctx {
            input,
            expr_types,
            errors: &mut local_errors,
            refs,
            hints,
            locals,
            requirements: &mut init_requirements,
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
            store_cells: HashMap::new(),
            store_maps: HashMap::new(),
            store_sets: HashMap::new(),
            store_caches: HashMap::new(),
            store_logs: HashMap::new(),
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
                "an initialiser is a compile-time value — a literal (including one admitted to a \
                 refined type), a sum variant, `Some`/`None`/`Ok`/`Err`, a record, or — for an \
                 opaque type — `T.unsafe(lit)` — with no reference to `self`, parameters, or \
                 capabilities",
            ),
        );
    }
}

/// v0.91 (ADR 0116 D6): the `bynk.list` free functions whose method forms
/// shipped with slice 1 are **deprecated** in favour of the method-chain
/// vocabulary. `reverse`/`traverse` stay (no method form yet). Emits a
/// non-failing warning (ADR 0117) with a machine-applicable rewrite to the
/// method form — `map(xs, f)` → `xs.map(f)`, `find(xs, p)` → `xs.filter(p).first()`.
fn warn_bynk_list_deprecation(name: &Ident, args: &[Expr], call_span: Span, ctx: &mut Ctx) {
    if ctx.input.imported_from.get(&name.name).map(String::as_str) != Some("bynk.list") {
        return;
    }
    // The method-form spelling each free function rewrites to.
    let method_form: &str = match name.name.as_str() {
        "map" => "xs.map(f)",
        "filter" => "xs.filter(p)",
        "any" => "xs.any(p)",
        "all" => "xs.all(p)",
        "find" => "xs.filter(p).first()",
        _ => return, // reverse / traverse keep their free-function form for now
    };
    let mut err = CompileError::new(
        "bynk.list.deprecated_function",
        name.span,
        format!(
            "`bynk.list.{}` is deprecated — use the `List` method form `{method_form}`",
            name.name
        ),
    )
    .with_note(
        "the `bynk.list.*` free functions are superseded by the method-chain vocabulary (ADR 0116); the method form reads left-to-right and chains",
    );
    // The auto-fix needs the (list, fn) shape — a wrong-arity call is reported
    // elsewhere; only offer the rewrite when it is well-formed.
    if args.len() == 2 {
        let mut edits = vec![
            // delete `name(` — the receiver becomes the first argument.
            (
                Span::new(name.span.start, args[0].span.start),
                String::new(),
            ),
            // the `, ` between the two args becomes `.<method>(`.
            (
                Span::new(args[0].span.end, args[1].span.start),
                format!(
                    ".{}(",
                    if name.name == "find" {
                        "filter"
                    } else {
                        &name.name
                    }
                ),
            ),
        ];
        if name.name == "find" {
            // `filter(p)` then `.first()` after the closing `)`.
            edits.push((
                Span::new(call_span.end, call_span.end),
                ".first()".to_string(),
            ));
        }
        err = err.with_suggestion(
            format!("rewrite to the `List` method form `{method_form}`"),
            edits,
            Applicability::MachineApplicable,
        );
    }
    ctx.errors.push(err);
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
        warn_bynk_list_deprecation(name, args, span, ctx);
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
            args: Vec::new(),
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
    // Nothing owns the call. The resolver's reference walk reports these in
    // `fn`/method bodies (and a resolve error stops the pipeline before the
    // checker runs), but handler/service/agent bodies never pass through
    // that walk — the checker is their only backstop, and a silent `None`
    // here admitted any unknown call and emitted it verbatim. Mirror the
    // resolver's ladder. Test bodies stay silent, matching the loosely
    // typed test-call surface (v0.25) — see the same gate in `check_ident`.
    if ctx.in_test_body {
        return None;
    }
    for a in args {
        let _ = type_of(a, None, ctx);
    }
    if owners.len() > 1 {
        ctx.errors.push(CompileError::new(
            "bynk.resolve.ambiguous_variant",
            name.span,
            format!(
                "the variant name `{}` is declared on multiple sum types — qualify it as `TypeName.{}(...)`",
                name.name, name.name
            ),
        ));
        return None;
    }
    if ctx.input.types.contains_key(&name.name) {
        ctx.errors.push(CompileError::new(
            "bynk.resolve.type_as_function",
            span,
            format!(
                "`{}` is a type, not a function — use `{}.of(value)` or `{} {{ ... }}` instead",
                name.name, name.name, name.name
            ),
        ));
        return None;
    }
    ctx.errors.push(
        CompileError::new(
            "bynk.resolve.unknown_function",
            span,
            format!("unknown function `{}`", name.name),
        )
        .with_note("only functions declared in this commons are callable"),
    );
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
        // Same backstop as the non-generic path: the resolver only covers
        // `fn`/method bodies, so the checker must report for handler bodies.
        ctx.errors.push(
            CompileError::new(
                "bynk.resolve.arity_mismatch",
                name.span,
                format!(
                    "function `{}` expects {} argument(s), but {} were given",
                    name.name,
                    fn_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(fn_decl.name.ident().span, "function declared here"),
        );
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
        // The resolver reports this in `fn`/method bodies; handler/service/
        // agent/test bodies only reach the checker, so report here too (a
        // resolve error stops the pipeline first, so this cannot double up).
        ctx.errors.push(
            CompileError::new(
                "bynk.resolve.arity_mismatch",
                name.span,
                format!(
                    "function `{}` expects {} argument(s), but {} were given",
                    name.name,
                    fn_decl.params.len(),
                    args.len()
                ),
            )
            .with_label(fn_decl.name.ident().span, "function declared here"),
        );
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
        // v0.99: an uncovered direct capability call — record it so the ghost
        // `given` inlay hint can offer the same clause at the declaration site
        // (DECISION D/E). `span` is the call site (the inner `given_insertion_edit`
        // binding shadowed it only within the `if let` above).
        record_requirement(
            ctx,
            &type_name.name,
            span,
            RequirementSource::DirectCall {
                op: method.name.clone(),
            },
            false,
        );
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
        // v0.99: a covered direct capability call — the call site *is* the
        // reason (DECISION C). Recorded so hover can explain what a declared
        // `given Cap` is for; no inlay hint (already covered).
        record_requirement(
            ctx,
            &type_name.name,
            span,
            RequirementSource::DirectCall {
                op: method.name.clone(),
            },
            true,
        );
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

/// v0.82 (ADR 0110): resolve a storage-map operation `<map>.<op>(args)` on a
/// `store Map[K, V]` field. The ops are effect-typed (storage I/O, awaited with
/// `<-`): `put`/`update`/`upsert`/`remove` → `Effect[()]`, `get` →
/// `Effect[Option[V]]`, `contains` → `Effect[Bool]`, `size` → `Effect[Int]`.
/// `update` on an absent key is a runtime fault; `upsert` is the default-if-absent
/// form. Dispatched by receiver provenance, so it never shadows the immutable
/// value `Map`'s pure methods.
pub(crate) fn check_store_map_op(
    method: &Ident,
    args: &[Expr],
    k: &Ty,
    v: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let vfn = || Ty::Fn {
        params: vec![v.clone()],
        ret: Box::new(v.clone()),
    };
    // v0.105 (slice 3b-ii): a held `Map[K, Connection]` stores connection ids and
    // resolves them by identity; `update`/`upsert` transform the value through a
    // `(V) -> V` function, which has no meaning for a held resource — you cannot
    // derive a new connection from an old one. Reject them (the other entry ops —
    // `put`/`get`/`remove`/`contains`/`size` — are admitted) so the program is a
    // clean compile error rather than a silent miscompile.
    if v.is_held() && matches!(method.name.as_str(), "update" | "upsert") {
        ctx.errors.push(
            CompileError::new(
                "bynk.held.unsupported_map_op",
                method.span,
                format!(
                    "a held `Map[K, Connection]` has no `{}` operation — a held resource cannot be transformed by a `(Connection) -> Connection` function",
                    method.name
                ),
            )
            .with_note(
                "held connections are stored and resolved by identity; use `put`/`get`/`remove`",
            ),
        );
        for a in args {
            type_of(a, None, ctx);
        }
        return None;
    }
    let (expected, result): (Vec<Ty>, Ty) = match method.name.as_str() {
        "put" => (vec![k.clone(), v.clone()], Ty::Unit),
        "get" => (vec![k.clone()], Ty::Option(Box::new(v.clone()))),
        "remove" => (vec![k.clone()], Ty::Unit),
        "contains" => (vec![k.clone()], Ty::Base(BaseType::Bool)),
        "size" => (vec![], Ty::Base(BaseType::Int)),
        "update" => (vec![k.clone(), vfn()], Ty::Unit),
        "upsert" => (vec![k.clone(), v.clone(), vfn()], Ty::Unit),
        other => {
            ctx.errors.push(
                CompileError::new(
                    "bynk.store.unknown_op",
                    method.span,
                    format!(
                        "a `Map` store field has no operation `{other}` — expected `put`, `get`, \
                         `update`, `upsert`, `remove`, `contains`, or `size`"
                    ),
                )
                .with_note("storage-map ops are entry-level and effectful (await with `<-`)"),
            );
            for a in args {
                type_of(a, None, ctx);
            }
            return None;
        }
    };
    let effect = Ty::Effect(Box::new(result));
    if args.len() != expected.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.call_arity",
            span,
            format!(
                "`Map.{}` takes {} argument(s), found {}",
                method.name,
                expected.len(),
                args.len()
            ),
        ));
        for a in args {
            type_of(a, None, ctx);
        }
        return Some(effect);
    }
    for (a, exp) in args.iter().zip(expected.iter()) {
        if let Some(at) = type_of(a, Some(exp), ctx)
            && !compatible(&at, exp)
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                a.span,
                format!("expected `{}`, found `{}`", exp.display(), at.display()),
            ));
        }
    }
    Some(effect)
}

/// v0.99: record a capability requirement at `site` into the ledger, and — when
/// the enclosing handler's `given` does not cover it — push the diagnostic. The
/// single producer behind both the bare diagnostic and the editor surfaces
/// (DECISION D): every requirement is *recorded*, covered or not; only an
/// uncovered one errors. The `code`/`message` are the consuming feature's, so a
/// store op keeps its precise diagnostic; the ledger's *reason* renders from
/// `source` alone (DECISION C), never from `code`.
fn require_capability(
    site: Span,
    capability: &str,
    source: RequirementSource,
    ctx: &mut Ctx,
    code: &'static str,
    message: &str,
) {
    let covered = ctx.caps.capabilities.contains_key(capability);
    if covered {
        ctx.caps.given_used.insert(capability.to_string());
    } else {
        ctx.errors
            .push(CompileError::new(code, site, message).with_note(format!(
                "add `{capability}` to the handler's `given` clause"
            )));
    }
    record_requirement(ctx, capability, site, source, covered);
}

/// Push a [`Requirement`] into the ledger. For an uncovered requirement it also
/// computes the materialization edit (the ghost `given` inlay hint's one-click
/// apply) from the handler's existing `given` entries and anchor — the same
/// `given_insertion_edit` the undeclared-capability quick-fix uses.
fn record_requirement(
    ctx: &mut Ctx,
    capability: &str,
    site: Span,
    source: RequirementSource,
    covered: bool,
) {
    let materialize = if covered {
        None
    } else {
        given_insertion_edit(&ctx.caps.given_entries, ctx.caps.given_anchor, capability).map(
            |(edit_span, edit_text)| Materialize {
                anchor: ctx.caps.given_anchor.unwrap_or(ctx.return_ty_span),
                edit_span,
                edit_text,
            },
        )
    };
    ctx.requirements.record(Requirement {
        capability: capability.to_string(),
        site,
        source,
        covered,
        materialize,
    });
}

/// v0.87 (ADR 0113): resolve a storage-`Cache` operation `<cache>.<op>(args)` on
/// a `store Cache[K, V]` field. The op set is the storage `Map`'s
/// (`put`/`get`/`update`/`upsert`/`remove`/`contains`/`size`); every op but
/// `remove` additionally requires `given Clock` (eviction reads the clock).
pub(crate) fn check_store_cache_op(
    method: &Ident,
    args: &[Expr],
    k: &Ty,
    v: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let vfn = || Ty::Fn {
        params: vec![v.clone()],
        ret: Box::new(v.clone()),
    };
    let (expected, result): (Vec<Ty>, Ty) = match method.name.as_str() {
        "put" => (vec![k.clone(), v.clone()], Ty::Unit),
        "get" => (vec![k.clone()], Ty::Option(Box::new(v.clone()))),
        "remove" => (vec![k.clone()], Ty::Unit),
        "contains" => (vec![k.clone()], Ty::Base(BaseType::Bool)),
        "size" => (vec![], Ty::Base(BaseType::Int)),
        "update" => (vec![k.clone(), vfn()], Ty::Unit),
        "upsert" => (vec![k.clone(), v.clone(), vfn()], Ty::Unit),
        other => {
            ctx.errors.push(
                CompileError::new(
                    "bynk.store.unknown_op",
                    method.span,
                    format!(
                        "a `Cache` store field has no operation `{other}` — expected `put`, \
                         `get`, `update`, `upsert`, `remove`, `contains`, or `size`"
                    ),
                )
                .with_note("storage-cache ops are entry-level and effectful (await with `<-`)"),
            );
            for a in args {
                type_of(a, None, ctx);
            }
            return None;
        }
    };
    // D4: every op but `remove` reads the clock for TTL expiry.
    if method.name != "remove" {
        require_capability(
            method.span,
            "Clock",
            RequirementSource::StoreOp {
                kind: StoreKind::Cache,
                op: method.name.clone(),
            },
            ctx,
            "bynk.store.cache_needs_clock",
            "a `Cache` operation applies TTL expiry, which reads the clock — the handler must declare `given Clock`",
        );
    }
    let effect = Ty::Effect(Box::new(result));
    if args.len() != expected.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.call_arity",
            span,
            format!(
                "`Cache.{}` takes {} argument(s), found {}",
                method.name,
                expected.len(),
                args.len()
            ),
        ));
        for a in args {
            type_of(a, None, ctx);
        }
        return Some(effect);
    }
    for (a, exp) in args.iter().zip(expected.iter()) {
        if let Some(at) = type_of(a, Some(exp), ctx)
            && !compatible(&at, exp)
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                a.span,
                format!("expected `{}`, found `{}`", exp.display(), at.display()),
            ));
        }
    }
    Some(effect)
}

/// v0.95 (ADR 0121): resolve a storage-`Log` operation `<log>.<op>(args)` on a
/// `store Log[T]` field. `append(e)` is the effectful, **non-idempotent** write
/// (`Effect[()]`) and the one clock-consuming op — it stamps `Clock.now()`, so it
/// requires `given Clock`. The time-window roots — `since(Instant)`/
/// `before(Instant)` / `between(Instant, Instant)` / `recent(Int)` / `reversed()`
/// — and the general query vocabulary lift the log into a lazy `Query[T]` over its
/// entry values; these need no clock (window bounds are explicit `Instant`s).
pub(crate) fn check_store_log_op(
    method: &Ident,
    args: &[Expr],
    elem: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let query = || Ty::Query(Box::new(elem.clone()));
    let arity = |n: usize, ctx: &mut Ctx| {
        if args.len() != n {
            ctx.errors.push(CompileError::new(
                "bynk.types.call_arity",
                span,
                format!(
                    "`Log.{}` takes {n} argument(s), found {}",
                    method.name,
                    args.len()
                ),
            ));
            for a in args {
                type_of(a, None, ctx);
            }
            return false;
        }
        true
    };
    let window_arg = |a: &Expr, what: &str, ctx: &mut Ctx| {
        if let Some(at) = type_of(a, Some(&Ty::Base(BaseType::Instant)), ctx)
            && !compatible(&at, &Ty::Base(BaseType::Instant))
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                a.span,
                format!("{what} expects `Instant`, found `{}`", at.display()),
            ));
        }
    };
    match method.name.as_str() {
        // The one effectful write — non-idempotent; stamps the clock.
        "append" => {
            require_capability(
                method.span,
                "Clock",
                RequirementSource::StoreOp {
                    kind: StoreKind::Log,
                    op: method.name.clone(),
                },
                ctx,
                "bynk.store.log_needs_clock",
                "`Log.append` stamps the current time, which reads the clock — the handler must declare `given Clock`",
            );
            if !arity(1, ctx) {
                return Some(Ty::Effect(Box::new(Ty::Unit)));
            }
            if let Some(at) = type_of(&args[0], Some(elem), ctx)
                && !compatible(&at, elem)
            {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    args[0].span,
                    format!("expected `{}`, found `{}`", elem.display(), at.display()),
                ));
            }
            Some(Ty::Effect(Box::new(Ty::Unit)))
        }
        // Time-window query roots → `Query[T]` (lazy; no clock).
        "since" | "before" => {
            if !arity(1, ctx) {
                return Some(query());
            }
            window_arg(&args[0], &format!("`Log.{}`", method.name), ctx);
            Some(query())
        }
        "between" => {
            if !arity(2, ctx) {
                return Some(query());
            }
            window_arg(&args[0], "`Log.between` start", ctx);
            window_arg(&args[1], "`Log.between` end", ctx);
            Some(query())
        }
        "recent" => {
            if !arity(1, ctx) {
                return Some(query());
            }
            check_arg(
                &args[0],
                &Ty::Base(BaseType::Int),
                "the `Log.recent` count",
                ctx,
            );
            Some(query())
        }
        "reversed" => {
            if !arity(0, ctx) {
                return Some(query());
            }
            Some(query())
        }
        // The general query vocabulary over the entry values.
        name if is_query_op(name) => check_query_kernel_method(method, args, elem, span, ctx),
        other => {
            ctx.errors.push(
                CompileError::new(
                    "bynk.store.unknown_op",
                    method.span,
                    format!(
                        "a `Log` store field has no operation `{other}` — `append`, the \
                         time-window roots (`since`/`before`/`between`/`recent`/`reversed`), \
                         and the query builders/terminals"
                    ),
                )
                .with_note(
                    "`Log` reads are lazy `Query[T]`; only `append` is effectful and writes",
                ),
            );
            for a in args {
                type_of(a, None, ctx);
            }
            None
        }
    }
}

/// v0.83: resolve a storage-set operation `<set>.<op>(args)` on a `store Set[T]`
/// field. Effect-typed entry ops: `add(t)`/`remove(t)` → `Effect[()]` (both
/// idempotent), `contains(t)` → `Effect[Bool]`, `size()` → `Effect[Int]`. Set
/// algebra (`union`/`intersection`/`difference`) is deferred (it needs a value
/// `Set` return type). Dispatched by receiver provenance.
pub(crate) fn check_store_set_op(
    method: &Ident,
    args: &[Expr],
    t: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let (expected, result): (Vec<Ty>, Ty) = match method.name.as_str() {
        "add" => (vec![t.clone()], Ty::Unit),
        "remove" => (vec![t.clone()], Ty::Unit),
        "contains" => (vec![t.clone()], Ty::Base(BaseType::Bool)),
        "size" => (vec![], Ty::Base(BaseType::Int)),
        other => {
            ctx.errors.push(
                CompileError::new(
                    "bynk.store.unknown_op",
                    method.span,
                    format!(
                        "a `Set` store field has no operation `{other}` — expected `add`, \
                         `remove`, `contains`, or `size`"
                    ),
                )
                .with_note(
                    "set algebra (`union`/`intersection`/`difference`) is not in this slice",
                ),
            );
            for a in args {
                type_of(a, None, ctx);
            }
            return None;
        }
    };
    let effect = Ty::Effect(Box::new(result));
    if args.len() != expected.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.call_arity",
            span,
            format!(
                "`Set.{}` takes {} argument(s), found {}",
                method.name,
                expected.len(),
                args.len()
            ),
        ));
        for a in args {
            type_of(a, None, ctx);
        }
        return Some(effect);
    }
    for (a, exp) in args.iter().zip(expected.iter()) {
        if let Some(at) = type_of(a, Some(exp), ctx)
            && !compatible(&at, exp)
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                a.span,
                format!("expected `{}`, found `{}`", exp.display(), at.display()),
            ));
        }
    }
    Some(effect)
}

/// v0.98 (ADR 0125): resolve a storage-`Cell` operation `<cell>.<op>(args)` on a
/// `store Cell[T]` field. The single method-shaped cell op is `update(f)` —
/// `f: (T) -> T` — a read-modify-write typed `Effect[()]`. Reading a cell is the
/// bare-name sugar and writing it is `:=`, so `read`/`write` are not callable
/// methods (DECISION B). The combiner is a non-effectful `Ty::Fn`, so an
/// effectful body (including a bare read of another cell) fails the existing
/// function-type check (DECISION E). Dispatched by receiver provenance.
pub(crate) fn check_store_cell_op(
    method: &Ident,
    args: &[Expr],
    t: &Ty,
    span: Span,
    ctx: &mut Ctx,
) -> Option<Ty> {
    let tfn = || Ty::Fn {
        params: vec![t.clone()],
        ret: Box::new(t.clone()),
    };
    let (expected, result): (Vec<Ty>, Ty) = match method.name.as_str() {
        "update" => (vec![tfn()], Ty::Unit),
        other => {
            ctx.errors.push(
                CompileError::new(
                    "bynk.store.unknown_op",
                    method.span,
                    format!("a `Cell` store field has no operation `{other}` — expected `update`"),
                )
                .with_note(
                    "a cell is read by its bare name and written with `:=`; `update` is the only \
                     method-shaped op",
                ),
            );
            for a in args {
                type_of(a, None, ctx);
            }
            return None;
        }
    };
    let effect = Ty::Effect(Box::new(result));
    if args.len() != expected.len() {
        ctx.errors.push(CompileError::new(
            "bynk.types.call_arity",
            span,
            format!(
                "`Cell.{}` takes {} argument(s), found {}",
                method.name,
                expected.len(),
                args.len()
            ),
        ));
        for a in args {
            type_of(a, None, ctx);
        }
        return Some(effect);
    }
    for (a, exp) in args.iter().zip(expected.iter()) {
        if let Some(at) = type_of(a, Some(exp), ctx)
            && !compatible(&at, exp)
        {
            ctx.errors.push(CompileError::new(
                "bynk.types.argument_mismatch",
                a.span,
                format!("expected `{}`, found `{}`", exp.display(), at.display()),
            ));
        }
    }
    Some(effect)
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
    // edge is real — record it so test-file references index. Returns here:
    // the loose typing must not fall through to the receiver walk, which
    // would report the service name as unknown (#504 backstop).
    if let ExprKind::Ident(id) = &receiver.kind
        && method.name == "call"
        && ctx.lookup(id.name.as_str()).is_none()
        && ctx.test_services.contains(&id.name)
    {
        if let Some(unit) = ctx.input.cross_context.self_context.clone() {
            ctx.refs
                .record_in_unit(id.span, SymbolKind::Service, &id.name, &unit);
        }
        for a in args {
            let _ = type_of(a, None, ctx);
        }
        return None;
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
    // v0.86 (ADR 0112): the `Duration.millis(n)` static constructor — the way to
    // build a `Duration` from a runtime `Int` (the literal covers constants).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == DURATION
        && ctx.lookup(DURATION).is_none()
        && !ctx.input.types.contains_key(DURATION)
    {
        return check_duration_static(method, args, span, ctx);
    }
    // v0.90 (ADR 0114 D6): the `Instant.fromEpochMillis(n)` static constructor.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == INSTANT
        && ctx.lookup(INSTANT).is_none()
        && !ctx.input.types.contains_key(INSTANT)
    {
        return check_instant_static(method, args, span, ctx);
    }
    // v0.110 (ADR 0142 D2): the `Bytes` static constructors —
    // `Bytes.fromUtf8(s)` / `Bytes.fromBase64(s)` / `Bytes.empty()`.
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == BYTES
        && ctx.lookup(BYTES).is_none()
        && !ctx.input.types.contains_key(BYTES)
    {
        return check_bytes_static(method, args, span, ctx);
    }
    // v0.22b: the typed JSON codec statics (ADR 0045).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == JSON
        && ctx.lookup(JSON).is_none()
        && !ctx.input.types.contains_key(JSON)
    {
        return check_json_static(method, type_args, args, span, expected, ctx);
    }
    // v0.100: the `Stream.of(xs)` static constructor (real-time track slice 0).
    if let ExprKind::Ident(id) = &receiver.kind
        && id.name == STREAM
        && ctx.lookup(STREAM).is_none()
        && !ctx.input.types.contains_key(STREAM)
    {
        return check_stream_static(method, args, span, ctx);
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
        // v0.91 (ADR 0115): a chained builder/terminal on a lazy `Query[T]`.
        Ty::Query(elem) => {
            return check_query_kernel_method(method, args, &elem, span, ctx);
        }
        // v0.100: a chained builder/terminal on a `Stream[T]`.
        Ty::Stream(elem) => {
            return check_stream_kernel_method(method, args, &elem, span, ctx);
        }
        // v0.102: the held-resource operations on a `Connection[F]` — `send(f)`
        // (non-consuming) and `close()` (consuming). The linearity pass tracks
        // the ownership transitions; this types the operations.
        Ty::Connection(frame) => {
            return check_connection_method(method, args, &frame, span, ctx);
        }
        Ty::Map(key, val) => {
            return check_map_kernel_method(method, args, &key, &val, span, ctx);
        }
        // v0.21: the numeric kernel — conversions as value methods on the
        // bare base types. A refined value reaches these too, but via the
        // refined-receiver fallback below (after its own declared methods),
        // not via any `.raw` surface — a refined type's only source-level
        // constructors are `.of` and literal admission (ADR 0182); unlike an
        // opaque type it exposes no `.unsafe` (that is opaque-only, confined
        // to the defining commons).
        Ty::Base(base @ (BaseType::Int | BaseType::Float)) => {
            return check_numeric_kernel_method(method, args, base, span, ctx);
        }
        // v0.86 (ADR 0112): the `Duration` kernel — `toMillis`/`toString`.
        Ty::Base(BaseType::Duration) => {
            return check_duration_kernel_method(method, args, span, ctx);
        }
        // v0.90 (ADR 0114): the `Instant` kernel — `toEpochMillis`/`toString`.
        Ty::Base(BaseType::Instant) => {
            return check_instant_kernel_method(method, args, span, ctx);
        }
        // v0.110 (ADR 0142): the `Bytes` kernel — `length`/`toBase64`/`decodeUtf8`.
        Ty::Base(BaseType::Bytes) => {
            return check_bytes_kernel_method(method, args, span, ctx);
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
        // The `Effect[Result[T, E]]` combinators (§2.8.3): `mapOk`/`mapErr`/
        // `flatMapOk`/`flatMapErr` on the universal cross-context shape. Only an
        // `Effect` wrapping a `Result` has methods; any other `Effect[_]` falls
        // through to the "no methods" error below.
        Ty::Effect(inner) => {
            if let Ty::Result(ok, err) = inner.as_ref() {
                return check_effect_result_kernel_method(method, args, ok, err, span, ctx);
            }
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
            let arg_ty = type_of(arg, pty.as_ref(), ctx);
            if let (Some(a), Some(p_ty)) = (arg_ty.as_ref(), pty.as_ref())
                && !compatible(a, p_ty)
            {
                ctx.errors.push(CompileError::new(
                    "bynk.types.argument_mismatch",
                    arg.span,
                    format!(
                        "argument has type `{}`, but `{}.{}` expects `{}`",
                        a.display(),
                        type_name,
                        method.name,
                        p_ty.display()
                    ),
                ));
            }
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
        // #561: a refined receiver inherits its base type's read-only kernel
        // methods as a fallback *after* its own declared methods (DECISION D).
        // A refined value erases to its base at runtime, so the base methods
        // already apply bit-for-bit; only the checker had to be taught to look.
        // Results are base-typed (DECISION B) and refined arguments widen via
        // `compatible`, so the base kernel checkers need no change. The match
        // is `Refined` only — opaque types deliberately do not widen, so they
        // keep reporting `method_not_found`. `Bool` (and any base without a
        // kernel) inherits nothing and falls through to the same error.
        if let Ty::Named {
            kind: NamedKind::Refined(base),
            ..
        } = &recv_ty
        {
            match base {
                BaseType::Int | BaseType::Float => {
                    return check_numeric_kernel_method(method, args, *base, span, ctx);
                }
                BaseType::String => return check_string_kernel_method(method, args, span, ctx),
                BaseType::Duration => return check_duration_kernel_method(method, args, span, ctx),
                BaseType::Instant => return check_instant_kernel_method(method, args, span, ctx),
                BaseType::Bytes => return check_bytes_kernel_method(method, args, span, ctx),
                BaseType::Bool => {}
            }
        }
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
