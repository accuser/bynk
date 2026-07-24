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
                    CommonsItem::Actor(a) => (
                        SymbolKind::Actor,
                        &a.name.name,
                        symbol_modifiers(&unit, None),
                    ),
                    CommonsItem::Messages(m) => {
                        (SymbolKind::Messages, &m.tag, symbol_modifiers(&unit, None))
                    }
                };
                builder.add_first_party_def(&unit, kind, name, modifiers);
            }
            continue;
        }
        let site = |id: &Ident| SiteRef {
            path: pf.identity_path.clone(),
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
                    // v0.129 (#259): record a refined/opaque type's builtin base
                    // for the refinement-family codelens. A plain alias
                    // (`type Age = Int`) counts — it parses as `Refined { …, base }`
                    // with no `where`, still declared over the base.
                    if let TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. } = &t.body
                    {
                        builder.add_refinement(&unit, &t.name.name, *base);
                    }
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
                        builder.add_owner(&unit, &f.name.display(), &pf.identity_path);
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
                    // #304: an agent handler is a first-class symbol keyed by
                    // the compound `"Agent.handler"` name, mirroring the
                    // v0.36 (ADR 0069) method/field/op convention. Service
                    // handlers have no per-handler name (`method_name` is
                    // always `None`), so this is naturally agent-only.
                    for h in &a.handlers {
                        if let Some(name) = &h.method_name {
                            builder.add_def(
                                &unit,
                                SymbolKind::Handler,
                                &format!("{}.{}", a.name.name, name.name),
                                site(name),
                                symbol_modifiers(&unit, None),
                            );
                        }
                    }
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
                CommonsItem::Actor(a) => {
                    builder.add_def(
                        &unit,
                        SymbolKind::Actor,
                        &a.name.name,
                        site(&a.name),
                        symbol_modifiers(&unit, None),
                    );
                }
                CommonsItem::Messages(m) => {
                    // The tag is a string literal, not an `Ident`, so build the
                    // `SiteRef` from its span directly rather than via `site`.
                    builder.add_def(
                        &unit,
                        SymbolKind::Messages,
                        &m.tag,
                        SiteRef {
                            path: pf.identity_path.clone(),
                            span: m.tag_span,
                        },
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
fn symbol_modifiers(
    unit: &str,
    type_decl: Option<&TypeDecl>,
) -> bynk_check::index::SymbolModifiers {
    let (refined, opaque) = match type_decl.map(|t| &t.body) {
        Some(TypeBody::Refined { refinement, .. }) => (refinement.is_some(), false),
        Some(TypeBody::Opaque { refinement, .. }) => (refinement.is_some(), true),
        _ => (false, false),
    };
    bynk_check::index::SymbolModifiers {
        refined,
        opaque,
        platform_native: bynk_check::firstparty::platform_of(unit).is_some(),
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
    /// v0.45: actors — boundary contracts consumed by handler `by` clauses.
    pub actors: HashMap<String, ActorDecl>,
    /// v0.15: capability names this context offers to consumers via
    /// `exports capability { … }`. Empty for commons.
    pub exported_capabilities: std::collections::HashSet<String>,
}

/// #696: each table-construction diagnostic is attributed to the project-relative
/// `identity_path` of the file whose item produced it. Every error-producing loop
/// below iterates `for &i in indices`, so it shadows a local `errors` vec and
/// drains it into `out`, tagged with `parsed[i].identity_path`, at the end of each
/// file's pass — leaving the many inner `errors.push(…)` sites untouched.
pub(crate) fn build_unit_table(
    _name: &str,
    kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    out: &mut Vec<(PathBuf, CompileError)>,
) -> UnitTable {
    let mut table = UnitTable {
        kind: Some(kind),
        ..UnitTable::default()
    };
    for &i in indices {
        let mut errors: Vec<CompileError> = Vec::new();
        for item in parsed[i].items() {
            if let CommonsItem::Type(t) = item {
                if let Some(prev) = table.types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.resolve.duplicate_type",
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
        out.extend(
            errors
                .into_iter()
                .map(|e| (parsed[i].identity_path.clone(), e)),
        );
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
        let mut errors: Vec<CompileError> = Vec::new();
        for item in parsed[i].items() {
            match item {
                CommonsItem::Capability(c) => {
                    if kind != UnitKind::Context && kind != UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "bynk.capability.outside_context",
                            c.span,
                            "`capability` declarations are only allowed inside a context or adapter",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.capabilities.get(&c.name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_capability",
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
                                    "bynk.context.external_provider",
                                    p.span,
                                    "an external (bodiless) provider is only allowed inside an `adapter` — a context provider must have a Bynk body",
                                ));
                                continue;
                            }
                        }
                        UnitKind::Adapter => {
                            // v0.17: an adapter provider must be external — its
                            // implementation comes from the binding.
                            if !p.external {
                                errors.push(CompileError::new(
                                    "bynk.adapter.provider_has_body",
                                    p.span,
                                    "a provider inside an `adapter` must be external (no body) — its implementation is supplied by the binding",
                                ));
                                continue;
                            }
                        }
                        _ => {
                            errors.push(CompileError::new(
                                "bynk.provider.outside_context",
                                p.span,
                                "`provides` declarations are only allowed inside a context or adapter",
                            ));
                            continue;
                        }
                    }
                    if let Some(prev) = table.providers.get(&p.capability.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_provider",
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
                            "bynk.adapter.disallowed_item",
                            s.span,
                            "an `adapter` may not declare a `service` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "bynk.service.outside_context",
                            s.span,
                            "`service` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.services.get(&s.name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_service",
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
                            "bynk.adapter.disallowed_item",
                            a.span,
                            "an `adapter` may not declare an `agent` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "bynk.agent.outside_context",
                            a.span,
                            "`agent` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.agents.get(&a.name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_agent",
                                a.name.span,
                                format!("agent `{}` is already declared", a.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.agents.insert(a.name.name.clone(), a.clone());
                    }
                }
                CommonsItem::Actor(a) => {
                    if kind == UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "bynk.adapter.disallowed_item",
                            a.span,
                            "an `adapter` may not declare an `actor` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.actors.get(&a.name.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_actor",
                                a.name.span,
                                format!("actor `{}` is already declared", a.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.actors.insert(a.name.name.clone(), a.clone());
                    }
                }
                _ => {}
            }
        }
        out.extend(
            errors
                .into_iter()
                .map(|e| (parsed[i].identity_path.clone(), e)),
        );
    }
    for &i in indices {
        let mut errors: Vec<CompileError> = Vec::new();
        for item in parsed[i].items() {
            let CommonsItem::Fn(f) = item else { continue };
            match &f.name {
                FnName::Free(id) => {
                    if let Some(prev) = table.fns.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.duplicate_fn",
                                id.span,
                                format!("function `{}` is already declared", id.name),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else if let Some(prev) = table.types.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.resolve.name_conflict",
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
                                "bynk.resolve.method_unknown_type",
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
                                "bynk.resolve.duplicate_method",
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
        out.extend(
            errors
                .into_iter()
                .map(|e| (parsed[i].identity_path.clone(), e)),
        );
    }
    // message-bundles slice 1 (#859): a commons declaring at least one
    // `messages` block also gets a synthetic `render(tag: LocaleTag, msg:
    // Message) -> String` in its own local function table — not just emitted
    // TS. Without this, a Bynk-source `render(...)` call has no local
    // declaration to resolve to and silently falls through to `bynk.locale`'s
    // *imported* `render` (same signature, wrong — bundle-free — behaviour):
    // resolution would "type-check" while quietly calling the wrong function.
    // Registering it here, in the same local `table.fns` a real `CommonsItem::Fn`
    // would populate, makes ordinary lexical precedence (local beats
    // `uses`-imported, `compose_unit_symbols`) and call-site type-checking
    // (`fns.get(name)`, never touching `.body`) work with no changes anywhere
    // else. The body is a placeholder — nothing ever type-checks it, since
    // body-checking walks `commons.items` (real AST items) directly, and this
    // entry is never added there.
    if kind == UnitKind::Commons
        && let Some(m) = indices.iter().find_map(|&i| {
            parsed[i].items().iter().find_map(|item| match item {
                CommonsItem::Messages(m) => Some(m),
                _ => None,
            })
        })
    {
        if let Some(prev) = table.fns.get("render") {
            out.push((
                parsed[indices[0]].identity_path.clone(),
                CompileError::new(
                    "bynk.resolve.duplicate_fn",
                    m.span,
                    "function `render` is already declared",
                )
                .with_label(prev.name.ident().span, "previously declared here")
                .with_note(
                    "a `messages` block in this commons implicitly declares its own \
                     `render(tag, msg) -> String` — name it something else",
                ),
            ));
        } else {
            table
                .fns
                .insert("render".to_string(), synthetic_render_fn());
        }
    }
    table
}

/// The synthetic `FnDecl` [`build_unit_table`] registers for a messages-bearing
/// commons. Its body is never checked (see the call site's comment) — it
/// exists only so `Param`/`TypeRef`/`FnDecl` construction has somewhere to put
/// a syntactically valid placeholder.
fn synthetic_render_fn() -> FnDecl {
    let span = Span::default();
    FnDecl {
        type_params: Vec::new(),
        name: FnName::Free(Ident {
            name: "render".to_string(),
            span,
        }),
        params: vec![
            Param {
                name: Ident {
                    name: "tag".to_string(),
                    span,
                },
                type_ref: TypeRef::Named(Ident {
                    name: "LocaleTag".to_string(),
                    span,
                }),
                span,
            },
            Param {
                name: Ident {
                    name: "msg".to_string(),
                    span,
                },
                type_ref: TypeRef::Named(Ident {
                    name: "Message".to_string(),
                    span,
                }),
                span,
            },
        ],
        return_type: TypeRef::Base(BaseType::String, span),
        requires: Vec::new(),
        ensures: Vec::new(),
        body: Block {
            statements: Vec::new(),
            tail: Box::new(Expr {
                kind: ExprKind::StrLit(String::new()),
                span,
            }),
            span,
            tail_leading_comments: Vec::new(),
            implicit_tail: false,
        },
        has_self: false,
        documentation: None,
        span,
        trivia: Trivia::default(),
    }
}

/// For each name declared in the unit (type, fn, method), record which
/// source file declared it. Used by the emitter to render relative imports.
#[derive(Clone)]
pub struct FileDeclIndex {
    pub types: HashMap<String, PathBuf>,
    pub fns: HashMap<String, PathBuf>,
    pub methods: HashMap<String, HashMap<String, PathBuf>>,
}

/// **Tree-relative, deliberately.** This is an *emit* structure, not an index:
/// `record_name_ref` compares these paths against `ctx.source_path`
/// (`emitter.rs`), which is the file's `include`-root-relative path. Keying it
/// by `identity_path` (ADR 0198) makes `path != &ctx.source_path` always true
/// for a split project, so a name declared in the *same* file is emitted as a
/// sibling import of itself — the module then cannot load, and a workers
/// runtime test hangs rather than fails. See ADR 0201 (E).
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
                | CommonsItem::Agent(_)
                | CommonsItem::Actor(_)
                // `messages` bundles aren't cross-file-imported by name in
                // slice 1 (no multi-file bundle merge yet).
                | CommonsItem::Messages(_) => {}
            }
        }
    }
    idx
}

/// #696: returns the `parsed` index of the owning file alongside the `uses`
/// clause span, so the caller can attribute the diagnostic to that file.
pub(crate) fn uses_span_of(
    parsed: &[ParsedFile],
    indices: &[usize],
    target: &str,
) -> Option<(usize, Span)> {
    for &i in indices {
        for u in parsed[i].uses() {
            if u.target.joined() == target {
                return Some((i, u.span));
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
                    "bynk.given.unknown_capability",
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
                "bynk.resolve.unconsumed_context",
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
                "bynk.given.cross_context_unknown_capability",
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
/// v0.177 (#643): the callee's own type namespace — its local declarations plus
/// the commons types it `uses`.
///
/// Shared deliberately. The **caller** reaches this table through
/// `consumed_types[callee]` and the **callee** builds it for itself; both must
/// canonicalise the callee's contract from the *same* table or their hashes
/// diverge and every call 409s. Routing both through one function makes that
/// agreement structural rather than a thing to keep in step by hand.
pub(crate) fn combined_types_for(
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

/// Locale capability track, slice 2 (#882): the message bundle a context's
/// `Locale.current()` negotiates against, auto-detected from the context's
/// *direct* `uses` (one level, not transitive — see [`combined_types_for`]
/// just above, the precedent for this rule). `None`/`One`/`Many` drive three
/// different behaviours: unchanged fixed-default `Locale`, real negotiation
/// wiring, or (when the context also consumes `Locale`)
/// `bynk.messages.multiple_message_bundles` — see `check_locale_bundle_ambiguity`
/// (`bynk-emit/src/project/validate.rs`) and the per-Worker composition loop
/// (`bynk-emit/src/project.rs`).
// `pub`, not `pub(crate)`: `MessageBundleInfo` appears in `emit_worker_compose`'s
// public signature (`bynk-emit/src/emitter/workers.rs`), which must expose
// types at least as visible as itself (matching `UnitTable`'s own `pub`).
pub enum ContextMessageBundle {
    /// No directly-`uses`d commons declares a `messages` block.
    None,
    /// Exactly one — the negotiable case.
    One(MessageBundleInfo),
    /// Two or more (each commons's own qualified name, for the diagnostic).
    Many(Vec<String>),
}

pub struct MessageBundleInfo {
    /// The commons's qualified unit name (e.g. `"app.msgs"`).
    pub commons: String,
    /// Project-relative path of the file carrying the `@reference` block —
    /// the import target for `messagesLocales`/`messagesReferenceLocale`.
    /// (A bundle genuinely split across multiple files, per the track doc's
    /// own §4.1, is not correctly merged by `emit_messages_bundle` today —
    /// each file emits independently, `bynk-emit/src/project.rs`'s per-file
    /// `emit_items` loop — this detection mirrors that same file-scoped
    /// reality rather than a wider, currently-unimplemented merge.)
    pub source_path: PathBuf,
}

/// Walks `ctx`'s own direct `uses` list for commons declaring a `messages`
/// bundle with exactly one `@reference` block (a bundle missing or
/// duplicating its own reference is already diagnosed by
/// `check_messages_bundles` — this function simply doesn't count it as
/// "found", rather than compounding an already-reported error).
pub(crate) fn detect_context_message_bundle(
    ctx: &str,
    unit_uses: &HashMap<String, Vec<String>>,
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
) -> ContextMessageBundle {
    let mut found: Vec<MessageBundleInfo> = Vec::new();
    for target in unit_uses.get(ctx).into_iter().flatten() {
        if kinds.get(target) != Some(&UnitKind::Commons) {
            continue;
        }
        let Some(indices) = groups.get(target) else {
            continue;
        };
        for &i in indices {
            let has_reference = parsed[i].items().iter().any(|item| {
                matches!(item, CommonsItem::Messages(m) if m.annotations.iter().any(|a| a.name.name == "reference"))
            });
            if has_reference {
                found.push(MessageBundleInfo {
                    commons: target.clone(),
                    source_path: parsed[i].source_path.clone(),
                });
                break;
            }
        }
    }
    match found.len() {
        0 => ContextMessageBundle::None,
        1 => ContextMessageBundle::One(found.pop().expect("len == 1")),
        _ => ContextMessageBundle::Many(found.into_iter().map(|b| b.commons).collect()),
    }
}

#[cfg(test)]
mod detect_context_message_bundle_tests {
    use super::*;

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: Span::default(),
        }
    }

    fn qualified(name: &str) -> QualifiedName {
        QualifiedName {
            parts: name.split('.').map(ident).collect(),
            span: Span::default(),
        }
    }

    /// A commons `ParsedFile` declaring one `messages <tag>` block, its
    /// `@reference` annotation present or not. `source_path` is derived from
    /// `name` so each test bundle gets a distinct, recognisable import
    /// target — real content doesn't matter, only that a path exists.
    fn commons_with_messages(name: &str, tag: &str, is_reference: bool) -> ParsedFile {
        let annotations = if is_reference {
            vec![Annotation {
                name: ident("reference"),
                args: Vec::new(),
                span: Span::default(),
            }]
        } else {
            Vec::new()
        };
        let messages = MessagesDecl {
            tag: tag.to_string(),
            tag_span: Span::default(),
            annotations,
            entries: Vec::new(),
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        };
        ParsedFile {
            source_path: PathBuf::from(format!("{}.bynk", name.replace('.', "/"))),
            identity_path: PathBuf::from(format!("src/{}.bynk", name.replace('.', "/"))),
            abs_path: None,
            source: String::new(),
            unit: SourceUnit::Commons(Commons {
                name: qualified(name),
                items: vec![CommonsItem::Messages(messages)],
                uses: Vec::new(),
                documentation: None,
                form: CommonsForm::Brace,
                span: Span::default(),
                trivia: Trivia::default(),
                trailing_comments: Vec::new(),
            }),
            kind: UnitKind::Commons,
            synthetic: false,
        }
    }

    /// A minimal context `ParsedFile` with no items of its own — only its
    /// `uses` list matters for this function.
    fn context_using(name: &str, targets: &[&str]) -> ParsedFile {
        ParsedFile {
            source_path: PathBuf::from(format!("{}.bynk", name.replace('.', "/"))),
            identity_path: PathBuf::from(format!("src/{}.bynk", name.replace('.', "/"))),
            abs_path: None,
            source: String::new(),
            unit: SourceUnit::Context(Context {
                name: qualified(name),
                uses: targets
                    .iter()
                    .map(|t| UsesDecl {
                        target: qualified(t),
                        span: Span::default(),
                        trivia: Trivia::default(),
                    })
                    .collect(),
                consumes: Vec::new(),
                exports: Vec::new(),
                items: Vec::new(),
                documentation: None,
                form: CommonsForm::Brace,
                span: Span::default(),
                trivia: Trivia::default(),
                trailing_comments: Vec::new(),
            }),
            kind: UnitKind::Context,
            synthetic: false,
        }
    }

    /// The four tables `detect_context_message_bundle`'s real callers
    /// already build — bundled here so [`scenario`] doesn't need a
    /// clippy-unfriendly four-tuple return type.
    struct Scenario {
        parsed: Vec<ParsedFile>,
        groups: HashMap<String, Vec<usize>>,
        kinds: HashMap<String, UnitKind>,
        unit_uses: HashMap<String, Vec<String>>,
    }

    /// Assembles a [`Scenario`] from a context plus its bundle files.
    fn scenario(ctx_name: &str, ctx_uses: &[&str], bundles: Vec<(&str, ParsedFile)>) -> Scenario {
        let mut parsed = vec![context_using(ctx_name, ctx_uses)];
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        let mut kinds: HashMap<String, UnitKind> = HashMap::new();
        groups.insert(ctx_name.to_string(), vec![0]);
        kinds.insert(ctx_name.to_string(), UnitKind::Context);
        for (name, pf) in bundles {
            let idx = parsed.len();
            parsed.push(pf);
            groups.entry(name.to_string()).or_default().push(idx);
            kinds.insert(name.to_string(), UnitKind::Commons);
        }
        let mut unit_uses: HashMap<String, Vec<String>> = HashMap::new();
        unit_uses.insert(
            ctx_name.to_string(),
            ctx_uses.iter().map(|s| s.to_string()).collect(),
        );
        Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        }
    }

    #[test]
    fn zero_bundles_when_uses_reaches_no_messages_commons() {
        let Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        } = scenario("app.web", &["app.other"], vec![]);
        assert!(matches!(
            detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed),
            ContextMessageBundle::None
        ));
    }

    #[test]
    fn zero_bundles_when_uses_is_empty() {
        let Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        } = scenario("app.web", &[], vec![]);
        assert!(matches!(
            detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed),
            ContextMessageBundle::None
        ));
    }

    #[test]
    fn one_bundle_is_found_by_its_reference_block() {
        let bundle = commons_with_messages("app.msgs", "en", true);
        let Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        } = scenario("app.web", &["app.msgs"], vec![("app.msgs", bundle)]);
        let found = detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed);
        let ContextMessageBundle::One(info) = found else {
            panic!("expected exactly one bundle");
        };
        assert_eq!(info.commons, "app.msgs");
        assert_eq!(info.source_path, PathBuf::from("app/msgs.bynk"));
    }

    #[test]
    fn a_bundle_missing_its_reference_block_is_not_counted() {
        // Not a `@reference` block — already diagnosed elsewhere
        // (`bynk.messages.missing_reference`); this function simply doesn't
        // count it, rather than compounding an already-reported error.
        let bundle = commons_with_messages("app.msgs", "en", false);
        let Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        } = scenario("app.web", &["app.msgs"], vec![("app.msgs", bundle)]);
        assert!(matches!(
            detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed),
            ContextMessageBundle::None
        ));
    }

    #[test]
    fn two_bundles_report_both_commons_names() {
        let a = commons_with_messages("app.msgs_a", "en", true);
        let b = commons_with_messages("app.msgs_b", "en", true);
        let Scenario {
            parsed,
            groups,
            kinds,
            unit_uses,
        } = scenario(
            "app.web",
            &["app.msgs_a", "app.msgs_b"],
            vec![("app.msgs_a", a), ("app.msgs_b", b)],
        );
        let found = detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed);
        let ContextMessageBundle::Many(names) = found else {
            panic!("expected two bundles");
        };
        let mut names = names;
        names.sort();
        assert_eq!(
            names,
            vec!["app.msgs_a".to_string(), "app.msgs_b".to_string()]
        );
    }

    #[test]
    fn a_commons_reached_only_transitively_is_not_counted() {
        // `app.web` uses `app.mid`, which itself uses the bundle — `uses` is
        // one level, not transitive (message-bundles' own established rule),
        // so this must still report `None`.
        let bundle = commons_with_messages("app.msgs", "en", true);
        let mut mid = context_using("app.mid", &["app.msgs"]);
        // `app.mid` needs to be a commons for this scenario to be legal, but
        // `context_using` builds a Context — for this narrow test only the
        // `uses` *resolution* (does app.web's own direct list reach the
        // bundle) matters, and app.web's own list never names `app.msgs`
        // directly, so the unit kind of the intermediate is irrelevant.
        mid.kind = UnitKind::Commons;
        let Scenario {
            mut parsed,
            mut groups,
            mut kinds,
            unit_uses,
        } = scenario("app.web", &["app.mid"], vec![("app.msgs", bundle)]);
        let mid_idx = parsed.len();
        parsed.push(mid);
        groups.insert("app.mid".to_string(), vec![mid_idx]);
        kinds.insert("app.mid".to_string(), UnitKind::Commons);
        assert!(matches!(
            detect_context_message_bundle("app.web", &unit_uses, &groups, &kinds, &parsed),
            ContextMessageBundle::None
        ));
    }
}

/// #696: returns the `parsed` index of the owning file alongside the `consumes`
/// clause span, so the caller can attribute the diagnostic to that file.
pub(crate) fn consumes_span_of(
    parsed: &[ParsedFile],
    indices: &[usize],
    target: &str,
) -> Option<(usize, Span)> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if c.target.joined() == target {
                return Some((i, c.span));
            }
        }
    }
    None
}

/// #696: returns the `parsed` index of the owning file alongside the alias span,
/// so the caller can attribute the diagnostic to that file.
pub(crate) fn parsed_alias_span(
    parsed: &[ParsedFile],
    indices: &[usize],
    alias: &str,
) -> Option<(usize, Span)> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if let Some(a) = &c.alias
                && a.name == alias
            {
                return Some((i, a.span));
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
