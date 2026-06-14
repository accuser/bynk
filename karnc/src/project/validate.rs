use super::*;

/// v0.19: the lock violation a deployment unit's native-platform set implies
/// under the selected `--platform`, if any. Pure — unit-tested below with
/// synthetic sets (the conflict arm is not yet reachable end-to-end while
/// only one platform ships native capabilities).
fn lock_violation(
    native: &std::collections::BTreeMap<Platform, String>,
    selected: Platform,
) -> Option<LockViolation> {
    let mut platforms = native.iter();
    let (first, first_unit) = platforms.next()?;
    if let Some((second, second_unit)) = platforms.next() {
        return Some(LockViolation::Conflict {
            a: (*first, first_unit.clone()),
            b: (*second, second_unit.clone()),
        });
    }
    if *first != selected {
        return Some(LockViolation::Required {
            needed: *first,
            unit: first_unit.clone(),
        });
    }
    None
}

/// A platform-lock violation (v0.19, `karn.target.*`).
#[derive(Debug, PartialEq, Eq)]
enum LockViolation {
    /// The deployment unit needs `needed` but another platform is selected.
    Required { needed: Platform, unit: String },
    /// The deployment unit's closure spans two mutually-exclusive platforms.
    Conflict {
        a: (Platform, String),
        b: (Platform, String),
    },
}

/// v0.19 (decisions 0017/0024): enforce the platform lock per deployment
/// unit — each context under `--target workers`, the whole program under
/// `bundle` (co-location shares the lock).
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_platform_lock(
    target: BuildTarget,
    selected: Platform,
    parsed: &[ParsedFile],
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
) {
    // Per-context native sets, with the context name kept for spans/messages.
    let mut per_context: Vec<(String, std::collections::BTreeMap<Platform, String>)> = Vec::new();
    let mut names: Vec<&String> = groups.keys().collect();
    names.sort();
    for name in names {
        if kinds.get(name.as_str()) != Some(&UnitKind::Context) {
            continue;
        }
        let Some(table) = unit_tables.get(name.as_str()) else {
            continue;
        };
        let native = native_platforms_of_context(
            name,
            table,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_flattened,
        );
        if !native.is_empty() {
            per_context.push((name.clone(), native));
        }
    }
    // The deployment units to check: per-context under workers; their union
    // under bundle (the whole program co-locates).
    let units: Vec<(String, std::collections::BTreeMap<Platform, String>)> = match target {
        BuildTarget::Workers => per_context,
        BuildTarget::Bundle => {
            let mut union = std::collections::BTreeMap::new();
            let mut owner: Option<String> = None;
            for (ctx, native) in per_context {
                owner.get_or_insert(ctx);
                for (p, unit) in native {
                    union.entry(p).or_insert(unit);
                }
            }
            match owner {
                Some(ctx) if !union.is_empty() => vec![(ctx, union)],
                _ => Vec::new(),
            }
        }
    };
    for (ctx, native) in units {
        let Some(violation) = lock_violation(&native, selected) else {
            continue;
        };
        let span_for = |unit: &str| {
            groups
                .get(&ctx)
                .and_then(|idx| consumes_span_of(parsed, idx, unit))
                .unwrap_or_default()
        };
        match violation {
            LockViolation::Required { needed, unit } => {
                errors.push(
                    CompileError::new(
                        "karn.target.vendor_required",
                        span_for(&unit),
                        format!(
                            "context `{ctx}` uses the platform-native capabilities of `{unit}`, which run only on the `{}` platform, but the build selects `--platform {}`",
                            needed.as_str(),
                            selected.as_str(),
                        ),
                    )
                    .with_note(
                        "build with the matching `--platform`, or remove the platform-native dependency to stay portable",
                    ),
                );
            }
            LockViolation::Conflict { a, b } => {
                errors.push(
                    CompileError::new(
                        "karn.target.vendor_conflict",
                        span_for(&a.1),
                        format!(
                            "one deployment unit (via context `{ctx}`) uses platform-native capabilities from two mutually-exclusive platforms: `{}` (from `{}`) and `{}` (from `{}`)",
                            a.0.as_str(),
                            a.1,
                            b.0.as_str(),
                            b.1,
                        ),
                    )
                    .with_note(
                        "split the consumers into separate deployment units (`--target workers`), or remove one of the platform-native dependencies",
                    ),
                );
            }
        }
    }
}

/// Enforce v0.4 construction rules: types owned by a consumed context can be
/// referenced (held, passed, read for transparent exports) but cannot be
/// constructed. This catches `OtherType { ... }`, `OtherType.of(...)`,
/// `OtherType.unsafe(...)`, and `OtherType.Variant(...)` expressions where
/// `OtherType` is from a consumed context.
pub(crate) fn check_context_constraints(
    typed: &checker::TypedCommons,
    consumed_types: &HashMap<String, ConsumedType>,
    local_type_names: &HashSet<String>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    for item in &typed.commons.items {
        if let CommonsItem::Fn(f) = item {
            walk_block_for_constraints(
                &f.body,
                typed,
                consumed_types,
                local_type_names,
                &mut errors,
            );
        }
    }
    errors
}

fn walk_block_for_constraints(
    block: &Block,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                walk_expr_for_constraints(&l.value, typed, consumed, local, errors);
            }
            Statement::Commit(c) => {
                walk_expr_for_constraints(&c.value, typed, consumed, local, errors);
            }
            Statement::Assert(a) => {
                walk_expr_for_constraints(&a.value, typed, consumed, local, errors);
            }
        }
    }
    walk_expr_for_constraints(&block.tail, typed, consumed, local, errors);
}

fn walk_expr_for_constraints(
    e: &Expr,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    match &e.kind {
        ExprKind::ListLit(elems) => {
            for el in elems {
                walk_expr_for_constraints(el, typed, consumed, local, errors);
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                errors.push(
                    CompileError::new(
                        "karn.context.external_construction",
                        type_name.span,
                        format!(
                            "cannot construct `{}` here — it is owned by context `{}`",
                            type_name.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "values of an externally-owned type can only be created inside the owning context",
                    ),
                );
            }
            for f in fields {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                let is_construct = method.name == "of"
                    || method.name == "unsafe"
                    || matches!(
                        typed.types.get(&type_name.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "karn.context.external_construction",
                            type_name.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                type_name.name, method.name, type_name.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // `T.method(...)` written as MethodCall with receiver Ident(T).
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
            {
                let is_construct = method.name == "of"
                    || method.name == "unsafe"
                    || matches!(
                        typed.types.get(&id.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "karn.context.external_construction",
                            id.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                id.name, method.name, id.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // For opaque-exported types from consumed contexts, field
            // access is forbidden — but record types have field access
            // anyway, so the visibility check applies only when the
            // receiver's type is a consumed type. To do this rigorously,
            // we'd consult the expr_types map. Easy path: peek at the
            // receiver if it's an Ident referring to a binding whose
            // declared type points to a consumed type.
            // For v0.4 we use a simpler conservative rule: if the
            // receiver is `T.X` syntax (FieldAccess from an Ident that's
            // a type name) and `T` is consumed and opaque, reject it.
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
                && ct.visibility == Visibility::Opaque
                && typed
                    .types
                    .get(&id.name)
                    .map(|d| matches!(d.body, TypeBody::Sum(_)))
                    .unwrap_or(false)
            {
                errors.push(
                    CompileError::new(
                        "karn.context.opaque_inspection",
                        id.span.merge(field.span),
                        format!(
                            "cannot inspect opaquely-exported type `{}` from outside context `{}`",
                            id.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "opaque exports hide the type's shape; the owning context did not expose variants or fields",
                    ),
                );
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
        }
        ExprKind::Match { discriminant, arms } => {
            // If the discriminant is typed as an opaquely-exported consumed
            // type, the match is forbidden because we can't reveal the
            // variants.
            if let Some(ty) = typed.expr_types.get(&discriminant.span) {
                let display = ty.display();
                if let Some(ct) = consumed.get(&display)
                    && ct.visibility == Visibility::Opaque
                {
                    errors.push(
                        CompileError::new(
                            "karn.context.opaque_inspection",
                            discriminant.span,
                            format!(
                                "cannot `match` on opaquely-exported type `{}` from outside context `{}`",
                                display, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "opaque exports hide the type's shape; the owning context did not expose variants",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(discriminant, typed, consumed, local, errors);
            for arm in arms {
                match &arm.body {
                    MatchBody::Expr(ex) => {
                        walk_expr_for_constraints(ex, typed, consumed, local, errors);
                    }
                    MatchBody::Block(b) => {
                        walk_block_for_constraints(b, typed, consumed, local, errors);
                    }
                }
            }
        }
        ExprKind::Is { value, pattern: _ } => {
            walk_expr_for_constraints(value, typed, consumed, local, errors);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::BinOp(_, l, r) => {
            walk_expr_for_constraints(l, typed, consumed, local, errors);
            walk_expr_for_constraints(r, typed, consumed, local, errors);
        }
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i) => {
            walk_expr_for_constraints(i, typed, consumed, local, errors);
        }
        // v0.20a: walk a lambda's body for construction constraints.
        ExprKind::Lambda(lambda) => {
            walk_expr_for_constraints(&lambda.body, typed, consumed, local, errors)
        }
        ExprKind::Block(b) => walk_block_for_constraints(b, typed, consumed, local, errors),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr_for_constraints(cond, typed, consumed, local, errors);
            walk_block_for_constraints(then_block, typed, consumed, local, errors);
            walk_block_for_constraints(else_block, typed, consumed, local, errors);
        }
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::FloatLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        ExprKind::EffectPure(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Assert(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Mock { args, .. } => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_expr_for_constraints(base, typed, consumed, local, errors);
            for f in overrides {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
    }
}

/// Check capability/provider/service/agent declaration bodies for a context (or
/// adapter) unit. Mutates `typed` to extend the expr_types map with bindings
/// observed in the new bodies.
///
/// The parent builds the shared state read by every per-kind validator — a
/// `resolved` commons snapshot and the `capability_info_map` (local capability
/// signatures, extended with the cross-context flattened caps) — then runs the
/// per-declaration-kind validators in a fixed order. The order is load-bearing:
/// multi-error fixtures assert the diagnostic sequence
/// (capabilities → providers → services → agents).
pub(crate) fn check_context_declarations(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    refs: &mut RefSink,
    hints: &mut HintSink,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    let no_vars: HashSet<String> = HashSet::new();

    // Build a resolved-commons snapshot for the per-handler checker.
    // We synthesise a ResolvedCommons by reusing typed.types / typed.fns /
    // typed.methods; the resolver wouldn't add anything new.
    let local_type_names: std::collections::HashSet<String> = typed.types.keys().cloned().collect();
    let resolved = ResolvedCommons {
        commons: typed.commons.clone(),
        types: typed.types.clone(),
        fns: typed.fns.clone(),
        methods: typed.methods.clone(),
        local_type_names,
        cross_context: cross_context.clone(),
        agents: table.agents.clone(),
    };

    // v0.25: capability operation signatures reference types.
    check_capability_decls(table, &typed.types, &no_vars, refs);

    // Capability info from the table.
    let mut capability_info_map: HashMap<String, CapabilityInfo> = table
        .capabilities
        .iter()
        .map(|(name, decl)| {
            let ops = decl
                .ops
                .iter()
                .map(|op| CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| checker::resolve_type_ref(&p.type_ref, &typed.types))
                        .map(|t| t.unwrap_or(Ty::Unit))
                        .collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                        .unwrap_or(Ty::Unit),
                })
                .collect();
            (
                name.clone(),
                CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            )
        })
        .collect();
    // v0.17: flattened capabilities (`consumes U { Cap }`) enter the local map
    // under their bare names, resolved from the consumed unit's exported
    // capability so bare `given Cap` / `Cap.op(…)` type-check as if local.
    for (cap, unit) in &cross_context.flattened_caps {
        let Some(xcap) = cross_context
            .consumed_capabilities
            .get(unit)
            .and_then(|m| m.get(cap))
        else {
            continue;
        };
        let ops = xcap
            .ops
            .iter()
            .map(|op| CapabilityOpInfo {
                name: op.name.clone(),
                params: op
                    .params
                    .iter()
                    .map(|(_, tr)| checker::resolve_type_ref(tr, &typed.types).unwrap_or(Ty::Unit))
                    .collect(),
                return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                    .unwrap_or(Ty::Unit),
            })
            .collect();
        capability_info_map.insert(
            cap.clone(),
            CapabilityInfo {
                name: cap.clone(),
                ops,
            },
        );
    }

    check_provider_decls(
        typed,
        table,
        cross_context,
        &resolved,
        &capability_info_map,
        refs,
        hints,
        &mut errors,
    );
    check_service_decls(
        typed,
        table,
        cross_context,
        &resolved,
        &capability_info_map,
        refs,
        hints,
        &mut errors,
    );
    check_agent_decls(
        typed,
        table,
        cross_context,
        &capability_info_map,
        &no_vars,
        refs,
        hints,
        &mut errors,
    );

    errors
}

/// v0.25: capability operation signatures reference types; record them under
/// the capability as owner (the table is unit-level — the owner re-attributes
/// spans to the declaring file at assembly).
fn check_capability_decls(
    table: &UnitTable,
    types: &HashMap<String, TypeDecl>,
    no_vars: &HashSet<String>,
    refs: &mut RefSink,
) {
    for (name, decl) in &table.capabilities {
        refs.set_owner(name);
        for op in &decl.ops {
            for p in &op.params {
                checker::record_type_refs(&p.type_ref, types, no_vars, refs);
            }
            checker::record_type_refs(&op.return_type, types, no_vars, refs);
        }
    }
    refs.clear_owner();
}

/// Check provider bodies. v0.12: a provider may declare `given` and use
/// those capabilities in its bodies (provider composition). Bodies are
/// effectful if the operation returns Effect[T]; no `self`. Also detects
/// provider dependency cycles over capabilities.
#[allow(clippy::too_many_arguments)]
fn check_provider_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    resolved: &ResolvedCommons,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    errors: &mut Vec<CompileError>,
) {
    for provider in table.providers.values() {
        refs.set_owner(&provider.provider_name.name);
        // v0.25: `provides Cap = …` references the capability.
        if table.capabilities.contains_key(&provider.capability.name)
            || cross_context
                .flattened_caps
                .contains_key(&provider.capability.name)
        {
            record_capability_clause_ref(&provider.capability, cross_context, refs);
        }
        // Build the provider's capability scope from its `given`, validating
        // each name is a declared capability.
        let mut provider_caps: HashMap<String, CapabilityInfo> = HashMap::new();
        for cap_ref in &provider.given {
            if let Some(info) =
                resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
            {
                provider_caps.insert(cap_ref.key().to_string(), info);
            }
        }
        for op in &provider.ops {
            checker::check_handler_body(
                &op.body,
                &op.return_type,
                op.return_type.span(),
                &op.params,
                resolved,
                &mut typed.expr_types,
                errors,
                refs,
                hints,
                provider_caps.clone(),
                capability_info_map.clone(),
                None,
                None,
                // The provider's `given` keys are in scope (so cross-context
                // capability calls resolve), but unused-`given` is not reported
                // per-op: a capability may be used in one op but not another.
                // No `given_anchor`: the clause lives on the `provides` line,
                // not at the op's return type, so an absent clause is not
                // synthesised here (v0.26).
                &provider.given,
                None,
                false,
            );
        }
    }

    // v0.12: providers form a dependency graph over capabilities (a provider's
    // `given` are the capabilities its provided capability depends on). Reject
    // a cycle — the composition root cannot instantiate one in dependency
    // order. Self-provision (`provides X = … given X`) is the trivial cycle.
    detect_provider_dependency_cycles(&table.providers, errors);
}

/// Check service handlers across all services in this context: HTTP/cron/queue
/// handler shape and per-kind duplicate detection (route/schedule/consumer),
/// then each handler's `given` clause and body. The duplicate-detection passes
/// run before the body pass so the `karn.<kind>.duplicate_*` diagnostics
/// precede the body diagnostics in multi-error fixtures.
#[allow(clippy::too_many_arguments)]
fn check_service_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    resolved: &ResolvedCommons,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    errors: &mut Vec<CompileError>,
) {
    // v0.9: validate HTTP handler shape and check for duplicate routes
    // across all services in this context.
    let mut route_first_span: HashMap<(HttpMethod, String), Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Http { method, path } = &handler.kind else {
                continue;
            };
            validate_http_handler(handler, *method, path, &typed.types, errors);
            let key = (*method, path.clone());
            if let Some(prev) = route_first_span.get(&key).copied() {
                errors.push(
                    CompileError::new(
                        "karn.http.duplicate_route",
                        handler.span,
                        format!(
                            "duplicate HTTP route: another handler already declares `{} {}`",
                            method.as_str(),
                            path,
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                route_first_span.insert(key, handler.span);
            }
        }
    }

    // v0.10a: validate `on cron` handler shape and check for duplicate
    // schedules across all services in this context (the generated
    // `scheduled` dispatcher routes on `event.cron`, so duplicates are
    // ambiguous).
    let mut schedule_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Cron { expr } = &handler.kind else {
                continue;
            };
            validate_cron_handler(handler, expr, errors);
            if let Some(prev) = schedule_first_span.get(expr).copied() {
                errors.push(
                    CompileError::new(
                        "karn.cron.duplicate_schedule",
                        handler.span,
                        format!(
                            "duplicate cron schedule: another handler already declares `{expr}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                schedule_first_span.insert(expr.clone(), handler.span);
            }
        }
    }

    // v0.10b: validate `on queue` handler shape and check for duplicate
    // consumers across all services in this context (the generated `queue`
    // dispatcher routes on `batch.queue`, so two consumers of the same queue
    // are ambiguous).
    let mut consumer_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Queue { name } = &handler.kind else {
                continue;
            };
            validate_queue_handler(handler, name, errors);
            if let Some(prev) = consumer_first_span.get(name).copied() {
                errors.push(
                    CompileError::new(
                        "karn.queue.duplicate_consumer",
                        handler.span,
                        format!(
                            "duplicate queue consumer: another handler already consumes `{name}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                consumer_first_span.insert(name.clone(), handler.span);
            }
        }
    }

    // Check service handlers.
    for service in table.services.values() {
        refs.set_owner(&service.name.name);
        for handler in &service.handlers {
            // The given clause must reference only declared (local) or
            // exported (cross-context) capabilities.
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "karn.service.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "service handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            checker::check_handler_body(
                &handler.body,
                &handler.return_type,
                handler.return_type.span(),
                &handler.params,
                resolved,
                &mut typed.expr_types,
                errors,
                refs,
                hints,
                handler_caps,
                capability_info_map.clone(),
                None,
                None,
                &handler.given,
                Some(handler.return_type.span()),
                true,
            );
        }
    }
}

/// Check agent handlers across all agents in this context: state-field type
/// refs, per-field initialiser / zero-value validity, and each handler's
/// `given` clause and body (with the synthetic `self` and state types in
/// scope).
#[allow(clippy::too_many_arguments)]
fn check_agent_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    no_vars: &HashSet<String>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    errors: &mut Vec<CompileError>,
) {
    for agent in table.agents.values() {
        refs.set_owner(&agent.name.name);
        // v0.25: the agent's key type and state field types reference types.
        checker::record_type_refs(&agent.key_type, &typed.types, no_vars, refs);
        for field in &agent.state_fields {
            checker::record_type_refs(&field.type_ref, &typed.types, no_vars, refs);
        }
        // Build the agent's state type as a synthetic record. We expose it
        // under the name `<AgentName>State` in the type table so the body
        // can reference it.
        let agent_state_name = format!("{}State", agent.name.name);
        // Build a synthetic Record TypeDecl and stuff it into a *clone* of
        // the resolved types so handler bodies see it.
        let synthetic_state = TypeDecl {
            name: Ident {
                name: agent_state_name.clone(),
                span: agent.state_span,
            },
            body: TypeBody::Record(RecordBody {
                fields: agent.state_fields.clone(),
                span: agent.state_span,
            }),
            documentation: None,
            span: agent.state_span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = typed.types.clone();
        types_for_handler.insert(agent_state_name.clone(), synthetic_state.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
        };
        // v0.11: every state field must have a defined initial value for a
        // fresh key — an explicit static initialiser, or (v0.9.2) an implicit
        // zero. A field with neither is rejected.
        for field in &agent.state_fields {
            if let Some(init) = &field.init {
                checker::check_state_initialiser(
                    init,
                    &field.type_ref,
                    &resolved_for_handler,
                    &mut typed.expr_types,
                    errors,
                    refs,
                    hints,
                );
            } else if checker::zero_value_ts(
                &field.type_ref,
                field.refinement.as_ref(),
                &typed.types,
            )
            .is_none()
            {
                errors.push(
                    CompileError::new(
                        "karn.agents.non_zeroable_state_field",
                        field.span,
                        format!(
                            "agent `{}` state field `{}` has no defined zero value, so a \
                             fresh key cannot be initialised",
                            agent.name.name, field.name.name
                        ),
                    )
                    .with_note(
                        "add an initialiser (`field: T = value`) to give a fresh key its \
                         starting value, or wrap the field in `Option[…]` (None means \
                         \"never set\")",
                    ),
                );
            }
        }
        let state_ty = Ty::Named {
            name: agent_state_name.clone(),
            kind: checker::NamedKind::Record,
        };
        let key_ty = checker::resolve_type_ref(&agent.key_type, &typed.types).unwrap_or(Ty::Unit);
        let mut self_scope: HashMap<String, Ty> = HashMap::new();
        // `self` is a synthetic record with two fields: the key and `state`.
        // But the parser treats `self.x` as FieldAccess on Ident("self"), so
        // we need to give `self` a record type with both. Easiest: a one-off
        // synthetic record type.
        let agent_self_name = format!("__{}Self", agent.name.name);
        let self_decl = TypeDecl {
            name: Ident {
                name: agent_self_name.clone(),
                span: agent.span,
            },
            body: TypeBody::Record(RecordBody {
                fields: vec![
                    RecordField {
                        name: Ident {
                            name: agent.key_name.name.clone(),
                            span: agent.key_name.span,
                        },
                        type_ref: agent.key_type.clone(),
                        refinement: None,
                        init: None,
                        span: agent.key_name.span,
                    },
                    RecordField {
                        name: Ident {
                            name: "state".to_string(),
                            span: agent.state_span,
                        },
                        type_ref: TypeRef::Named(Ident {
                            name: agent_state_name.clone(),
                            span: agent.state_span,
                        }),
                        refinement: None,
                        init: None,
                        span: agent.state_span,
                    },
                ],
                span: agent.span,
            }),
            documentation: None,
            span: agent.span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = resolved_for_handler.types.clone();
        types_for_handler.insert(agent_self_name.clone(), self_decl.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
        };
        self_scope.insert(
            "self".to_string(),
            Ty::Named {
                name: agent_self_name.clone(),
                kind: checker::NamedKind::Record,
            },
        );
        let _ = key_ty;

        for handler in &agent.handlers {
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "karn.agent.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "agent handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            checker::check_handler_body(
                &handler.body,
                &handler.return_type,
                handler.return_type.span(),
                &handler.params,
                &resolved_for_handler,
                &mut typed.expr_types,
                errors,
                refs,
                hints,
                handler_caps,
                capability_info_map.clone(),
                Some(state_ty.clone()),
                Some(self_scope.clone()),
                &handler.given,
                Some(handler.return_type.span()),
                true,
            );
        }
    }
}

/// Structural equality for TypeRef, used by v0.5 capability/provider signature
/// matching. Doesn't resolve names — it compares the surface syntax. Named
/// types match by their literal identifier; built-ins match by variant.
pub(crate) fn type_refs_match(a: &TypeRef, b: &TypeRef) -> bool {
    match (a, b) {
        (TypeRef::Base(x, _), TypeRef::Base(y, _)) => x == y,
        (TypeRef::Named(x), TypeRef::Named(y)) => x.name == y.name,
        (TypeRef::Result(t1, e1, _), TypeRef::Result(t2, e2, _)) => {
            type_refs_match(t1, t2) && type_refs_match(e1, e2)
        }
        (TypeRef::Option(t1, _), TypeRef::Option(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::Effect(t1, _), TypeRef::Effect(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::HttpResult(t1, _), TypeRef::HttpResult(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::ValidationError(_), TypeRef::ValidationError(_)) => true,
        (TypeRef::JsonError(_), TypeRef::JsonError(_)) => true,
        (TypeRef::Unit(_), TypeRef::Unit(_)) => true,
        _ => false,
    }
}

/// Validate an `on http METHOD "path"` handler (v0.9 §4.1):
///
/// - Path must start with `/`, must not be `/_karn/...` (reserved).
/// - Every `:name` segment binds to a handler parameter of the same name.
/// - Every parameter is either a path parameter or named `body`.
/// - Path parameter types are constructible from `String` (`String`, refined
///   `String`, or opaque `String`).
/// - GET / DELETE handlers may not have a `body` parameter.
/// - The handler return type must be `Effect[HttpResult[T]]`.
fn validate_http_handler(
    handler: &Handler,
    method: HttpMethod,
    path: &str,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    if !path.starts_with('/') {
        errors.push(CompileError::new(
            "karn.http.invalid_path",
            handler.span,
            format!("HTTP path `{path}` must start with `/`"),
        ));
    }
    if path.starts_with("/_karn/") || path == "/_karn" {
        errors.push(
            CompileError::new(
                "karn.http.reserved_prefix",
                handler.span,
                format!("HTTP path `{path}` uses the reserved `/_karn/` prefix",),
            )
            .with_note("paths under `/_karn/` are reserved for internal Karn dispatch"),
        );
    }
    // Parse segments and collect path-parameter names.
    let mut path_param_names: Vec<&str> = Vec::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if let Some(rest) = seg.strip_prefix(':') {
            if rest.is_empty() {
                errors.push(CompileError::new(
                    "karn.http.invalid_path",
                    handler.span,
                    format!("HTTP path `{path}` has an empty parameter segment `:`"),
                ));
            } else {
                path_param_names.push(rest);
            }
        }
    }
    // Every :name must have a matching handler parameter.
    for name in &path_param_names {
        if !handler.params.iter().any(|p| p.name.name == *name) {
            errors.push(CompileError::new(
                "karn.http.unbound_path_param",
                handler.span,
                format!("path parameter `:{name}` has no matching handler parameter `{name}`",),
            ));
        }
    }
    // Every handler parameter must be either a path param or `body`.
    for p in &handler.params {
        let is_path = path_param_names.iter().any(|n| n == &p.name.name.as_str());
        let is_body = p.name.name == "body";
        if !is_path && !is_body {
            errors.push(
                CompileError::new(
                    "karn.http.extra_param",
                    p.span,
                    format!(
                        "handler parameter `{}` is not a path parameter and is not named `body`",
                        p.name.name
                    ),
                )
                .with_note(
                    "HTTP handler parameters must either match a `:name` path segment or be named `body`",
                ),
            );
        }
        // Path params must be constructible from String.
        if is_path && !is_string_constructible(&p.type_ref, types) {
            errors.push(
                CompileError::new(
                    "karn.http.path_param_not_stringy",
                    p.type_ref.span(),
                    format!(
                        "path parameter `{}` must have a type constructible from `String` (got `{}`)",
                        p.name.name,
                        ts_type_ref_display(&p.type_ref),
                    ),
                )
                .with_note(
                    "use `String`, a refined `String`, or an opaque type whose base is `String`",
                ),
            );
        }
        if is_body && method.forbids_body() {
            errors.push(
                CompileError::new(
                    "karn.http.body_on_get_or_delete",
                    p.span,
                    format!(
                        "`on http {}` handlers may not declare a `body` parameter",
                        method.as_str()
                    ),
                )
                .with_note("GET and DELETE requests conventionally carry no body in Karn v0.9"),
            );
        }
    }
    // Validate return type shape.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => matches!(inner.as_ref(), TypeRef::HttpResult(_, _)),
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.http.return_not_effect_http_result",
            handler.return_type.span(),
            format!(
                "`on http` handler must return `Effect[HttpResult[T]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// Validate an `on cron "expr" (at: Int?) -> Effect[Result[(), E]]` handler
/// (v0.10a §4.1): at most one `Int` parameter (the scheduled time, Unix epoch
/// milliseconds), a structurally well-formed schedule, and the unit-Result
/// return shape. The service-only rule is enforced earlier, in the parser
/// (`karn.parse.cron_in_agent`).
fn validate_cron_handler(handler: &Handler, expr: &str, errors: &mut Vec<CompileError>) {
    // A cron handler takes at most one parameter — the scheduled time, typed
    // `Int` (epoch milliseconds). A scheduled trigger has no other payload.
    if handler.params.len() > 1 {
        errors.push(
            CompileError::new(
                "karn.cron.bad_params",
                handler.params[1].span,
                "`on cron` handlers take at most one parameter (the scheduled time)",
            )
            .with_note("a scheduled trigger's only input is the time it fired"),
        );
    } else if let Some(p) = handler.params.first()
        && !matches!(p.type_ref, TypeRef::Base(BaseType::Int, _))
    {
        errors.push(
            CompileError::new(
                "karn.cron.bad_params",
                p.type_ref.span(),
                format!(
                    "an `on cron` parameter must be `Int` (the scheduled time in epoch milliseconds), got `{}`",
                    ts_type_ref_display(&p.type_ref),
                ),
            )
            .with_note("wrap it in your own time type inside the body if you want stronger typing"),
        );
    }
    // The schedule must be five whitespace-separated fields (light structural
    // check; per-field validation is deferred — v0.10 §4.1, [DECISION 4]).
    let fields = expr.split_whitespace().count();
    if fields != 5 {
        errors.push(
            CompileError::new(
                "karn.cron.invalid_schedule",
                handler.span,
                format!(
                    "cron expression `{expr}` must have exactly five whitespace-separated fields (got {fields})",
                ),
            )
            .with_note("the fields are: minute hour day-of-month month day-of-week"),
        );
    }
    // The return type must be `Effect[Result[(), E]]`.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => match inner.as_ref() {
            TypeRef::Result(ok, _err, _) => matches!(ok.as_ref(), TypeRef::Unit(_)),
            _ => false,
        },
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.cron.return_not_effect_result",
            handler.return_type.span(),
            format!(
                "`on cron` handler must return `Effect[Result[(), E]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// Validate an `on queue "name" (message: T) -> Effect[Result[(), E]]` handler
/// (v0.10b §4.2): a non-empty queue name, exactly one parameter (the message,
/// any wire-deserialisable type), and the unit-Result return shape. `Ok(())`
/// acknowledges the message at emission; `Err` retries it. The service-only
/// rule is enforced earlier, in the parser (`karn.parse.queue_in_agent`).
fn validate_queue_handler(handler: &Handler, name: &str, errors: &mut Vec<CompileError>) {
    if name.is_empty() {
        errors.push(CompileError::new(
            "karn.queue.invalid_name",
            handler.span,
            "`on queue` requires a non-empty queue name",
        ));
    }
    // Exactly one parameter — the message. (Conventionally named `message`.)
    if handler.params.len() != 1 {
        errors.push(
            CompileError::new(
                "karn.queue.bad_params",
                handler.span,
                format!(
                    "`on queue` handlers take exactly one parameter (the message), got {}",
                    handler.params.len(),
                ),
            )
            .with_note("a queue consumer processes one message per invocation"),
        );
    }
    // The return type must be `Effect[Result[(), E]]`.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => match inner.as_ref() {
            TypeRef::Result(ok, _err, _) => matches!(ok.as_ref(), TypeRef::Unit(_)),
            _ => false,
        },
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.queue.return_not_effect_result",
            handler.return_type.span(),
            format!(
                "`on queue` handler must return `Effect[Result[(), E]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// True when `r` resolves to `String`, a refined-base `String`, or an
/// opaque-base `String`. v0.9 path parameter requirement.
fn is_string_constructible(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> bool {
    match r {
        TypeRef::Base(BaseType::String, _) => true,
        TypeRef::Named(id) => match types.get(&id.name).map(|t| &t.body) {
            Some(TypeBody::Refined { base, .. }) => *base == BaseType::String,
            Some(TypeBody::Opaque { base, .. }) => *base == BaseType::String,
            _ => false,
        },
        _ => false,
    }
}

/// v0.20a: function types are confined to non-boundary positions — fn/lambda
/// parameters, returns, and locals. Walk a type reference and reject any
/// function type found in a position that would serialise, persist, or cross
/// a boundary (`karn.types.function_at_boundary`).
fn reject_fn_types(r: &TypeRef, what: &str, errors: &mut Vec<CompileError>) {
    match r {
        TypeRef::Fn(_, _, span) => {
            errors.push(
                CompileError::new(
                    "karn.types.function_at_boundary",
                    *span,
                    format!(
                        "a function type cannot appear in {what} — functions cannot serialise or cross a boundary"
                    ),
                )
                .with_note(
                    "function types are confined to fn/lambda parameters, returns, and locals",
                ),
            );
        }
        // v0.20b: the boundary rule looks through collections — a
        // `List[Int -> Int]` field is still `function_at_boundary`.
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            reject_fn_types(a, what, errors);
            reject_fn_types(b, what, errors);
        }
        TypeRef::Option(a, _)
        | TypeRef::Effect(a, _)
        | TypeRef::HttpResult(a, _)
        | TypeRef::List(a, _) => reject_fn_types(a, what, errors),
        TypeRef::Base(..)
        | TypeRef::Named(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
    }
}

/// v0.20a: apply the function-type boundary confinement to every serialisable
/// or boundary-crossing position in a file's items: record fields and sum
/// payloads (types can cross contexts and persist), service/agent handler
/// signatures (the Workers wire), capability operation signatures (kept out
/// in v0.20a — see ADR 0030), agent state fields, and agent keys. Free `fn`
/// signatures are deliberately NOT walked — they are the non-boundary home
/// of function types.
pub(crate) fn check_function_type_boundaries(
    parsed: &[ParsedFile],
    errors: &mut Vec<CompileError>,
) {
    for pf in parsed {
        check_function_type_boundary_items(pf.items(), errors);
    }
}

/// Item-level body of the boundary confinement, shared with the single-file
/// (legacy) compile path in `lib.rs`.
pub(crate) fn check_function_type_boundary_items(
    items: &[CommonsItem],
    errors: &mut Vec<CompileError>,
) {
    {
        for item in items {
            match item {
                CommonsItem::Type(t) => match &t.body {
                    TypeBody::Record(r) => {
                        for f in &r.fields {
                            reject_fn_types(&f.type_ref, "a record field", errors);
                        }
                    }
                    TypeBody::Sum(s) => {
                        for v in &s.variants {
                            for p in &v.payload {
                                reject_fn_types(&p.type_ref, "a sum-variant payload", errors);
                            }
                        }
                    }
                    TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
                },
                CommonsItem::Capability(c) => {
                    for op in &c.ops {
                        for p in &op.params {
                            reject_fn_types(
                                &p.type_ref,
                                "a capability operation signature",
                                errors,
                            );
                        }
                        reject_fn_types(
                            &op.return_type,
                            "a capability operation signature",
                            errors,
                        );
                    }
                }
                CommonsItem::Service(s) => {
                    for h in &s.handlers {
                        for p in &h.params {
                            reject_fn_types(&p.type_ref, "a service handler signature", errors);
                        }
                        reject_fn_types(&h.return_type, "a service handler signature", errors);
                    }
                }
                CommonsItem::Agent(a) => {
                    reject_fn_types(&a.key_type, "an agent key", errors);
                    for f in &a.state_fields {
                        reject_fn_types(&f.type_ref, "an agent state field", errors);
                    }
                    for h in &a.handlers {
                        for p in &h.params {
                            reject_fn_types(&p.type_ref, "an agent handler signature", errors);
                        }
                        reject_fn_types(&h.return_type, "an agent handler signature", errors);
                    }
                }
                CommonsItem::Fn(_) | CommonsItem::Provider(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod platform_lock_tests {
    use super::{LockViolation, Platform, lock_violation};
    use std::collections::BTreeMap;

    fn native(entries: &[(Platform, &str)]) -> BTreeMap<Platform, String> {
        entries
            .iter()
            .map(|(p, u)| (*p, (*u).to_string()))
            .collect()
    }

    #[test]
    fn empty_closure_imposes_no_lock() {
        assert_eq!(lock_violation(&native(&[]), Platform::Node), None);
    }

    #[test]
    fn matching_platform_is_fine() {
        let n = native(&[(Platform::Cloudflare, "karn.cloudflare")]);
        assert_eq!(lock_violation(&n, Platform::Cloudflare), None);
    }

    #[test]
    fn mismatched_platform_is_required() {
        let n = native(&[(Platform::Cloudflare, "karn.cloudflare")]);
        assert_eq!(
            lock_violation(&n, Platform::Node),
            Some(LockViolation::Required {
                needed: Platform::Cloudflare,
                unit: "karn.cloudflare".to_string(),
            })
        );
    }

    // The conflict arm is not yet reachable end-to-end (only one platform
    // ships native capabilities until `karn.aws`); the rule is exercised here
    // with a synthetic two-platform set so it does not ship untested
    // (proposal v0.19, review call).
    #[test]
    fn two_platforms_conflict_regardless_of_selection() {
        let n = native(&[
            (Platform::Cloudflare, "karn.cloudflare"),
            (Platform::Node, "karn.synthetic"),
        ]);
        let v = lock_violation(&n, Platform::Cloudflare);
        assert_eq!(
            v,
            Some(LockViolation::Conflict {
                a: (Platform::Cloudflare, "karn.cloudflare".to_string()),
                b: (Platform::Node, "karn.synthetic".to_string()),
            })
        );
    }
}
