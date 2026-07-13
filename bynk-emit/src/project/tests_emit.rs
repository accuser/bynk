use super::*;
use crate::emitter::source_map::SourceMapBuilder;

/// Render a type-ref in the same form the user wrote it, for diagnostics.
pub(crate) fn ts_type_ref_display(r: &TypeRef) -> String {
    match r {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(t, e, _) => format!(
            "Result[{}, {}]",
            ts_type_ref_display(t),
            ts_type_ref_display(e)
        ),
        TypeRef::Option(t, _) => format!("Option[{}]", ts_type_ref_display(t)),
        TypeRef::Effect(t, _) => format!("Effect[{}]", ts_type_ref_display(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult[{}]", ts_type_ref_display(t)),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::List(t, _) => format!("List[{}]", ts_type_ref_display(t)),
        TypeRef::Query(t, _) => format!("Query[{}]", ts_type_ref_display(t)),
        TypeRef::Stream(t, _) => format!("Stream[{}]", ts_type_ref_display(t)),
        TypeRef::Connection(t, _) => format!("Connection[{}]", ts_type_ref_display(t)),
        TypeRef::History(t, _) => format!("History[{}]", ts_type_ref_display(t)),
        TypeRef::Map(k, v, _) => format!(
            "Map[{}, {}]",
            ts_type_ref_display(k),
            ts_type_ref_display(v)
        ),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
        // v0.157 (ADR 0183): render a generic-type application as written.
        TypeRef::App { name, args, .. } => format!(
            "{}[{}]",
            name.name,
            args.iter()
                .map(ts_type_ref_display)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeRef::Fn(params, ret, _) => {
            let lhs = match params.len() {
                0 => "()".to_string(),
                1 if !matches!(params[0], TypeRef::Fn(..)) => ts_type_ref_display(&params[0]),
                _ => format!(
                    "({})",
                    params
                        .iter()
                        .map(ts_type_ref_display)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            format!("{lhs} -> {}", ts_type_ref_display(ret))
        }
    }
}

// -- v0.7 / v0.118: test declaration processing --

/// v0.118: a capability seam with one or more `stub` overrides applied
/// (testing track slice 6). Groups every `stub Cap.method(…)` clause — both
/// suite-scoped and case-scoped — targeting the same capability `cap`. The
/// resolved [`CapabilityDecl`] supplies each overridden method's parameter names
/// and return type for stub emission.
#[derive(Debug, Clone)]
struct ResolvedStub {
    /// The capability being overridden (a declared/consumed seam of the target).
    cap: String,
    /// The capability declaration, for op parameter names and return types.
    cap_decl: CapabilityDecl,
    /// The `stub` clauses for this capability, in match order (case-scoped
    /// first so they take precedence over suite-scoped in the emitted if-chain).
    clauses: Vec<StubClause>,
    /// The test file declaring the first clause — the recording context for
    /// edges in its value expressions (v0.25).
    source_path: PathBuf,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn process_tests(
    test_groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    // v0.132: production unit name -> its `parsed` file indices, so a barrel can
    // resolve a multi-file commons the test module imports back to its files.
    groups: &HashMap<String, Vec<usize>>,
    tests_prefix: &Path,
    import_ext: ImportExt,
    // v0.115: whether the build emits the contract guard (dev/test). The runner
    // attack relies on the guard to assert `ensures`, so it is emitted only when
    // the guard is (they are always paired — `bynkc test` sets both).
    contracts: bool,
    // v0.132: running set of already-emitted barrel output paths, shared with the
    // integration pass so a commons imported by both is barrelled exactly once.
    emitted_barrels: &mut HashSet<PathBuf>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) -> (Vec<CompiledFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<CompiledFile> = Vec::new();
    let mut runnable_tests: Vec<RunnableTest> = Vec::new();

    let mut sorted_targets: Vec<&String> = test_groups.keys().collect();
    sorted_targets.sort();

    for target_name in sorted_targets {
        let indices = test_groups.get(target_name).unwrap();
        // -- Phase 2: target resolution --
        let target_kind = match kinds.get(target_name) {
            Some(k) => *k,
            None => {
                let span = first_test_target_span(indices, parsed);
                errors.push(
                    CompileError::new(
                        "bynk.suite.unknown_target",
                        span,
                        format!(
                            "test target `{target_name}` is not a declared commons or context in this project",
                        ),
                    )
                    .with_note(
                        "the target of a `test` declaration must be a commons or context declared elsewhere in the project",
                    ),
                );
                continue;
            }
        };

        // -- Phase 2: duplicate test case names --
        let mut seen_cases: HashMap<String, Span> = HashMap::new();
        let mut had_dup = false;
        for &i in indices {
            if let Some(t) = parsed[i].test() {
                for case in &t.cases {
                    if let Some(prev) = seen_cases.get(&case.name) {
                        had_dup = true;
                        errors.push(
                            CompileError::new(
                                "bynk.suite.duplicate_case_name",
                                case.name_span,
                                format!(
                                    "test case `\"{}\"` is declared more than once in tests targeting `{target_name}`",
                                    case.name
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        seen_cases.insert(case.name.clone(), case.name_span);
                    }
                }
            }
        }

        // -- Phase 3: resolve `stub` clauses (v0.118, testing track slice 6).
        // Both suite-scoped and case-scoped `stub` fold into one per-seam
        // override map. Case-scoped clauses are collected first so they take
        // precedence over suite-scoped ones in the emitted first-match if-chain
        // (the case > suite > default order; a first-cut global merge — a
        // case-scoped clause is not yet re-scoped to its own case).
        let target_stubs = resolve_stubs(
            target_name,
            target_kind,
            indices,
            parsed,
            unit_tables,
            unit_consumes,
            errors,
        );

        if had_dup {
            // Skip body/type-checking for this target; we have name conflicts.
            continue;
        }

        // -- Phase 4: type-check bodies. --
        // (We build a resolved view targeting either commons or context;
        // mock bodies are type-checked with the mocked entity's privileges.)
        let bodies_errs = check_test_bodies(
            target_name,
            target_kind,
            indices,
            parsed,
            &target_stubs,
            unit_tables,
            exports_visibility,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
            refs,
        );
        let bodies_failed = !bodies_errs.is_empty();
        errors.extend(bodies_errs);

        if bodies_failed {
            continue;
        }

        // -- Phase 5: emit TypeScript test module. --
        let emit_out = emit_test_module(
            target_name,
            target_kind,
            indices,
            parsed,
            &target_stubs,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
            exports_visibility,
            tests_prefix,
            import_ext,
            contracts,
        );
        if let Some((path, source, source_map, runnable)) = emit_out {
            outputs.push(CompiledFile {
                source_path: path.clone(),
                output_path: path,
                typescript: source,
                source_map,
                debug_metadata: None,
            });
            runnable_tests.push(runnable);

            // v0.132: the module imports the target and each of its `consumes` /
            // `uses` targets as a namespace (`import * as ns from "./<name>.js"`).
            // Any of those that is a multi-file commons has no `out/<name>.ts`, so
            // emit an aggregating barrel (deduped) to make the import resolve.
            let mut imported: Vec<&str> = vec![target_name.as_str()];
            if let Some(consumed) = unit_consumes.get(target_name) {
                imported.extend(consumed.iter().map(String::as_str));
            }
            if let Some(used) = unit_uses.get(target_name) {
                imported.extend(used.iter().map(String::as_str));
            }
            for name in imported {
                if let Some(barrel) =
                    emit_commons_barrel(name, groups, parsed, import_ext, emitted_barrels)
                {
                    outputs.push(barrel);
                }
            }
        }
    }

    // v0.16: the top-level `tests/main.ts` runner is emitted once by the caller
    // after both unit- and integration-test passes, so it can aggregate both.
    (outputs, runnable_tests)
}

/// v0.118: resolve every `stub Cap.method(…)` clause targeting a unit into a
/// per-capability [`ResolvedStub`] (testing track slice 6, ADR 0154). Both
/// suite-scoped and case-scoped clauses fold in; a capability that is neither a
/// declared seam of the target nor reachable through a consumed context is
/// `bynk.stub.not_a_seam`, an unknown method is `bynk.stub.unknown_op`,
/// and an empty `returns each []` is `bynk.stub.bad_sequence`.
fn resolve_stubs(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    errors: &mut Vec<CompileError>,
) -> HashMap<String, ResolvedStub> {
    let target_table = unit_tables.get(target_name);
    let target_consumed = unit_consumes.get(target_name).cloned().unwrap_or_default();

    // Collect clauses tagged with the declaring file. Case-scoped first so they
    // precede suite-scoped clauses in each capability's match order.
    let mut collected: Vec<(StubClause, PathBuf)> = Vec::new();
    for &i in indices {
        let Some(t) = parsed[i].test() else { continue };
        for case in &t.cases {
            for pc in &case.stubs {
                collected.push((pc.clone(), parsed[i].source_path.clone()));
            }
        }
    }
    for &i in indices {
        let Some(t) = parsed[i].test() else { continue };
        for pc in &t.stubs {
            collected.push((pc.clone(), parsed[i].source_path.clone()));
        }
    }

    // Resolve a capability name to its declaration: a capability the target
    // declares (or has flattened in via `consumes U { Cap }`), else a capability
    // of a consumed context.
    let resolve_cap = |name: &str| -> Option<CapabilityDecl> {
        target_table
            .and_then(|t| t.capabilities.get(name).cloned())
            .or_else(|| {
                target_consumed.iter().find_map(|q| {
                    unit_tables
                        .get(q)
                        .and_then(|t| t.capabilities.get(name).cloned())
                })
            })
    };

    let mut out: HashMap<String, ResolvedStub> = HashMap::new();
    for (pc, source_path) in collected {
        let cap_name = pc.capability.name.clone();
        let Some(cap_decl) = resolve_cap(&cap_name) else {
            // Commons have no seams at all; contexts may still name a
            // non-existent capability. Either way it is not a seam.
            let note = if target_kind == UnitKind::Commons {
                "commons have no capability seams — `stub` overrides a capability the target context declares or consumes"
            } else {
                "a `stub` clause names a capability the target context declares or reaches through a consumed context"
            };
            errors.push(
                CompileError::new(
                    "bynk.stub.not_a_seam",
                    pc.capability.span,
                    format!("`{cap_name}` is not a capability seam of `{target_name}`",),
                )
                .with_note(note),
            );
            continue;
        };
        if !cap_decl.ops.iter().any(|o| o.name.name == pc.method.name) {
            errors.push(CompileError::new(
                "bynk.stub.unknown_op",
                pc.method.span,
                format!(
                    "`{}` is not an operation of capability `{cap_name}`",
                    pc.method.name
                ),
            ));
            continue;
        }
        if let StubRhs::ReturnsEach(outcomes, span) = &pc.rhs
            && outcomes.is_empty()
        {
            errors.push(CompileError::new(
                "bynk.stub.bad_sequence",
                *span,
                format!(
                    "`stub {cap_name}.{} returns each []` has no outcomes — a sequence needs at least one",
                    pc.method.name
                ),
            ));
            continue;
        }
        let entry = out.entry(cap_name.clone()).or_insert_with(|| ResolvedStub {
            cap: cap_name.clone(),
            cap_decl: cap_decl.clone(),
            clauses: Vec::new(),
            source_path: source_path.clone(),
        });
        entry.clauses.push(pc);
    }
    out
}

/// v0.16: process every `test integration "name"` suite. Validates the `wires`
/// participant set (existence, ≥ 2, no duplicates, full `consumes` closure),
/// type-checks each case body as a cross-context call from a synthetic harness
/// root that consumes every participant, and emits a TypeScript module that
/// stands the participants up as in-process Workers wired by simulated Service
/// Bindings and runs the cases across the real serialise/deserialise wire.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_integration_tests(
    integration_groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    // v0.132: production unit name -> its `parsed` file indices (see `process_tests`).
    groups: &HashMap<String, Vec<usize>>,
    tests_prefix: &Path,
    // v0.132: barrel-path dedup set shared with the unit-test pass.
    emitted_barrels: &mut HashSet<PathBuf>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) -> (Vec<CompiledFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<CompiledFile> = Vec::new();
    let mut runnables: Vec<RunnableTest> = Vec::new();

    let mut sorted: Vec<&String> = integration_groups.keys().collect();
    sorted.sort();

    let _ = kinds;

    for group_name in sorted {
        let indices = integration_groups.get(group_name).unwrap();
        let first = indices[0];
        let Some(decl) = parsed[first].integration() else {
            continue;
        };
        // v0.118: there is no `suite` string any more — the wired suite is named
        // for its target context. The participant set is INFERRED from the
        // target's transitive `consumes` closure (no `wires` list).
        let suite_target = decl.target.joined();
        let suite_name = suite_target.clone();
        let participants = infer_participants(&suite_target, unit_consumes);

        let mut bad = false;

        // v0.118: a `system`-tier suite needs a real wire — the target plus at
        // least one consumed context. Fewer than two participants means there is
        // nothing to serialise across (replaces `too_few_participants`).
        if participants.len() < 2 {
            errors.push(
                CompileError::new(
                    "bynk.tier.system_needs_wire",
                    decl.target.span,
                    format!(
                        "`system`-tier suite for `{suite_target}` has nothing to wire — the target consumes no other context",
                    ),
                )
                .with_note(
                    "a `system` test wires the target across the real serialisation boundary to the contexts it consumes; test a single context with `unit` or `integration`",
                ),
            );
            bad = true;
        }

        // -- Duplicate case names within the suite. --
        let mut seen_cases: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            let Some(d) = parsed[i].integration() else {
                continue;
            };
            for case in &d.cases {
                if let Some(prev) = seen_cases.get(&case.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.suite.duplicate_case_name",
                            case.name_span,
                            format!(
                                "test case `\"{}\"` is declared more than once in tests targeting `{suite_target}`",
                                case.name
                            ),
                        )
                        .with_label(*prev, "previously declared here"),
                    );
                    bad = true;
                } else {
                    seen_cases.insert(case.name.clone(), case.name_span);
                }
            }
        }

        if bad {
            continue;
        }

        // -- Build the harness-root cross-context view (consumes all). --
        let harness_name = group_name.clone();
        let mut uses_targets: Vec<String> = Vec::new();
        for &i in indices {
            if let Some(d) = parsed[i].integration() {
                for u in &d.uses {
                    let q = u.target.joined();
                    if !uses_targets.contains(&q) {
                        uses_targets.push(q);
                    }
                }
            }
        }
        let mut harness_consumes = unit_consumes.clone();
        harness_consumes.insert(harness_name.clone(), participants.clone());
        let mut harness_uses = unit_uses.clone();
        harness_uses.insert(harness_name.clone(), uses_targets.clone());
        let cross_context = build_cross_context_info(
            &harness_name,
            &harness_consumes,
            unit_consumes_aliases,
            &harness_uses,
            unit_tables,
        );

        // -- Type-check each case body. --
        let mut body_errs: Vec<CompileError> = Vec::new();
        // v0.25: the harness root is a synthetic namespace — declare its
        // resolution order (uses first, then participants) for assembly.
        let mut harness_resolution = uses_targets.clone();
        harness_resolution.extend(participants.iter().cloned());
        refs.declare_namespace(&harness_name, harness_resolution);
        for &i in indices {
            let Some(d) = parsed[i].integration() else {
                continue;
            };
            refs.enter_file(&parsed[i].source_path, &harness_name, parsed[i].synthetic);
            for case in &d.cases {
                check_integration_case_body(
                    &participants,
                    &uses_targets,
                    case,
                    &cross_context,
                    unit_tables,
                    &mut body_errs,
                    refs,
                );
            }
        }
        let bodies_failed = !body_errs.is_empty();
        errors.extend(body_errs);
        if bodies_failed {
            continue;
        }

        // -- Emit the integration module. --
        // Collect each case with the fragment file it came from, so a suite
        // split across files maps each case body under its own source.
        let mut case_inputs: Vec<SystemCaseInput> = Vec::new();
        for &i in indices {
            let Some(d) = parsed[i].integration() else {
                continue;
            };
            let rel_path = tests_prefix
                .join(&parsed[i].source_path)
                .to_string_lossy()
                .into_owned();
            let map_source = parsed[i].map_source_name();
            for case in &d.cases {
                case_inputs.push(SystemCaseInput {
                    case,
                    source: &parsed[i].source,
                    rel_path: rel_path.clone(),
                    map_source: map_source.clone(),
                });
            }
        }
        if let Some((path, source, source_map, runnable)) = emit_integration_module(
            &suite_name,
            &participants,
            &uses_targets,
            &cross_context,
            unit_consumes,
            unit_tables,
            &case_inputs,
        ) {
            outputs.push(CompiledFile {
                source_path: path.clone(),
                output_path: path,
                typescript: source,
                source_map,
                debug_metadata: None,
            });
            runnables.push(runnable);

            // v0.132: the integration module imports each `uses` commons as a
            // namespace (`import * as ns from "./<name>.js"`); participants come
            // in through `../workers/`, so only `uses_targets` need a barrel.
            // Emit one (deduped) for each that is a multi-file commons.
            for name in &uses_targets {
                if let Some(barrel) =
                    emit_commons_barrel(name, groups, parsed, ImportExt::Js, emitted_barrels)
                {
                    outputs.push(barrel);
                }
            }
        }
    }

    (outputs, runnables)
}

/// v0.118: infer a `system`-tier suite's wired participants — the target's
/// transitive `consumes` closure (testing track slice 6). A BFS from the target
/// following `consumes` edges; the returned list starts with the target and
/// includes every context reachable through it (deterministic breadth order).
fn infer_participants(target: &str, unit_consumes: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut queue: Vec<String> = vec![target.to_string()];
    seen.insert(target.to_string());
    let mut head = 0;
    while head < queue.len() {
        let node = queue[head].clone();
        head += 1;
        order.push(node.clone());
        if let Some(deps) = unit_consumes.get(&node) {
            for d in deps {
                if seen.insert(d.clone()) {
                    queue.push(d.clone());
                }
            }
        }
    }
    order
}

/// v0.118: one `system`-tier case paired with the fragment file it was declared
/// in, so [`emit_integration_module`] maps each case body under its own source.
struct SystemCaseInput<'a> {
    case: &'a Case,
    source: &'a str,
    rel_path: String,
    map_source: String,
}

/// Type-check one integration test case body. The body lives in a synthetic
/// harness root that consumes every participant; entry calls
/// (`ctx.service(args)`) are therefore ordinary cross-context calls. The body
/// has type `Effect[Result[(), ExpectationError]]` (modelled as
/// `Effect[Result[(), ValidationError]]`, as in unit tests).
fn check_integration_case_body(
    participants: &[String],
    uses_targets: &[String],
    case: &Case,
    cross_context: &resolver::CrossContextInfo,
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) {
    // Names in scope: types/fns/methods from `uses` commons (for constructing
    // arguments) plus each participant's types/methods (so return types rebrand
    // and variant patterns resolve).
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
    let mut methods: HashMap<String, ResolverMethodTable> = HashMap::new();
    let mut merge = |src: Option<&UnitTable>, with_fns: bool| {
        let Some(t) = src else { return };
        for (n, d) in &t.types {
            types.entry(n.clone()).or_insert_with(|| d.clone());
        }
        if with_fns {
            for (n, f) in &t.fns {
                fns.entry(n.clone()).or_insert_with(|| f.clone());
            }
        }
        for (n, mt) in &t.methods {
            let entry = methods.entry(n.clone()).or_default();
            for (m, decl) in &mt.instance {
                entry
                    .instance
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
            for (m, decl) in &mt.statics {
                entry
                    .statics
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
        }
    };
    for u in uses_targets {
        merge(unit_tables.get(u), true);
    }
    for p in participants {
        merge(unit_tables.get(p), false);
    }

    let synthetic_commons = Commons {
        name: QualifiedName {
            parts: vec![Ident {
                name: "integration".to_string(),
                span: Span::default(),
            }],
            span: Span::default(),
        },
        items: Vec::new(),
        uses: Vec::new(),
        documentation: None,
        form: CommonsForm::Brace,
        span: Span::default(),
        trivia: Trivia::default(),
        trailing_comments: Vec::new(),
    };
    let resolved = bynk_check::resolver::ResolvedCommons {
        commons: synthetic_commons,
        types,
        fns,
        methods,
        local_type_names: HashSet::new(),
        cross_context: cross_context.clone(),
        agents: HashMap::new(),
        imported_from: HashMap::new(),
    };

    let unit_span = case.span;
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );
    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types).unwrap();
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    // Test bodies record no hints (out of v0.27 scope) — a throwaway sink.
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    // Test bodies record no capability requirements either — muted sink.
    let mut no_requirements = RequirementSink::new();
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        refs,
        hints: &mut no_hints,
        locals: &mut no_locals,
        requirements: &mut no_requirements,
        scopes: vec![HashMap::new()],
        return_ty: return_ty.clone(),
        return_ty_span: case.span,
        effectful: true,
        agent_state_ty: None,
        commit_seen: false,
        caps: checker::CapabilityCtx::default(),
        in_test_body: true,
        test_services: HashSet::new(),
        type_vars: std::collections::HashSet::new(),
        store_cells: std::collections::HashMap::new(),
        store_maps: std::collections::HashMap::new(),
        store_sets: std::collections::HashMap::new(),
        store_caches: std::collections::HashMap::new(),
        store_logs: std::collections::HashMap::new(),
    };
    let _ = checker::type_of_block(&case.body, Some(&return_ty), &mut ctx);
}

/// Emit a single integration-test module plus its [`RunnableTest`] pointer.
/// The module imports each participant's workers-mode handler namespace (for
/// serialise/deserialise) and Worker entry (for dispatch), builds an in-process
/// env graph wiring the Service Bindings, and runs each case across the wire.
#[allow(clippy::too_many_arguments)]
fn emit_integration_module(
    suite: &str,
    participants: &[String],
    uses_targets: &[String],
    cross_context: &resolver::CrossContextInfo,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    cases: &[SystemCaseInput],
) -> Option<(PathBuf, String, Option<String>, RunnableTest)> {
    let sanitized = sanitise_suite(suite);
    let module_path = PathBuf::from(format!("tests/integration_{sanitized}.test.ts"));
    let mut out = String::new();
    // v0.70: the integration module's source map. A `system` suite may span
    // several fragment files, so the builder is multi-source; each case's body is
    // merged under its own source (registered on first sight in the case loop).
    // v0.72: keyed on each fragment's absolute path so an editor breakpoint on the
    // real `.bynk` binds; the per-case `rel_path` stays the test-runner location.
    let mut module_smb = SourceMapBuilder::new();
    out.push_str("// Generated by bynkc — do not edit by hand.\n");
    out.push_str(&format!("// system test: {suite}\n\n"));

    // Runtime imports. When a participant owns agents, also pull in the
    // Durable-Object namespace helper + types for the in-memory DO stubs.
    let has_agents = participants
        .iter()
        .any(|p| unit_tables.get(p).is_some_and(|t| !t.agents.is_empty()));
    let runtime_import = emitter::runtime_import_for(&module_path, ImportExt::Js);
    let agent_imports = if has_agents {
        ", makeIntegrationDoNamespace, type DurableObjectState, type DurableObjectNamespace"
    } else {
        ""
    };
    out.push_str(&format!(
        "import {{ Ok, Err, Some, None, callService, type Result, type Option, type ValidationError, type JsonError, type JsonValue, type BoundaryError, type ServiceBinding{agent_imports} }} from \"{runtime_import}\";\n"
    ));

    // Per-participant: workers handler namespace + Worker entry default export.
    for p in participants {
        let ns = p.replace('.', "_");
        let dir = worker_dir_name(p);
        out.push_str(&format!(
            "import * as {ns} from \"../workers/{dir}/handlers.js\";\n"
        ));
        out.push_str(&format!(
            "import worker_{ns} from \"../workers/{dir}/index.js\";\n"
        ));
    }

    // `uses` commons (for constructing arguments).
    let mut uses_imports: Vec<(String, String)> = Vec::new();
    for u in uses_targets {
        let ns = u.replace('.', "_");
        let path = relative_import_for_test(&commons_dir_for(u));
        uses_imports.push((ns, path));
    }
    uses_imports.sort();
    uses_imports.dedup();
    for (ns, path) in &uses_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.js\";\n"));
    }
    out.push('\n');

    out.push_str(&expectation_runtime_helpers());

    // The env-graph harness: stand each participant up as an in-process Worker
    // and wire its Service Bindings to its siblings; the root env binds to all.
    out.push_str(&emit_integration_harness(
        participants,
        unit_consumes,
        unit_tables,
    ));
    out.push('\n');

    // One async function per case.
    let mut typed = integration_typed_commons(uses_targets, participants, unit_tables);
    let mut case_runners: Vec<String> = Vec::new();
    let mut discovered: Vec<DiscoveredCase> = Vec::new();
    for input in cases {
        let case = input.case;
        discovered.push(DiscoveredCase {
            name: case.name.clone(),
            location: Some(discovered_location(
                input.source,
                &input.rel_path,
                case.name_span,
            )),
        });
        let runner_name = sanitise_case_name(&case.name, &mut case_runners.len());
        case_runners.push(runner_name.clone());
        out.push_str(&format!("async function {runner_name}() {{\n"));
        out.push_str("  try {\n");
        out.push_str("    const deps = makeHarness();\n");
        // Bring `uses` commons names into scope for argument construction.
        for u in uses_targets {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                let mut names: Vec<&String> = table.types.keys().chain(table.fns.keys()).collect();
                names.sort();
                names.dedup();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
        }
        let (body_src, body_smb) = emitter::lower_integration_case_body(
            &case.body,
            &mut typed,
            cross_context,
            input.source,
            &input.rel_path,
        );
        let src_id = module_smb.add_source(input.map_source.clone(), input.source.to_string());
        let body_base = out.len();
        for line in body_src.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        module_smb.merge(&body_smb, &body_src, &out, body_base, src_id);
        out.push_str("    return { pass: true };\n");
        out.push_str("  } catch (e) {\n");
        out.push_str("    if (e instanceof ExpectationError) {\n");
        out.push_str(
            "      return { pass: false, error: { message: e.message, location: e.location } };\n",
        );
        out.push_str("    }\n");
        out.push_str(
            "    return { pass: false, error: { message: String(e), location: \"unknown\" } };\n",
        );
        out.push_str("  }\n");
        out.push_str("}\n\n");
    }

    // Module runner. v0.127: `only` filters to a single case by name (the
    // per-case run lens); undefined runs every case.
    out.push_str("export async function run(only?: string) {\n");
    out.push_str("  const results = [];\n");
    out.push_str("  const want = (n: string): boolean => only === undefined || only === n;\n");
    for (idx, input) in cases.iter().enumerate() {
        let runner_name = &case_runners[idx];
        let escaped = emitter::escape_ts_string(&input.case.name);
        out.push_str(&format!(
            "  if (want(\"{escaped}\")) results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
        ));
    }
    out.push_str("  return results;\n");
    out.push_str("}\n");

    let module_file = module_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module.ts".to_string());
    let source_map = module_smb.to_v3(&out, &module_file);
    Some((
        module_path.clone(),
        out,
        source_map,
        RunnableTest {
            target_name: format!("integration · {suite}"),
            module_path,
            kind: "integration",
            suite_name: suite.to_string(),
            cases: discovered,
        },
    ))
}

/// Emit the `makeHarness()` factory: an in-process env per participant whose
/// Service Bindings call the sibling participants' real Worker `fetch` and whose
/// Durable-Object namespaces back the participant's own agents in memory, plus a
/// root env binding every participant (the test cases call in through it). A
/// fresh harness per case gives each case clean agent state.
fn emit_integration_harness(
    participants: &[String],
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> String {
    let mut out = String::new();
    out.push_str("function makeHarness() {\n");
    // Declare every participant env first so sibling references resolve.
    for p in participants {
        let ns = p.replace('.', "_");
        out.push_str(&format!("  const env_{ns}: any = {{}};\n"));
    }
    // Wire each participant's consumed Service Bindings to its sibling Workers,
    // and back its own agents with in-memory Durable Object namespaces.
    for p in participants {
        let ns = p.replace('.', "_");
        if let Some(deps) = unit_consumes.get(p) {
            let mut deps_sorted = deps.clone();
            deps_sorted.sort();
            for d in &deps_sorted {
                let dns = d.replace('.', "_");
                let binding = crate::emitter::wrangler::consumed_binding_name(d);
                out.push_str(&format!(
                    "  env_{ns}.{binding} = {{ fetch: (req: Request) => worker_{dns}.fetch(req, env_{dns}) }} as ServiceBinding;\n"
                ));
            }
        }
        if let Some(table) = unit_tables.get(p) {
            let mut agents: Vec<&String> = table.agents.keys().collect();
            agents.sort();
            for agent in agents {
                let binding = crate::emitter::wrangler::agent_binding_name(agent);
                out.push_str(&format!(
                    "  env_{ns}.{binding} = makeIntegrationDoNamespace((state) => new {ns}.{agent}(state));\n"
                ));
            }
        }
    }
    // Root env binds to every participant.
    out.push_str("  const rootEnv: any = {};\n");
    for p in participants {
        let ns = p.replace('.', "_");
        let binding = crate::emitter::wrangler::consumed_binding_name(p);
        out.push_str(&format!(
            "  rootEnv.{binding} = {{ fetch: (req: Request) => worker_{ns}.fetch(req, env_{ns}) }} as ServiceBinding;\n"
        ));
    }
    out.push_str("  return { env: rootEnv };\n");
    out.push_str("}\n");
    out
}

/// Build the [`checker::TypedCommons`] used to lower integration case bodies —
/// `uses` commons plus participant types/fns/methods, so static calls and
/// constructors resolve.
fn integration_typed_commons(
    uses_targets: &[String],
    participants: &[String],
    unit_tables: &HashMap<String, UnitTable>,
) -> checker::TypedCommons {
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
    let mut methods: HashMap<String, ResolverMethodTable> = HashMap::new();
    let mut add = |t: Option<&UnitTable>, with_fns: bool| {
        let Some(t) = t else { return };
        for (n, d) in &t.types {
            types.entry(n.clone()).or_insert_with(|| d.clone());
        }
        if with_fns {
            for (n, f) in &t.fns {
                fns.entry(n.clone()).or_insert_with(|| f.clone());
            }
        }
        for (n, mt) in &t.methods {
            let entry = methods.entry(n.clone()).or_default();
            for (m, decl) in &mt.instance {
                entry
                    .instance
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
            for (m, decl) in &mt.statics {
                entry
                    .statics
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
        }
    };
    for u in uses_targets {
        add(unit_tables.get(u), true);
    }
    for p in participants {
        add(unit_tables.get(p), false);
    }
    checker::TypedCommons {
        commons: Commons {
            name: QualifiedName {
                parts: vec![Ident {
                    name: "integration".to_string(),
                    span: Span::default(),
                }],
                span: Span::default(),
            },
            items: Vec::new(),
            uses: Vec::new(),
            documentation: None,
            form: CommonsForm::Brace,
            span: Span::default(),
            trivia: Trivia::default(),
            trailing_comments: Vec::new(),
        },
        types,
        fns,
        methods,
        expr_types: HashMap::new(),
        warnings: vec![],
    }
}

fn sanitise_suite(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "suite".to_string()
    } else {
        trimmed
    }
}

/// Discovered, named test ready to be invoked from the top-level runner.
pub(crate) struct RunnableTest {
    /// Joined target name (e.g., `commerce.payment`), or `integration · <suite>`
    /// for an integration suite — the runner's module identity and sort key.
    target_name: String,
    /// The module's output path relative to the project root.
    module_path: PathBuf,
    /// v0.67: `"unit"` or `"integration"` — the suite kind for discovery, mirrors
    /// the runner's `suite-begin` `kind`.
    kind: &'static str,
    /// v0.67: the JSON suite name for discovery — the joined target name (unit)
    /// or the bare suite name (integration). Differs from `target_name` only for
    /// integration, which the runner prefixes with `integration · `.
    suite_name: String,
    /// v0.67: the suite's cases, in declaration (emission) order, retained for
    /// `--no-run` discovery.
    cases: Vec<DiscoveredCase>,
}

/// v0.67: the `path:line:col` of a test-name literal, structured for discovery.
/// Reuses [`bynk_syntax::span::line_col`] and the same forward-slash
/// normalisation `assert_location` applies (bynk-emit/src/emitter/lower.rs), so a
/// discovered case and a run-failure resolve to consistent coordinates.
fn discovered_location(source: &str, rel_path: &str, span: Span) -> TestLocation {
    let (line, col) = bynk_syntax::span::line_col(source, span.start);
    TestLocation {
        path: rel_path.replace('\\', "/"),
        line: line as u32,
        col: col as u32,
    }
}

/// v0.67: fold the combined runnable manifest into the discovery suites, ordered
/// by the runner's sort key (`target_name`) so the discovery document matches a
/// run's suite order.
pub(crate) fn discovery_manifest(tests: &[RunnableTest]) -> Vec<DiscoveredSuite> {
    let mut sorted: Vec<&RunnableTest> = tests.iter().collect();
    sorted.sort_by(|a, b| a.target_name.cmp(&b.target_name));
    sorted
        .into_iter()
        .map(|t| DiscoveredSuite {
            name: t.suite_name.clone(),
            kind: t.kind,
            cases: t.cases.clone(),
        })
        .collect()
}

fn first_test_target_span(indices: &[usize], parsed: &[ParsedFile]) -> Span {
    indices
        .first()
        .and_then(|&i| parsed[i].test().map(|t| t.target.span))
        .unwrap_or_default()
}

/// Type-check test/property bodies for a target and validate `stub` RHS
/// value types (v0.118). Bodies use the target's privileged view; a `stub`
/// value whose type disagrees with the overridden op's return is
/// `bynk.stub.rhs_type`.
#[allow(clippy::too_many_arguments)]
fn check_test_bodies(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    stubs: &HashMap<String, ResolvedStub>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    refs: &mut RefSink,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    let _ = exports_visibility;

    // v0.118: validate each `stub` RHS value's type against the overridden
    // op's declared return type, in the target's privileged view. A best-effort
    // check: the value expression is type-checked as if it were the op body's
    // tail; any resulting error surfaces as `bynk.stub.rhs_type`.
    if !stubs.is_empty()
        && let Some((resolved, _)) = build_privileged_resolved(
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        )
    {
        for rp in stubs.values() {
            refs.enter_file(&rp.source_path, target_name, false);
            for clause in &rp.clauses {
                let Some(op) = rp
                    .cap_decl
                    .ops
                    .iter()
                    .find(|o| o.name.name == clause.method.name)
                else {
                    continue;
                };
                let check_value = |e: &Expr, errors: &mut Vec<CompileError>| {
                    if !stub_value_typechecks(e, op, &resolved) {
                        errors.push(CompileError::new(
                            "bynk.stub.rhs_type",
                            e.span,
                            format!(
                                "the value provided for `{}.{}` does not match the operation's declared return type `{}`",
                                rp.cap,
                                op.name.name,
                                ts_type_ref_display(&op.return_type),
                            ),
                        ));
                    }
                };
                match &clause.rhs {
                    StubRhs::Returns(e) => check_value(e, &mut errors),
                    StubRhs::ReturnsEach(outcomes, _) => {
                        for o in outcomes {
                            if let SeqOutcome::Value(e) = o {
                                check_value(e, &mut errors);
                            }
                        }
                    }
                    StubRhs::Fails(_) => {}
                }
            }
        }
    }

    // Type-check test case bodies — they live in the target's privileged
    // view, with `stub` overriding individual capability seams.
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        // v0.25: test-case edges record in the test file, resolving bare
        // names through the *target* unit's namespace.
        refs.enter_file(&parsed[i].source_path, target_name, parsed[i].synthetic);
        for case in &test_decl.cases {
            check_test_case_body(
                target_name,
                target_kind,
                case,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &mut errors,
                refs,
            );
        }
        // v0.114: generative `property` blocks — check their `for all` bindings,
        // `where` filter, and predicate body (testing track slice 2).
        for prop in &test_decl.properties {
            // v0.118: a `property` never carries a tier — `as <tier>` is a
            // `case`-only affordance and the grammar has no property-tier
            // production. Guard defensively so a future surface that attaches one
            // is rejected rather than silently mis-tiered.
            if property_tier(prop).is_some() {
                errors.push(CompileError::new(
                    "bynk.tier.property_has_tier",
                    prop.name_span,
                    format!(
                        "property `\"{}\"` cannot declare a tier — tiers are a `case`-only affordance",
                        prop.name
                    ),
                ));
            }
            check_property_body(
                target_name,
                target_kind,
                prop,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &mut errors,
                refs,
            );
        }
    }

    errors
}

/// v0.118: the tier a `property` carries, if any. Always `None` — a `property`
/// has no tier field (the `as <tier>` clause is a `case`-only affordance). A
/// dedicated accessor so the defensive `bynk.tier.property_has_tier` guard reads
/// as a real check against a future surface rather than a hard-coded `false`.
fn property_tier(_prop: &PropertyDecl) -> Option<bynk_syntax::ast::TestTier> {
    None
}

/// v0.118: wrap a single expression as a `{ tail: e }` block, so a `stub`
/// value can be type-checked or lowered in the same op-body position a provider
/// operation's tail occupies.
fn value_block(e: &Expr) -> Block {
    Block {
        statements: Vec::new(),
        tail: Box::new(e.clone()),
        span: e.span,
        tail_leading_comments: Vec::new(),
        implicit_tail: false,
    }
}

/// v0.118: whether a `stub` value expression type-checks against the
/// overridden capability op's declared return type (best-effort — a throwaway
/// check against the target's privileged view). A mismatch drives
/// `bynk.stub.rhs_type`.
fn stub_value_typechecks(
    e: &Expr,
    op: &CapabilityOp,
    resolved: &bynk_check::resolver::ResolvedCommons,
) -> bool {
    let block = value_block(e);
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    let mut errs: Vec<CompileError> = Vec::new();
    checker::check_handler_body(
        resolved,
        checker::HandlerBodyCheck::new(&block, &op.return_type, &op.params, &[]),
        checker::CheckSinks {
            expr_types: &mut expr_types,
            errors: &mut errs,
            refs: &mut RefSink::new(),
            hints: &mut HintSink::new(),
            locals: &mut LocalsSink::new(),
            requirements: &mut RequirementSink::new(),
        },
    );
    errs.is_empty()
}

/// Whether a `case` body uses an observation (`Cap.op called …`) or
/// `trace(Cap.op)` anywhere (v0.117) — the signal to wrap `deps` with the
/// recording proxy. Bodies that don't observe emit unchanged.
fn block_uses_observation(block: &Block) -> bool {
    let mut found = false;
    let mut check = |e: &Expr| {
        if matches!(e.kind, ExprKind::Observation(_) | ExprKind::Trace { .. }) {
            found = true;
        }
    };
    for s in &block.statements {
        let e = match s {
            Statement::Let(l) => &l.value,
            Statement::EffectLet(l) => &l.value,
            Statement::Expect(x) => &x.value,
            Statement::Send(x) => &x.value,
            Statement::Do(d) => &d.value,
            Statement::Assign(a) => &a.value,
        };
        crate::emitter::walk_exprs(e, &mut check);
    }
    crate::emitter::walk_exprs(&block.tail, &mut check);
    found
}

/// Register a synthetic call-record type per capability operation of the target
/// context (v0.117, testing track slice 5), so `trace(Cap.op)` — typed
/// `List[<CallRecord>]` — supports field access on its records. The record's
/// fields are the operation's parameters.
fn register_call_record_types(
    resolved: &mut bynk_check::resolver::ResolvedCommons,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
) {
    let Some(table) = unit_tables.get(target_name) else {
        return;
    };
    for (cap_name, decl) in &table.capabilities {
        for op in &decl.ops {
            let fields: Vec<RecordField> = op
                .params
                .iter()
                .map(|p| RecordField {
                    name: p.name.clone(),
                    type_ref: p.type_ref.clone(),
                    refinement: None,
                    init: None,
                    span: p.span,
                })
                .collect();
            let name = checker::call_record_type_name(cap_name, &op.name.name);
            resolved.types.insert(
                name.clone(),
                TypeDecl {
                    type_params: Vec::new(),
                    name: Ident {
                        name,
                        span: op.name.span,
                    },
                    body: TypeBody::Record(RecordBody {
                        fields,
                        span: op.name.span,
                    }),
                    documentation: None,
                    span: op.name.span,
                    trivia: Trivia::default(),
                },
            );
        }
    }
}

/// Type-check a test `case`/`property` body against the target unit's privileges,
/// returning the inferred `expr_types` map. The **check** path feeds real
/// diagnostic/ref sinks; the **emit** path reuses it with throwaway sinks to give
/// the case-body lowering full type information (so collection kernels — notably
/// `trace(Cap.op)`'s `List[…]` methods — dispatch on the receiver's checked type).
#[allow(clippy::too_many_arguments)]
fn typecheck_case_body(
    target_name: &str,
    body: &Block,
    unit_span: Span,
    unit_tables: &HashMap<String, UnitTable>,
    resolved: &ResolvedCommons,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    // v0.119: bindings already in scope for the body — empty for a `case`, the
    // `run: List[Step]` binding for a history property.
    initial_scope: HashMap<String, checker::Ty>,
) -> HashMap<Span, checker::Ty> {
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    // Synthesise an Effect[Result[(), ValidationError]] return type as a
    // stand-in for Effect[Result[(), ExpectationError]]. v0.7 doesn't model an
    // explicit ExpectationError type — the runtime catches it instead.
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );

    // Capabilities of the target context, if any (so the test body can
    // call capabilities directly when targeting a context).
    let mut capability_info_map: HashMap<String, checker::CapabilityInfo> = HashMap::new();
    if let Some(table) = unit_tables.get(target_name) {
        for (name, decl) in &table.capabilities {
            let ops = decl
                .ops
                .iter()
                .map(|op| checker::CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| {
                            checker::resolve_type_ref(&p.type_ref, &resolved.types)
                                .unwrap_or(checker::Ty::Unit)
                        })
                        .collect(),
                    param_names: op.params.iter().map(|p| p.name.name.clone()).collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &resolved.types)
                        .unwrap_or(checker::Ty::Unit),
                })
                .collect();
            capability_info_map.insert(
                name.clone(),
                checker::CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            );
        }
    }

    // All declared capabilities are implicitly "given" inside a test body;
    // the test runner wires them via the mocked deps. We feed the same map
    // to both `capabilities` (in-scope) and `declared_capabilities`.
    let given_declared: Vec<String> = capability_info_map.keys().cloned().collect();

    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types).unwrap();
    let return_ty_span = unit_span;
    let effectful = matches!(return_ty, checker::Ty::Effect(_));
    // Test bodies record no hints (out of v0.27 scope) — a throwaway sink.
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    // Test bodies record no capability requirements either — muted sink.
    let mut no_requirements = RequirementSink::new();
    let mut ctx = checker::Ctx {
        input: resolved,
        expr_types: &mut expr_types,
        errors,
        refs,
        hints: &mut no_hints,
        locals: &mut no_locals,
        requirements: &mut no_requirements,
        scopes: vec![initial_scope],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        caps: checker::CapabilityCtx {
            capabilities: capability_info_map.clone(),
            declared_capabilities: capability_info_map,
            given_remaining: given_declared.iter().cloned().collect(),
            given_used: HashSet::new(),
            given_entries: Vec::new(),
            given_anchor: None,
        },
        in_test_body: true,
        test_services: unit_tables
            .get(target_name)
            .map(|t| t.services.keys().cloned().collect())
            .unwrap_or_default(),
        type_vars: std::collections::HashSet::new(),
        store_cells: std::collections::HashMap::new(),
        store_maps: std::collections::HashMap::new(),
        store_sets: std::collections::HashMap::new(),
        store_caches: std::collections::HashMap::new(),
        store_logs: std::collections::HashMap::new(),
    };
    let _ = checker::type_of_block(body, Some(&return_ty), &mut ctx);
    expr_types
}

#[allow(clippy::too_many_arguments)]
fn check_test_case_body(
    target_name: &str,
    target_kind: UnitKind,
    case: &Case,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) {
    let Some((mut resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    register_call_record_types(&mut resolved, target_name, unit_tables);
    let _ = target_kind;
    let _ = typecheck_case_body(
        target_name,
        &case.body,
        case.span,
        unit_tables,
        &resolved,
        errors,
        refs,
        HashMap::new(),
    );
    // Don't enforce return-type equality; the test runner discards the
    // tail expression and recovers success/failure from expectation outcome.
    // Don't enforce "every given used" — capabilities are implicitly
    // available in a test body.

    // v0.115: flag a `case` that merely restates a contract already declared at
    // the source (`bynk.contract.restated_by_test`) — an `expect` that is
    // α-equivalent to an `ensures` clause over the same bound arguments. The dev
    // guard and the runner attack already check it. Conservative: under-flagging
    // is acceptable, over-flagging is not.
    check_restated_contract(&case.body, &resolved, errors);
}

/// v0.115: within a test body, flag an `expect` that re-states a contract's
/// `ensures`. Fires only on the clearest restatement: a binding `let r = f(args)`
/// (or `r <- f(args)`) of a contracted free function's result, followed by an
/// `expect E` that is α-equivalent to one of `f`'s `ensures` predicates under the
/// substitution `result → r`, `params → args`. Syntactic — never semantic — so a
/// merely-equivalent (but differently written) test is not flagged.
fn check_restated_contract(
    body: &Block,
    resolved: &ResolvedCommons,
    errors: &mut Vec<CompileError>,
) {
    // Map each locally-bound name to the contracted free function + call args it
    // was bound from (`let r = f(a, b)`).
    let mut bound: HashMap<String, (&FnDecl, &[Expr])> = HashMap::new();
    for stmt in &body.statements {
        let (name, value) = match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => (&l.name.name, &l.value),
            _ => continue,
        };
        if let ExprKind::Call {
            name: callee, args, ..
        } = &value.kind
            && let Some(f) = resolved.fns.get(&callee.name)
            && matches!(&f.name, FnName::Free(_))
            && !f.ensures.is_empty()
            && f.params.len() == args.len()
        {
            bound.insert(name.clone(), (f, args.as_slice()));
        }
    }
    if bound.is_empty() {
        return;
    }
    for stmt in &body.statements {
        let Statement::Expect(e) = stmt else { continue };
        for (result_name, (f, args)) in &bound {
            // subst: result → r, each param → its call argument.
            let result_ident = Expr {
                kind: ExprKind::Ident(Ident {
                    name: result_name.clone(),
                    span: e.span,
                }),
                span: e.span,
            };
            let mut subst: HashMap<&str, &Expr> = HashMap::new();
            subst.insert("result", &result_ident);
            for (p, a) in f.params.iter().zip(args.iter()) {
                subst.insert(p.name.name.as_str(), a);
            }
            for c in &f.ensures {
                if expr_alpha_eq_subst(&c.predicate, &e.value, &subst) {
                    let FnName::Free(fname) = &f.name else {
                        continue;
                    };
                    errors.push(
                        CompileError::new(
                            "bynk.contract.restated_by_test",
                            e.span,
                            format!(
                                "this `expect` restates the `ensures {}` contract of `{}`, which is already checked at every call and by the runner",
                                c.name.name, fname.name
                            ),
                        )
                        .with_note(
                            "a contract is checked everywhere for free — delete the restating test, or keep a `case` only for a specific witnessed value",
                        ),
                    );
                    break;
                }
            }
        }
    }
}

/// Structural (α-)equality of two predicate expressions, ignoring spans, where a
/// bare identifier in `pattern` that appears in `subst` must match the
/// corresponding substituted expression in `actual` (the rest compares by shape).
/// Deliberately conservative — only the operators/leaves a contract predicate can
/// contain are compared; anything unrecognised is unequal.
fn expr_alpha_eq_subst(pattern: &Expr, actual: &Expr, subst: &HashMap<&str, &Expr>) -> bool {
    if let ExprKind::Ident(id) = &pattern.kind
        && let Some(replacement) = subst.get(id.name.as_str())
    {
        return expr_struct_eq(replacement, actual);
    }
    match (&pattern.kind, &actual.kind) {
        (ExprKind::Ident(a), ExprKind::Ident(b)) => a.name == b.name,
        (ExprKind::IntLit { value: a, .. }, ExprKind::IntLit { value: b, .. }) => a == b,
        (ExprKind::BoolLit(a), ExprKind::BoolLit(b)) => a == b,
        (ExprKind::StrLit(a), ExprKind::StrLit(b)) => a == b,
        (ExprKind::Paren(a), _) => expr_alpha_eq_subst(a, actual, subst),
        (_, ExprKind::Paren(b)) => expr_alpha_eq_subst(pattern, b, subst),
        (ExprKind::BinOp(oa, la, ra), ExprKind::BinOp(ob, lb, rb)) => {
            oa == ob && expr_alpha_eq_subst(la, lb, subst) && expr_alpha_eq_subst(ra, rb, subst)
        }
        (ExprKind::UnaryOp(oa, a), ExprKind::UnaryOp(ob, b)) => {
            oa == ob && expr_alpha_eq_subst(a, b, subst)
        }
        (
            ExprKind::MethodCall {
                receiver: ra,
                method: ma,
                args: aa,
                ..
            },
            ExprKind::MethodCall {
                receiver: rb,
                method: mb,
                args: ab,
                ..
            },
        ) => {
            ma.name == mb.name
                && aa.len() == ab.len()
                && expr_alpha_eq_subst(ra, rb, subst)
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(x, y)| expr_alpha_eq_subst(x, y, subst))
        }
        _ => false,
    }
}

/// Plain structural equality of two expressions ignoring spans — used to compare
/// a substituted argument against its use in the test predicate.
fn expr_struct_eq(a: &Expr, b: &Expr) -> bool {
    match (&a.kind, &b.kind) {
        (ExprKind::Ident(x), ExprKind::Ident(y)) => x.name == y.name,
        (ExprKind::IntLit { value: x, .. }, ExprKind::IntLit { value: y, .. }) => x == y,
        (ExprKind::BoolLit(x), ExprKind::BoolLit(y)) => x == y,
        (ExprKind::StrLit(x), ExprKind::StrLit(y)) => x == y,
        (ExprKind::Paren(x), _) => expr_struct_eq(x, b),
        (_, ExprKind::Paren(y)) => expr_struct_eq(a, y),
        (ExprKind::BinOp(oa, la, ra), ExprKind::BinOp(ob, lb, rb)) => {
            oa == ob && expr_struct_eq(la, lb) && expr_struct_eq(ra, rb)
        }
        (ExprKind::UnaryOp(oa, x), ExprKind::UnaryOp(ob, y)) => oa == ob && expr_struct_eq(x, y),
        (
            ExprKind::MethodCall {
                receiver: ra,
                method: ma,
                args: aa,
                ..
            },
            ExprKind::MethodCall {
                receiver: rb,
                method: mb,
                args: ab,
                ..
            },
        ) => {
            ma.name == mb.name
                && aa.len() == ab.len()
                && expr_struct_eq(ra, rb)
                && aa.iter().zip(ab.iter()).all(|(x, y)| expr_struct_eq(x, y))
        }
        _ => false,
    }
}

/// v0.114: the recursion cap for property-binding generability (mirrors the
/// checker's `MOCK_DEPTH` for bare `Val`).
const PROP_GEN_DEPTH: u32 = 12;

/// Whether a `for all x: T` binding's type is refinement-generable: refined
/// types must not carry a `Matches` predicate (no refinement-driven generator),
/// and sums/records must have every component recursively generable within the
/// depth cap. Mirrors the checker's `can_mock_bare`.
fn prop_binding_generable(ty: &checker::Ty, types: &HashMap<String, TypeDecl>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match ty {
        checker::Ty::Base(_) => true,
        checker::Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return false;
            };
            match &decl.body {
                TypeBody::Refined { refinement, .. } | TypeBody::Opaque { refinement, .. } => {
                    !refinement.as_ref().is_some_and(|r| {
                        r.predicates
                            .iter()
                            .any(|p| matches!(p.kind, PredKind::Matches(_)))
                    })
                }
                TypeBody::Sum(s) => s.variants.first().is_some_and(|v| {
                    v.payload.iter().all(|f| {
                        checker::resolve_type_ref(&f.type_ref, types)
                            .is_some_and(|t| prop_binding_generable(&t, types, depth - 1))
                    })
                }),
                TypeBody::Record(r) => r.fields.iter().all(|f| {
                    checker::resolve_type_ref(&f.type_ref, types)
                        .is_some_and(|t| prop_binding_generable(&t, types, depth - 1))
                }),
            }
        }
        _ => false,
    }
}

/// The refinement of a resolved refined/opaque named type, if any — used by the
/// conservative restates-refinement check.
fn named_refinement<'a>(
    ty: &checker::Ty,
    types: &'a HashMap<String, TypeDecl>,
) -> Option<&'a Refinement> {
    let checker::Ty::Named { name, .. } = ty else {
        return None;
    };
    match &types.get(name)?.body {
        TypeBody::Refined { refinement, .. } | TypeBody::Opaque { refinement, .. } => {
            refinement.as_ref()
        }
        _ => None,
    }
}

/// v0.114 (DECISION P): does `pred` merely restate a refinement `bound_var`
/// already guarantees? A **conservative, syntactic** check — it fires only when
/// the predicate is exactly the refinement over the bound variable, never
/// guessing (under-flagging is acceptable; over-flagging is not). Handles the
/// `Positive` (`v > 0` / `v >= 1`) and `NonNegative` (`v >= 0`) numeric cases.
fn predicate_restates_refinement(pred: &Expr, bound_var: &str, refinement: &Refinement) -> bool {
    let ExprKind::BinOp(op, lhs, rhs) = &pred.kind else {
        return false;
    };
    // `<var> <op> <int-literal>` only.
    let ExprKind::Ident(id) = &lhs.kind else {
        return false;
    };
    if id.name != bound_var {
        return false;
    }
    let ExprKind::IntLit { value: n, .. } = &rhs.kind else {
        return false;
    };
    let n = *n;
    let positive = refinement
        .predicates
        .iter()
        .any(|p| matches!(p.kind, PredKind::Positive));
    let non_negative = refinement
        .predicates
        .iter()
        .any(|p| matches!(p.kind, PredKind::NonNegative));
    match op {
        // `v > 0` / `v >= 1` restate `Positive`.
        BinOp::Gt if n == 0 => positive,
        BinOp::GtEq if n == 1 => positive,
        // `v >= 0` restates `NonNegative`.
        BinOp::GtEq if n == 0 => non_negative,
        _ => false,
    }
}

/// v0.119 (DECISION D): which state-projection rewrite maps a history predicate
/// back into the space an `invariant` / `transition` is written in.
#[derive(Clone, Copy)]
enum HistoryRestate {
    /// An `invariant` reads bare state fields: `s.new.F` ≡ `F`.
    Invariant,
    /// A `transition` reads `old` / `new`: `s.old` ≡ `old`, `s.new` ≡ `new`.
    Transition,
}

/// `Some(field)` when `e` is `s.new.<field>` (the reached-state projection an
/// invariant-restating history predicate uses).
fn as_new_field<'a>(e: &'a Expr, s: &str) -> Option<&'a str> {
    let ExprKind::FieldAccess { receiver, field } = &e.kind else {
        return None;
    };
    let ExprKind::FieldAccess {
        receiver: inner,
        field: which,
    } = &receiver.kind
    else {
        return None;
    };
    let ExprKind::Ident(id) = &inner.kind else {
        return None;
    };
    (id.name == s && which.name == "new").then_some(field.name.as_str())
}

/// `Some("old"|"new")` when `e` is `s.old` / `s.new` (the step projections a
/// transition-restating history predicate uses).
fn as_step_root<'a>(e: &'a Expr, s: &str) -> Option<&'a str> {
    let ExprKind::FieldAccess { receiver, field } = &e.kind else {
        return None;
    };
    let ExprKind::Ident(id) = &receiver.kind else {
        return None;
    };
    (id.name == s && (field.name == "old" || field.name == "new")).then_some(field.name.as_str())
}

/// Conservative, span-insensitive structural match (DECISION D): does the history
/// predicate `body` (over the step binding `s`) restate the declared predicate
/// `decl`, modulo the `mode` state-projection rewrite? Under-flags by design — any
/// construct not modelled here compares unequal, so a valid test is never blocked.
fn history_pred_matches(body: &Expr, s: &str, decl: &Expr, mode: HistoryRestate) -> bool {
    // Leaf equivalences the rewrite establishes.
    match mode {
        HistoryRestate::Invariant => {
            if let (Some(f), ExprKind::Ident(id)) = (as_new_field(body, s), &decl.kind) {
                return f == id.name;
            }
        }
        HistoryRestate::Transition => {
            if let (Some(root), ExprKind::Ident(id)) = (as_step_root(body, s), &decl.kind) {
                return root == id.name;
            }
        }
    }
    match (&body.kind, &decl.kind) {
        (ExprKind::Paren(x), _) => history_pred_matches(x, s, decl, mode),
        (_, ExprKind::Paren(y)) => history_pred_matches(body, s, y, mode),
        (ExprKind::IntLit { value: x, .. }, ExprKind::IntLit { value: y, .. }) => x == y,
        (ExprKind::BoolLit(x), ExprKind::BoolLit(y)) => x == y,
        (ExprKind::StrLit(x), ExprKind::StrLit(y)) => x == y,
        (ExprKind::Ident(x), ExprKind::Ident(y)) => x.name == y.name,
        (ExprKind::None, ExprKind::None) => true,
        (ExprKind::Some(x), ExprKind::Some(y)) => history_pred_matches(x, s, y, mode),
        (ExprKind::UnaryOp(o1, x), ExprKind::UnaryOp(o2, y)) => {
            o1 == o2 && history_pred_matches(x, s, y, mode)
        }
        (ExprKind::BinOp(o1, l1, r1), ExprKind::BinOp(o2, l2, r2)) => {
            o1 == o2
                && history_pred_matches(l1, s, l2, mode)
                && history_pred_matches(r1, s, r2, mode)
        }
        (
            ExprKind::FieldAccess {
                receiver: r1,
                field: f1,
            },
            ExprKind::FieldAccess {
                receiver: r2,
                field: f2,
            },
        ) => f1.name == f2.name && history_pred_matches(r1, s, r2, mode),
        (
            ExprKind::MethodCall {
                receiver: r1,
                method: m1,
                args: a1,
                ..
            },
            ExprKind::MethodCall {
                receiver: r2,
                method: m2,
                args: a2,
                ..
            },
        ) => {
            m1.name == m2.name
                && a1.len() == a2.len()
                && history_pred_matches(r1, s, r2, mode)
                && a1
                    .iter()
                    .zip(a2)
                    .all(|(x, y)| history_pred_matches(x, s, y, mode))
        }
        (
            ExprKind::Call {
                name: n1, args: a1, ..
            },
            ExprKind::Call {
                name: n2, args: a2, ..
            },
        ) => {
            n1.name == n2.name
                && a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2)
                    .all(|(x, y)| history_pred_matches(x, s, y, mode))
        }
        _ => false,
    }
}

/// v0.119 (DECISION D): a history property that merely restates a snapshot/step
/// invariant is redundant — the driver only commits states the invariants already
/// admit. Recognise the canonical shape `for all run: History[A] { expect
/// run.all((s) => P) }` (or `.any`) whose `P` α-matches a declared
/// `invariant` (over `s.new`) or `transition` (over `s.old`/`s.new`). Returns the
/// body span to flag. Conservative — near-duplicates slip through by design.
fn history_restates_invariant(prop: &PropertyDecl, run_var: &str, agent: &AgentDecl) -> bool {
    let [stmt] = prop.forall.body.statements.as_slice() else {
        return false;
    };
    let Statement::Expect(e) = stmt else {
        return false;
    };
    // `run.all((s) => P)` / `run.any((s) => P)`.
    let ExprKind::MethodCall {
        receiver,
        method,
        args,
        ..
    } = &e.value.kind
    else {
        return false;
    };
    if method.name != "all" && method.name != "any" {
        return false;
    }
    let ExprKind::Ident(recv) = &receiver.kind else {
        return false;
    };
    if recv.name != run_var {
        return false;
    }
    let [arg] = args.as_slice() else {
        return false;
    };
    let ExprKind::Lambda(lam) = &arg.kind else {
        return false;
    };
    let [param] = lam.params.as_slice() else {
        return false;
    };
    let s = &param.name.name;
    agent
        .invariants
        .iter()
        .any(|inv| history_pred_matches(&lam.body, s, &inv.predicate, HistoryRestate::Invariant))
        || agent
            .transitions
            .iter()
            .any(|tr| history_pred_matches(&lam.body, s, &tr.predicate, HistoryRestate::Transition))
}

/// v0.119 (ADR 0155): the synthetic type names a `History[Agent]` binding
/// registers — a call sum, a step record, and a state record — all keyed off the
/// agent name so distinct agents never collide.
fn history_call_type_name(agent: &str) -> String {
    format!("__History_{agent}_Call")
}
fn history_step_type_name(agent: &str) -> String {
    format!("__History_{agent}_Step")
}
fn history_state_type_name(agent: &str) -> String {
    format!("__History_{agent}_State")
}

/// The `.call` variant tag for a handler: the handler name with its first letter
/// upper-cased (`spend` → `Spend`, `topUp` → `TopUp`). The reader matches this
/// with `is` / `match` (`s.call is Spend`).
pub(crate) fn history_variant_name(handler: &str) -> String {
    let mut chars = handler.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => handler.to_string(),
    }
}

/// The agent's drivable `on call` handlers — the ones a history sequences. Other
/// handler kinds (`http`/`cron`/`message`/`open`/`close`) are not RPC entry points
/// and are never part of a generated call-history.
fn history_handlers(agent: &AgentDecl) -> Vec<&Handler> {
    agent
        .handlers
        .iter()
        .filter(|h| matches!(h.kind, HandlerKind::Call) && h.method_name.is_some())
        .collect()
}

/// v0.119: `Some((run_var, agent_name))` when `prop` is a history property — its
/// single `for all` binding is `run: History[Agent]`.
fn prop_history_binding(prop: &PropertyDecl) -> Option<(&str, &str)> {
    prop.forall.bindings.iter().find_map(|b| match &b.type_ref {
        TypeRef::History(inner, _) => match &**inner {
            TypeRef::Named(id) => Some((b.name.name.as_str(), id.name.as_str())),
            _ => None,
        },
        _ => None,
    })
}

fn prop_is_history(prop: &PropertyDecl) -> bool {
    prop_history_binding(prop).is_some()
}

/// v0.119 (testing track slice 7, ADR 0155): type-check a `for all run:
/// History[Agent]` binding. The subject is a *run* of the agent — a generated,
/// driven call-history — bound as an ordinary `List[Step]`. Validates the
/// DECISION-B rules (agent-only, every handler parameter generable), registers the
/// synthetic call-sum / step / state record types into `resolved.types` so the
/// predicate's `List` + value surface (`.call is …`, `.old`/`.new`, `.accepted`)
/// type-checks, and returns the bound `List[Step]` type.
fn check_history_binding(
    inner: &TypeRef,
    span: Span,
    resolved: &mut ResolvedCommons,
    refs: &mut RefSink,
) -> Result<checker::Ty, CompileError> {
    // DECISION B: only an agent has handlers to sequence and reachable states to
    // observe. `History[Value]` / `History[List[…]]` is `not_an_agent`.
    let TypeRef::Named(agent_id) = inner else {
        return Err(CompileError::new(
            "bynk.history.not_an_agent",
            span,
            format!(
                "`for all` cannot generate `History[{}]` — only an agent has handlers to sequence",
                ts_type_ref_display(inner)
            ),
        )
        .with_note("generate a driven call-history over an agent: `for all run: History[Agent]`"));
    };
    let Some(agent) = resolved.agents.get(&agent_id.name).cloned() else {
        return Err(CompileError::new(
            "bynk.history.not_an_agent",
            span,
            format!(
                "`for all run: History[{}]` names `{}`, which is not an agent in scope",
                agent_id.name, agent_id.name
            ),
        )
        .with_note(
            "only an agent (with handlers and reachable state) can be driven as a history",
        ));
    };
    refs.record(agent_id.span, SymbolKind::Type, &agent_id.name);

    let handlers = history_handlers(&agent);
    // DECISION B: the agent must be *drivable* — every handler parameter must be
    // refinement-generable (the same rule a value `for all` binding obeys), else
    // the runner cannot synthesise a call.
    for h in &handlers {
        for p in &h.params {
            let generable = checker::resolve_type_ref(&p.type_ref, &resolved.types)
                .is_some_and(|t| prop_binding_generable(&t, &resolved.types, PROP_GEN_DEPTH));
            if !generable {
                return Err(CompileError::new(
                    "bynk.history.not_generable",
                    span,
                    format!(
                        "`History[{}]` cannot be driven — handler `{}`'s parameter `{}: {}` is not generable (e.g. a `Matches` refinement)",
                        agent_id.name,
                        h.method_name.as_ref().map(|m| m.name.as_str()).unwrap_or(""),
                        p.name.name,
                        ts_type_ref_display(&p.type_ref),
                    ),
                )
                .with_note(
                    "every handler parameter must be refinement-generable for the run to be seeded",
                ));
            }
        }
    }

    // Register the synthetic types (mirrors `register_call_record_types`). The
    // driver returns plain objects of exactly these shapes; the checker sees them
    // as ordinary record/sum types so `is`, field access, and `implies` apply
    // unchanged (the typed-step shape resolving the track's open question).
    let state_name = history_state_type_name(&agent_id.name);
    let call_name = history_call_type_name(&agent_id.name);
    let step_name = history_step_type_name(&agent_id.name);

    // `<Agent>State` — the agent's `Cell` fields, exactly as the emitted state
    // record (so `.old.balance` / `.new.balance` read a reached state).
    let state_fields: Vec<RecordField> = agent
        .store_fields
        .iter()
        .filter(|f| f.kind.head.name == "Cell" && f.kind.args.len() == 1)
        .map(|f| RecordField {
            name: f.name.clone(),
            type_ref: f.kind.args[0].clone(),
            refinement: None,
            init: None,
            span: f.span,
        })
        .collect();
    resolved.types.insert(
        state_name.clone(),
        TypeDecl {
            type_params: Vec::new(),
            name: Ident {
                name: state_name.clone(),
                span,
            },
            body: TypeBody::Record(RecordBody {
                fields: state_fields,
                span,
            }),
            documentation: None,
            span,
            trivia: Trivia::default(),
        },
    );

    // `.call` — a sum over the agent's handlers, each variant carrying the
    // handler's generated arguments (`Spend { amount }`, `TopUp { amount }`).
    let variants: Vec<Variant> = handlers
        .iter()
        .map(|h| {
            let hname = h.method_name.as_ref().expect("call handler has a name");
            Variant {
                name: Ident {
                    name: history_variant_name(&hname.name),
                    span: hname.span,
                },
                payload: h
                    .params
                    .iter()
                    .map(|p| VariantField {
                        name: p.name.clone(),
                        type_ref: p.type_ref.clone(),
                        span: p.span,
                    })
                    .collect(),
                span: hname.span,
            }
        })
        .collect();
    resolved.types.insert(
        call_name.clone(),
        TypeDecl {
            type_params: Vec::new(),
            name: Ident {
                name: call_name.clone(),
                span,
            },
            body: TypeBody::Sum(SumBody {
                variants,
                embeds: Vec::new(),
                span,
            }),
            documentation: None,
            span,
            trivia: Trivia::default(),
        },
    );

    // A `Step` — the driven edge: which call ran (`.call`), whether it committed
    // (`.accepted`), and the committed `old` → `new` state pair.
    let step_fields = vec![
        RecordField {
            name: Ident {
                name: "call".to_string(),
                span,
            },
            type_ref: TypeRef::Named(Ident {
                name: call_name.clone(),
                span,
            }),
            refinement: None,
            init: None,
            span,
        },
        RecordField {
            name: Ident {
                name: "accepted".to_string(),
                span,
            },
            type_ref: TypeRef::Base(BaseType::Bool, span),
            refinement: None,
            init: None,
            span,
        },
        RecordField {
            name: Ident {
                name: "old".to_string(),
                span,
            },
            type_ref: TypeRef::Named(Ident {
                name: state_name.clone(),
                span,
            }),
            refinement: None,
            init: None,
            span,
        },
        RecordField {
            name: Ident {
                name: "new".to_string(),
                span,
            },
            type_ref: TypeRef::Named(Ident {
                name: state_name.clone(),
                span,
            }),
            refinement: None,
            init: None,
            span,
        },
    ];
    resolved.types.insert(
        step_name.clone(),
        TypeDecl {
            type_params: Vec::new(),
            name: Ident {
                name: step_name.clone(),
                span,
            },
            body: TypeBody::Record(RecordBody {
                fields: step_fields,
                span,
            }),
            documentation: None,
            span,
            trivia: Trivia::default(),
        },
    );

    Ok(checker::Ty::List(Box::new(checker::Ty::Named {
        name: step_name,
        kind: checker::NamedKind::Record,
        args: Vec::new(),
    })))
}

/// v0.114: type-check a generative `property` — its `for all` bindings, the
/// optional `where` filter, and the predicate body — in the target's privileged
/// view. Bindings type each `x: T`; `where`/`expect` predicates type as pure
/// `Bool`; each binding's `T` must be refinement-generable (agents are rejected;
/// a `Matches` type must pin); and the body is flagged if it merely restates a
/// refinement (DECISION P). v0.119: a `for all run: History[Agent]` binding is a
/// driven call-history (the history rung — see [`check_history_binding`]).
#[allow(clippy::too_many_arguments)]
fn check_property_body(
    target_name: &str,
    target_kind: UnitKind,
    prop: &PropertyDecl,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) {
    let Some((mut resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    register_call_record_types(&mut resolved, target_name, unit_tables);
    let _ = target_kind;

    // Bind each `for all x: T` into the predicate scope, checking generability.
    let mut binding_scope: HashMap<String, checker::Ty> = HashMap::new();
    let mut binding_types: Vec<(String, Option<checker::Ty>)> = Vec::new();
    // v0.119: the single `History[Agent]` binding (run-var, agent), for the
    // post-body `restates_invariant` check (DECISION D).
    let mut history_binding: Option<(String, AgentDecl)> = None;
    for b in &prop.forall.bindings {
        // v0.119 (ADR 0155): `for all run: History[Agent]` — the history rung. A
        // driven call-history, bound as an ordinary `List[Step]`.
        if let TypeRef::History(inner, hspan) = &b.type_ref {
            match check_history_binding(inner, *hspan, &mut resolved, refs) {
                Ok(step_ty) => {
                    if let TypeRef::Named(agent_id) = &**inner
                        && let Some(agent) = resolved.agents.get(&agent_id.name)
                    {
                        history_binding = Some((b.name.name.clone(), agent.clone()));
                    }
                    binding_scope.insert(b.name.name.clone(), step_ty.clone());
                    binding_types.push((b.name.name.clone(), Some(step_ty)));
                }
                Err(err) => {
                    errors.push(err);
                    binding_types.push((b.name.name.clone(), None));
                }
            }
            continue;
        }
        // Agents are not a value type — a fabricated state that satisfies every
        // invariant need not be reachable (DECISION P); reject up front.
        if let TypeRef::Named(id) = &b.type_ref
            && resolved.agents.contains_key(&id.name)
        {
            errors.push(
                CompileError::new(
                    "bynk.val.agent_not_generable",
                    b.type_ref.span(),
                    format!(
                        "`for all {}: {}` cannot generate an agent — a fabricated agent state need not be reachable",
                        b.name.name, id.name
                    ),
                )
                .with_note(
                    "generate behaviour over an agent via handler sequences (the history rung), not fabricated states",
                ),
            );
            binding_types.push((b.name.name.clone(), None));
            continue;
        }
        let ty = match checker::resolve_type_ref(&b.type_ref, &resolved.types) {
            Some(t) => {
                record_type_refs_in_property(&b.type_ref, &resolved, refs);
                t
            }
            None => {
                errors.push(CompileError::new(
                    "bynk.val.unknown_type",
                    b.type_ref.span(),
                    format!(
                        "`for all {}: {}` names a type that does not resolve",
                        b.name.name,
                        ts_type_ref_display(&b.type_ref)
                    ),
                ));
                binding_types.push((b.name.name.clone(), None));
                continue;
            }
        };
        if !prop_binding_generable(&ty, &resolved.types, PROP_GEN_DEPTH) {
            errors.push(
                CompileError::new(
                    "bynk.val.needs_pin",
                    b.type_ref.span(),
                    format!(
                        "`for all {}: {}` cannot generate a value (e.g. a `Matches` refinement); a property cannot bind it",
                        b.name.name,
                        ts_type_ref_display(&b.type_ref)
                    ),
                )
                .with_note("supply the witness in a `case` with a pinned `Val[T](...)` instead"),
            );
        }
        binding_scope.insert(b.name.name.clone(), ty.clone());
        binding_types.push((b.name.name.clone(), Some(ty)));
    }

    // Type the `where`/body predicates in the target's privileged view with the
    // bindings in scope — mirroring the `case` body context.
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    let unit_span = prop.span;
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );
    let mut capability_info_map: HashMap<String, checker::CapabilityInfo> = HashMap::new();
    if let Some(table) = unit_tables.get(target_name) {
        for (name, decl) in &table.capabilities {
            let ops = decl
                .ops
                .iter()
                .map(|op| checker::CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| {
                            checker::resolve_type_ref(&p.type_ref, &resolved.types)
                                .unwrap_or(checker::Ty::Unit)
                        })
                        .collect(),
                    param_names: op.params.iter().map(|p| p.name.name.clone()).collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &resolved.types)
                        .unwrap_or(checker::Ty::Unit),
                })
                .collect();
            capability_info_map.insert(
                name.clone(),
                checker::CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            );
        }
    }
    let given_declared: Vec<String> = capability_info_map.keys().cloned().collect();
    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types).unwrap();
    let return_ty_span = prop.span;
    let effectful = matches!(return_ty, checker::Ty::Effect(_));
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    let mut no_requirements = RequirementSink::new();
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        refs,
        hints: &mut no_hints,
        locals: &mut no_locals,
        requirements: &mut no_requirements,
        scopes: vec![binding_scope],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        caps: checker::CapabilityCtx {
            capabilities: capability_info_map.clone(),
            declared_capabilities: capability_info_map,
            given_remaining: given_declared.iter().cloned().collect(),
            given_used: HashSet::new(),
            given_entries: Vec::new(),
            given_anchor: None,
        },
        in_test_body: true,
        test_services: unit_tables
            .get(target_name)
            .map(|t| t.services.keys().cloned().collect())
            .unwrap_or_default(),
        type_vars: std::collections::HashSet::new(),
        store_cells: std::collections::HashMap::new(),
        store_maps: std::collections::HashMap::new(),
        store_sets: std::collections::HashMap::new(),
        store_caches: std::collections::HashMap::new(),
        store_logs: std::collections::HashMap::new(),
    };
    // The optional `where` filter is a pure `Bool` over the bindings.
    if let Some(w) = &prop.forall.where_pred {
        let bool_ty = checker::Ty::Base(BaseType::Bool);
        let t = checker::type_of(w, Some(&bool_ty), &mut ctx);
        if let Some(actual) = t
            && actual.base() != Some(BaseType::Bool)
        {
            ctx.errors.push(CompileError::new(
                "bynk.property.where_not_bool",
                w.span,
                format!(
                    "a `for all ... where` filter has type `{}`, but a `Bool` is required",
                    actual.display()
                ),
            ));
        }
    }
    // The body is the one predicate surface: `expect`s self-check as `Bool`.
    let _ = checker::type_of_block(&prop.forall.body, Some(&return_ty), &mut ctx);

    // Conservative restates-refinement flag: a single-binding property whose
    // body is exactly `expect <pred>` restating the bound var's refinement.
    if let [(var, Some(ty))] = binding_types.as_slice()
        && let Some(refinement) = named_refinement(ty, &resolved.types)
        && let [stmt] = prop.forall.body.statements.as_slice()
        && let Statement::Expect(e) = stmt
        && predicate_restates_refinement(&e.value, var, refinement)
    {
        errors.push(
            CompileError::new(
                "bynk.property.restates_refinement",
                prop.forall.body.span,
                format!(
                    "property `{}` merely re-checks a refinement type `{}` already guarantees",
                    prop.name,
                    ty.display()
                ),
            )
            .with_note(
                "a property earns its keep by asserting behaviour over valid inputs, not by restating the type's refinement",
            ),
        );
    }

    // v0.119 (DECISION D): a history property that merely restates a declared
    // `invariant` / `transition` re-checks a guarantee every reached state already
    // has (the driver only commits admissible states). Conservative — near-
    // duplicates slip through by design.
    if let Some((run_var, agent)) = &history_binding
        && history_restates_invariant(prop, run_var, agent)
    {
        errors.push(
            CompileError::new(
                "bynk.history.restates_invariant",
                prop.forall.body.span,
                format!(
                    "history property `{}` merely re-checks a guarantee agent `{}`'s `invariant`/`transition` already enforces on every reached state",
                    prop.name, agent.name.name
                ),
            )
            .with_note(
                "a history property earns its keep by asserting a cross-step protocol, not by restating a per-state invariant",
            ),
        );
    }
}

/// Record type references named by a `for all` binding so cross-file edges and
/// go-to-definition resolve for a property's generated types.
fn record_type_refs_in_property(
    type_ref: &TypeRef,
    resolved: &bynk_check::resolver::ResolvedCommons,
    refs: &mut RefSink,
) {
    checker::record_type_refs(type_ref, &resolved.types, &HashSet::new(), refs);
}

/// Build a [`resolver::ResolvedCommons`] backed by `owning_unit`'s privileged
/// view: its types, fns, methods, plus types/fns from every commons it
/// `uses`, plus exported types from every consumed context. The same
/// shape used by the production pipeline. Returns the [`ResolvedCommons`]
/// plus a synthetic commons span for the test.
fn build_privileged_resolved(
    owning_unit: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> Option<(bynk_check::resolver::ResolvedCommons, ())> {
    let local = unit_tables.get(owning_unit)?;
    let mut types = local.types.clone();
    let mut fns = local.fns.clone();
    let mut methods = local.methods.clone();
    if let Some(targets) = unit_uses.get(owning_unit) {
        for t in targets {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, d) in &used.fns {
                    fns.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, mt) in &used.methods {
                    let entry = methods.entry(n.clone()).or_default();
                    for (m, decl) in &mt.instance {
                        entry
                            .instance
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                    for (m, decl) in &mt.statics {
                        entry
                            .statics
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                }
            }
        }
    }
    // Consumed-context types come in too (only the exported ones).
    if let Some(consumed) = unit_consumes.get(owning_unit) {
        for t in consumed {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, mt) in &used.methods {
                    let entry = methods.entry(n.clone()).or_default();
                    for (m, decl) in &mt.instance {
                        entry
                            .instance
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                }
            }
        }
    }
    let local_type_names: HashSet<String> = local.types.keys().cloned().collect();
    let cross_context = build_cross_context_info(
        owning_unit,
        unit_consumes,
        unit_consumes_aliases,
        unit_uses,
        unit_tables,
    );
    let synthetic_commons = Commons {
        name: QualifiedName {
            parts: owning_unit
                .split('.')
                .map(|part| Ident {
                    name: part.to_string(),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        },
        items: Vec::new(),
        uses: Vec::new(),
        documentation: None,
        form: CommonsForm::Brace,
        span: Span::default(),
        trivia: Trivia::default(),
        trailing_comments: Vec::new(),
    };
    let agents_for_resolved = unit_tables
        .get(owning_unit)
        .map(|t| t.agents.clone())
        .unwrap_or_default();
    let resolved = bynk_check::resolver::ResolvedCommons {
        commons: synthetic_commons,
        types,
        fns,
        methods,
        local_type_names,
        cross_context,
        agents: agents_for_resolved,
        imported_from: HashMap::new(),
    };
    Some((resolved, ()))
}

/// Emit a single test module TypeScript file plus the [`RunnableTest`]
/// pointer used by the top-level runner.
#[allow(clippy::too_many_arguments)]
/// v0.115 (testing track slice 3): can this contracted free function be attacked
/// by the runner — *a contract is a property that is always on.* The runner
/// generates arguments over the parameters' refinement domains, filters by the
/// `requires` (inputs failing a precondition are discarded, exactly as a
/// `for all … where` does), calls the function, and the dev/test call-site guard
/// asserts each `ensures`, throwing a shrinkable `BynkContractError` on
/// violation. Returns `false` when there is nothing to attack: no `ensures`, no
/// (nameable) parameters, an effectful return (its inputs include the world —
/// dev-guard only), a non-primitive parameter (its generated fields are `bigint`
/// and could mix with `number` arithmetic — dev-guard only), or a parameter the
/// generator cannot inhabit (an over-narrow / `Matches`-pinned refinement) —
/// never a false error, mirroring an over-narrow `where`.
fn is_attackable_contract(f: &FnDecl, resolved: &ResolvedCommons) -> bool {
    if !matches!(&f.name, FnName::Free(_)) {
        return false;
    }
    if f.ensures.is_empty() || f.params.is_empty() {
        return false;
    }
    if matches!(&f.return_type, TypeRef::Effect(_, _)) {
        return false;
    }
    if f.params.iter().any(|p| p.name.name == "_") {
        return false;
    }
    for p in &f.params {
        let Some(ty) = checker::resolve_type_ref(&p.type_ref, &resolved.types) else {
            return false;
        };
        // Restrict to primitive (or refined-over-primitive) parameters: a
        // generated composite carries `bigint` fields that would mix with the
        // function's `number` arithmetic. Composite-param contracts are covered
        // by the dev guard.
        if numeric_or_scalar_base(&ty, &resolved.types).is_none() {
            return false;
        }
        if !prop_binding_generable(&ty, &resolved.types, PROP_GEN_DEPTH) {
            return false;
        }
    }
    true
}

/// The primitive base type a parameter erases to — `Some(base)` for a base type
/// or a refinement/opaque over one, `None` for a composite (record/sum/list/map/
/// option/result/etc.). Used to (a) gate a contract as attackable and (b) decide
/// whether a generated argument needs `Number(…)` coercion before the call
/// (`Int` generates a `bigint`, but functions do `number` arithmetic — v0.114's
/// generator/erasure split, harmless until a generated value flows into a call).
fn numeric_or_scalar_base(ty: &checker::Ty, types: &HashMap<String, TypeDecl>) -> Option<BaseType> {
    match ty {
        checker::Ty::Base(b) => Some(*b),
        checker::Ty::Named { name, .. } => match &types.get(name)?.body {
            TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. } => Some(*base),
            _ => None,
        },
        _ => None,
    }
}

/// v0.115: the target unit's contracted free functions the runner can attack, in
/// deterministic (name) order, paired with the resolved view for gen/coercion.
fn attackable_contracts(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> Option<(ResolvedCommons, Vec<FnDecl>)> {
    let (resolved, _) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    )?;
    let table = unit_tables.get(target_name)?;
    let mut fns: Vec<FnDecl> = table
        .fns
        .values()
        .filter(|f| is_attackable_contract(f, &resolved))
        .cloned()
        .collect();
    fns.sort_by_key(|a| a.name.display());
    Some((resolved, fns))
}

#[allow(clippy::too_many_arguments)]
fn emit_test_module(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    stubs: &HashMap<String, ResolvedStub>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    tests_prefix: &Path,
    import_ext: ImportExt,
    contracts: bool,
) -> Option<(PathBuf, String, Option<String>, RunnableTest)> {
    let _ = exports_visibility;
    let ext = import_ext.as_str();
    let mut out = String::new();
    // v0.70: the test module's source map. A test group can aggregate cases from
    // several `.bynk` files, so the builder is multi-source; each case's body is
    // merged under its own source (registered on first sight in the case loop).
    let mut module_smb = SourceMapBuilder::new();
    let target_ns = target_name.replace('.', "_");
    let target_dir = commons_dir_for(target_name);
    // Output file: tests/<sanitised-target>.test.ts
    let module_path = PathBuf::from(format!("tests/{}.test.ts", target_name.replace('.', "_")));

    out.push_str("// Generated by bynkc — do not edit by hand.\n");
    out.push_str(&format!("// test target: {target_name}\n\n"));

    // Result/Option helpers — same shape as the production runtime imports.
    // The test module lives at `tests/<file>.test.ts`, so the runtime is one
    // directory up. Compute through the same depth machinery used by the
    // per-context emitter. If the target context declares agents, also pull
    // in `makeTestState` so agent invocations can synthesise DO state.
    let has_agents = unit_tables
        .get(target_name)
        .map(|t| !t.agents.is_empty())
        .unwrap_or(false);
    let runtime_import = emitter::runtime_import_for(&module_path, import_ext);
    let extra = if has_agents { ", makeTestState" } else { "" };
    out.push_str(&format!(
        "import {{ Ok, Err, Some, None{extra}, type Result, type Option, type ValidationError }} from \"{runtime_import}\";\n"
    ));

    // Compute relative import path from tests/ to the target's output dir.
    let import_target = relative_import_for_test(&target_dir);
    out.push_str(&format!(
        "import * as {target_ns} from \"./{import_target}.{ext}\";\n"
    ));

    // Consumed contexts (for the target context, if any).
    let mut consumed_imports: Vec<(String, String)> = Vec::new();
    if let Some(consumed) = unit_consumes.get(target_name) {
        for q in consumed {
            let ns = q.replace('.', "_");
            let dir = commons_dir_for(q);
            let import_path = relative_import_for_test(&dir);
            consumed_imports.push((ns, import_path));
        }
    }
    consumed_imports.sort();
    for (ns, path) in &consumed_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.{ext}\";\n"));
    }

    // `uses` commons reachable from the test fragments — needed for `Money`,
    // etc., used inside test bodies. We pull from the target context's uses.
    let mut uses_imports: Vec<(String, String)> = Vec::new();
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            let dir = commons_dir_for(u);
            let import_path = relative_import_for_test(&dir);
            uses_imports.push((ns, import_path));
        }
    }
    uses_imports.sort();
    for (ns, path) in &uses_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.{ext}\";\n"));
    }
    out.push('\n');

    // Expectation helper used by lowered `expect` statements.
    out.push_str(&expectation_runtime_helpers());

    // v0.115: the target unit's contracted free functions the runner can attack
    // (each `ensures` is a generative check that is always on). Emitted only when
    // the contract guard is (the attack relies on the guard to assert `ensures`);
    // `bynkc test` sets both, `bynkc compile` neither.
    let (attack_resolved, attack_fns) = if contracts {
        attackable_contracts(
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        )
        .map(|(r, fns)| (Some(r), fns))
        .unwrap_or((None, Vec::new()))
    } else {
        (None, Vec::new())
    };

    // v0.114: the generative-property runtime — emitted only when this module
    // declares a `property` (or v0.115 attacks a contract), so modules with only
    // `case`s stay byte-for-byte unchanged.
    let has_properties = !attack_fns.is_empty()
        || indices
            .iter()
            .any(|&i| parsed[i].test().is_some_and(|t| !t.properties.is_empty()));
    if has_properties {
        out.push_str(&property_runtime_helpers());
        out.push('\n');
    }
    // v0.119: the history-property runtime — emitted only when a module declares a
    // `for all run: History[Agent]` property, so value-only property modules stay
    // byte-for-byte unchanged.
    let has_history_properties = indices.iter().any(|&i| {
        parsed[i]
            .test()
            .is_some_and(|t| t.properties.iter().any(prop_is_history))
    });
    if has_history_properties {
        out.push_str(&history_runtime_helpers());
        out.push('\n');
    }

    // v0.117: the observation runtime — emitted only when a `case` in this module
    // observes (`Cap.op called …` / `trace(Cap.op)`), so modules without
    // observation stay byte-for-byte unchanged.
    let uses_observation = indices.iter().any(|&i| {
        parsed[i]
            .test()
            .is_some_and(|t| t.cases.iter().any(|c| block_uses_observation(&c.body)))
    });
    if uses_observation {
        out.push_str(&observation_runtime_helpers());
        out.push('\n');
        // The synthetic per-op call-record types a `trace(Cap.op)` result's
        // elements carry — so `trace(…).filter((c) => c.field …)` type-checks
        // against the operation's parameter names.
        out.push_str(&observation_call_record_types(target_name, unit_tables));
        out.push('\n');
    }

    // v0.118: emit one `__Stub_<Cap>` stub class per overridden capability
    // seam, plus the deep-equality helper its arg-pattern matching relies on.
    // Sorted by capability so emission is deterministic regardless of the map's
    // hash iteration order.
    if !stubs.is_empty() {
        out.push_str(&stub_runtime_helpers());
        out.push('\n');
    }
    let mut sorted_stubs: Vec<(&String, &ResolvedStub)> = stubs.iter().collect();
    sorted_stubs.sort_by(|a, b| a.0.cmp(b.0));
    for (_, rp) in sorted_stubs {
        out.push_str(&emit_stub_class(
            rp,
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        ));
        out.push('\n');
    }

    // Emit the deps factory.
    out.push_str(&emit_test_deps(
        target_name,
        target_kind,
        stubs,
        unit_tables,
        unit_consumes,
        unit_consumes_aliases,
    ));
    out.push('\n');

    // Emit one async function per test case. Capture each case's name + source
    // location for `--no-run` discovery as we go (same order the runner reports).
    let mut case_runners: Vec<String> = Vec::new();
    let mut discovered: Vec<DiscoveredCase> = Vec::new();
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        let rel_path = tests_prefix.join(&parsed[i].source_path);
        let rel_path = rel_path.to_string_lossy();
        for case in &test_decl.cases {
            discovered.push(DiscoveredCase {
                name: case.name.clone(),
                location: Some(discovered_location(
                    &parsed[i].source,
                    &rel_path,
                    case.name_span,
                )),
            });
            let runner_name = sanitise_case_name(&case.name, &mut case_runners.len());
            case_runners.push(runner_name.clone());
            // v0.118: record each case's effective tier (its own `as <tier>`, else
            // the suite default, else `unit`). unit and integration share this
            // in-process harness; the tier rides the emitted module for reporting.
            out.push_str(&format!(
                "// case tier: {}\n",
                super::discovery::case_effective_tier(case, test_decl).as_str()
            ));
            let (case_text, case_smb) = emit_test_case_function(
                &runner_name,
                case,
                target_name,
                target_kind,
                stubs,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &parsed[i].source,
                &rel_path,
            );
            // v0.70: merge this case's body checkpoints into the module map under
            // the case's `.bynk` source (a test group can span several files).
            let base = out.len();
            out.push_str(&case_text);
            // Forward slashes so the map's `sources` are portable (Windows joins
            // with `\`), matching the emitter's other specifier rendering.
            // v0.72: the map `source` is the file's absolute path (not the
            // project-relative `rel_path`, which a debugger would resolve against
            // the emitted `.ts`'s dir) so an editor breakpoint on the real
            // `.bynk` test file binds.
            let src_id =
                module_smb.add_source(parsed[i].map_source_name(), parsed[i].source.clone());
            module_smb.merge(&case_smb, &case_text, &out, base, src_id);
            out.push('\n');
        }
    }

    // v0.114: emit one async runner per generative `property`. Each property's
    // seed derives from the run's root seed via a stable ordinal, so a run
    // reproduces byte-for-byte under `--seed`.
    let mut prop_runners: Vec<String> = Vec::new();
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        let rel_path = tests_prefix.join(&parsed[i].source_path);
        let rel_path = rel_path.to_string_lossy();
        for prop in &test_decl.properties {
            discovered.push(DiscoveredCase {
                name: prop.name.clone(),
                location: Some(discovered_location(
                    &parsed[i].source,
                    &rel_path,
                    prop.name_span,
                )),
            });
            let mut idx = prop_runners.len();
            let runner_name = format!("__prop_{}", sanitise_case_name(&prop.name, &mut idx));
            let prop_ordinal = prop_runners.len();
            prop_runners.push(runner_name.clone());
            // v0.119: a `for all run: History[Agent]` property routes to the driven-
            // sequence runner; a value property keeps the existing path.
            let prop_text = if prop_is_history(prop) {
                emit_test_history_property_function(
                    &runner_name,
                    prop,
                    prop_ordinal,
                    target_name,
                    target_kind,
                    unit_tables,
                    unit_uses,
                    unit_consumes,
                    unit_consumes_aliases,
                    &parsed[i].source,
                    &rel_path,
                )
            } else {
                emit_test_property_function(
                    &runner_name,
                    prop,
                    prop_ordinal,
                    target_name,
                    target_kind,
                    unit_tables,
                    unit_uses,
                    unit_consumes,
                    unit_consumes_aliases,
                    &parsed[i].source,
                    &rel_path,
                )
            };
            out.push_str(&prop_text);
            out.push('\n');
        }
    }

    // v0.115: emit one async runner per contract attack. Each shares the property
    // runtime and the run's seed lineage, so contract failures shrink and
    // reproduce exactly like a hand-written `property`. The report location uses a
    // representative test file of the target (the attack has no source line).
    let rep_rel_path: String = indices
        .iter()
        .find_map(|&i| {
            parsed[i].test().map(|_| {
                tests_prefix
                    .join(&parsed[i].source_path)
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .unwrap_or_default();
    for f in &attack_fns {
        let FnName::Free(fname) = &f.name else {
            continue;
        };
        let attack_name = format!("contract {}", fname.name);
        discovered.push(DiscoveredCase {
            name: attack_name.clone(),
            location: None,
        });
        let mut idx = prop_runners.len();
        let runner_name = format!("__prop_{}", sanitise_case_name(&attack_name, &mut idx));
        let prop_ordinal = prop_runners.len();
        prop_runners.push(runner_name.clone());
        let attack_text = emit_contract_attack_function(
            &runner_name,
            f,
            attack_resolved.as_ref().unwrap(),
            prop_ordinal,
            target_name,
            target_kind,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
            &rep_rel_path,
        );
        out.push_str(&attack_text);
        out.push('\n');
    }

    // Module-level runner. v0.127: `only` filters to a single case/property by
    // name (the per-case run lens); undefined runs every one.
    out.push_str("export async function run(only?: string) {\n");
    out.push_str("  const results = [];\n");
    out.push_str("  const want = (n: string): boolean => only === undefined || only === n;\n");
    let mut case_index = 0;
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            let runner_name = &case_runners[case_index];
            let escaped = emitter::escape_ts_string(&case.name);
            out.push_str(&format!(
                "  if (want(\"{escaped}\")) results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
            ));
            case_index += 1;
        }
    }
    let mut prop_index = 0;
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for prop in &test_decl.properties {
            let runner_name = &prop_runners[prop_index];
            let escaped = emitter::escape_ts_string(&prop.name);
            out.push_str(&format!(
                "  if (want(\"{escaped}\")) results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
            ));
            prop_index += 1;
        }
    }
    // v0.115: run the contract attacks (they follow every source-declared
    // property in `prop_runners`, so the index continues).
    for f in &attack_fns {
        let FnName::Free(fname) = &f.name else {
            continue;
        };
        let runner_name = &prop_runners[prop_index];
        let escaped = emitter::escape_ts_string(&format!("contract {}", fname.name));
        out.push_str(&format!(
            "  if (want(\"{escaped}\")) results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
        ));
        prop_index += 1;
    }
    out.push_str("  return results;\n");
    out.push_str("}\n");

    let module_file = module_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module.ts".to_string());
    let source_map = module_smb.to_v3(&out, &module_file);
    Some((
        module_path.clone(),
        out,
        source_map,
        RunnableTest {
            target_name: target_name.to_string(),
            module_path,
            kind: "unit",
            suite_name: target_name.to_string(),
            cases: discovered,
        },
    ))
}

/// v0.132: an aggregating barrel for a commons split across a directory.
///
/// Production emits a multi-file commons per file (`out/<name>/*.ts`) and never
/// an aggregate `out/<name>.ts`, but every test/integration import references the
/// commons as one namespace (`import * as ns from "./<name>.js"`). Emit
/// `out/<name>.ts` that `export *`s each of the commons' source files so that
/// namespace import resolves for the directory layout exactly as it does for a
/// single file. The flat merge cannot collide: intra-commons symbol names are
/// already unique across every kind (`bynk.resolve.duplicate_type`/`_fn`/
/// `_method`), so no two files re-export the same name.
///
/// Returns `None` — no barrel — when the commons is *not* multi-file (a
/// single-file commons already owns `out/<name>.ts`; a barrel would collide), or
/// when this barrel path was already emitted (`emitted` dedups across the several
/// test/integration modules that may import the same commons).
///
/// `groups` maps each production unit name to the `parsed` indices of its files;
/// the multi-file predicate and the re-exported file set both read from there, so
/// the barrel can never drop a file that declares nothing type/fn/method-shaped
/// (which `FileDeclIndex` would omit).
fn emit_commons_barrel(
    name: &str,
    groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    import_ext: ImportExt,
    emitted: &mut HashSet<PathBuf>,
) -> Option<CompiledFile> {
    let indices = groups.get(name)?;
    // Multi-file only: *every* file must sit under a `<name>/` directory, the
    // layout where no `out/<name>.ts` is otherwise produced. A unit with any file
    // at `<name>.bynk` already owns `out/<name>.ts`, so it must not get a barrel.
    if indices.is_empty()
        || !indices
            .iter()
            .all(|&i| is_multi_file_layout(&parsed[i].source_path, name))
    {
        return None;
    }
    let output_path = commons_dir_for(name).with_extension("ts");
    if !emitted.insert(output_path.clone()) {
        return None; // already emitted for an earlier importing module
    }
    // Barrel body: `export *` per distinct source file, sorted for determinism.
    // Specifiers run through the emitter's cross-commons machinery so the path is
    // correct for dotted names (`commons a.b` sits at `out/a/b.ts`, re-exporting
    // `./b/<file>.js`) and forward-slash-normalised on Windows. `commons_dir_for`
    // stands in for the barrel's own location — its parent is the directory the
    // barrel lives in, which is all the relative computation needs.
    let barrel_loc = commons_dir_for(name);
    let mut files: Vec<PathBuf> = indices
        .iter()
        .map(|&i| parsed[i].source_path.clone())
        .collect();
    files.sort();
    files.dedup();
    let mut body = String::from("// Generated by bynkc — do not edit by hand.\n");
    for file in &files {
        let spec = emitter::cross_commons_import_specifier_for_path(&barrel_loc, file, import_ext);
        body.push_str(&format!("export * from \"{spec}\";\n"));
    }
    Some(CompiledFile {
        source_path: barrel_loc.with_extension("bynk"),
        output_path,
        typescript: body,
        source_map: None,
        debug_metadata: None,
    })
}

/// Render the relative import path from the `tests/` output directory to the
/// directory holding a target unit's TypeScript output.
fn relative_import_for_test(target_dir: &Path) -> String {
    let parts: Vec<String> = target_dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        "../index".to_string()
    } else {
        format!("../{}", parts.join("/"))
    }
}

fn expectation_runtime_helpers() -> String {
    let mut out = String::new();
    // Fields are declared and assigned explicitly rather than via TS parameter
    // properties: parameter properties are a transform-only construct that Node's
    // strip-only type-stripping rejects (ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX), and
    // `bynkc test --inspect` runs this `.ts` directly under strip-only Node (slice
    // 2, ADR 0104). The explicit form is equivalent and strip-clean.
    out.push_str("class ExpectationError extends Error {\n");
    out.push_str("  location: string;\n");
    out.push_str("  start: number;\n");
    out.push_str("  end: number;\n");
    out.push_str("  constructor(location: string, start: number, end: number, detail: string) {\n");
    out.push_str("    super(`${detail}\\n  at ${location}`);\n");
    out.push_str("    this.location = location;\n");
    out.push_str("    this.start = start;\n");
    out.push_str("    this.end = end;\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(
        "function __bynkExpectFailure(location: string, start: number, end: number, detail: string) {\n",
    );
    out.push_str("  return new ExpectationError(location, start, end, detail);\n");
    out.push_str("}\n");
    out.push_str(
        "function __bynkExpect(cond: boolean, location: string, start: number, end: number, detail: string): void {\n",
    );
    out.push_str("  if (!cond) { throw __bynkExpectFailure(location, start, end, detail); }\n");
    out.push_str("}\n");
    // v0.112: render a runtime value for the expected-vs-actual failure report.
    out.push_str("function __bynkShow(v: unknown): string {\n");
    out.push_str(
        "  try { return typeof v === \"bigint\" ? String(v) : (JSON.stringify(v) ?? String(v)); } catch { return String(v); }\n",
    );
    out.push_str("}\n\n");
    out
}

/// v0.118: the runtime helper the `__Stub_<Cap>` stubs rely on — a
/// structural deep-equality over lowered argument patterns (bigint-safe, since
/// `Int` erases to `bigint` and `JSON.stringify` rejects it raw).
fn stub_runtime_helpers() -> String {
    let mut out = String::new();
    out.push_str("function __bynkDeepEqual(a: unknown, b: unknown): boolean {\n");
    out.push_str(
        "  const s = (v: unknown) => JSON.stringify(v, (_k, val) => typeof val === \"bigint\" ? \"__bigint__\" + String(val) : val);\n",
    );
    out.push_str("  try { return s(a) === s(b); } catch { return a === b; }\n");
    out.push_str("}\n");
    out
}

/// v0.118: emit the `__Stub_<Cap>` stub class for a capability seam
/// overridden by `stub` clauses (testing track slice 6). One async method
/// per overridden operation renders its clauses as a first-match-wins if-chain
/// over the call's argument patterns; a matched clause returns its lowered
/// value, throws an injected fault, or advances a per-call sequence cursor.
fn emit_stub_class(
    rp: &ResolvedStub,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let mut out = String::new();
    let cap = &rp.cap;
    // Value expressions are lowered in the target context's privileged view, so
    // its types, variants and `uses` vocabulary resolve unqualified.
    let owning_unit = target_name.to_string();
    let scope_ns = owning_unit.replace('.', "_");
    let mut scope_type_names: HashSet<String> = unit_tables
        .get(&owning_unit)
        .map(|t| t.types.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(used) = unit_uses.get(&owning_unit) {
        for u in used {
            if let Some(table) = unit_tables.get(u) {
                scope_type_names.extend(table.types.keys().cloned());
            }
        }
    }
    let scope_names: Vec<String> = if let Some(table) = unit_tables.get(&owning_unit) {
        let mut v: Vec<String> = table
            .types
            .keys()
            .chain(table.fns.keys())
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    } else {
        Vec::new()
    };

    // Group clause indices by method, preserving resolution order (case-scoped
    // clauses precede suite-scoped ones, so they win the first-match chain).
    let mut by_method: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (idx, clause) in rp.clauses.iter().enumerate() {
        by_method
            .entry(clause.method.name.clone())
            .or_default()
            .push(idx);
    }

    out.push_str(&format!("class __Stub_{cap} {{\n"));
    // One per-call sequence cursor field per `returns each` clause.
    for (idx, clause) in rp.clauses.iter().enumerate() {
        if matches!(clause.rhs, StubRhs::ReturnsEach(..)) {
            out.push_str(&format!("  __seq_{idx} = 0;\n"));
        }
    }

    for (method, clause_idxs) in &by_method {
        let Some(op) = rp.cap_decl.ops.iter().find(|o| &o.name.name == method) else {
            continue;
        };
        let params = op
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}: {}",
                    p.name.name,
                    emitter::ts_type_ref_qualified(&p.type_ref, &scope_type_names, &scope_ns)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_ty =
            emitter::ts_type_ref_qualified(&op.return_type, &scope_type_names, &scope_ns);
        out.push_str(&format!("  async {method}({params}): {return_ty} {{\n"));
        if !scope_names.is_empty() {
            out.push_str(&format!(
                "    const {{ {} }} = {scope_ns} as any;\n",
                scope_names.join(", ")
            ));
        }
        for &idx in clause_idxs {
            let clause = &rp.clauses[idx];
            // Argument-pattern consts: a `Value(e)` pattern lowers to a const the
            // condition compares structurally; an `Any` pattern contributes none.
            let mut cond_parts: Vec<String> = Vec::new();
            for (i, pat) in clause.args.iter().enumerate() {
                if let ArgPattern::Value(e) = pat
                    && let Some(param) = op.params.get(i)
                {
                    let body = lower_stub_value_block(
                        e,
                        &param.type_ref,
                        &[],
                        target_name,
                        unit_tables,
                        unit_uses,
                        unit_consumes,
                        unit_consumes_aliases,
                    );
                    let vname = format!("__pv_{idx}_{i}");
                    out.push_str(&format!("    const {vname} = await (async () => {{\n"));
                    for line in body.lines() {
                        out.push_str("      ");
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push_str("    })();\n");
                    cond_parts.push(format!("__bynkDeepEqual({}, {vname})", param.name.name));
                }
            }
            let cond = if cond_parts.is_empty() {
                "true".to_string()
            } else {
                cond_parts.join(" && ")
            };
            out.push_str(&format!("    if ({cond}) {{\n"));
            let rhs_body = emit_stub_rhs(
                clause,
                idx,
                op,
                target_name,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
            );
            for line in rhs_body.lines() {
                out.push_str("      ");
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("    }\n");
        }
        out.push_str(&format!(
            "    throw new Error(\"bynk: no stub clause matched for {cap}.{method}\");\n"
        ));
        out.push_str("  }\n");
    }
    out.push_str("}\n");
    out
}

/// v0.118: render the body of one matched `stub` clause — a `returns` value,
/// a `fails` fault, or a `returns each` per-call sequence (last outcome repeats).
#[allow(clippy::too_many_arguments)]
fn emit_stub_rhs(
    clause: &StubClause,
    clause_idx: usize,
    op: &CapabilityOp,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    const FAULT: &str = "throw new Error(\"bynk: injected capability fault (stubs … fails)\");";
    let lower = |e: &Expr| {
        lower_stub_value_block(
            e,
            &op.return_type,
            &op.params,
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        )
    };
    match &clause.rhs {
        StubRhs::Returns(e) => lower(e),
        StubRhs::Fails(_) => format!("{FAULT}\n"),
        StubRhs::ReturnsEach(outcomes, _) => {
            let n = outcomes.len();
            let mut out = String::new();
            out.push_str(&format!("const __k = this.__seq_{clause_idx};\n"));
            if n > 1 {
                out.push_str(&format!(
                    "if (this.__seq_{clause_idx} < {}) this.__seq_{clause_idx}++;\n",
                    n - 1
                ));
            }
            let outcome_body = |o: &SeqOutcome| match o {
                SeqOutcome::Value(e) => lower(e),
                SeqOutcome::Fails(_) => format!("{FAULT}\n"),
            };
            out.push_str("switch (__k) {\n");
            for (j, o) in outcomes.iter().enumerate().take(n.saturating_sub(1)) {
                out.push_str(&format!("  case {j}: {{\n"));
                for line in outcome_body(o).lines() {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("  }\n");
            }
            out.push_str("  default: {\n");
            for line in outcome_body(&outcomes[n - 1]).lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("  }\n");
            out.push_str("}\n");
            out
        }
    }
}

/// v0.118: lower a single `stub` value expression as if it were a provider
/// operation's tail — type-check it in the target's privileged view (so variant
/// constructors and `uses` names resolve) then lower it to an async body ending
/// in `return <value>;`. Mirrors the retired mock-op-body lowering.
#[allow(clippy::too_many_arguments)]
fn lower_stub_value_block(
    e: &Expr,
    ret_type: &TypeRef,
    params: &[Param],
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let owning_unit = target_name.to_string();
    let mut typed = synthetic_typed_commons_for_target(&owning_unit, unit_tables, unit_uses);
    let block = value_block(e);
    if let Some((resolved, _)) = build_privileged_resolved(
        &owning_unit,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        let mut errs: Vec<CompileError> = Vec::new();
        // Build-mode re-check for the lowering's expr types; the analyse exit has
        // already passed, so nothing records (fresh sink).
        checker::check_handler_body(
            &resolved,
            checker::HandlerBodyCheck::new(&block, ret_type, params, &[]),
            checker::CheckSinks {
                expr_types: &mut typed.expr_types,
                errors: &mut errs,
                refs: &mut RefSink::new(),
                hints: &mut HintSink::new(),
                locals: &mut LocalsSink::new(),
                requirements: &mut RequirementSink::new(),
            },
        );
    }
    let cross = bynk_check::resolver::CrossContextInfo::default();
    // v0.70: `stub` value scaffolding is not user test logic, so its source
    // map is discarded — it stays unmapped (a deliberate scope cut).
    emitter::lower_block_to_async_body(&block, ret_type, &mut typed, &cross).0
}

fn synthetic_typed_commons_for_target(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> checker::TypedCommons {
    let table = unit_tables.get(target_name).cloned().unwrap_or_default();
    let mut types = table.types;
    let mut fns = table.fns;
    let mut methods = table.methods;
    // v0.117: carry the target's capability declarations into the synthetic
    // commons items so observation lowering (`with` param destructure,
    // `trace(Cap.op)` record fields) can look up each op's parameter names.
    let capability_items: Vec<CommonsItem> = {
        let mut caps: Vec<&String> = table.capabilities.keys().collect();
        caps.sort();
        caps.into_iter()
            .map(|c| CommonsItem::Capability(table.capabilities[c].clone()))
            .collect()
    };
    // Pull in names that come into scope via the target's `uses` clauses, so
    // the test-body lowering's static-call check (`<Type>.of(...)` etc.)
    // resolves against the same set of names the source can mention.
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            if let Some(t) = unit_tables.get(u) {
                for (n, d) in &t.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, f) in &t.fns {
                    fns.entry(n.clone()).or_insert_with(|| f.clone());
                }
                for (n, mt) in &t.methods {
                    let entry = methods.entry(n.clone()).or_default();
                    for (m, decl) in &mt.instance {
                        entry
                            .instance
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                    for (m, decl) in &mt.statics {
                        entry
                            .statics
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                }
            }
        }
    }
    checker::TypedCommons {
        commons: Commons {
            name: QualifiedName {
                parts: target_name
                    .split('.')
                    .map(|p| Ident {
                        name: p.to_string(),
                        span: Span::default(),
                    })
                    .collect(),
                span: Span::default(),
            },
            items: capability_items,
            uses: Vec::new(),
            documentation: None,
            form: CommonsForm::Brace,
            span: Span::default(),
            trivia: Trivia::default(),
            trailing_comments: Vec::new(),
        },
        types,
        fns,
        methods,
        expr_types: HashMap::new(),
        warnings: vec![],
    }
}

fn emit_test_deps(
    target_name: &str,
    target_kind: UnitKind,
    stubs: &HashMap<String, ResolvedStub>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let mut out = String::new();
    out.push_str("function makeTestDeps() {\n");
    let mut entries: Vec<String> = Vec::new();
    if target_kind == UnitKind::Context
        && let Some(table) = unit_tables.get(target_name)
    {
        let ns = target_name.replace('.', "_");
        // Sorted so `makeTestDeps` field order is deterministic across the
        // capability map's hash iteration order.
        let mut caps: Vec<&String> = table.capabilities.keys().collect();
        caps.sort();
        for cap in caps {
            // v0.118: a capability with a `stub` override plugs its
            // `__Stub_<Cap>` stub; otherwise the declared provider (its real
            // implementation) is used, as an un-overridden seam.
            let entry = if stubs.contains_key(cap) {
                format!("{cap}: new __Stub_{cap}()")
            } else if let Some(provider) = table.providers.get(cap) {
                format!("{cap}: new {ns}.{}()", provider.provider_name.name)
            } else {
                format!("{cap}: undefined as unknown as {ns}.{cap}")
            };
            entries.push(entry);
        }
        // Cross-context surface: consumed contexts run with their real surface
        // (v0.118 `stub` is capability-only — a consumed-context capability
        // flattened via `consumes U { Cap }` is a target capability above).
        let consumed = unit_consumes.get(target_name).cloned().unwrap_or_default();
        let aliases = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let mut alias_for_target: HashMap<String, String> = HashMap::new();
        for (alias, q) in &aliases {
            alias_for_target.insert(q.clone(), alias.clone());
        }
        let mut surface_entries: Vec<String> = Vec::new();
        for q in &consumed {
            let key = alias_for_target
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            let other_ns = q.replace('.', "_");
            surface_entries.push(format!(
                "{key}: undefined as unknown as ReturnType<typeof {other_ns}.makeSurface>"
            ));
        }
        if !surface_entries.is_empty() {
            entries.push(format!("surface: {{ {} }}", surface_entries.join(", ")));
        }
    }
    out.push_str(&format!("  return {{ {} }};\n", entries.join(", ")));
    out.push_str("}\n");
    out
}

/// Emit the shared per-runner scope setup — agent reset, the `deps` factory, and
/// the `const { … } = <ns> as any` destructurings that bring the target's,
/// `uses`', and consumed contexts' names into scope. Shared by `case` and
/// `property` runners so a property body resolves names exactly as a case does.
#[allow(clippy::too_many_arguments)]
fn emit_test_scope_setup(
    out: &mut String,
    target_name: &str,
    target_kind: UnitKind,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    // v0.117: when the body observes (`Cap.op called …` / `trace(Cap.op)`), wrap
    // `deps` with the recording proxy and declare the per-case trace `__obs`. Off
    // for bodies that don't observe, so their emitted output is unchanged.
    record_calls: bool,
) {
    let target_ns = target_name.replace('.', "_");
    // v0.9.2: reset the target context's agent registries so each test sees a
    // fresh per-key state (finding #10's "fresh per test" half).
    let target_has_agents = unit_tables
        .get(target_name)
        .is_some_and(|t| !t.agents.is_empty());
    if target_has_agents {
        out.push_str(&format!("    {target_ns}.__resetAgents();\n"));
    }
    // v0.117: the per-case recorded-call trace, and — for a context target with
    // capabilities — a `deps` wrapped so each capability operation records its
    // calls into `__obs`. Observations and `trace(Cap.op)` in the body read it.
    let obs_spec: Option<String> = if record_calls && target_kind == UnitKind::Context {
        unit_tables.get(target_name).and_then(|table| {
            if table.capabilities.is_empty() {
                return None;
            }
            let mut caps: Vec<&String> = table.capabilities.keys().collect();
            caps.sort();
            let entries: Vec<String> = caps
                .iter()
                .map(|c| {
                    let mut ops: Vec<String> = table.capabilities[*c]
                        .ops
                        .iter()
                        .map(|o| format!("{:?}", o.name.name))
                        .collect();
                    ops.sort();
                    format!("{c}: [{}]", ops.join(", "))
                })
                .collect();
            Some(format!("{{ {} }}", entries.join(", ")))
        })
    } else {
        None
    };
    if target_kind == UnitKind::Context {
        if let Some(spec) = &obs_spec {
            out.push_str("    const __obs = { log: {} as Record<string, { args: any[]; order: number }[]>, n: 0 };\n");
            out.push_str(&format!(
                "    const deps = __bynkRecordDeps(makeTestDeps(), {spec}, __obs);\n"
            ));
        } else {
            out.push_str("    const deps = makeTestDeps();\n");
        }
    } else {
        out.push_str("    const deps = {};\n");
    }
    // Bring the target's top-level names into local scope so the lowered
    // body can reference them unqualified. The target's types and fns are
    // exported from its namespace by the production emitter.
    if let Some(table) = unit_tables.get(target_name) {
        let mut names: Vec<String> = table
            .types
            .keys()
            .chain(table.fns.keys())
            .cloned()
            .collect();
        // For contexts, also bring services and providers into scope.
        let extras: Vec<String> = table
            .services
            .keys()
            .chain(table.agents.keys())
            .cloned()
            .collect();
        names.extend(extras);
        // v0.9.2: bring each agent's construction factory into scope so a test
        // body's `AgentName(key)` lowers to `__makeAgentName(key)`.
        for agent in table.agents.keys() {
            names.push(crate::emitter::agent_factory_name(agent));
        }
        names.sort();
        names.dedup();
        if !names.is_empty() {
            let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
            out.push_str(&format!(
                "    const {{ {} }} = {target_ns} as any;\n",
                joined.join(", ")
            ));
        }
    }
    // Bring in `uses` commons names too — the target's body can use them.
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                let mut names: Vec<&String> = table.types.keys().chain(table.fns.keys()).collect();
                names.sort();
                names.dedup();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
        }
    }
    // Bring consumed-context exported names into scope, plus a `Payment`
    // alias for the consumed surface (so `Payment.authorise.call(...)` works).
    if let Some(consumed) = unit_consumes.get(target_name) {
        let aliases = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let mut alias_for: HashMap<String, String> = HashMap::new();
        for (alias, q) in &aliases {
            alias_for.insert(q.clone(), alias.clone());
        }
        for q in consumed {
            let ns = q.replace('.', "_");
            if let Some(table) = unit_tables.get(q) {
                let mut names: Vec<&String> = table.types.keys().collect();
                names.sort();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
            let key = alias_for
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            out.push_str(&format!(
                "    const {key} = (deps as any).surface?.{key};\n"
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_test_case_function(
    runner_name: &str,
    case: &Case,
    target_name: &str,
    target_kind: UnitKind,
    stubs: &HashMap<String, ResolvedStub>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    source: &str,
    rel_path: &str,
) -> (String, SourceMapBuilder) {
    let _ = stubs;
    let mut out = String::new();
    out.push_str(&format!("async function {runner_name}() {{\n"));
    out.push_str("  try {\n");
    emit_test_scope_setup(
        &mut out,
        target_name,
        target_kind,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
        block_uses_observation(&case.body),
    );
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
    // v0.117: re-type-check the case body (with the call-record types registered)
    // so the lowering has full expr types — collection kernels, notably a
    // `trace(Cap.op)` result's `List[…]` methods, dispatch on the checked type.
    if let Some((mut resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        register_call_record_types(&mut resolved, target_name, unit_tables);
        let mut throwaway_errors: Vec<CompileError> = Vec::new();
        let mut throwaway_refs = RefSink::new();
        typed.expr_types = typecheck_case_body(
            target_name,
            &case.body,
            case.span,
            unit_tables,
            &resolved,
            &mut throwaway_errors,
            &mut throwaway_refs,
            HashMap::new(),
        );
    }
    let cross = bynk_check::resolver::CrossContextInfo::default();
    let test_services: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.services.keys().cloned().collect())
        .unwrap_or_default();
    let test_agents: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.agents.keys().cloned().collect())
        .unwrap_or_default();
    let (body_src, body_smb) = emitter::lower_test_case_body(
        &case.body,
        &mut typed,
        &cross,
        test_services,
        test_agents,
        source,
        rel_path,
    );
    // v0.70: splice the case body (line-by-line, indented) and merge its source-map
    // sub-builder into the case builder, line-anchored at the splice. The caller
    // (emit_test_module) merges this case builder into the module map under the
    // case's source file.
    let body_base = out.len();
    for line in body_src.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    let mut case_smb = SourceMapBuilder::new();
    case_smb.merge(&body_smb, &body_src, &out, body_base, 0);
    out.push_str("    return { pass: true };\n");
    out.push_str("  } catch (e) {\n");
    out.push_str("    if (e instanceof ExpectationError) {\n");
    out.push_str(
        "      return { pass: false, error: { message: e.message, location: e.location } };\n",
    );
    out.push_str("    }\n");
    out.push_str(
        "    return { pass: false, error: { message: String(e), location: \"unknown\" } };\n",
    );
    out.push_str("  }\n");
    out.push_str("}\n");
    (out, case_smb)
}

/// v0.114 (testing track slice 2): the runtime the generative `property` runner
/// relies on — a seeded PRNG, per-type shrinkers, the case loop, and shrinking.
/// Emitted once per test module that declares a `property`, alongside the
/// expectation helpers. `__bynkSeed` is the run's root seed (from
/// `BYNK_TEST_SEED`, else random), from which each property derives its seed via
/// `__bynkMix`, so `bynkc test --seed <hex>` reproduces a run byte-for-byte.
/// v0.117: emit a TypeScript `type` alias for each observed capability
/// operation's call record (`type __Cap_op_Call = { param: T, … }`), so a
/// `trace(Cap.op)` list — whose elements are these records — type-checks under
/// `tsc` when a test projects a field (`c.msg`). Names mirror
/// [`checker::call_record_type_name`]. Ordered by capability then operation for
/// deterministic output.
fn observation_call_record_types(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
) -> String {
    let Some(table) = unit_tables.get(target_name) else {
        return String::new();
    };
    // Named/opaque parameter types are re-exported under the target's namespace,
    // so qualify them (`AuthId` → `commerce_payment.AuthId`); base types are
    // unaffected. Matches the mock-signature qualification.
    let scope_ns = target_name.replace('.', "_");
    let scope_type_names: HashSet<String> = table.types.keys().cloned().collect();
    let mut caps: Vec<&String> = table.capabilities.keys().collect();
    caps.sort();
    let mut out = String::new();
    for cap in caps {
        for op in &table.capabilities[cap].ops {
            let name = checker::call_record_type_name(cap, &op.name.name);
            let fields: Vec<String> = op
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        p.name.name,
                        emitter::ts_type_ref_qualified(&p.type_ref, &scope_type_names, &scope_ns)
                    )
                })
                .collect();
            out.push_str(&format!("type {name} = {{ {} }};\n", fields.join("; ")));
        }
    }
    out
}

/// v0.117: the observation runtime — wraps each observed capability operation on
/// the test `deps` so every call records its arguments and a monotonic order
/// index into the per-case trace `__obs`. Emitted once per module that observes.
fn observation_runtime_helpers() -> String {
    r#"function __bynkRecordDeps(deps: any, spec: Record<string, string[]>, obs: { log: Record<string, { args: any[]; order: number }[]>; n: number }): any {
  for (const cap of Object.keys(spec)) {
    if (!deps || !deps[cap]) continue;
    for (const op of spec[cap]) {
      const orig = deps[cap][op];
      if (typeof orig !== "function") continue;
      const key = cap + "." + op;
      obs.log[key] = obs.log[key] ?? [];
      deps[cap][op] = (...args: any[]) => {
        obs.log[key].push({ args, order: obs.n++ });
        return orig.apply(deps[cap], args);
      };
    }
  }
  return deps;
}
"#
    .to_string()
}

fn property_runtime_helpers() -> String {
    r#"function __bynkRootSeed(): number {
  const env = (globalThis as any) && (globalThis as any).process && (globalThis as any).process.env;
  const s = env && env.BYNK_TEST_SEED;
  if (s) { const n = parseInt(String(s), 16); if (Number.isFinite(n)) return n >>> 0; }
  return (Math.floor(Math.random() * 0x100000000)) >>> 0;
}
const __bynkSeed: number = __bynkRootSeed();
function __bynkMix(a: number, b: number): number {
  let h = (a ^ Math.imul(b + 1, 0x9e3779b1)) >>> 0;
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b) >>> 0;
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b) >>> 0;
  return (h ^ (h >>> 16)) >>> 0;
}
function __bynkRng(seed: number) {
  let s = seed >>> 0;
  const next = () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int(lo: bigint, hi: bigint): bigint {
      if (hi <= lo) return lo;
      const span = hi - lo + 1n;
      let r = 0n, bound = span;
      while (bound > 0n) {
        r = (r << 32n) | BigInt(Math.floor(next() * 0x100000000));
        bound >>= 32n;
      }
      return lo + (r % span);
    },
    float(lo: number, hi: number): number { return lo + next() * (hi - lo); },
    str(min: number, max: number): string {
      const len = min + Math.floor(next() * (max - min + 1));
      const alpha = "abcdefghijklmnopqrstuvwxyz";
      let out = "";
      for (let i = 0; i < len; i++) out += alpha[Math.floor(next() * alpha.length)];
      return out;
    },
    bool(): boolean { return next() < 0.5; },
    pick(thunks: Array<() => any>): any { return thunks[Math.floor(next() * thunks.length)](); },
  };
}
function __bynkShrinkInt(v: bigint, floor: bigint): bigint[] {
  const out: bigint[] = [];
  if (v === floor) return out;
  out.push(floor);
  let cur = v;
  for (let i = 0; i < 8; i++) {
    const mid = floor + (cur - floor) / 2n;
    if (mid === cur || mid === floor) break;
    out.push(mid);
    cur = mid;
  }
  if (v > floor) out.push(v - 1n);
  return out;
}
function __bynkShrinkString(v: string, min: number): string[] {
  const out: string[] = [];
  if (v.length > min) {
    out.push(v.slice(0, min));
    if (v.length - 1 > min) out.push(v.slice(0, v.length - 1));
  }
  return out;
}
function __bynkIsFailure(e: any): e is Error {
  // A shrinkable property/contract failure: an `expect` assertion, or a v0.115
  // function-contract guard violation thrown from the attacked function. The
  // `e is Error` predicate narrows the caught value so `e.message` type-checks.
  return e instanceof ExpectationError || (!!e && e.name === "BynkContractError");
}
async function __bynkShrink(gens: any[], where: ((v: any[]) => boolean) | null, body: (v: any[]) => Promise<void>, vals: any[]): Promise<any[]> {
  let current = vals.slice();
  let improved = true;
  let budget = 400;
  while (improved && budget > 0) {
    improved = false;
    for (let i = 0; i < gens.length; i++) {
      const cands = gens[i].shrink(current[i]);
      for (const c of cands) {
        if (--budget <= 0) break;
        const trial = current.slice();
        trial[i] = c;
        if (where && !where(trial)) continue;
        let failed = false;
        try { await body(trial); } catch (e) { failed = __bynkIsFailure(e); }
        if (failed) { current = trial; improved = true; break; }
      }
    }
  }
  return current;
}
async function __bynkRunProperty(spec: { seed: number, cases: number, gens: any[], where: ((v: any[]) => boolean) | null, body: (v: any[]) => Promise<void>, name: string, location: string, file: string }): Promise<{ pass: boolean, error?: { message: string, location: string } }> {
  const rng = __bynkRng(spec.seed);
  const maxAttempts = spec.cases * 25 + 50;
  let ran = 0, attempts = 0;
  while (ran < spec.cases && attempts < maxAttempts) {
    const bi = attempts;
    attempts++;
    const vals = spec.gens.map((g) => (bi < g.boundaries.length ? g.boundaries[bi] : g.gen(rng)));
    if (spec.where && !spec.where(vals)) continue;
    ran++;
    try {
      await spec.body(vals);
    } catch (e) {
      if (!__bynkIsFailure(e)) {
        return { pass: false, error: { message: String(e), location: spec.location } };
      }
      const shrunk = await __bynkShrink(spec.gens, spec.where, spec.body, vals);
      // Report the run's *root* seed — each property derives its own from it via
      // `__bynkMix`, so `--seed <root>` reproduces the whole run byte-for-byte.
      const seedHex = "0x" + (__bynkSeed >>> 0).toString(16);
      const shown = spec.gens.map((g, i) => `${g.name} = ${g.show(shrunk[i])}`).join(", ");
      let detail = e.message;
      try { await spec.body(shrunk); } catch (e2) { if (__bynkIsFailure(e2)) detail = (e2 as any).message; }
      const firstLine = String(detail).split("\n")[0];
      const message = `property failed after ${ran} cases (seed ${seedHex})\n  shrunk counterexample:  ${shown}\n  ${firstLine}\n  reproduce: bynkc test ${spec.file} --seed ${seedHex}`;
      return { pass: false, error: { message, location: spec.location } };
    }
  }
  return { pass: true };
}
"#
    .to_string()
}

/// v0.119 (testing track slice 7, ADR 0155): the history-property runtime, emitted
/// alongside [`property_runtime_helpers`] only when a module declares a
/// `for all run: History[Agent]` property. Generates a bounded, seeded sequence of
/// handler calls, drives it through the real handlers (`spec.drive`, the agent
/// module's `__bynkDriveHistory_<Agent>`), evaluates the predicate over the
/// observed `run`, and on failure delta-debugs the *sequence* (re-driving after
/// each reduction so the counterexample stays reachable) then shrinks the surviving
/// arguments — reporting the seed, the shrunk sequence, and a reproduce line.
fn history_runtime_helpers() -> String {
    r#"type __BynkHistoryHandler = { tag: string, gens: Array<{ boundaries: any[], gen: (rng: any) => any, shrink: (v: any) => any[], show: (v: any) => string }> };
type __BynkHistorySpec = { seed: number, cases: number, maxLen: number, handlers: __BynkHistoryHandler[], drive: (seq: any[]) => Promise<any[]>, body: (run: any[]) => Promise<void>, name: string, location: string, file: string };
function __bynkGenHistory(rng: any, spec: __BynkHistorySpec): Array<{ h: number, args: any[] }> {
  const len = Math.floor(rng.next() * (spec.maxLen + 1));
  const seq: Array<{ h: number, args: any[] }> = [];
  for (let i = 0; i < len; i++) {
    const h = spec.handlers.length > 0 ? Math.floor(rng.next() * spec.handlers.length) : 0;
    const args = spec.handlers[h] ? spec.handlers[h].gens.map((g) => g.gen(rng)) : [];
    seq.push({ h, args });
  }
  return seq;
}
async function __bynkHistoryStillFails(spec: __BynkHistorySpec, seq: Array<{ h: number, args: any[] }>): Promise<boolean> {
  let run: any[];
  try { run = await spec.drive(seq); } catch { return false; }
  try { await spec.body(run); return false; } catch (e) { return __bynkIsFailure(e); }
}
async function __bynkShrinkHistory(spec: __BynkHistorySpec, seq: Array<{ h: number, args: any[] }>): Promise<Array<{ h: number, args: any[] }>> {
  let cur = seq.slice();
  let budget = 300;
  // Delta-debug the sequence: drop a step, re-drive, keep the reduction only if it
  // still reproduces the failure (so the printed counterexample stays reachable).
  let improved = true;
  while (improved && budget > 0) {
    improved = false;
    for (let i = 0; i < cur.length && budget > 0; i++) {
      budget--;
      const trial = cur.slice(0, i).concat(cur.slice(i + 1));
      if (await __bynkHistoryStillFails(spec, trial)) { cur = trial; improved = true; break; }
    }
  }
  // Then shrink each surviving step's arguments with the value shrinker.
  improved = true;
  while (improved && budget > 0) {
    improved = false;
    for (let i = 0; i < cur.length; i++) {
      const step = cur[i];
      const gens = spec.handlers[step.h] ? spec.handlers[step.h].gens : [];
      for (let j = 0; j < gens.length; j++) {
        const cands = gens[j].shrink(step.args[j]);
        for (const c of cands) {
          if (--budget <= 0) break;
          const nargs = step.args.slice(); nargs[j] = c;
          const trial = cur.slice(); trial[i] = { h: step.h, args: nargs };
          if (await __bynkHistoryStillFails(spec, trial)) { cur = trial; improved = true; break; }
        }
        if (budget <= 0) break;
      }
    }
  }
  return cur;
}
function __bynkShowHistory(spec: __BynkHistorySpec, seq: Array<{ h: number, args: any[] }>): string {
  return "[" + seq.map((st) => {
    const h = spec.handlers[st.h];
    if (!h) return "?";
    const args = st.args.map((a: any, j: number) => h.gens[j] ? h.gens[j].show(a) : __bynkShow(a)).join(", ");
    return h.tag + "(" + args + ")";
  }).join(", ") + "]";
}
async function __bynkRunHistory(spec: __BynkHistorySpec): Promise<{ pass: boolean, error?: { message: string, location: string } }> {
  const rng = __bynkRng(spec.seed);
  for (let c = 0; c < spec.cases; c++) {
    const seq = __bynkGenHistory(rng, spec);
    let run: any[];
    try { run = await spec.drive(seq); } catch (e) {
      return { pass: false, error: { message: String(e), location: spec.location } };
    }
    try {
      await spec.body(run);
    } catch (e) {
      if (!__bynkIsFailure(e)) {
        return { pass: false, error: { message: String(e), location: spec.location } };
      }
      const shrunk = await __bynkShrinkHistory(spec, seq);
      const seedHex = "0x" + (__bynkSeed >>> 0).toString(16);
      const shown = __bynkShowHistory(spec, shrunk);
      let detail = (e as any).message;
      try { const __r2 = await spec.drive(shrunk); await spec.body(__r2); } catch (e2) { if (__bynkIsFailure(e2)) detail = (e2 as any).message; }
      const firstLine = String(detail).split("\n")[0];
      const message = `history property failed after ${c + 1} runs (seed ${seedHex})\n  shrunk sequence:  ${shown}\n  ${firstLine}\n  reproduce: bynkc test ${spec.file} --seed ${seedHex}`;
      return { pass: false, error: { message, location: spec.location } };
    }
  }
  return { pass: true };
}
"#
    .to_string()
}

/// Integer generation bounds `(lo, hi, floor)` derived from a refinement: `lo`
/// and `hi` bound the random draw (and the boundary values), `floor` is the
/// shrink target. Unrefined `Int` draws over `[-1000, 1000]` toward `0`.
fn int_bounds(refinement: Option<&Refinement>) -> (i64, i64, i64) {
    let mut lo = -1000i64;
    let mut hi = 1000i64;
    let mut floor = 0i64;
    if let Some(r) = refinement {
        for p in &r.predicates {
            match &p.kind {
                PredKind::Positive => {
                    lo = lo.max(1);
                    floor = floor.max(1);
                }
                PredKind::NonNegative => {
                    lo = lo.max(0);
                    floor = floor.max(0);
                }
                PredKind::InRange(a, b) => {
                    lo = a.value;
                    hi = b.value;
                    floor = a.value;
                }
                _ => {}
            }
        }
    }
    if lo > hi {
        hi = lo;
    }
    if floor < lo {
        floor = lo;
    }
    (lo, hi, floor)
}

/// Float generation bounds `(lo, hi)` from a refinement (`lo` doubles as the
/// shrink target). Unrefined `Float` draws over `[-1000, 1000]`.
fn float_bounds(refinement: Option<&Refinement>) -> (f64, f64) {
    let mut lo = -1000.0f64;
    let mut hi = 1000.0f64;
    if let Some(r) = refinement {
        for p in &r.predicates {
            match &p.kind {
                PredKind::Positive => lo = lo.max(1.0),
                PredKind::NonNegative => lo = lo.max(0.0),
                PredKind::InRangeF(a, b) => {
                    lo = a.value;
                    hi = b.value;
                }
                _ => {}
            }
        }
    }
    if lo > hi {
        hi = lo;
    }
    (lo, hi)
}

/// Minimum string length a refinement demands (`0` if none).
fn str_min(refinement: Option<&Refinement>) -> i64 {
    let mut min = 0i64;
    if let Some(r) = refinement {
        for p in &r.predicates {
            match p.kind {
                PredKind::NonEmpty => min = min.max(1),
                PredKind::MinLength(k) | PredKind::Length(k) => min = min.max(k),
                _ => {}
            }
        }
    }
    min
}

/// A canonical (deterministic) TypeScript literal inhabiting a base type — used
/// for opaque `.unsafe` wrapping of the exotic bases (`Duration`, `Bytes`, …).
fn base_canon(base: BaseType) -> String {
    match base {
        BaseType::Int => "0n".to_string(),
        BaseType::String => "\"\"".to_string(),
        BaseType::Bool => "true".to_string(),
        BaseType::Float => "0".to_string(),
        BaseType::Duration | BaseType::Instant => "0".to_string(),
        BaseType::Bytes => "new Uint8Array()".to_string(),
    }
}

/// A TypeScript expression that draws a random inhabitant of `base` (refined by
/// `refinement`), branded to `<name>` — via `<name>.unsafe(...)` for an opaque
/// type, or an inline `as` cast for a refined/alias type (ADR 0182).
fn refined_gen_ts(
    name: &str,
    base: BaseType,
    refinement: Option<&Refinement>,
    is_opaque: bool,
) -> String {
    let draw = match base {
        BaseType::Int => {
            let (lo, hi, _) = int_bounds(refinement);
            format!("rng.int({lo}n, {hi}n)")
        }
        BaseType::Float => {
            let (lo, hi) = float_bounds(refinement);
            format!("rng.float({lo}, {hi})")
        }
        BaseType::String => {
            let min = str_min(refinement);
            format!("rng.str({min}, {})", min + 8)
        }
        BaseType::Bool => "rng.bool()".to_string(),
        _ => base_canon(base),
    };
    emitter::unchecked_construct_test(name, &draw, is_opaque)
}

/// A TypeScript expression drawing a random inhabitant of a resolved type using
/// the in-scope `rng` — the property generator (DECISION P: a type is its own
/// inhabitant space). Sums pick a random variant; records generate every field.
fn gen_ts_for_ty(ty: &checker::Ty, types: &HashMap<String, TypeDecl>, depth: u32) -> String {
    if depth == 0 {
        return canon_ts_for_ty(ty, types, 1);
    }
    match ty {
        checker::Ty::Base(BaseType::Int) => {
            let (lo, hi, _) = int_bounds(None);
            format!("rng.int({lo}n, {hi}n)")
        }
        checker::Ty::Base(BaseType::String) => "rng.str(0, 8)".to_string(),
        checker::Ty::Base(BaseType::Bool) => "rng.bool()".to_string(),
        checker::Ty::Base(BaseType::Float) => "rng.float(-1000, 1000)".to_string(),
        checker::Ty::Base(b) => base_canon(*b),
        checker::Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return "undefined".to_string();
            };
            match &decl.body {
                TypeBody::Refined {
                    base, refinement, ..
                }
                | TypeBody::Opaque {
                    base, refinement, ..
                } => refined_gen_ts(
                    name,
                    *base,
                    refinement.as_ref(),
                    matches!(decl.body, TypeBody::Opaque { .. }),
                ),
                TypeBody::Sum(s) => {
                    let thunks: Vec<String> = s
                        .variants
                        .iter()
                        .map(|v| {
                            if v.payload.is_empty() {
                                format!("() => {name}.{}", v.name.name)
                            } else {
                                let args: Vec<String> = v
                                    .payload
                                    .iter()
                                    .map(|f| {
                                        checker::resolve_type_ref(&f.type_ref, types)
                                            .map(|t| gen_ts_for_ty(&t, types, depth - 1))
                                            .unwrap_or_else(|| "undefined".to_string())
                                    })
                                    .collect();
                                format!("() => {name}.{}({})", v.name.name, args.join(", "))
                            }
                        })
                        .collect();
                    if thunks.is_empty() {
                        "undefined".to_string()
                    } else {
                        format!("rng.pick([{}])", thunks.join(", "))
                    }
                }
                TypeBody::Record(r) => {
                    let fields: Vec<String> = r
                        .fields
                        .iter()
                        .map(|f| {
                            let g = checker::resolve_type_ref(&f.type_ref, types)
                                .map(|t| gen_ts_for_ty(&t, types, depth - 1))
                                .unwrap_or_else(|| "undefined".to_string());
                            format!("{}: {}", f.name.name, g)
                        })
                        .collect();
                    format!("{{ {} }}", fields.join(", "))
                }
            }
        }
        _ => "undefined".to_string(),
    }
}

/// A canonical (deterministic, boundary) inhabitant of a resolved type — the
/// boundary value the runner draws first (refinement floor / minimum length /
/// first variant), and the shrink target for sums.
fn canon_ts_for_ty(ty: &checker::Ty, types: &HashMap<String, TypeDecl>, depth: u32) -> String {
    if depth == 0 {
        return "undefined".to_string();
    }
    match ty {
        checker::Ty::Base(BaseType::Int) => "0n".to_string(),
        checker::Ty::Base(BaseType::String) => "\"\"".to_string(),
        checker::Ty::Base(BaseType::Bool) => "true".to_string(),
        checker::Ty::Base(BaseType::Float) => "0".to_string(),
        checker::Ty::Base(b) => base_canon(*b),
        checker::Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return "undefined".to_string();
            };
            match &decl.body {
                TypeBody::Refined {
                    base, refinement, ..
                }
                | TypeBody::Opaque {
                    base, refinement, ..
                } => {
                    let lit = match base {
                        BaseType::Int => {
                            let (lo, _, _) = int_bounds(refinement.as_ref());
                            format!("{lo}n")
                        }
                        BaseType::Float => {
                            let (lo, _) = float_bounds(refinement.as_ref());
                            lo.to_string()
                        }
                        BaseType::String => {
                            let min = str_min(refinement.as_ref());
                            format!("\"{}\"", "x".repeat(min.max(0) as usize))
                        }
                        BaseType::Bool => "true".to_string(),
                        other => base_canon(*other),
                    };
                    emitter::unchecked_construct_test(
                        name,
                        &lit,
                        matches!(decl.body, TypeBody::Opaque { .. }),
                    )
                }
                TypeBody::Sum(s) => match s.variants.first() {
                    None => "undefined".to_string(),
                    Some(v) if v.payload.is_empty() => format!("{name}.{}", v.name.name),
                    Some(v) => {
                        let args: Vec<String> = v
                            .payload
                            .iter()
                            .map(|f| {
                                checker::resolve_type_ref(&f.type_ref, types)
                                    .map(|t| canon_ts_for_ty(&t, types, depth - 1))
                                    .unwrap_or_else(|| "undefined".to_string())
                            })
                            .collect();
                        format!("{name}.{}({})", v.name.name, args.join(", "))
                    }
                },
                TypeBody::Record(r) => {
                    let fields: Vec<String> = r
                        .fields
                        .iter()
                        .map(|f| {
                            let g = checker::resolve_type_ref(&f.type_ref, types)
                                .map(|t| canon_ts_for_ty(&t, types, depth - 1))
                                .unwrap_or_else(|| "undefined".to_string());
                            format!("{}: {}", f.name.name, g)
                        })
                        .collect();
                    format!("{{ {} }}", fields.join(", "))
                }
            }
        }
        _ => "undefined".to_string(),
    }
}

/// The generator descriptor for one `for all` binding: boundary values, a random
/// generator, and a shrinker (all TypeScript source, evaluated in the runner's
/// scope where the type constructors are in scope).
struct BindingGen {
    boundaries: Vec<String>,
    gen_ts: String,
    shrink: String,
}

/// Build the generator descriptor for a binding whose resolved type is `ty`.
fn binding_gen(ty: &checker::Ty, types: &HashMap<String, TypeDecl>) -> BindingGen {
    let gen_ts = gen_ts_for_ty(ty, types, PROP_GEN_DEPTH);
    let (boundaries, shrink) = match ty {
        checker::Ty::Base(BaseType::Int) => {
            let (lo, hi, floor) = int_bounds(None);
            (
                vec![format!("{floor}n"), format!("{hi}n"), format!("{lo}n")],
                format!("__bynkShrinkInt(v, {floor}n)"),
            )
        }
        checker::Ty::Base(BaseType::String) => (
            vec!["\"\"".to_string()],
            "__bynkShrinkString(v, 0)".to_string(),
        ),
        checker::Ty::Base(BaseType::Bool) => (
            vec!["true".to_string(), "false".to_string()],
            "(v ? [false] : [])".to_string(),
        ),
        checker::Ty::Named { name, .. } => match types.get(name).map(|d| &d.body) {
            Some(TypeBody::Refined {
                base, refinement, ..
            })
            | Some(TypeBody::Opaque {
                base, refinement, ..
            }) => {
                // ADR 0182: brand via `.unsafe` for opaque, inline `as` cast for
                // refined/alias. The or-pattern can't capture which variant, so
                // re-derive it by name.
                let is_opaque = matches!(
                    types.get(name).map(|d| &d.body),
                    Some(TypeBody::Opaque { .. })
                );
                match base {
                    BaseType::Int => {
                        let (lo, hi, floor) = int_bounds(refinement.as_ref());
                        let shrunk = emitter::unchecked_construct_test(name, "__n", is_opaque);
                        (
                            vec![
                                emitter::unchecked_construct_test(
                                    name,
                                    &format!("{lo}n"),
                                    is_opaque,
                                ),
                                emitter::unchecked_construct_test(
                                    name,
                                    &format!("{hi}n"),
                                    is_opaque,
                                ),
                            ],
                            format!("__bynkShrinkInt(v, {floor}n).map((__n: bigint) => {shrunk})"),
                        )
                    }
                    BaseType::String => {
                        let min = str_min(refinement.as_ref());
                        let lit = format!("\"{}\"", "x".repeat(min.max(0) as usize));
                        let shrunk = emitter::unchecked_construct_test(name, "__s", is_opaque);
                        (
                            vec![emitter::unchecked_construct_test(name, &lit, is_opaque)],
                            format!("__bynkShrinkString(v, {min}).map((__s: string) => {shrunk})"),
                        )
                    }
                    _ => (
                        vec![canon_ts_for_ty(ty, types, PROP_GEN_DEPTH)],
                        "[]".to_string(),
                    ),
                }
            }
            Some(TypeBody::Sum(_)) => (
                vec![canon_ts_for_ty(ty, types, PROP_GEN_DEPTH)],
                format!("[{}]", canon_ts_for_ty(ty, types, PROP_GEN_DEPTH)),
            ),
            _ => (
                vec![canon_ts_for_ty(ty, types, PROP_GEN_DEPTH)],
                "[]".to_string(),
            ),
        },
        _ => (Vec::new(), "[]".to_string()),
    };
    BindingGen {
        boundaries,
        gen_ts,
        shrink,
    }
}

/// v0.114: emit one async runner for a generative `property` — the binding
/// generators, the `where` filter and predicate body as closures over the
/// generated tuple, and the `__bynkRunProperty` call that draws cases, shrinks a
/// counterexample, and reports the seed + shrunk tuple + reproduce line.
#[allow(clippy::too_many_arguments)]
fn emit_test_property_function(
    runner_name: &str,
    prop: &PropertyDecl,
    prop_ordinal: usize,
    target_name: &str,
    target_kind: UnitKind,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    source: &str,
    rel_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("async function {runner_name}() {{\n"));
    emit_test_scope_setup(
        &mut out,
        target_name,
        target_kind,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
        false,
    );

    // Generator descriptors, one per binding, over the target's privileged type
    // view (so refined/opaque constructors resolve).
    let resolved = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    )
    .map(|(r, _)| r);
    out.push_str("    const __gens = [\n");
    for b in &prop.forall.bindings {
        let bg = resolved
            .as_ref()
            .and_then(|r| checker::resolve_type_ref(&b.type_ref, &r.types).map(|t| (t, r)))
            .map(|(t, r)| binding_gen(&t, &r.types))
            .unwrap_or(BindingGen {
                boundaries: Vec::new(),
                gen_ts: "undefined".to_string(),
                shrink: "[]".to_string(),
            });
        out.push_str(&format!(
            "      {{ name: \"{}\", boundaries: [{}], gen: (rng: any) => {}, shrink: (v: any) => {}, show: (v: any) => __bynkShow(v) }},\n",
            emitter::escape_ts_string(&b.name.name),
            bg.boundaries.join(", "),
            bg.gen_ts,
            bg.shrink
        ));
    }
    out.push_str("    ];\n");

    // The `where` filter and the predicate body, as closures over the tuple.
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
    let cross = bynk_check::resolver::CrossContextInfo::default();
    let binding_names: Vec<String> = prop
        .forall
        .bindings
        .iter()
        .map(|b| b.name.name.clone())
        .collect();
    let destructure = format!("const [{}] = __vals;", binding_names.join(", "));

    if let Some(w) = &prop.forall.where_pred {
        let synth = Block {
            statements: Vec::new(),
            tail: Box::new(w.clone()),
            span: w.span,
            tail_leading_comments: Vec::new(),
            implicit_tail: false,
        };
        let (src, _) = emitter::lower_block_to_async_body(
            &synth,
            &TypeRef::Base(BaseType::Bool, w.span),
            &mut typed,
            &cross,
        );
        out.push_str("    const __where = (__vals: any[]) => {\n");
        out.push_str(&format!("      {destructure}\n"));
        for line in src.lines() {
            out.push_str("      ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    };\n");
    } else {
        out.push_str("    const __where = null;\n");
    }

    let test_services: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.services.keys().cloned().collect())
        .unwrap_or_default();
    let test_agents: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.agents.keys().cloned().collect())
        .unwrap_or_default();
    // Property bodies are collaborator-free predicate scaffolding; like mock op
    // bodies, their source map is a deliberate scope cut (the `expect` location
    // still binds through `assert_loc`).
    let (body_src, _body_smb) = emitter::lower_test_case_body(
        &prop.forall.body,
        &mut typed,
        &cross,
        test_services,
        test_agents,
        source,
        rel_path,
    );
    out.push_str("    const __body = async (__vals: any[]) => {\n");
    out.push_str(&format!("      {destructure}\n"));
    for line in body_src.lines() {
        out.push_str("      ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    };\n");

    // Forward slashes so the emitted location/reproduce path is portable — on
    // Windows `tests_prefix.join(...)` yields `\`, which must not leak into the
    // golden `.ts` (mirrors `discovered_location`'s normalisation).
    let rel_path_fwd = rel_path.replace('\\', "/");
    out.push_str(&format!(
        "    return await __bynkRunProperty({{ seed: __bynkMix(__bynkSeed, {prop_ordinal}), cases: 100, gens: __gens, where: __where, body: __body, name: \"{}\", location: \"{}\", file: \"{}\" }});\n",
        emitter::escape_ts_string(&prop.name),
        emitter::escape_ts_string(&rel_path_fwd),
        emitter::escape_ts_string(&rel_path_fwd),
    ));
    out.push_str("}\n");
    out
}

/// v0.119 (testing track slice 7, ADR 0155): emit one async runner for a history
/// property — `for all run: History[Agent]`. Builds a per-handler argument
/// generator table, a predicate `__body` over the driven `run` (a `List[Step]`),
/// and a `__drive` closure calling the agent module's `__bynkDriveHistory_<Agent>`,
/// then hands them to `__bynkRunHistory`, which generates, drives, shrinks, and
/// reports exactly like a value `property`. Mirrors [`emit_test_property_function`].
#[allow(clippy::too_many_arguments)]
fn emit_test_history_property_function(
    runner_name: &str,
    prop: &PropertyDecl,
    prop_ordinal: usize,
    target_name: &str,
    target_kind: UnitKind,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    source: &str,
    rel_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("async function {runner_name}() {{\n"));
    emit_test_scope_setup(
        &mut out,
        target_name,
        target_kind,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
        false,
    );

    let Some((run_var, agent_name)) = prop_history_binding(prop) else {
        // Defensive: the checker rejects a malformed history property before emit,
        // so this is unreachable — emit a trivially-passing runner rather than panic.
        out.push_str("    return { pass: true };\n}\n");
        return out;
    };

    // The synthesised call/step/state types are checker-only (never emitted as
    // real TS), but the lowered predicate annotates the driven history with them.
    // Alias each to `any` so the driven plain objects type-check structurally.
    out.push_str(&format!(
        "    type __History_{agent_name}_Step = any; type __History_{agent_name}_Call = any; type __History_{agent_name}_State = any;\n"
    ));

    // The privileged view, plus the synthetic call/step/state types and the body's
    // expr types (with `run: List[Step]` in scope), so the lowering resolves the
    // predicate's `List` and value surface (`.call is …`, `.old`/`.new`, `.upTo`).
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
    let mut handler_descs: Vec<String> = Vec::new();
    if let Some((mut resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        register_call_record_types(&mut resolved, target_name, unit_tables);
        let inner = TypeRef::Named(Ident {
            name: agent_name.to_string(),
            span: prop.span,
        });
        let mut throwaway_refs = RefSink::new();
        let step_ty =
            check_history_binding(&inner, prop.span, &mut resolved, &mut throwaway_refs).ok();

        // Per-handler argument generators (the slice-2 value generator over each
        // handler parameter), in declaration order — the sequence generator picks a
        // handler uniformly and draws its arguments here.
        if let Some(agent) = resolved.agents.get(agent_name) {
            for h in history_handlers(agent) {
                let tag = history_variant_name(&h.method_name.as_ref().unwrap().name);
                let gens: Vec<String> = h
                    .params
                    .iter()
                    .map(|p| {
                        let bg = checker::resolve_type_ref(&p.type_ref, &resolved.types)
                            .map(|t| binding_gen(&t, &resolved.types))
                            .unwrap_or(BindingGen {
                                boundaries: Vec::new(),
                                gen_ts: "undefined".to_string(),
                                shrink: "[]".to_string(),
                            });
                        format!(
                            "{{ boundaries: [{}], gen: (rng: any) => {}, shrink: (v: any) => {}, show: (v: any) => __bynkShow(v) }}",
                            bg.boundaries.join(", "),
                            bg.gen_ts,
                            bg.shrink
                        )
                    })
                    .collect();
                handler_descs.push(format!(
                    "      {{ tag: \"{}\", gens: [{}] }},",
                    emitter::escape_ts_string(&tag),
                    gens.join(", ")
                ));
            }
        }

        if let Some(step_ty) = step_ty {
            let mut scope: HashMap<String, checker::Ty> = HashMap::new();
            scope.insert(run_var.to_string(), step_ty);
            let mut throwaway_errors: Vec<CompileError> = Vec::new();
            let mut throwaway_refs2 = RefSink::new();
            typed.expr_types = typecheck_case_body(
                target_name,
                &prop.forall.body,
                prop.span,
                unit_tables,
                &resolved,
                &mut throwaway_errors,
                &mut throwaway_refs2,
                scope,
            );
        }
        // Carry the synthetic call/step/state types into the lowering commons so
        // `is` on the call sum and field access on the step resolve.
        for (n, d) in &resolved.types {
            typed.types.entry(n.clone()).or_insert_with(|| d.clone());
        }
    }

    out.push_str("    const __handlers = [\n");
    for hd in &handler_descs {
        out.push_str(hd);
        out.push('\n');
    }
    out.push_str("    ];\n");

    // The predicate body, as a closure over the driven history `run`.
    let cross = bynk_check::resolver::CrossContextInfo::default();
    let test_services: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.services.keys().cloned().collect())
        .unwrap_or_default();
    let test_agents: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.agents.keys().cloned().collect())
        .unwrap_or_default();
    let (body_src, _body_smb) = emitter::lower_test_case_body(
        &prop.forall.body,
        &mut typed,
        &cross,
        test_services,
        test_agents,
        source,
        rel_path,
    );
    out.push_str("    const __body = async (__run: any[]) => {\n");
    out.push_str(&format!("      const {run_var} = __run;\n"));
    for line in body_src.lines() {
        out.push_str("      ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    };\n");

    // Drive a generated sequence through the real handlers via the agent module's
    // exported test driver, threading the test `deps` (real or `stub`-stubbed).
    let target_ns = target_name.replace('.', "_");
    out.push_str(&format!(
        "    const __drive = (seq: any[]) => ({target_ns} as any).__bynkDriveHistory_{agent_name}(seq, deps);\n"
    ));

    let rel_path_fwd = rel_path.replace('\\', "/");
    out.push_str(&format!(
        "    return await __bynkRunHistory({{ seed: __bynkMix(__bynkSeed, {prop_ordinal}), cases: 60, maxLen: 16, handlers: __handlers, drive: __drive, body: __body, name: \"{}\", location: \"{}\", file: \"{}\" }});\n",
        emitter::escape_ts_string(&prop.name),
        emitter::escape_ts_string(&rel_path_fwd),
        emitter::escape_ts_string(&rel_path_fwd),
    ));
    out.push_str("}\n");
    out
}

/// v0.115 (testing track slice 3): emit one async runner that *attacks* a
/// contracted free function — generate arguments over the parameter domains
/// (v0.114 engine), filter by the conjunction of `requires` (`__where`), call
/// the function (`__body`), and let the dev/test call-site guard assert each
/// `ensures`, throwing a shrinkable `BynkContractError` on violation. Mirrors
/// [`emit_test_property_function`], but the body is a direct call (the guard is
/// the assertion) with `Int` arguments coerced to `number` (the generator makes
/// `bigint`; functions do `number` arithmetic).
#[allow(clippy::too_many_arguments)]
fn emit_contract_attack_function(
    runner_name: &str,
    f: &FnDecl,
    resolved: &ResolvedCommons,
    prop_ordinal: usize,
    target_name: &str,
    target_kind: UnitKind,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    rel_path: &str,
) -> String {
    let FnName::Free(fname) = &f.name else {
        return String::new();
    };
    let mut out = String::new();
    out.push_str(&format!("async function {runner_name}() {{\n"));
    emit_test_scope_setup(
        &mut out,
        target_name,
        target_kind,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
        false,
    );
    let _ = target_kind;

    // Generator descriptors, one per parameter, over the target's privileged view.
    out.push_str("    const __gens = [\n");
    for p in &f.params {
        let bg = checker::resolve_type_ref(&p.type_ref, &resolved.types)
            .map(|t| binding_gen(&t, &resolved.types))
            .unwrap_or(BindingGen {
                boundaries: Vec::new(),
                gen_ts: "undefined".to_string(),
                shrink: "[]".to_string(),
            });
        out.push_str(&format!(
            "      {{ name: \"{}\", boundaries: [{}], gen: (rng: any) => {}, shrink: (v: any) => {}, show: (v: any) => __bynkShow(v) }},\n",
            emitter::escape_ts_string(&p.name.name),
            bg.boundaries.join(", "),
            bg.gen_ts,
            bg.shrink
        ));
    }
    out.push_str("    ];\n");

    let param_names: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
    let destructure = format!("const [{}] = __vals;", param_names.join(", "));

    // `__where` — the conjunction of `requires`, lowered over the parameter tuple
    // (comparisons tolerate the `bigint`/`number` split, so no coercion here).
    let where_pred = f.requires.iter().rev().fold(None, |acc: Option<Expr>, c| {
        Some(match acc {
            None => c.predicate.clone(),
            Some(rest) => Expr {
                kind: ExprKind::BinOp(BinOp::And, Box::new(c.predicate.clone()), Box::new(rest)),
                span: f.span,
            },
        })
    });
    if let Some(w) = where_pred {
        let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
        let cross = bynk_check::resolver::CrossContextInfo::default();
        let synth = Block {
            statements: Vec::new(),
            tail: Box::new(w.clone()),
            span: w.span,
            tail_leading_comments: Vec::new(),
            implicit_tail: false,
        };
        let (src, _) = emitter::lower_block_to_async_body(
            &synth,
            &TypeRef::Base(BaseType::Bool, w.span),
            &mut typed,
            &cross,
        );
        out.push_str("    const __where = (__vals: any[]) => {\n");
        out.push_str(&format!("      {destructure}\n"));
        for line in src.lines() {
            out.push_str("      ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    };\n");
    } else {
        out.push_str("    const __where = null;\n");
    }

    // `__body` — call the (guarded) function with coerced arguments. `Int`
    // parameters generate a `bigint`; coerce to `number` so the function's
    // arithmetic doesn't mix types. The guard asserts the `ensures`.
    let call_args: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let base = checker::resolve_type_ref(&p.type_ref, &resolved.types)
                .and_then(|t| numeric_or_scalar_base(&t, &resolved.types));
            if base == Some(BaseType::Int) {
                format!("Number({})", p.name.name)
            } else {
                p.name.name.clone()
            }
        })
        .collect();
    out.push_str("    const __body = async (__vals: any[]) => {\n");
    out.push_str(&format!("      {destructure}\n"));
    out.push_str(&format!(
        "      {}({});\n",
        fname.name,
        call_args.join(", ")
    ));
    out.push_str("    };\n");

    let rel_path_fwd = rel_path.replace('\\', "/");
    let name = format!("contract {}", fname.name);
    out.push_str(&format!(
        "    return await __bynkRunProperty({{ seed: __bynkMix(__bynkSeed, {prop_ordinal}), cases: 100, gens: __gens, where: __where, body: __body, name: \"{}\", location: \"{}\", file: \"{}\" }});\n",
        emitter::escape_ts_string(&name),
        emitter::escape_ts_string(&rel_path_fwd),
        emitter::escape_ts_string(&rel_path_fwd),
    ));
    out.push_str("}\n");
    out
}

pub(crate) fn emit_test_main(tests: &[RunnableTest], import_ext: ImportExt) -> String {
    let ext = import_ext.as_str();
    let mut out = String::new();
    out.push_str("// Generated by bynkc — do not edit by hand.\n");
    out.push_str("// top-level test runner\n\n");
    // Node's `process` global isn't declared without @types/node. The runner
    // uses `process.exit` and reads `process.env.BYNK_TEST_FORMAT` (v0.59: set
    // to `ndjson` by `bynkc test --format json`), so we narrow the global with a
    // minimal ambient declaration rather than pulling in a dependency.
    out.push_str(
        "declare const process: { exit(code: number): never; env: { [k: string]: string | undefined } };\n\n",
    );
    let mut sorted: Vec<&RunnableTest> = tests.iter().collect();
    sorted.sort_by(|a, b| a.target_name.cmp(&b.target_name));
    for (i, t) in sorted.iter().enumerate() {
        let module_stem = t
            .module_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("test");
        out.push_str(&format!(
            "import * as test_{i} from \"./{module_stem}.{ext}\";\n"
        ));
    }
    out.push('\n');
    out.push_str("async function main() {\n");
    // v0.127 (editor-currency slice 6): `bynkc test --case <name>` sets
    // BYNK_TEST_CASE, threaded here as an `only` filter into every suite's
    // `run(only)`. Unset runs the whole project unchanged.
    out.push_str("  const only = process.env.BYNK_TEST_CASE;\n");
    out.push_str("  const modules = [\n");
    for (i, t) in sorted.iter().enumerate() {
        out.push_str(&format!(
            "    {{ name: \"{}\", run: test_{i}.run }},\n",
            t.target_name
        ));
    }
    out.push_str("  ];\n");
    out.push_str("  let passed = 0;\n");
    out.push_str("  let failed = 0;\n");
    // v0.59: `--format json` sets BYNK_TEST_FORMAT=ndjson and captures stdout;
    // the runner then emits one JSON event per line (an internal protocol the
    // driver re-renders into the pinned document). Otherwise the human ✓ / ✗
    // output is byte-for-byte unchanged.
    out.push_str("  const PREFIX = \"integration \\u00b7 \";\n");
    out.push_str("  if (process.env.BYNK_TEST_FORMAT === \"ndjson\") {\n");
    out.push_str("    const emit = (o: unknown) => console.log(JSON.stringify(o));\n");
    out.push_str("    emit({ type: \"run-begin\", suites: modules.length });\n");
    out.push_str("    for (const m of modules) {\n");
    out.push_str("      const integration = m.name.startsWith(PREFIX);\n");
    out.push_str("      const suite = integration ? m.name.slice(PREFIX.length) : m.name;\n");
    out.push_str("      const kind = integration ? \"integration\" : \"unit\";\n");
    out.push_str("      const results = await m.run(only);\n");
    out.push_str(
        "      emit({ type: \"suite-begin\", name: suite, kind, tests: results.length });\n",
    );
    out.push_str("      for (const r of results) {\n");
    out.push_str("        if (r.pass) {\n");
    out.push_str("          passed++;\n");
    out.push_str("          emit({ type: \"case\", suite, name: r.name, outcome: \"pass\" });\n");
    out.push_str("        } else {\n");
    out.push_str("          failed++;\n");
    out.push_str(
        "          emit({ type: \"case\", suite, name: r.name, outcome: \"fail\", message: r.error && r.error.message, location: r.error && r.error.location });\n",
    );
    out.push_str("        }\n");
    out.push_str("      }\n");
    out.push_str("      emit({ type: \"suite-end\", name: suite });\n");
    out.push_str("    }\n");
    out.push_str("    emit({ type: \"run-end\", passed, failed });\n");
    out.push_str("  } else {\n");
    out.push_str("    console.log(\"Running tests...\\n\");\n");
    out.push_str("    for (const m of modules) {\n");
    out.push_str("      console.log(`${m.name}:`);\n");
    out.push_str("      const results = await m.run(only);\n");
    out.push_str("      for (const r of results) {\n");
    out.push_str(
        "        if (r.pass) { passed++; console.log(`  \\u2713 ${r.name}`); } else { failed++; console.log(`  \\u2717 ${r.name}`); if (r.error) console.log(`    ${r.error.message}`); }\n",
    );
    out.push_str("      }\n");
    out.push_str("      console.log(\"\");\n");
    out.push_str("    }\n");
    out.push_str("    console.log(`${passed} passed, ${failed} failed.`);\n");
    out.push_str("  }\n");
    out.push_str("  if (failed > 0) process.exit(1);\n");
    out.push_str("}\n\n");
    out.push_str("main();\n");
    out
}

fn sanitise_case_name(name: &str, index: &mut usize) -> String {
    let mut s = String::from("test_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
        } else {
            s.push('_');
        }
    }
    if s == "test_" {
        s.push_str(&index.to_string());
    }
    *index += 1;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_syntax::ast::{BaseType, Ident, TypeRef};
    use std::collections::HashSet;

    // -- sanitise_suite / sanitise_case_name ----------------------------------
    #[test]
    fn sanitise_suite_lowercases_collapses_and_trims() {
        assert_eq!(sanitise_suite("My Suite"), "my_suite");
        assert_eq!(sanitise_suite("Foo__Bar"), "foo_bar");
        assert_eq!(sanitise_suite("  Hello  "), "hello");
        assert_eq!(sanitise_suite("a1B2"), "a1b2");
        assert_eq!(sanitise_suite("!!!"), "suite"); // empty after trim -> fallback
        assert_eq!(sanitise_suite(""), "suite");
    }

    // v0.127 (editor-currency slice 6): the top-level runner reads
    // `BYNK_TEST_CASE` and threads it into every suite's `run(only)`, so a
    // `bynkc test --case <name>` filters which cases execute.
    #[test]
    fn emit_test_main_threads_the_case_filter() {
        let tests = vec![RunnableTest {
            target_name: "commerce.money".to_string(),
            module_path: std::path::PathBuf::from("tests/commerce_money.test.ts"),
            kind: "unit",
            suite_name: "commerce.money".to_string(),
            cases: vec![DiscoveredCase {
                name: "rounds".to_string(),
                location: None,
            }],
        }];
        let out = emit_test_main(&tests, ImportExt::Js);
        assert!(
            out.contains("const only = process.env.BYNK_TEST_CASE;"),
            "the runner must read the case filter from the environment, got:\n{out}"
        );
        // Threaded into both the NDJSON and human dispatch loops.
        assert_eq!(
            out.matches("await m.run(only)").count(),
            2,
            "both dispatch branches must pass the filter to each suite's run(), got:\n{out}"
        );
    }

    #[test]
    fn sanitise_case_name_prefixes_and_advances_index() {
        let mut idx = 0;
        assert_eq!(
            sanitise_case_name("hello world", &mut idx),
            "test_hello_world"
        );
        assert_eq!(idx, 1); // index advances on every call
        assert_eq!(sanitise_case_name("a-b.c", &mut idx), "test_a_b_c");
        assert_eq!(idx, 2);
    }

    #[test]
    fn sanitise_case_name_appends_index_only_for_empty_name() {
        let mut idx = 5;
        assert_eq!(sanitise_case_name("", &mut idx), "test_5"); // empty -> index suffix
        assert_eq!(idx, 6);
        // A name of only non-alphanumeric chars is NOT "test_", so no suffix.
        let mut idx2 = 9;
        assert_eq!(sanitise_case_name(" ", &mut idx2), "test__");
        assert_eq!(idx2, 10);
    }

    // -- the unified emitter type-ref renderers -------------------------------
    fn named(n: &str) -> TypeRef {
        TypeRef::Named(Ident {
            name: n.to_string(),
            span: Span::default(),
        })
    }
    fn base(b: BaseType) -> TypeRef {
        TypeRef::Base(b, Span::default())
    }

    #[test]
    fn ts_type_ref_bases_and_generics() {
        assert_eq!(emitter::ts_type_ref(&base(BaseType::Int)), "number");
        assert_eq!(emitter::ts_type_ref(&base(BaseType::Float)), "number");
        assert_eq!(emitter::ts_type_ref(&base(BaseType::String)), "string");
        assert_eq!(emitter::ts_type_ref(&base(BaseType::Bool)), "boolean");
        assert_eq!(emitter::ts_type_ref(&named("Order")), "Order");
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::List(Box::new(named("Order")), Span::default())),
            "readonly Order[]"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::Option(
                Box::new(base(BaseType::Int)),
                Span::default()
            )),
            "Option<number>"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::Effect(
                Box::new(TypeRef::Unit(Span::default())),
                Span::default()
            )),
            "Promise<void>"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::Map(
                Box::new(base(BaseType::String)),
                Box::new(named("V")),
                Span::default()
            )),
            "ReadonlyMap<string, V>"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::Result(
                Box::new(named("T")),
                Box::new(named("E")),
                Span::default()
            )),
            "Result<T, E>"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::HttpResult(Box::new(named("T")), Span::default())),
            "HttpResult<T>"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::ValidationError(Span::default())),
            "ValidationError"
        );
        assert_eq!(
            emitter::ts_type_ref(&TypeRef::JsonError(Span::default())),
            "JsonError"
        );
    }

    #[test]
    fn ts_type_ref_fn_uses_positional_param_names() {
        let f = TypeRef::Fn(
            vec![base(BaseType::Int), named("Order")],
            Box::new(TypeRef::Unit(Span::default())),
            Span::default(),
        );
        assert_eq!(emitter::ts_type_ref(&f), "(a0: number, a1: Order) => void");
    }

    #[test]
    fn ts_type_ref_qualified_prefixes_only_scoped_names() {
        let mut scope: HashSet<String> = HashSet::new();
        scope.insert("Order".to_string());
        // A named type in the privileged scope is qualified with the namespace.
        assert_eq!(
            emitter::ts_type_ref_qualified(&named("Order"), &scope, "Ns"),
            "Ns.Order"
        );
        // A named type outside the scope is left bare.
        assert_eq!(
            emitter::ts_type_ref_qualified(&named("Other"), &scope, "Ns"),
            "Other"
        );
        // Qualification recurses through generic arguments.
        assert_eq!(
            emitter::ts_type_ref_qualified(
                &TypeRef::List(Box::new(named("Order")), Span::default()),
                &scope,
                "Ns"
            ),
            "readonly Ns.Order[]"
        );
        // Base types are unaffected by qualification.
        assert_eq!(
            emitter::ts_type_ref_qualified(&base(BaseType::Int), &scope, "Ns"),
            "number"
        );
    }
}
