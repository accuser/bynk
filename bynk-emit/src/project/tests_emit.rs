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
        TypeRef::Map(k, v, _) => format!(
            "Map[{}, {}]",
            ts_type_ref_display(k),
            ts_type_ref_display(v)
        ),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
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

// -- v0.7: test declaration processing --

/// Classification of a mock target inside a test declaration.
#[derive(Debug, Clone)]
enum MockTarget {
    /// Provider mock — replaces a capability the target context declares.
    Capability(String),
    /// Consumed-context mock — replaces the consumed context with the given
    /// qualified name (resolved through the target context's `consumes`
    /// table, including aliases).
    ConsumedContext { qualified: String, alias: String },
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
    tests_prefix: &Path,
    import_ext: ImportExt,
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
                        "bynk.test.unknown_target",
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
                                "bynk.test.duplicate_case_name",
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

        // -- Phase 3: validate mocks --
        let mut target_mocks: HashMap<String, ResolvedMock> = HashMap::new();
        // The target's per-context info we'll use during mock resolution.
        let target_table = unit_tables.get(target_name);
        let target_aliases_map = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let target_consumed = unit_consumes.get(target_name).cloned().unwrap_or_default();

        for &i in indices {
            let Some(t) = parsed[i].test() else { continue };
            for mock in &t.mocks {
                // Tests targeting a commons have no providers and no
                // consumed contexts to mock.
                if target_kind == UnitKind::Commons {
                    errors.push(
                        CompileError::new(
                            "bynk.mock.in_commons_test",
                            mock.span,
                            format!(
                                "`mocks` declarations are not allowed in a test of commons `{target_name}` — commons have no providers or consumes to replace",
                            ),
                        )
                        .with_note(
                            "remove the mock, or move the test to target a context",
                        ),
                    );
                    continue;
                }
                if let Some(prev) = target_mocks.get(&mock.target_name.name) {
                    errors.push(
                        CompileError::new(
                            "bynk.mock.duplicate_target",
                            mock.target_name.span,
                            format!(
                                "name `{}` is mocked more than once in tests of `{target_name}`",
                                mock.target_name.name
                            ),
                        )
                        .with_label(prev.decl.span, "previously mocked here"),
                    );
                    continue;
                }
                // Disambiguate capability vs consumed-context.
                let cap_match =
                    target_table.and_then(|tbl| tbl.capabilities.get(&mock.target_name.name));
                let alias_match = target_aliases_map.get(&mock.target_name.name).cloned();
                let qualified_match = target_consumed
                    .iter()
                    .find(|q| {
                        q.as_str() == mock.target_name.name
                            || q.rsplit('.').next() == Some(mock.target_name.name.as_str())
                    })
                    .cloned();
                let resolution: Option<MockTarget> = if cap_match.is_some() {
                    Some(MockTarget::Capability(mock.target_name.name.clone()))
                } else if let Some(qual) = alias_match {
                    Some(MockTarget::ConsumedContext {
                        qualified: qual,
                        alias: mock.target_name.name.clone(),
                    })
                } else {
                    qualified_match.map(|qual| MockTarget::ConsumedContext {
                        qualified: qual,
                        alias: mock.target_name.name.clone(),
                    })
                };
                let resolved_target = match resolution {
                    Some(r) => r,
                    None => {
                        errors.push(
                            CompileError::new(
                                "bynk.mock.unknown_target",
                                mock.target_name.span,
                                format!(
                                    "`{}` is not a capability of context `{target_name}` and not a consumed-context alias",
                                    mock.target_name.name
                                ),
                            )
                            .with_note(
                                "mocks must target either a capability declared in the test's target context, or the alias / qualified name of a consumed context",
                            ),
                        );
                        continue;
                    }
                };

                // -- Phase 3: validate signatures --
                let signature_errs =
                    check_mock_signatures(mock, &resolved_target, target_name, unit_tables);
                let had_sig_err = !signature_errs.is_empty();
                errors.extend(signature_errs);

                target_mocks.insert(
                    mock.target_name.name.clone(),
                    ResolvedMock {
                        decl: mock.clone(),
                        target: resolved_target,
                        had_sig_err,
                        source_path: parsed[i].source_path.clone(),
                    },
                );
            }
        }

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
            &target_mocks,
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
            &target_mocks,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
            exports_visibility,
            tests_prefix,
            import_ext,
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
        }
    }

    // v0.16: the top-level `tests/main.ts` runner is emitted once by the caller
    // after both unit- and integration-test passes, so it can aggregate both.
    (outputs, runnable_tests)
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
    tests_prefix: &Path,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) -> (Vec<CompiledFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<CompiledFile> = Vec::new();
    let mut runnables: Vec<RunnableTest> = Vec::new();

    let mut sorted: Vec<&String> = integration_groups.keys().collect();
    sorted.sort();

    let mut seen_suites: HashMap<String, Span> = HashMap::new();

    for group_name in sorted {
        let indices = integration_groups.get(group_name).unwrap();
        // Each suite is a single declaration. Two declarations sharing a name
        // collide into one group → a duplicate suite.
        let first = indices[0];
        let Some(decl) = parsed[first].integration() else {
            continue;
        };
        let mut duplicate = false;
        if let Some(prev) = seen_suites.get(&decl.suite) {
            duplicate = true;
            errors.push(
                CompileError::new(
                    "bynk.integration.duplicate_suite",
                    decl.suite_span,
                    format!(
                        "integration test `\"{}\"` is declared more than once",
                        decl.suite
                    ),
                )
                .with_label(*prev, "previously declared here"),
            );
        } else {
            seen_suites.insert(decl.suite.clone(), decl.suite_span);
        }
        for &i in &indices[1..] {
            if let Some(other) = parsed[i].integration() {
                errors.push(
                    CompileError::new(
                        "bynk.integration.duplicate_suite",
                        other.suite_span,
                        format!(
                            "integration test `\"{}\"` is declared more than once",
                            other.suite
                        ),
                    )
                    .with_label(decl.suite_span, "previously declared here"),
                );
                duplicate = true;
            }
        }

        // -- Validate participants. --
        let mut participants: Vec<String> = Vec::new();
        let mut participant_set: HashSet<String> = HashSet::new();
        let mut bad_participant = false;
        for p in &decl.participants {
            let q = p.joined();
            match kinds.get(&q) {
                Some(UnitKind::Context) => {}
                _ => {
                    errors.push(
                        CompileError::new(
                            "bynk.integration.unknown_participant",
                            p.span,
                            format!("`{q}` is not a declared context in this project"),
                        )
                        .with_note(
                            "every name in a `wires` clause must be a context the project declares",
                        ),
                    );
                    bad_participant = true;
                    continue;
                }
            }
            if !participant_set.insert(q.clone()) {
                errors.push(CompileError::new(
                    "bynk.integration.duplicate_participant",
                    p.span,
                    format!("context `{q}` is listed more than once in `wires`"),
                ));
                continue;
            }
            participants.push(q);
        }

        if participant_set.len() < 2 {
            errors.push(
                CompileError::new(
                    "bynk.integration.too_few_participants",
                    decl.suite_span,
                    "an integration test must wire at least two contexts",
                )
                .with_note(
                    "to test a single context in isolation, use a unit test (`test <context> { … }`)",
                ),
            );
            bad_participant = true;
        }

        // -- Closure: every transitively-consumed context must be a participant.
        for p in &participants {
            if let Some(deps) = unit_consumes.get(p) {
                for d in deps {
                    if !participant_set.contains(d) {
                        errors.push(
                            CompileError::new(
                                "bynk.integration.unwired_dependency",
                                decl.suite_span,
                                format!(
                                    "participant `{p}` consumes `{d}`, which is not wired into this integration test",
                                ),
                            )
                            .with_note(format!(
                                "add `{d}` to the `wires` clause — an integration test runs each participant as a real Worker, so every consumed context needs one",
                            )),
                        );
                        bad_participant = true;
                    }
                }
            }
        }

        // -- Duplicate case names within the suite. --
        let mut seen_cases: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            if let Some(d) = parsed[i].integration() {
                for case in &d.cases {
                    if let Some(prev) = seen_cases.get(&case.name) {
                        errors.push(
                            CompileError::new(
                                "bynk.test.duplicate_case_name",
                                case.name_span,
                                format!(
                                    "test case `\"{}\"` is declared more than once in integration test `\"{}\"`",
                                    case.name, decl.suite
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                        bad_participant = true;
                    } else {
                        seen_cases.insert(case.name.clone(), case.name_span);
                    }
                }
            }
        }

        if duplicate || bad_participant {
            continue;
        }

        // -- Build the harness-root cross-context view (consumes all). --
        let harness_name = group_name.clone();
        let uses_targets: Vec<String> = decl.uses.iter().map(|u| u.target.joined()).collect();
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
        let decl_rel_path = tests_prefix.join(&parsed[first].source_path);
        let decl_rel_path = decl_rel_path.to_string_lossy();
        if let Some((path, source, source_map, runnable)) = emit_integration_module(
            decl,
            &participants,
            &uses_targets,
            &cross_context,
            unit_consumes,
            unit_tables,
            &parsed[first].source,
            &decl_rel_path,
            &parsed[first].map_source_name(),
        ) {
            outputs.push(CompiledFile {
                source_path: path.clone(),
                output_path: path,
                typescript: source,
                source_map,
                debug_metadata: None,
            });
            runnables.push(runnable);
        }
    }

    (outputs, runnables)
}

/// Type-check one integration test case body. The body lives in a synthetic
/// harness root that consumes every participant; entry calls
/// (`ctx.service(args)`) are therefore ordinary cross-context calls. The body
/// has type `Effect[Result[(), AssertionError]]` (modelled as
/// `Effect[Result[(), ValidationError]]`, as in unit tests).
fn check_integration_case_body(
    participants: &[String],
    uses_targets: &[String],
    case: &TestCase,
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
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        refs,
        hints: &mut no_hints,
        locals: &mut no_locals,
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
    };
    let _ = checker::type_of_block(&case.body, Some(&return_ty), &mut ctx);
}

/// Emit a single integration-test module plus its [`RunnableTest`] pointer.
/// The module imports each participant's workers-mode handler namespace (for
/// serialise/deserialise) and Worker entry (for dispatch), builds an in-process
/// env graph wiring the Service Bindings, and runs each case across the wire.
#[allow(clippy::too_many_arguments)]
fn emit_integration_module(
    decl: &IntegrationDecl,
    participants: &[String],
    uses_targets: &[String],
    cross_context: &resolver::CrossContextInfo,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    source: &str,
    rel_path: &str,
    map_source: &str,
) -> Option<(PathBuf, String, Option<String>, RunnableTest)> {
    let sanitized = sanitise_suite(&decl.suite);
    let module_path = PathBuf::from(format!("tests/integration_{sanitized}.test.ts"));
    let mut out = String::new();
    // v0.70: the integration module's source map (single source — the suite file).
    // v0.72: keyed on the suite file's absolute path (`map_source`) so an editor
    // breakpoint on the real `.bynk` binds; `rel_path` stays the test-runner
    // location (project-relative, resolved against the workspace).
    let mut module_smb = SourceMapBuilder::new();
    let src_id = module_smb.add_source(map_source.to_string(), source.to_string());
    out.push_str("// Generated by bynkc — do not edit by hand.\n");
    out.push_str(&format!("// integration test: {}\n\n", decl.suite));

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

    out.push_str(&assertion_runtime_helpers());

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
    for case in &decl.cases {
        discovered.push(DiscoveredCase {
            name: case.name.clone(),
            location: Some(discovered_location(source, rel_path, case.name_span)),
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
            source,
            rel_path,
        );
        let body_base = out.len();
        for line in body_src.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        module_smb.merge(&body_smb, &body_src, &out, body_base, src_id);
        out.push_str("    return { pass: true };\n");
        out.push_str("  } catch (e) {\n");
        out.push_str("    if (e instanceof AssertionError) {\n");
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

    // Module runner.
    out.push_str("export async function run() {\n");
    out.push_str("  const results = [];\n");
    for (idx, case) in decl.cases.iter().enumerate() {
        let runner_name = &case_runners[idx];
        let escaped = emitter::escape_ts_string(&case.name);
        out.push_str(&format!(
            "  results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
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
            target_name: format!("integration · {}", decl.suite),
            module_path,
            kind: "integration",
            suite_name: decl.suite.clone(),
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

#[derive(Debug, Clone)]
struct ResolvedMock {
    decl: MockDecl,
    target: MockTarget,
    had_sig_err: bool,
    /// The test file declaring the mock — the recording context for edges
    /// in its op bodies (v0.25).
    source_path: PathBuf,
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

fn check_mock_signatures(
    mock: &MockDecl,
    target: &MockTarget,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    match target {
        MockTarget::Capability(cap_name) => {
            let Some(table) = unit_tables.get(target_name) else {
                return errors;
            };
            let Some(cap) = table.capabilities.get(cap_name) else {
                return errors;
            };
            for cap_op in &cap.ops {
                if !mock.ops.iter().any(|o| o.name.name == cap_op.name.name) {
                    errors.push(CompileError::new(
                        "bynk.mock.signature_mismatch",
                        mock.span,
                        format!(
                            "mock `{}` for capability `{}` is missing operation `{}`",
                            mock.impl_name.name, cap_name, cap_op.name.name
                        ),
                    ));
                }
            }
            for op in &mock.ops {
                let Some(cap_op) = cap.ops.iter().find(|o| o.name.name == op.name.name) else {
                    errors.push(CompileError::new(
                        "bynk.mock.signature_mismatch",
                        op.span,
                        format!(
                            "mock operation `{}.{}` does not match any operation in capability `{}`",
                            mock.impl_name.name, op.name.name, cap_name
                        ),
                    ));
                    continue;
                };
                check_mock_op_signature(op, &cap_op.params, &cap_op.return_type, &mut errors);
            }
        }
        MockTarget::ConsumedContext { qualified, .. } => {
            let Some(table) = unit_tables.get(qualified) else {
                return errors;
            };
            // Each mock op must match a service in the consumed context.
            for op in &mock.ops {
                let Some(service) = table.services.get(&op.name.name) else {
                    errors.push(CompileError::new(
                        "bynk.mock.signature_mismatch",
                        op.span,
                        format!(
                            "mock operation `{}.{}` does not match any service in consumed context `{qualified}`",
                            mock.impl_name.name, op.name.name
                        ),
                    ));
                    continue;
                };
                // Find an `on call` handler and compare signatures.
                let Some(handler) = service
                    .handlers
                    .iter()
                    .find(|h| matches!(h.kind, HandlerKind::Call))
                else {
                    errors.push(CompileError::new(
                        "bynk.mock.signature_mismatch",
                        op.span,
                        format!(
                            "service `{}` in consumed context `{qualified}` has no `on call` handler to mock",
                            op.name.name
                        ),
                    ));
                    continue;
                };
                check_mock_op_signature(op, &handler.params, &handler.return_type, &mut errors);
            }
        }
    }
    errors
}

fn check_mock_op_signature(
    op: &MockOp,
    target_params: &[Param],
    target_return: &TypeRef,
    errors: &mut Vec<CompileError>,
) {
    if op.params.len() != target_params.len() {
        errors.push(CompileError::new(
            "bynk.mock.signature_mismatch",
            op.span,
            format!(
                "mock operation `{}` has {} parameter(s), but the target declares {}",
                op.name.name,
                op.params.len(),
                target_params.len()
            ),
        ));
        return;
    }
    for (i, (target_p, mock_p)) in target_params.iter().zip(op.params.iter()).enumerate() {
        if !type_refs_match(&target_p.type_ref, &mock_p.type_ref) {
            errors.push(CompileError::new(
                "bynk.mock.signature_mismatch",
                mock_p.span,
                format!(
                    "mock operation `{}` parameter {} has type `{}`, but the target declares `{}`",
                    op.name.name,
                    i + 1,
                    ts_type_ref_display(&mock_p.type_ref),
                    ts_type_ref_display(&target_p.type_ref),
                ),
            ));
        }
    }
    if !type_refs_match(target_return, &op.return_type) {
        errors.push(CompileError::new(
            "bynk.mock.signature_mismatch",
            op.return_type.span(),
            format!(
                "mock operation `{}` returns `{}`, but the target declares `{}`",
                op.name.name,
                ts_type_ref_display(&op.return_type),
                ts_type_ref_display(target_return),
            ),
        ));
    }
}

/// Type-check all mocks and test bodies for a target. Bodies use the target's
/// privileged view; consumed-context mock bodies use the consumed context's
/// privileged view.
#[allow(clippy::too_many_arguments)]
fn check_test_bodies(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    refs: &mut RefSink,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    let _ = exports_visibility;

    // Type-check mock bodies. Provider mock bodies share the target context's
    // privileges; consumed-context mock bodies use the consumed context's
    // privileged view (so they can construct opaque types from there).
    for mock_entry in mocks.values() {
        if mock_entry.had_sig_err {
            continue;
        }
        let owning_unit = match &mock_entry.target {
            MockTarget::Capability(_) => target_name.to_string(),
            MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
        };
        // v0.25: mock op bodies record in the declaring test file, resolving
        // bare names through the owning unit's namespace.
        refs.enter_file(&mock_entry.source_path, &owning_unit, false);
        for op in &mock_entry.decl.ops {
            check_op_body_with_privileged_view(
                &owning_unit,
                op,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &mut errors,
                /* in_test_body */ false,
                refs,
            );
        }
    }

    // Type-check test case bodies — they live in the target's privileged
    // view, with mocked surfaces replacing the target's normal providers /
    // consumed contexts.
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
    }

    errors
}

#[allow(clippy::too_many_arguments)]
fn check_op_body_with_privileged_view(
    owning_unit: &str,
    op: &MockOp,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
    in_test_body: bool,
    refs: &mut RefSink,
) {
    let Some((resolved, _)) = build_privileged_resolved(
        owning_unit,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    checker::check_handler_body(
        &op.body,
        &op.return_type,
        op.return_type.span(),
        &op.params,
        &resolved,
        &mut expr_types,
        errors,
        refs,
        // Mock op bodies live in test files — out of v0.27 hint scope.
        &mut HintSink::new(),
        &mut LocalsSink::new(),
        HashMap::new(),
        HashMap::new(),
        None,
        None,
        &[],
        None,
        false,
        None,
    );
    let _ = in_test_body; // Mock op bodies are not test bodies; assert is not valid here.
}

#[allow(clippy::too_many_arguments)]
fn check_test_case_body(
    target_name: &str,
    target_kind: UnitKind,
    case: &TestCase,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
) {
    let Some((resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    let _ = target_kind;
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    // Synthesise an Effect[Result[(), ValidationError]] return type as a
    // stand-in for Effect[Result[(), AssertionError]]. v0.7 doesn't model an
    // explicit AssertionError type — the runtime catches it instead.
    let unit_span = case.span;
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
    let return_ty_span = case.span;
    let effectful = matches!(return_ty, checker::Ty::Effect(_));
    // Test bodies record no hints (out of v0.27 scope) — a throwaway sink.
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        refs,
        hints: &mut no_hints,
        locals: &mut no_locals,
        scopes: vec![HashMap::new()],
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
    };
    let _ = checker::type_of_block(&case.body, Some(&return_ty), &mut ctx);
    // Don't enforce return-type equality; the test runner discards the
    // tail expression and recovers success/failure from assertion outcome.
    // Don't enforce "every given used" — capabilities are implicitly
    // available in a test body.
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
    };
    Some((resolved, ()))
}

/// Emit a single test module TypeScript file plus the [`RunnableTest`]
/// pointer used by the top-level runner.
#[allow(clippy::too_many_arguments)]
fn emit_test_module(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    tests_prefix: &Path,
    import_ext: ImportExt,
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

    // Assertion helper used by lowered `assert` statements.
    out.push_str(&assertion_runtime_helpers());

    // Emit mock implementations. Sort by target name so emission is
    // deterministic regardless of the mock map's hash iteration order (a test
    // with more than one mock would otherwise flake).
    let mut sorted_mocks: Vec<(&String, &ResolvedMock)> = mocks.iter().collect();
    sorted_mocks.sort_by(|a, b| a.0.cmp(b.0));
    for (_, mock) in sorted_mocks {
        out.push_str(&emit_mock_class(
            mock,
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
        mocks,
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
            let (case_text, case_smb) = emit_test_case_function(
                &runner_name,
                case,
                target_name,
                target_kind,
                mocks,
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

    // Module-level runner.
    out.push_str("export async function run() {\n");
    out.push_str("  const results = [];\n");
    let mut case_index = 0;
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            let runner_name = &case_runners[case_index];
            let escaped = emitter::escape_ts_string(&case.name);
            out.push_str(&format!(
                "  results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
            ));
            case_index += 1;
        }
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

fn assertion_runtime_helpers() -> String {
    let mut out = String::new();
    // Fields are declared and assigned explicitly rather than via TS parameter
    // properties: parameter properties are a transform-only construct that Node's
    // strip-only type-stripping rejects (ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX), and
    // `bynkc test --inspect` runs this `.ts` directly under strip-only Node (slice
    // 2, ADR 0104). The explicit form is equivalent and strip-clean.
    out.push_str("class AssertionError extends Error {\n");
    out.push_str("  location: string;\n");
    out.push_str("  start: number;\n");
    out.push_str("  end: number;\n");
    out.push_str("  constructor(location: string, start: number, end: number) {\n");
    out.push_str("    super(`assertion failed at ${location}`);\n");
    out.push_str("    this.location = location;\n");
    out.push_str("    this.start = start;\n");
    out.push_str("    this.end = end;\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(
        "function __bynkAssertionFailure(location: string, start: number, end: number) {\n",
    );
    out.push_str("  return new AssertionError(location, start, end);\n");
    out.push_str("}\n");
    out.push_str(
        "function __bynkAssert(cond: boolean, location: string, start: number, end: number): void {\n",
    );
    out.push_str("  if (!cond) { throw __bynkAssertionFailure(location, start, end); }\n");
    out.push_str("}\n\n");
    out
}

fn emit_mock_class(
    mock: &ResolvedMock,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let mut out = String::new();
    let impl_name = &mock.decl.impl_name.name;
    out.push_str(&format!("class {impl_name} {{\n"));
    // Bring the mocked entity's privileged namespace into local scope so the
    // body can reference its types and variants unqualified.
    let owning_unit = match &mock.target {
        MockTarget::Capability(_) => target_name.to_string(),
        MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
    };
    let scope_ns = owning_unit.replace('.', "_");
    let mut scope_type_names: HashSet<String> = unit_tables
        .get(&owning_unit)
        .map(|t| t.types.keys().cloned().collect())
        .unwrap_or_default();
    // v0.9.2: the owning context re-exports the commons types it `uses` under
    // its own namespace (branded), so a mock signature that names one — e.g.
    // `track(code: ShortCode)` — must qualify it to `<ns>.ShortCode` too.
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
    for op in &mock.decl.ops {
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
        out.push_str(&format!(
            "  async {}({params}): {return_ty} {{\n",
            op.name.name
        ));
        if !scope_names.is_empty() {
            out.push_str(&format!(
                "    const {{ {} }} = {scope_ns} as any;\n",
                scope_names.join(", ")
            ));
        }
        let body_src = emit_mock_op_body(
            op,
            mock,
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        );
        for line in body_src.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("  }\n");
    }
    out.push_str("}\n");
    out
}

/// Render a mock operation body using the same lowering the production
/// emitter applies to provider operations. We don't have direct access to
/// the typed-commons machinery here, so we hand-roll a small lowerer.
fn emit_mock_op_body(
    op: &MockOp,
    mock: &ResolvedMock,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    // For consumed-context mocks the body has the consumed context's
    // privileges; for provider mocks the body shares the target context.
    let owning_unit = match &mock.target {
        MockTarget::Capability(_) => target_name.to_string(),
        MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
    };
    // Run the type checker first so the lowering knows the type of each
    // expression (notably: variant constructor references).
    let mut typed = synthetic_typed_commons_for_target(&owning_unit, unit_tables, unit_uses);
    if let Some((resolved, _)) = build_privileged_resolved(
        &owning_unit,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        let mut errs: Vec<CompileError> = Vec::new();
        // Build-mode re-check for the lowering's expr types; the analyse
        // exit has already passed, so nothing records (fresh sink).
        checker::check_handler_body(
            &op.body,
            &op.return_type,
            op.return_type.span(),
            &op.params,
            &resolved,
            &mut typed.expr_types,
            &mut errs,
            &mut RefSink::new(),
            &mut HintSink::new(),
            &mut LocalsSink::new(),
            HashMap::new(),
            HashMap::new(),
            None,
            None,
            &[],
            None,
            false,
            None,
        );
    }
    let cross = bynk_check::resolver::CrossContextInfo::default();
    // v0.70: mock op bodies are collaborator scaffolding, not user test logic, so
    // their source map is discarded — they stay unmapped (a deliberate scope cut).
    emitter::lower_block_to_async_body(&op.body, &op.return_type, &mut typed, &cross).0
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
    }
}

fn emit_test_deps(
    target_name: &str,
    target_kind: UnitKind,
    mocks: &HashMap<String, ResolvedMock>,
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
            // Find a mock for this capability, otherwise fall back to the
            // declared provider.
            let entry = match mocks.get(cap) {
                Some(m) if matches!(m.target, MockTarget::Capability(_)) => {
                    format!("{cap}: new {}()", m.decl.impl_name.name)
                }
                _ => {
                    if let Some(provider) = table.providers.get(cap) {
                        format!("{cap}: new {ns}.{}()", provider.provider_name.name)
                    } else {
                        format!("{cap}: undefined as unknown as {ns}.{cap}")
                    }
                }
            };
            entries.push(entry);
        }
        // Cross-context surface: substitute mocks when present.
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
            let mock_for_key = mocks.values().find(|m| match &m.target {
                MockTarget::ConsumedContext { qualified, alias } => {
                    qualified == q && (alias == &key || alias == q)
                }
                _ => false,
            });
            if let Some(m) = mock_for_key {
                surface_entries.push(format!("{key}: new {}()", m.decl.impl_name.name));
            } else {
                let other_ns = q.replace('.', "_");
                surface_entries.push(format!(
                    "{key}: undefined as unknown as ReturnType<typeof {other_ns}.makeSurface>"
                ));
            }
        }
        if !surface_entries.is_empty() {
            entries.push(format!("surface: {{ {} }}", surface_entries.join(", ")));
        }
    }
    out.push_str(&format!("  return {{ {} }};\n", entries.join(", ")));
    out.push_str("}\n");
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_test_case_function(
    runner_name: &str,
    case: &TestCase,
    target_name: &str,
    target_kind: UnitKind,
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    source: &str,
    rel_path: &str,
) -> (String, SourceMapBuilder) {
    let _ = mocks;
    let mut out = String::new();
    let target_ns = target_name.replace('.', "_");
    out.push_str(&format!("async function {runner_name}() {{\n"));
    out.push_str("  try {\n");
    // v0.9.2: reset the target context's agent registries so each test sees a
    // fresh per-key state (finding #10's "fresh per test" half).
    let target_has_agents = unit_tables
        .get(target_name)
        .is_some_and(|t| !t.agents.is_empty());
    if target_has_agents {
        out.push_str(&format!("    {target_ns}.__resetAgents();\n"));
    }
    if target_kind == UnitKind::Context {
        out.push_str("    const deps = makeTestDeps();\n");
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
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
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
    out.push_str("    if (e instanceof AssertionError) {\n");
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
    out.push_str("      const results = await m.run();\n");
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
    out.push_str("      const results = await m.run();\n");
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
