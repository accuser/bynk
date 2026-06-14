use super::*;

/// v0.25 (ADR 0053): walk every parsed file's top-level declarations into
/// the def table (synthetic first-party units and test files excluded —
/// neither declares user-editable symbols), then qualify and attach the
/// recorded edges. Methods register as owners only (attribution), not as
/// symbols — they are deferred along with fields and op names.
pub(crate) fn assemble_index(
    parsed: &[ParsedFile],
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    refs: RefSink,
) -> ProjectIndex {
    let mut builder = IndexBuilder::default();
    let mut uses = unit_uses.clone();
    uses.extend(refs.extra_uses);
    builder.set_uses(uses);
    builder.set_consumes(unit_consumes.clone());
    for pf in parsed {
        if matches!(pf.kind, UnitKind::Test | UnitKind::Integration) {
            continue;
        }
        let unit = pf.unit.name().joined();
        // v0.28 (ADR 0057): synthetic first-party units stay out of
        // `symbols` (their defs point at files not on disk — the v0.25
        // rule), but their declarations register for the second
        // qualification pass so references to them colour as tokens.
        if pf.synthetic {
            for item in pf.items() {
                let (kind, name, modifiers) = match item {
                    CommonsItem::Type(t) => (
                        SymbolKind::Type,
                        &t.name.name,
                        symbol_modifiers(&unit, Some(t)),
                    ),
                    CommonsItem::Fn(f) => match &f.name {
                        FnName::Free(id) => {
                            (SymbolKind::Fn, &id.name, symbol_modifiers(&unit, None))
                        }
                        FnName::Method { .. } => continue,
                    },
                    CommonsItem::Capability(c) => (
                        SymbolKind::Capability,
                        &c.name.name,
                        symbol_modifiers(&unit, None),
                    ),
                    CommonsItem::Service(s) => (
                        SymbolKind::Service,
                        &s.name.name,
                        symbol_modifiers(&unit, None),
                    ),
                    CommonsItem::Agent(a) => (
                        SymbolKind::Agent,
                        &a.name.name,
                        symbol_modifiers(&unit, None),
                    ),
                    CommonsItem::Provider(p) => (
                        SymbolKind::Provider,
                        &p.provider_name.name,
                        symbol_modifiers(&unit, None),
                    ),
                };
                builder.add_first_party_def(&unit, kind, name, modifiers);
            }
            continue;
        }
        let site = |id: &Ident| SiteRef {
            path: pf.source_path.clone(),
            span: id.span,
        };
        for item in pf.items() {
            match item {
                CommonsItem::Type(t) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Type,
                        &t.name.name,
                        site(&t.name),
                        symbol_modifiers(&unit, Some(t)),
                    );
                    // v0.36 (ADR 0069, slice 2): record fields are first-class
                    // symbols keyed by the compound `"Type.field"` name.
                    if let TypeBody::Record(r) = &t.body {
                        for field in &r.fields {
                            builder.add_def(
                                &unit,
                                SymbolKind::Field,
                                &format!("{}.{}", t.name.name, field.name.name),
                                site(&field.name),
                                symbol_modifiers(&unit, None),
                            );
                        }
                    }
                }
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(id) => {
                        builder.add_def(
                            &unit,
                            SymbolKind::Fn,
                            &id.name,
                            site(id),
                            symbol_modifiers(&unit, None),
                        );
                    }
                    FnName::Method { .. } => {
                        // v0.36 (ADR 0069): a method is a first-class symbol
                        // keyed by the compound `"Type.method"` name, and (as
                        // before) an attribution owner for call-hierarchy.
                        builder.add_owner(&unit, &f.name.display(), &pf.source_path);
                        builder.add_def(
                            &unit,
                            SymbolKind::Method,
                            &f.name.display(),
                            site(f.name.ident()),
                            symbol_modifiers(&unit, None),
                        );
                    }
                },
                CommonsItem::Capability(c) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Capability,
                        &c.name.name,
                        site(&c.name),
                        symbol_modifiers(&unit, None),
                    );
                    // v0.36 (ADR 0069, slice 2): capability operations are
                    // first-class symbols keyed by the compound `"Cap.op"` name.
                    for op in &c.ops {
                        builder.add_def(
                            &unit,
                            SymbolKind::CapabilityOp,
                            &format!("{}.{}", c.name.name, op.name.name),
                            site(&op.name),
                            symbol_modifiers(&unit, None),
                        );
                    }
                }
                CommonsItem::Service(s) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Service,
                        &s.name.name,
                        site(&s.name),
                        symbol_modifiers(&unit, None),
                    );
                }
                CommonsItem::Agent(a) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Agent,
                        &a.name.name,
                        site(&a.name),
                        symbol_modifiers(&unit, None),
                    );
                }
                CommonsItem::Provider(p) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Provider,
                        &p.provider_name.name,
                        site(&p.provider_name),
                        symbol_modifiers(&unit, None),
                    );
                }
            }
        }
    }
    builder.build(refs.edges)
}

/// v0.28 (ADR 0057): a symbol's semantic-token modifiers from its
/// declaration. `refined` only when a refinement is present — `type X = Int`
/// is `Refined { refinement: None }`, a plain alias, and carries neither;
/// `opaque` is orthogonal (an `opaque … where` type carries both).
/// `platform_native` when the declaring unit is a platform adapter.
fn symbol_modifiers(unit: &str, type_decl: Option<&TypeDecl>) -> crate::index::SymbolModifiers {
    let (refined, opaque) = match type_decl.map(|t| &t.body) {
        Some(TypeBody::Refined { refinement, .. }) => (refinement.is_some(), false),
        Some(TypeBody::Opaque { refinement, .. }) => (refinement.is_some(), true),
        _ => (false, false),
    };
    crate::index::SymbolModifiers {
        refined,
        opaque,
        platform_native: crate::firstparty::platform_of(unit).is_some(),
    }
}

/// Combined symbol tables for a single logical commons or context.
#[derive(Clone, Default)]
pub struct UnitTable {
    #[allow(dead_code)]
    pub kind: Option<UnitKind>,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub methods: HashMap<String, ResolverMethodTable>,
    /// Per-context capabilities (v0.5). Empty for commons.
    pub capabilities: HashMap<String, CapabilityDecl>,
    /// Per-context providers (v0.5). One provider per capability in v0.5.
    /// Key: capability name. Value: provider declaration.
    pub providers: HashMap<String, ProviderDecl>,
    /// Per-context services (v0.5). Empty for commons.
    pub services: HashMap<String, ServiceDecl>,
    /// Per-context agents (v0.5). Empty for commons.
    pub agents: HashMap<String, AgentDecl>,
    /// v0.15: capability names this context offers to consumers via
    /// `exports capability { … }`. Empty for commons.
    pub exported_capabilities: std::collections::HashSet<String>,
}

pub(crate) fn build_unit_table(
    _name: &str,
    kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    errors: &mut Vec<CompileError>,
) -> UnitTable {
    let mut table = UnitTable {
        kind: Some(kind),
        ..UnitTable::default()
    };
    for &i in indices {
        for item in parsed[i].items() {
            if let CommonsItem::Type(t) = item {
                if let Some(prev) = table.types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_type",
                            t.name.span,
                            format!("type `{}` is already declared", t.name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else {
                    table.types.insert(t.name.name.clone(), t.clone());
                    table.methods.entry(t.name.name.clone()).or_default();
                }
            }
        }
    }
    // v0.15: collect the names a context exports as capabilities.
    // v0.17: adapters export capabilities too.
    for &i in indices {
        {
            for clause in parsed[i].exports() {
                if matches!(clause.kind, ExportKind::Capability) {
                    for n in &clause.names {
                        table.exported_capabilities.insert(n.name.clone());
                    }
                }
            }
        }
    }
    // v0.5: collect capabilities, providers, services, agents.
    for &i in indices {
        for item in parsed[i].items() {
            match item {
                CommonsItem::Capability(c) => {
                    if kind != UnitKind::Context && kind != UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.capability.outside_context",
                            c.span,
                            "`capability` declarations are only allowed inside a context or adapter",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.capabilities.get(&c.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_capability",
                                c.name.span,
                                format!("capability `{}` is already declared", c.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.capabilities.insert(c.name.name.clone(), c.clone());
                    }
                }
                CommonsItem::Provider(p) => {
                    match kind {
                        UnitKind::Context => {
                            // v0.17: a bodiless (external) provider is only legal
                            // inside an adapter.
                            if p.external {
                                errors.push(CompileError::new(
                                    "karn.context.external_provider",
                                    p.span,
                                    "an external (bodiless) provider is only allowed inside an `adapter` — a context provider must have a Karn body",
                                ));
                                continue;
                            }
                        }
                        UnitKind::Adapter => {
                            // v0.17: an adapter provider must be external — its
                            // implementation comes from the binding.
                            if !p.external {
                                errors.push(CompileError::new(
                                    "karn.adapter.provider_has_body",
                                    p.span,
                                    "a provider inside an `adapter` must be external (no body) — its implementation is supplied by the binding",
                                ));
                                continue;
                            }
                        }
                        _ => {
                            errors.push(CompileError::new(
                                "karn.provider.outside_context",
                                p.span,
                                "`provides` declarations are only allowed inside a context or adapter",
                            ));
                            continue;
                        }
                    }
                    if let Some(prev) = table.providers.get(&p.capability.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_provider",
                                p.span,
                                format!(
                                    "capability `{}` already has a provider in this context",
                                    p.capability.name
                                ),
                            )
                            .with_label(prev.span, "previously provided here"),
                        );
                    } else {
                        table.providers.insert(p.capability.name.clone(), p.clone());
                    }
                }
                CommonsItem::Service(s) => {
                    if kind == UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.adapter.disallowed_item",
                            s.span,
                            "an `adapter` may not declare a `service` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.service.outside_context",
                            s.span,
                            "`service` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.services.get(&s.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_service",
                                s.name.span,
                                format!("service `{}` is already declared", s.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.services.insert(s.name.name.clone(), s.clone());
                    }
                }
                CommonsItem::Agent(a) => {
                    if kind == UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.adapter.disallowed_item",
                            a.span,
                            "an `adapter` may not declare an `agent` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.agent.outside_context",
                            a.span,
                            "`agent` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.agents.get(&a.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_agent",
                                a.name.span,
                                format!("agent `{}` is already declared", a.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.agents.insert(a.name.name.clone(), a.clone());
                    }
                }
                _ => {}
            }
        }
    }
    for &i in indices {
        for item in parsed[i].items() {
            let CommonsItem::Fn(f) = item else { continue };
            match &f.name {
                FnName::Free(id) => {
                    if let Some(prev) = table.fns.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_fn",
                                id.span,
                                format!("function `{}` is already declared", id.name),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else if let Some(prev) = table.types.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.name_conflict",
                                id.span,
                                format!(
                                    "function `{}` conflicts with a type of the same name",
                                    id.name
                                ),
                            )
                            .with_label(prev.name.span, "type declared here"),
                        );
                    } else {
                        table.fns.insert(id.name.clone(), f.clone());
                    }
                }
                FnName::Method {
                    type_name,
                    method_name,
                } => {
                    if !table.types.contains_key(&type_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.method_unknown_type",
                                type_name.span,
                                format!(
                                    "method `{}.{}` attached to an unknown type `{}`",
                                    type_name.name, method_name.name, type_name.name
                                ),
                            )
                            .with_note(
                                "methods can only be declared on types defined in the same commons or context (across all of its files)",
                            ),
                        );
                        continue;
                    }
                    let mt = table.methods.entry(type_name.name.clone()).or_default();
                    let bucket = if f.has_self {
                        &mut mt.instance
                    } else {
                        &mut mt.statics
                    };
                    if let Some(prev) = bucket.get(&method_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_method",
                                method_name.span,
                                format!(
                                    "method `{}.{}` is already declared",
                                    type_name.name, method_name.name
                                ),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else {
                        bucket.insert(method_name.name.clone(), f.clone());
                    }
                }
            }
        }
    }
    table
}

/// For each name declared in the unit (type, fn, method), record which
/// source file declared it. Used by the emitter to render relative imports.
#[derive(Clone)]
pub struct FileDeclIndex {
    pub types: HashMap<String, PathBuf>,
    pub fns: HashMap<String, PathBuf>,
    pub methods: HashMap<String, HashMap<String, PathBuf>>,
}

pub(crate) fn build_file_decl_index(indices: &[usize], parsed: &[ParsedFile]) -> FileDeclIndex {
    let mut idx = FileDeclIndex {
        types: HashMap::new(),
        fns: HashMap::new(),
        methods: HashMap::new(),
    };
    for &i in indices {
        let path = parsed[i].source_path.clone();
        for item in parsed[i].items() {
            match item {
                CommonsItem::Type(t) => {
                    idx.types
                        .entry(t.name.name.clone())
                        .or_insert_with(|| path.clone());
                }
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(id) => {
                        idx.fns
                            .entry(id.name.clone())
                            .or_insert_with(|| path.clone());
                    }
                    FnName::Method {
                        type_name,
                        method_name,
                    } => {
                        idx.methods
                            .entry(type_name.name.clone())
                            .or_default()
                            .entry(method_name.name.clone())
                            .or_insert_with(|| path.clone());
                    }
                },
                CommonsItem::Capability(_)
                | CommonsItem::Provider(_)
                | CommonsItem::Service(_)
                | CommonsItem::Agent(_) => {}
            }
        }
    }
    idx
}

pub(crate) fn uses_span_of(parsed: &[ParsedFile], indices: &[usize], target: &str) -> Option<Span> {
    for &i in indices {
        for u in parsed[i].uses() {
            if u.target.joined() == target {
                return Some(u.span);
            }
        }
    }
    None
}

/// Build the [`resolver::CrossContextInfo`] for a given consuming context.
/// Used by both the resolver/checker (per-file processing) and the emitter
/// (composition root + boundary casts).
pub(crate) fn build_cross_context_info(
    name: &str,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> resolver::CrossContextInfo {
    let consumed_contexts: Vec<String> = unit_consumes.get(name).cloned().unwrap_or_default();
    let aliases: HashMap<String, String> =
        unit_consumes_aliases.get(name).cloned().unwrap_or_default();
    let mut consumed_services: HashMap<String, HashMap<String, resolver::CrossContextService>> =
        HashMap::new();
    let mut consumed_types: HashMap<String, HashMap<String, TypeDecl>> = HashMap::new();
    let mut consumed_capabilities: HashMap<
        String,
        HashMap<String, resolver::CrossContextCapability>,
    > = HashMap::new();
    for t in &consumed_contexts {
        let other_types_combined = combined_types_for(t, unit_tables, unit_uses);
        consumed_types.insert(t.clone(), other_types_combined.clone());
        let Some(other_table) = unit_tables.get(t) else {
            continue;
        };
        let mut svcs: HashMap<String, resolver::CrossContextService> = HashMap::new();
        for (sname, sdecl) in &other_table.services {
            let Some(handler) = sdecl
                .handlers
                .iter()
                .find(|h| matches!(h.kind, HandlerKind::Call))
            else {
                continue;
            };
            let params: Vec<(String, TypeRef)> = handler
                .params
                .iter()
                .map(|p| (p.name.name.clone(), p.type_ref.clone()))
                .collect();
            svcs.insert(
                sname.clone(),
                resolver::CrossContextService {
                    name: sname.clone(),
                    params,
                    return_type: handler.return_type.clone(),
                    span: sdecl.span,
                },
            );
        }
        consumed_services.insert(t.clone(), svcs);

        // v0.15: gather the consumed context's exported capabilities, each
        // paired with the provider that implements it.
        let mut caps: HashMap<String, resolver::CrossContextCapability> = HashMap::new();
        for cap_name in &other_table.exported_capabilities {
            let Some(decl) = other_table.capabilities.get(cap_name) else {
                continue;
            };
            let Some(provider) = other_table.providers.get(cap_name) else {
                continue;
            };
            let ops = decl
                .ops
                .iter()
                .map(|op| resolver::CrossContextCapabilityOp {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| (p.name.name.clone(), p.type_ref.clone()))
                        .collect(),
                    return_type: op.return_type.clone(),
                })
                .collect();
            caps.insert(
                cap_name.clone(),
                resolver::CrossContextCapability {
                    name: cap_name.clone(),
                    ops,
                    provider_name: provider.provider_name.name.clone(),
                    provider_given: provider
                        .given
                        .iter()
                        .filter(|c| !c.is_cross_context())
                        .map(|c| c.key().to_string())
                        .collect(),
                    span: decl.span,
                },
            );
        }
        consumed_capabilities.insert(t.clone(), caps);
    }
    resolver::CrossContextInfo {
        self_context: Some(name.to_string()),
        consumed_contexts,
        aliases,
        consumed_services,
        consumed_types,
        consumed_capabilities,
        // Set by the caller from the unit's `consumes U { … }` clauses.
        flattened_caps: HashMap::new(),
    }
}

/// v0.15: validate one `given` capability reference. A bare reference must name
/// a capability declared in this context; a cross-context reference (`given
/// B.Cap`) must name a capability the consumed context exports. Returns the
/// local [`CapabilityInfo`] to add to the in-scope map for bare references;
/// cross-context references return `None` (their calls are type-checked via
/// `consumed_capabilities` at the call site) but are still validated here.
/// v0.25: record a clause-position capability reference (`provides Cap`,
/// bare `given Cap`), qualifying a flattened bare name to its providing
/// unit. The span is the name segment only.
pub(crate) fn record_capability_clause_ref(
    name: &Ident,
    cross_context: &resolver::CrossContextInfo,
    refs: &mut RefSink,
) {
    record_capability_clause_ref_inner(name, cross_context, refs, false);
}

/// v0.35 (ADR 0068): the `Cap` of a `provides Cap = Provider` clause — a
/// capability reference *and* an implementation edge (the ambient owner is the
/// provider). Flagged so assembly can tell it apart from the provider's own
/// `given` deps, which are capability refs owned by the same provider.
pub(crate) fn record_provides_clause_ref(
    name: &Ident,
    cross_context: &resolver::CrossContextInfo,
    refs: &mut RefSink,
) {
    record_capability_clause_ref_inner(name, cross_context, refs, true);
}

fn record_capability_clause_ref_inner(
    name: &Ident,
    cross_context: &resolver::CrossContextInfo,
    refs: &mut RefSink,
    provides: bool,
) {
    let unit = cross_context.flattened_caps.get(&name.name);
    if provides {
        refs.record_provides(name.span, &name.name, unit.map(String::as_str));
    } else if let Some(unit) = unit {
        refs.record_in_unit(name.span, SymbolKind::Capability, &name.name, unit);
    } else {
        refs.record(name.span, SymbolKind::Capability, &name.name);
    }
}

pub(crate) fn resolve_given_cap_ref(
    cap_ref: &CapRef,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    cross_context: &resolver::CrossContextInfo,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) -> Option<CapabilityInfo> {
    let Some(prefix) = cap_ref.prefix() else {
        // Local capability.
        match capability_info_map.get(cap_ref.key()) {
            Some(info) => {
                record_capability_clause_ref(&cap_ref.name, cross_context, refs);
                return Some(info.clone());
            }
            None => {
                errors.push(CompileError::new(
                    "karn.given.unknown_capability",
                    cap_ref.span,
                    format!(
                        "capability `{}` is not declared in this context",
                        cap_ref.key()
                    ),
                ));
                return None;
            }
        }
    };
    // Cross-context capability (`given B.Cap` / `given Alias.Cap`).
    let Some(ctx_name) = cross_context.resolve_prefix(&prefix) else {
        errors.push(
            CompileError::new(
                "karn.resolve.unconsumed_context",
                cap_ref.span,
                format!(
                    "`given {}.{}` refers to a context that this context does not `consumes`",
                    prefix,
                    cap_ref.key()
                ),
            )
            .with_note(
                "add a `consumes` clause for the providing context (optionally with an alias) at the top of this context",
            ),
        );
        return None;
    };
    let exports_it = cross_context
        .consumed_capabilities
        .get(&ctx_name)
        .is_some_and(|m| m.contains_key(cap_ref.key()));
    if exports_it {
        // v0.25: dotted `given B.Cap` — the name segment, in the consumed
        // unit's namespace.
        refs.record_in_unit(
            cap_ref.name.span,
            SymbolKind::Capability,
            cap_ref.key(),
            &ctx_name,
        );
    }
    if !exports_it {
        errors.push(
            CompileError::new(
                "karn.given.cross_context_unknown_capability",
                cap_ref.span,
                format!(
                    "context `{}` does not export a capability named `{}`",
                    ctx_name,
                    cap_ref.key()
                ),
            )
            .with_note(
                "the providing context must list the capability in an `exports capability { … }` clause",
            ),
        );
    }
    None
}

/// Build the combined type table for `unit`: its own types merged with the
/// types of every commons it `uses`. Used by cross-context resolution so we
/// can resolve a consumed context's service signatures against that context's
/// own view of types (v0.6 §4.5).
fn combined_types_for(
    unit: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> HashMap<String, TypeDecl> {
    let mut out: HashMap<String, TypeDecl> = HashMap::new();
    if let Some(table) = unit_tables.get(unit) {
        for (n, d) in &table.types {
            out.insert(n.clone(), d.clone());
        }
    }
    if let Some(targets) = unit_uses.get(unit) {
        for t in targets {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    out.entry(n.clone()).or_insert_with(|| d.clone());
                }
            }
        }
    }
    out
}

pub(crate) fn consumes_span_of(
    parsed: &[ParsedFile],
    indices: &[usize],
    target: &str,
) -> Option<Span> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if c.target.joined() == target {
                return Some(c.span);
            }
        }
    }
    None
}

pub(crate) fn parsed_alias_span(
    parsed: &[ParsedFile],
    indices: &[usize],
    alias: &str,
) -> Option<Span> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if let Some(a) = &c.alias
                && a.name == alias
            {
                return Some(a.span);
            }
        }
    }
    None
}

/// A type imported into a context via `consumes`. Carries enough metadata for
/// the checker and emitter to enforce / express visibility.
#[derive(Debug, Clone)]
pub struct ConsumedType {
    pub owning_context: String,
    pub visibility: Visibility,
}
