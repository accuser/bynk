//! Test/integration-suite processing: `suite`/`test integration` checking +
//! TypeScript emission.
//!
//! P5.4 (`design/tracks/semantics-in-the-checker.md` §6): the checking half
//! of [`process_tests`]/[`process_integration_tests`] — target/participant
//! resolution, duplicate-case-name detection, `stub`-clause resolution, and
//! case/property body type-checking — relocated to
//! `bynk_check::test_suites` (as [`test_suites::phase_test_bodies`]/
//! [`test_suites::phase_integration_bodies`]), shared with
//! `bynk_check::analysis::analyse_project` so a `.bynk` test file gets
//! diagnostics and `RefSink` bindings (go-to-definition/find-references)
//! through the LSP again, closing category 7 of
//! `bynk-check/src/analysis.rs`'s own residual-gap accounting — the last of
//! the seven. This file keeps both functions' exact signatures (`run_checks`
//! needs no change) and everything Phase 5 (TypeScript emission) needs: the
//! two functions now call the relocated checking phase first, then emit only
//! for what it returns as ready. Several functions the checking phase needs
//! are genuinely dual-use — also needed here to lower a checked body to
//! TypeScript — and stayed `pub` in `bynk_check::test_suites` rather than
//! being duplicated; every call into that module below is qualified
//! (`test_suites::foo(...)`) for exactly that reason (see that module's own
//! doc comment for the full accounting).

use super::*;
use crate::emitter::RuntimeUse;
use bynk_check::checker::Types;
use bynk_check::test_suites::{self, ResolvedStub};
use bynk_syntax::ast::{
    ArgPattern, BaseType, BinOp, Block, CapabilityOp, Case, Commons, CommonsForm, CommonsItem,
    Expr, ExprKind, FnDecl, FnName, Ident, Param, PredKind, PropertyDecl, QualifiedName,
    Refinement, SeqOutcome, Statement, StubClause, StubRhs, Trivia, TypeBody, TypeDecl, TypeRef,
    Visibility,
};
use bynk_syntax::span::Span;
use bynk_ts::{
    SourceMapBuilder, TsArrowBody, TsBinaryOp, TsBindingName, TsClassMethod, TsDecl, TsExpr, TsLit,
    TsObjectEntry, TsParam, TsProgram, TsStmt, TsSwitchCase, TsType, TsTypeMember,
};
use std::sync::Arc;

// -- v0.7 / v0.118: test declaration processing --

/// P5.4: `test <target>` suite processing. The checking half (target/`stub`
/// resolution, duplicate-case-name detection, case/property body
/// type-checking — formerly this function's own Phases 2-4) now lives in
/// [`test_suites::phase_test_bodies`]; only a target that passed every gate
/// comes back, keyed by its resolved `stub` map, so the loop below is Phase
/// 5 (TypeScript emission) alone, over exactly the targets ready for it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_tests(
    test_groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    // v0.17 (Locale capability track, slice 1, #844): capability name -> owning
    // unit, per target, for capabilities flattened in via `consumes U { Cap }`
    // (this never includes anything a target declares itself). Needed so
    // `makeTestDeps()` wires an adapter-flattened capability (real *or*
    // stubbed) — it is not in `UnitTable.capabilities`, which holds only
    // locally-declared capabilities.
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    // v0.132: production unit name -> its `parsed` file indices, so a barrel can
    // resolve a multi-file commons the test module imports back to its files.
    groups: &BTreeMap<String, Vec<usize>>,
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
    tys: &Arc<Types>,
) -> (Vec<StagedFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<StagedFile> = Vec::new();
    let mut runnable_tests: Vec<RunnableTest> = Vec::new();

    let ready = test_suites::phase_test_bodies(
        test_groups,
        parsed,
        kinds,
        unit_tables,
        exports_visibility,
        unit_consumes,
        unit_consumes_aliases,
        unit_uses,
        errors,
        refs,
        tys,
    );

    let mut sorted_targets: Vec<&String> = ready.keys().collect();
    sorted_targets.sort();

    for target_name in sorted_targets {
        let target_stubs = &ready[target_name];
        // `ready` only contains targets `phase_test_bodies` resolved against
        // `kinds` — this lookup cannot miss.
        let target_kind = *kinds
            .get(target_name)
            .expect("phase_test_bodies only returns targets it resolved against `kinds`");
        let indices = test_groups.get(target_name).unwrap();

        // -- Phase 5: emit TypeScript test module. --
        let emit_out = emit_test_module(
            target_name,
            target_kind,
            indices,
            parsed,
            target_stubs,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
            unit_flattened,
            exports_visibility,
            import_ext,
            contracts,
            tys,
        );
        if let Some((path, source, source_map, runnable)) = emit_out {
            outputs.push(StagedFile {
                output_path: path,
                document: Document::Ts(bynk_ts::TsProgram {
                    stmts: vec![bynk_ts::TsStmt::verbatim(
                        bynk_ts::VerbatimOrigin::NotYetConverted,
                        source,
                        None,
                    )],
                }),
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

/// P5.4: `test integration "name"` suite processing. Validates the `wires`
/// participant set (existence, ≥ 2, no duplicates, full `consumes` closure),
/// type-checks each case body as a cross-context call from a synthetic
/// harness root that consumes every participant, and emits a TypeScript
/// module that stands the participants up as in-process Workers wired by
/// simulated Service Bindings and runs the cases across the real
/// serialise/deserialise wire. The checking half (participant inference, the
/// `system`-needs-a-serialisation-edge gate, duplicate-case-name detection,
/// the harness cross-context view, and per-case body type-checking) now
/// lives in [`test_suites::phase_integration_bodies`], which hands back each
/// ready group's harness [`resolver::CrossContextInfo`] — the one piece
/// worth not recomputing here (built from clone-heavy maps).
/// `participants`/`uses_targets`/`case_inputs` are cheap and pure, so the
/// loop below recomputes them exactly as this function always did, using the
/// now-relocated [`test_suites::infer_participants`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_integration_tests(
    integration_groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    // v0.132: production unit name -> its `parsed` file indices (see `process_tests`).
    groups: &BTreeMap<String, Vec<usize>>,
    // v0.132: barrel-path dedup set shared with the unit-test pass.
    emitted_barrels: &mut HashSet<PathBuf>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    tys: &Arc<Types>,
) -> (Vec<StagedFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<StagedFile> = Vec::new();
    let mut runnables: Vec<RunnableTest> = Vec::new();

    let _ = kinds;

    let ready = test_suites::phase_integration_bodies(
        integration_groups,
        parsed,
        unit_tables,
        unit_consumes,
        unit_consumes_aliases,
        unit_uses,
        errors,
        refs,
        tys,
    );

    let mut sorted: Vec<&String> = ready.keys().collect();
    sorted.sort();

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
        let participants = test_suites::infer_participants(&suite_target, unit_consumes);
        // `ready` only contains groups `phase_integration_bodies` built a
        // harness cross-context view for — this lookup cannot miss.
        let cross_context = ready
            .get(group_name)
            .expect("phase_integration_bodies only returns groups it resolved");

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

        // -- Emit the integration module. --
        // Collect each case with the fragment file it came from, so a suite
        // split across files maps each case body under its own source.
        let mut case_inputs: Vec<SystemCaseInput> = Vec::new();
        for &i in indices {
            let Some(d) = parsed[i].integration() else {
                continue;
            };
            let rel_path = parsed[i].identity_path().to_string_lossy().into_owned();
            let map_source = parsed[i].map_source_name();
            for case in &d.cases {
                case_inputs.push(SystemCaseInput {
                    case,
                    source: parsed[i].source(),
                    rel_path: rel_path.clone(),
                    map_source: map_source.clone(),
                });
            }
        }
        if let Some((path, source, source_map, runnable)) = emit_integration_module(
            &suite_name,
            &participants,
            &uses_targets,
            cross_context,
            unit_consumes,
            unit_tables,
            &case_inputs,
            tys,
        ) {
            outputs.push(StagedFile {
                output_path: path,
                document: Document::Ts(bynk_ts::TsProgram {
                    stmts: vec![bynk_ts::TsStmt::verbatim(
                        bynk_ts::VerbatimOrigin::NotYetConverted,
                        source,
                        None,
                    )],
                }),
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

/// v0.118: one `system`-tier case paired with the fragment file it was declared
/// in, so [`emit_integration_module`] maps each case body under its own source.
struct SystemCaseInput<'a> {
    case: &'a Case,
    source: &'a str,
    rel_path: String,
    map_source: String,
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
    tys: &Arc<Types>,
) -> Option<(PathBuf, String, Option<String>, RunnableTest)> {
    // #914: the test-scaffold module's runtime import list is emitted below as a
    // fixed set, but the bodies spliced into it lower through the ordinary `Bytes`
    // and boundary-codec paths. What those reach for is recorded here as they emit
    // and injected into the import line as a post-pass, the same shape
    // `emit_project` and `emit_worker_entry` use.
    //
    // #917: a case body here can still call `Json.decode[T]`/`Json.encode` on a
    // named type, and `lower_json_codec_call` still records the root on this
    // `runtime_use` (it is gated on `cx.test_scaffold`, which this harness sets
    // regardless). Nothing drains it — `emit_test_module`'s codec-closure pass is
    // unit-tier-only — so a delegating call here is left exactly as broken as it
    // was before this fix. Deliberate scope, per the issue's own hedge ("the
    // driver path may already be correct — worth confirming rather than
    // assuming"); not yet confirmed either way.
    let runtime_use = RuntimeUse::default();
    let sanitized = sanitise_suite(suite);
    let module_path = PathBuf::from(format!("tests/integration_{sanitized}.test.ts"));
    let mut out = String::new();
    // v0.70: the integration module's source map. A `system` suite may span
    // several fragment files, so the builder is multi-source; each case's body is
    // merged under its own source (registered on first sight in the case loop).
    // v0.72: keyed on each fragment's absolute path so an editor breakpoint on the
    // real `.bynk` binds; the per-case `rel_path` stays the test-runner location.
    let mut module_smb = SourceMapBuilder::new();
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::comment("Generated by bynkc — do not edit by hand.", None),
        0,
    ));
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::comment(format!("system test: {suite}"), None),
        0,
    ));
    out.push('\n');

    // Runtime imports. When a participant owns agents, also pull in the
    // Durable-Object namespace helper + types for the in-memory DO stubs.
    let has_agents = participants
        .iter()
        .any(|p| unit_tables.get(p).is_some_and(|t| !t.agents.is_empty()));
    let runtime_import = emitter::runtime_import_for(&module_path, ImportExt::Js);
    let mut runtime_names: Vec<String> = [
        "Ok",
        "Err",
        "Some",
        "None",
        "callService",
        "type Result",
        "type Option",
        "type ValidationError",
        "type JsonError",
        "type JsonValue",
        "type BoundaryError",
        "type ServiceBinding",
        "responseToHttpResult",
        "responseToHttpOutcome",
        "responseToUnauthOutcome",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if has_agents {
        runtime_names.push("makeIntegrationDoNamespace".to_string());
        runtime_names.push("type DurableObjectState".to_string());
        runtime_names.push("type DurableObjectNamespace".to_string());
    }
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::decl(
            TsDecl::Import {
                type_only: false,
                names: runtime_names,
                from: runtime_import.clone(),
            },
            None,
        ),
        0,
    ));

    // Per-participant: workers handler namespace + Worker entry default export.
    for p in participants {
        let ns = p.replace('.', "_");
        let dir = worker_dir_name(p);
        out.push_str(&bynk_ts::print_stmt(
            &TsStmt::decl(
                TsDecl::ImportNamespace {
                    type_only: false,
                    alias: ns.clone(),
                    from: format!("../workers/{dir}/handlers.js"),
                },
                None,
            ),
            0,
        ));
        out.push_str(&bynk_ts::print_stmt(
            &TsStmt::decl(
                TsDecl::ImportDefault {
                    alias: format!("worker_{ns}"),
                    from: format!("../workers/{dir}/index.js"),
                },
                None,
            ),
            0,
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
        out.push_str(&bynk_ts::print_stmt(
            &TsStmt::decl(
                TsDecl::ImportNamespace {
                    type_only: false,
                    alias: ns.clone(),
                    from: format!("./{path}.js"),
                },
                None,
            ),
            0,
        ));
    }
    out.push('\n');

    out.push_str(&expectation_runtime_helpers());

    // The env-graph harness: stand each participant up as an in-process Worker
    // and wire its Service Bindings to its siblings; the root env binds to all.
    out.push_str(&bynk_ts::print_stmt(
        &emit_integration_harness(participants, unit_consumes, unit_tables),
        0,
    ));
    out.push('\n');

    // Slice B: the test-only signer + per-route drivers for the target's http
    // service, and the set of http service names so the lowering calls a driver.
    let http_support = emit_system_http_support(suite, unit_tables, &runtime_use);
    if !http_support.code.is_empty() {
        crate::emitter::extend_printed_at(&mut out, http_support.code, 0);
        out.push('\n');
    }

    // One async function per case.
    let mut typed = integration_typed_commons(uses_targets, participants, unit_tables, tys);
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
                let mut names: Vec<String> = table
                    .types
                    .keys()
                    .chain(table.fns.keys())
                    .cloned()
                    .collect();
                names.sort();
                names.dedup();
                let mut type_names: Vec<String> = table.types.keys().cloned().collect();
                type_names.sort();
                type_names.dedup();
                crate::emitter::extend_printed_at(
                    &mut out,
                    emit_ns_destructure(&ns, &names, &type_names),
                    2,
                );
            }
        }
        let (body_src, body_smb) = emitter::lower_integration_case_body(
            &case.body,
            &mut typed,
            cross_context,
            http_support.http_services.clone(),
            http_support.declared_routes.clone(),
            http_support.route_body.clone(),
            http_support.type_ns.clone(),
            input.source,
            &input.rel_path,
            &runtime_use,
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
    let dispatch_stmts: Vec<TsStmt> = cases
        .iter()
        .enumerate()
        .map(|(idx, input)| run_dispatch_stmt(&input.case.name, &case_runners[idx]))
        .collect();
    out.push_str(&bynk_ts::print_stmt(&build_run_function(dispatch_stmts), 0));

    let module_file = module_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module.ts".to_string());
    let source_map = module_smb.to_v3(&out, &module_file);
    // #914: fold in whatever the spliced bodies actually reached for. After
    // `to_v3`, and into the existing import line rather than a new one, so the map
    // computed above stays valid — the same ordering `emit_project` relies on.
    // `inject_runtime_imports` drops any binding the fixed list already carries.
    if runtime_use.boundary_codec() {
        out = emitter::inject_runtime_imports(
            out,
            &runtime_import,
            emitter::BOUNDARY_CODEC_RUNTIME_IMPORTS,
        );
    }
    if runtime_use.json_codec() {
        out = emitter::inject_runtime_imports(
            out,
            &runtime_import,
            emitter::JSON_CODEC_RUNTIME_IMPORTS,
        );
    }
    if runtime_use.bytes() {
        out = emitter::inject_runtime_imports(out, &runtime_import, emitter::BYTES_RUNTIME_IMPORTS);
    }
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

/// testing-the-boundary Slice B: for a `system` suite whose target exposes an
/// `http` service, emit a **test-only** HS256 signer and one **driver** per
/// route. A driver builds a real `Request` (concrete path, JSON body, a signed
/// `Authorization: Bearer …`), drives the target Worker's public `fetch`, and
/// decodes the `Response` back to `HttpResult[T]`. The developer's `by
/// User("bob")` supplies the `sub`; the framework signs it (the developer never
/// hand-crafts auth — real-token ceremony is an e2e concern), and the real,
/// unmodified emitted Worker verifies it. Returns the emitted TS plus the set of
/// the target's http service names, so the case-body lowering knows to call a
/// driver (`__sysdrive_<svc>_<key>`) rather than `callService`.
/// The declared `(service, method, path)` http routes of a system target — the
/// set the lowering checks to tell a normal call from a **wrong-method** call
/// (#707): a `(method, path)` not in the set, whose *path* is, drives the `405`
/// fall-through through the generic `__sysdrive_wrongmethod_<svc>` driver.
type DeclaredRoutes = std::collections::HashSet<(String, String, String)>;
/// #708: for each declared route with a body param, the body's zero-based
/// position among the route's positional call args and its declared type —
/// what the raw driver's mixed typed+`Wire` call-site lowering needs to
/// serialise a typed body into the raw driver's `string` slot.
type RouteBodyMap = HashMap<(String, String, String), (usize, bynk_syntax::ast::TypeRef)>;

/// The emitted `emit_system_http_support` output: the driver/signer TS
/// source plus the metadata the case-body lowering needs to route and
/// convert calls.
///
/// #1479: `code` is now real [`TsStmt`]s (was pre-printed `String`) — the
/// HS256 signer block (`__bynkNow`/`__b64url`/`__bytesB64url`/
/// `__bynkSignHs256`, #1485's own separate scope) is still hand-written
/// text, carried as one [`bynk_ts::TsStmtKind::Raw`] statement, the same
/// carrier this track's earlier still-`String`-typed-sibling conversions
/// already used.
struct SystemHttpSupport {
    code: Vec<TsStmt>,
    http_services: std::collections::HashSet<String>,
    declared_routes: DeclaredRoutes,
    /// #708: per-route body-param position/type, for the raw driver's
    /// mixed typed+`Wire` call-site conversion.
    route_body: RouteBodyMap,
    /// The target's type namespace (`<target>.`), `route_body`'s types
    /// resolve `serialise_*` calls through.
    type_ns: String,
}

fn emit_system_http_support(
    target: &str,
    unit_tables: &HashMap<String, UnitTable>,
    runtime_use: &RuntimeUse,
) -> SystemHttpSupport {
    use bynk_syntax::ast::{HandlerKind, ServiceProtocol};
    let mut http_services: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut declared: DeclaredRoutes = std::collections::HashSet::new();
    let mut route_body: RouteBodyMap = HashMap::new();
    let Some(table) = unit_tables.get(target) else {
        return SystemHttpSupport {
            code: Vec::new(),
            http_services,
            declared_routes: declared,
            route_body,
            type_ns: String::new(),
        };
    };
    let ns = target.replace('.', "_");
    let binding = crate::emitter::wrangler::consumed_binding_name(target);
    let type_ns = format!("{ns}.");

    let mut routes: Vec<TsStmt> = Vec::new();
    let mut secrets: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let mut svc_names: Vec<&String> = table.services.keys().collect();
    svc_names.sort();
    for sname in svc_names {
        let decl = &table.services[sname];
        if !matches!(decl.protocol, ServiceProtocol::Http) {
            continue;
        }
        http_services.insert(sname.clone());
        for h in &decl.handlers {
            let HandlerKind::Http { method, path } = &h.kind else {
                continue;
            };
            declared.insert((sname.clone(), method.as_str().to_string(), path.clone()));
            let key = crate::emitter::http_handler_method_name(*method, path);
            // Split the handler's params into path params (matching `:name` in
            // the pattern, in order) and the optional body (the remaining param).
            let path_params: Vec<&str> = path
                .split('/')
                .filter_map(|seg| seg.strip_prefix(':'))
                .collect();
            let (_path_ps, body_ps): (Vec<_>, Vec<_>) = h
                .params
                .iter()
                .partition(|p| path_params.contains(&p.name.name.as_str()));
            // #708: record the body param's position (within the call's
            // positional args, matching `h.params` declaration order) and
            // declared type, so a mixed typed+`Wire` call can serialise a
            // typed body into the raw driver's `string` slot.
            if let Some(bp) = body_ps.first() {
                let idx = h
                    .params
                    .iter()
                    .position(|p| p.name.name == bp.name.name)
                    .expect("body param is drawn from h.params");
                route_body.insert(
                    (sname.clone(), method.as_str().to_string(), path.clone()),
                    (idx, bp.type_ref.clone()),
                );
            }
            // The concrete URL: substitute each `:name` with its param.
            let concrete_path = {
                let mut s = String::new();
                for seg in path.split('/').filter(|x| !x.is_empty()) {
                    s.push('/');
                    if let Some(name) = seg.strip_prefix(':') {
                        s.push_str(&format!("${{{}}}", crate::emitter::ts_ident(name)));
                    } else {
                        s.push_str(seg);
                    }
                }
                if s.is_empty() {
                    s.push('/');
                }
                s
            };
            // The signed Authorization header, if the handler's `by` is Bearer.
            let (auth_header, _secret_name) = match bynk_check::actors::bearer_seam_for(
                h,
                &table.actors,
            ) {
                Some(seam) => {
                    secrets.insert(seam.secret.clone());
                    // P7.2: `Record<string, string>`, not `emitter/workers.rs`'s own
                    // `Record<string, unknown>` marker — this expression feeds
                    // `__bynkSignHs256` directly with no `typeof` narrowing step in
                    // between (unlike workers.rs's own use), and real `process.env`
                    // values genuinely are `string | undefined`, so `string` is both
                    // accurate and what makes `?? ""` actually type as `string`.
                    let secret_read = format!(
                        "((globalThis as {{ process?: {{ env?: Record<string, string> }} }}).process?.env?.[{:?}] ?? \"\")",
                        seam.secret
                    );
                    (
                        format!(
                            "\"authorization\": `Bearer ${{await __bynkSignHs256({{ sub: __sub, exp: __bynkNow() + 3600 }}, {secret_read})}}`, "
                        ),
                        Some(seam.secret),
                    )
                }
                None => (String::new(), None),
            };
            // Body param → serialise; path params → the URL.
            let body_arg = body_ps.first();
            let (body_stmt, body_init): (Option<TsStmt>, &str) = match body_arg {
                Some(p) => {
                    // #1435 (Arc E slice 1): `serialise_expr_via` now returns
                    // a real `bynk_ts::TsExpr` — this call site is still
                    // `String`-based (not yet converted, out of this
                    // slice's scope), so it prints at the boundary
                    // (`bynk_ts::print_expr`), the same treatment
                    // `lower.rs`'s own call sites use.
                    let ser =
                        bynk_ts::print_expr(&crate::emitter::serialisation::serialise_expr_via(
                            &p.type_ref,
                            &crate::emitter::ts_ident(&p.name.name),
                            &type_ns,
                            runtime_use,
                        ));
                    let stmt = TsStmt::const_stmt(
                        TsBindingName::Ident("__body".to_string()),
                        None,
                        call(member(ident("JSON"), "stringify"), vec![ident(ser)]),
                        None,
                    );
                    (Some(stmt), "body: __body, ")
                }
                None => (None, ""),
            };
            // The response payload deserialiser: the `T` of `Effect[HttpResult[T]]`.
            let payload_deser = match strip_effect_httpresult(&h.return_type) {
                // #1435 (Arc E slice 1): boundary-print, same treatment as
                // `ser` above.
                Some(inner) => {
                    bynk_ts::print_expr(&crate::emitter::serialisation::deserialise_ref_via(
                        inner,
                        &type_ns,
                        runtime_use,
                    ))
                }
                None => format!("{type_ns}deserialise_unit"),
            };
            // Driver params mirror the handler's params (path params, then body).
            let driver_params: Vec<TsParam> = h
                .params
                .iter()
                .map(|p| TsParam {
                    name: crate::emitter::ts_ident(&p.name.name),
                    ty: Some(TsType::named(driver_param_ty(&p.type_ref, &ns))),
                    optional: false,
                })
                .collect();
            let raw_params: Vec<TsParam> = h
                .params
                .iter()
                .map(|p| TsParam {
                    name: crate::emitter::ts_ident(&p.name.name),
                    ty: Some(TsType::named("string")),
                    optional: false,
                })
                .collect();
            let content_type = if body_arg.is_some() {
                "\"content-type\": \"application/json\", "
            } else {
                ""
            };
            let url = template(format!("https://test{concrete_path}"));
            // Decision A (#1407): the request-init options object (`{ method,
            // headers: {...}, body, }`) keeps its own hand-formatted text —
            // every branch below bakes in an unconditional trailing `, ` before
            // its closing brace, and prints `{ }` (one space, not
            // `TsExpr::Object`'s own tight `{}`) when a sub-object has zero
            // entries — a shape `TsExpr::Object`'s general single-line algebra
            // (comma-separated, no trailing comma, tight `{}` for zero
            // entries) cannot reproduce byte-for-byte. The same "odd, one-off
            // shape stays text" call this track has made before (Decision B,
            // #1327; Decision C, #1359). Everything AROUND it — the function
            // declaration, its params, the `new Request(...)` call, the
            // `await ... .fetch(...)` chain, the final `return
            // responseTo*(...)` — is real `bynk_ts` structure.
            let typed_options = ident(format!(
                "{{ method: {method:?}, headers: {{ {content_type}{auth_header}}}, {body_init}}}",
                method = method.as_str(),
            ));
            routes.push(sysdrive_driver(
                "",
                sname,
                &key,
                driver_params.clone(),
                body_stmt.clone(),
                url.clone(),
                typed_options,
                &binding,
                "responseToHttpResult",
                &payload_deser,
            ));
            // Slice C: the raw driver for a `Wire(…)`-carrying call. Every slot is
            // a raw `string` (the wire form the boundary receives *unvalidated*):
            // path params flow into the URL, the body string is sent verbatim (no
            // `serialise`), and the response decodes to an `HttpOutcome` —
            // `Rejected(detail)` when the router refused the input before the
            // handler, `Handled(httpResult)` when it ran. Emitted only for a route
            // with a `Wire`-eligible slot (a body or a path param); a bodyless,
            // path-param-less route (e.g. `GET /cart/size`) can carry no `Wire`
            // argument, so its raw driver would be dead code.
            if !h.params.is_empty() {
                let raw_body_init = match body_ps.first() {
                    Some(p) => format!("body: {}, ", crate::emitter::ts_ident(&p.name.name)),
                    None => String::new(),
                };
                let raw_options = ident(format!(
                    "{{ method: {method:?}, headers: {{ {content_type}{auth_header}}}, {raw_body_init}}}",
                    method = method.as_str(),
                ));
                routes.push(sysdrive_driver(
                    "raw_",
                    sname,
                    &key,
                    raw_params.clone(),
                    None,
                    url.clone(),
                    raw_options,
                    &binding,
                    "responseToHttpOutcome",
                    &payload_deser,
                ));
            }
            // #706: the no-auth driver for a `by Nobody` call — the same request
            // the typed driver builds, minus the `Authorization` header, so the
            // real auth seam rejects it. A `401` decodes to `Rejected(
            // Unauthorized)` (`responseToUnauthOutcome`); anything else decodes
            // normally. Emitted only for a Bearer-secured route (one that carries
            // an auth header) — an unsecured route has no seam to reject a missing
            // credential, so a `by Nobody` there is meaningless.
            if !auth_header.is_empty() {
                let noauth_options = ident(format!(
                    "{{ method: {method:?}, headers: {{ {content_type}}}, {body_init}}}",
                    method = method.as_str(),
                ));
                routes.push(sysdrive_driver(
                    "noauth_",
                    sname,
                    &key,
                    driver_params,
                    body_stmt,
                    url.clone(),
                    noauth_options,
                    &binding,
                    "responseToUnauthOutcome",
                    &payload_deser,
                ));
            }
            // #821: the raw *and* no-auth driver combined, for a call mixing
            // `Wire(…)` with `by Nobody` — every slot is a raw `string` (as
            // `__sysdrive_raw`) and the `Authorization` header is dropped (as
            // `__sysdrive_noauth`), so the seam rejects the missing credential
            // before the (possibly malformed) raw body is even read.
            // `responseToUnauthOutcome` already delegates a non-`401` status to
            // `responseToHttpOutcome`'s shape-based classification, so a raw
            // body that would have been rejected on its own shape still decodes
            // correctly here. Emitted under the same conditions as both parent
            // drivers: a `Wire`-eligible slot and a Bearer-secured route.
            if !h.params.is_empty() && !auth_header.is_empty() {
                let raw_body_init = match body_ps.first() {
                    Some(p) => format!("body: {}, ", crate::emitter::ts_ident(&p.name.name)),
                    None => String::new(),
                };
                let rawnoauth_options = ident(format!(
                    "{{ method: {method:?}, headers: {{ {content_type}}}, {raw_body_init}}}",
                    method = method.as_str(),
                ));
                routes.push(sysdrive_driver(
                    "rawnoauth_",
                    sname,
                    &key,
                    raw_params,
                    None,
                    url,
                    rawnoauth_options,
                    &binding,
                    "responseToUnauthOutcome",
                    &payload_deser,
                ));
            }
        }
        // #707: one generic wrong-method driver per service — drives an arbitrary
        // `(method, path)` (an existing path, an undeclared method) and decodes
        // the router's `405` fall-through to `Rejected(MethodNotAllowed)`. The
        // handler never runs, so there is no body to serialise and the payload
        // deserialiser is unused (the `405` takes the `Rejected` arm). Unlike the
        // per-route drivers' own options object (Decision A above), `{ method }`
        // is a fixed single shorthand entry — never conditionally empty — so it
        // matches `TsExpr::Object`'s own real single-line rendering exactly and
        // is built as a real node, not opaque text.
        let wrongmethod = TsStmt::decl(
            TsDecl::Function {
                name: format!("__sysdrive_wrongmethod_{sname}"),
                generics: Vec::new(),
                params: vec![
                    TsParam {
                        name: "method".to_string(),
                        ty: Some(TsType::named("string")),
                        optional: false,
                    },
                    TsParam {
                        name: "path".to_string(),
                        ty: Some(TsType::named("string")),
                        optional: false,
                    },
                ],
                return_type: None,
                body: vec![
                    TsStmt::const_stmt(
                        TsBindingName::Ident("__h".to_string()),
                        None,
                        call(ident("makeHarness"), vec![]),
                        None,
                    ),
                    TsStmt::const_stmt(
                        TsBindingName::Ident("__req".to_string()),
                        None,
                        TsExpr::New {
                            callee: Box::new(ident("Request")),
                            args: vec![
                                TsExpr::template_lit(
                                    vec!["https://test".to_string(), String::new()],
                                    vec![ident("path")],
                                ),
                                TsExpr::object_entries(vec![TsObjectEntry::Shorthand(
                                    "method".to_string(),
                                )]),
                            ],
                        },
                        None,
                    ),
                    TsStmt::const_stmt(
                        TsBindingName::Ident("__res".to_string()),
                        None,
                        await_expr(method_call(
                            member(member(ident("__h"), "env"), &binding),
                            "fetch",
                            vec![ident("__req")],
                        )),
                        None,
                    ),
                    TsStmt::return_stmt(
                        Some(call(
                            ident("responseToHttpOutcome"),
                            vec![
                                ident("__res"),
                                TsExpr::Arrow {
                                    params: vec![TsParam {
                                        name: "__j".to_string(),
                                        ty: Some(TsType::named("JsonValue")),
                                        optional: false,
                                    }],
                                    is_async: false,
                                    generics: Vec::new(),
                                    return_type: None,
                                    body: Box::new(TsArrowBody::Expr(Box::new(call(
                                        ident("Ok"),
                                        vec![TsExpr::As {
                                            expr: Box::new(ident("__j")),
                                            ty: TsType::named("never"),
                                        }],
                                    )))),
                                },
                            ],
                        )),
                        None,
                    ),
                ],
                is_async: true,
                inline: false,
            },
            None,
        );
        routes.push(wrongmethod);
    }

    if http_services.is_empty() {
        return SystemHttpSupport {
            code: Vec::new(),
            http_services,
            declared_routes: declared,
            route_body,
            type_ns: String::new(),
        };
    }

    // #1479: the HS256 signer block below (#1485's own separate scope) stays
    // exactly the hand-written text it always was — built into a local
    // buffer instead of this function's own former `out`, then carried
    // forward as one `TsStmt::raw`, the same carrier `SystemHttpSupport`'s
    // own doc above names.
    let mut signer = String::new();
    // A monotonic clock the signer's `exp` uses; kept out of `bundle`d runtime.
    signer.push_str("function __bynkNow(): number { return Math.floor(Date.now() / 1000); }\n");
    // Test-only HS256 signer (never in the deployable app; e2e owns real auth).
    signer.push_str(
        "function __b64url(s: string): string { return btoa(s).replace(/\\+/g, \"-\").replace(/\\//g, \"_\").replace(/=+$/, \"\"); }\n\
         function __bytesB64url(bytes: Uint8Array): string { let bin = \"\"; for (const b of bytes) bin += String.fromCharCode(b); return btoa(bin).replace(/\\+/g, \"-\").replace(/\\//g, \"_\").replace(/=+$/, \"\"); }\n\
         async function __bynkSignHs256(payload: Record<string, unknown>, secret: string): Promise<string> {\n\
         \x20 const h = __b64url(JSON.stringify({ alg: \"HS256\", typ: \"JWT\" }));\n\
         \x20 const p = __b64url(JSON.stringify(payload));\n\
         \x20 const enc = new TextEncoder();\n\
         \x20 const key = await crypto.subtle.importKey(\"raw\", enc.encode(secret) as BufferSource, { name: \"HMAC\", hash: \"SHA-256\" }, false, [\"sign\"]);\n\
         \x20 const sig = await crypto.subtle.sign(\"HMAC\", key, enc.encode(`${h}.${p}`) as BufferSource);\n\
         \x20 return `${h}.${p}.${__bytesB64url(new Uint8Array(sig))}`;\n\
         }\n",
    );
    let mut code = vec![TsStmt::raw(signer, None)];
    // Set each secret the target's actors read, so the real Bearer seam verifies.
    // P7.2: `Record<string, string>` — same reasoning as `__bynkSignHs256`'s own
    // `secret: string` parameter above.
    let record_string_string = TsType::named_with_args(
        "Record",
        vec![TsType::named("string"), TsType::named("string")],
    );
    let cast_globalthis_process = |process_optional: bool| -> TsExpr {
        let env_ty = TsType::Object(vec![TsTypeMember::prop(
            "env",
            record_string_string.clone(),
        )]);
        let process_member = if process_optional {
            TsTypeMember::optional_prop("process", env_ty)
        } else {
            TsTypeMember::prop("process", env_ty)
        };
        TsExpr::As {
            expr: Box::new(TsExpr::As {
                expr: Box::new(ident("globalThis")),
                ty: TsType::named("unknown"),
            }),
            ty: TsType::Object(vec![process_member]),
        }
    };
    for s in &secrets {
        let assign_process = TsStmt::assign(
            member(cast_globalthis_process(false), "process"),
            TsExpr::Binary {
                op: TsBinaryOp::NullishCoalescing,
                left: Box::new(member(cast_globalthis_process(true), "process")),
                right: Box::new(TsExpr::object(vec![(
                    "env".to_string(),
                    TsExpr::object(vec![]),
                )])),
            },
            None,
        );
        let assign_secret = TsStmt::assign(
            TsExpr::Index {
                object: Box::new(member(
                    member(cast_globalthis_process(false), "process"),
                    "env",
                )),
                index: Box::new(str_lit(s.clone())),
            },
            str_lit("__bynk_test_secret"),
            None,
        );
        code.push(assign_process);
        code.push(assign_secret);
    }
    code.extend(routes);
    SystemHttpSupport {
        code,
        http_services,
        declared_routes: declared,
        route_body,
        type_ns,
    }
}

/// Type a system-http driver parameter (v0.182): a named type is reached
/// through the target's handler namespace (`todos.AddRequest`); primitives and
/// compound shapes fall back to `unknown` — the driver only forwards the value
/// to the namespace's `serialise_*`, and the Bynk checker already typed the call.
fn driver_param_ty(t: &bynk_syntax::ast::TypeRef, _ns: &str) -> String {
    use bynk_syntax::ast::TypeRef;
    match t {
        // A named type may embed a branded refined field (`AddRequest.title:
        // Title`), and the case body passes a plain object literal — the Bynk
        // checker already type-checked the call's args against the handler
        // (Slice A), so the driver forwards the value to the namespace's
        // `serialise_*`. Primitives stay precise.
        TypeRef::Named(_) => "any".to_string(),
        other => crate::emitter::ts_type_ref(other),
    }
}

/// Extract `T` from a handler return type `Effect[HttpResult[T]]` (v0.182).
fn strip_effect_httpresult(t: &bynk_syntax::ast::TypeRef) -> Option<&bynk_syntax::ast::TypeRef> {
    use bynk_syntax::ast::TypeRef;
    let inner = match t {
        TypeRef::Effect(b, _) => b.as_ref(),
        other => other,
    };
    match inner {
        TypeRef::HttpResult(payload, _) => Some(payload.as_ref()),
        _ => None,
    }
}

/// A `{ fetch: (req: Request) => worker_X.fetch(req, env_X) } as ServiceBinding`
/// expression — the identical shape both the per-participant and root-env
/// binding wiring in [`emit_integration_harness`] build, differing only in
/// which worker/env pair they close over.
fn service_binding_forward(worker_ident: &str, env_ident: &str) -> TsExpr {
    TsExpr::As {
        expr: Box::new(TsExpr::object(vec![(
            "fetch".to_string(),
            TsExpr::Arrow {
                params: vec![TsParam {
                    name: "req".to_string(),
                    ty: Some(TsType::named("Request")),
                    optional: false,
                }],
                is_async: false,
                generics: Vec::new(),
                return_type: None,
                body: Box::new(TsArrowBody::Expr(Box::new(method_call(
                    ident(worker_ident),
                    "fetch",
                    vec![ident("req"), ident(env_ident)],
                )))),
            },
        )])),
        ty: TsType::named("ServiceBinding"),
    }
}

/// Emit the `makeHarness()` factory: an in-process env per participant whose
/// Service Bindings call the sibling participants' real Worker `fetch` and whose
/// Durable-Object namespaces back the participant's own agents in memory, plus a
/// root env binding every participant (the test cases call in through it). A
/// fresh harness per case gives each case clean agent state.
/// #1479: returns the real [`TsStmt`] itself (was pre-printed `String`) —
/// the one real declaration this function ever built; its caller now prints
/// it directly, the same shape `emit_test_deps`'s own identical conversion
/// used.
fn emit_integration_harness(
    participants: &[String],
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> TsStmt {
    let mut body = Vec::new();
    // Declare every participant env first so sibling references resolve.
    //
    // P7.2: deferred, not narrowed. `env_{ns}` is later passed positionally into
    // `worker_{dns}.fetch(req, env_{dns})`, whose own generated `Env` interface
    // (`emitter/workers.rs`'s `export interface Env { ... }`) types each binding
    // precisely (`ServiceBinding`, `DurableObjectNamespace`, ...) — `Record<string,
    // unknown>` would not structurally satisfy that on assignment, and correctly
    // referencing each participant's own generated `Env` type from this shared
    // harness scope needs its own naming/qualification scheme this slice hasn't
    // worked out. Left as `any`, named here rather than guessed at.
    for p in participants {
        let ns = p.replace('.', "_");
        body.push(TsStmt::const_stmt(
            TsBindingName::Ident(format!("env_{ns}")),
            Some(TsType::named("any")),
            TsExpr::object(vec![]),
            None,
        ));
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
                body.push(TsStmt::assign(
                    member(ident(format!("env_{ns}")), binding),
                    service_binding_forward(&format!("worker_{dns}"), &format!("env_{dns}")),
                    None,
                ));
            }
        }
        if let Some(table) = unit_tables.get(p) {
            let mut agents: Vec<&String> = table.agents.keys().collect();
            agents.sort();
            for agent in agents {
                let binding = crate::emitter::wrangler::agent_binding_name(agent);
                body.push(TsStmt::assign(
                    member(ident(format!("env_{ns}")), binding),
                    call(
                        ident("makeIntegrationDoNamespace"),
                        vec![TsExpr::Arrow {
                            params: vec![TsParam {
                                name: "state".to_string(),
                                ty: None,
                                optional: false,
                            }],
                            is_async: false,
                            generics: Vec::new(),
                            return_type: None,
                            body: Box::new(TsArrowBody::Expr(Box::new(TsExpr::New {
                                callee: Box::new(member(ident(ns.clone()), agent)),
                                args: vec![ident("state")],
                            }))),
                        }],
                    ),
                    None,
                ));
            }
        }
    }
    // Root env binds to every participant.
    // P7.2: deferred, same reason as `env_{ns}` above.
    body.push(TsStmt::const_stmt(
        TsBindingName::Ident("rootEnv".to_string()),
        Some(TsType::named("any")),
        TsExpr::object(vec![]),
        None,
    ));
    for p in participants {
        let ns = p.replace('.', "_");
        let binding = crate::emitter::wrangler::consumed_binding_name(p);
        body.push(TsStmt::assign(
            member(ident("rootEnv"), binding),
            service_binding_forward(&format!("worker_{ns}"), &format!("env_{ns}")),
            None,
        ));
    }
    body.push(TsStmt::return_stmt(
        Some(TsExpr::object(vec![("env".to_string(), ident("rootEnv"))])),
        None,
    ));
    TsStmt::decl(
        TsDecl::Function {
            name: "makeHarness".to_string(),
            generics: Vec::new(),
            params: Vec::new(),
            return_type: None,
            body,
            is_async: false,
            inline: false,
        },
        None,
    )
}

/// Build the [`checker::TypedCommons`] used to lower integration case bodies —
/// `uses` commons plus participant types/fns/methods, so static calls and
/// constructors resolve.
fn integration_typed_commons(
    uses_targets: &[String],
    participants: &[String],
    unit_tables: &HashMap<String, UnitTable>,
    tys: &Arc<checker::Types>,
) -> checker::TypedCommons {
    let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
    let mut fns: HashMap<String, Arc<FnDecl>> = HashMap::new();
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
        callees: HashMap::new(),
        warnings: vec![],
        // T3.6b (R4.1): the *caller's* table, not a fresh one. A synthesised
        // commons starts with an empty `expr_types`, but the case/property
        // checks below fill it in — interning into the table they were handed
        // — and the lowering then resolves those ids back through this field.
        // A table of its own would make every one of those reads a
        // wrong-table lookup.
        ty_intern: Arc::clone(tys),
        actor_bindings: HashMap::new(),
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
        bynk_ir::walk_exprs(e, &mut check);
    }
    bynk_ir::walk_exprs(&block.tail, &mut check);
    found
}

/// v0.178 (#662): build the target's service table for a test-body check —
/// each service's protocol word and its `on call` handler signature, so a
/// `svc.call(args)` in a case can be resolved rather than string-matched. A
/// service with no `on call` handler carries `None`, which turns `svc.call(...)`
/// into `bynk.test.service_no_call_handler` instead of a silent runtime crash.
fn target_service_handler_kinds(
    table: Option<&UnitTable>,
) -> HashMap<String, Vec<bynk_ir::IrHandlerKind>> {
    let Some(t) = table else {
        return HashMap::new();
    };
    t.services
        .iter()
        .map(|(name, decl)| {
            (
                name.clone(),
                decl.handlers
                    .iter()
                    .map(|h| bynk_lower::lower_handler_kind_ir(&h.kind))
                    .collect(),
            )
        })
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
fn is_attackable_contract(f: &FnDecl, resolved: &ResolvedCommons, tys: &Arc<Types>) -> bool {
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
        let Some(ty) = checker::resolve_type_ref(&p.type_ref, &resolved.types, tys) else {
            return false;
        };
        // Restrict to primitive (or refined-over-primitive) parameters: a
        // generated composite carries `bigint` fields that would mix with the
        // function's `number` arithmetic. Composite-param contracts are covered
        // by the dev guard.
        if numeric_or_scalar_base(ty, &resolved.types, tys).is_none() {
            return false;
        }
        if !test_suites::prop_binding_generable(
            ty,
            &resolved.types,
            test_suites::PROP_GEN_DEPTH,
            tys,
        ) {
            return false;
        }
    }
    true
}

/// The primitive base type a parameter erases to — `Some(base)` for a base type
/// or a refinement/opaque over one, `None` for a composite (record/sum/list/map/
/// option/result/etc.). Used to gate a contract as attackable — every param
/// must erase to a primitive `binding_gen`/`destructure_vals` can actually
/// generate/coerce. (Previously also used to conditionally `Number(…)`-wrap a
/// call argument at the attacked function's own call site — v0.114's
/// generator/erasure split; #1426 made that redundant: `destructure_vals`
/// coerces every `Int`-drawing local at the bind site now, so every argument
/// reaching the call is already a real `number`.)
fn numeric_or_scalar_base(
    ty: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
) -> Option<BaseType> {
    match &*tys.get(ty) {
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
    tys: &Arc<Types>,
) -> Option<(ResolvedCommons, Vec<FnDecl>)> {
    let (resolved, _) = test_suites::build_privileged_resolved(
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
        .map(std::sync::Arc::as_ref)
        .filter(|f| is_attackable_contract(f, &resolved, tys))
        .cloned()
        .collect();
    fns.sort_by_key(|a| a.name.display());
    Some((resolved, fns))
}

/// #917: the type-only namespace qualifier every named type reachable from
/// `target_name` (or one of its `uses`) renders through inside the test module
/// that targets it — a test module imports the target/`uses` commons only as
/// a namespace (`import * as orders from …`), never inlining their
/// declarations the way a `uses`r module's own production emission does, so
/// *every* name here is foreign and needs one. The target's own name wins a
/// collision against a `uses`d one, matching `synthetic_typed_commons_for_target`'s
/// merge precedence.
fn json_codec_qual_for_target(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut qual = HashMap::new();
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                for n in table.types.keys() {
                    qual.entry(n.clone()).or_insert_with(|| format!("{ns}."));
                }
            }
        }
    }
    let target_ns = target_name.replace('.', "_");
    if let Some(table) = unit_tables.get(target_name) {
        for n in table.types.keys() {
            qual.insert(n.clone(), format!("{target_ns}."));
        }
    }
    qual
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
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    import_ext: ImportExt,
    contracts: bool,
    tys: &Arc<Types>,
) -> Option<(PathBuf, String, Option<String>, RunnableTest)> {
    // #914: the test-scaffold module's runtime import list is emitted below as a
    // fixed set, but the bodies spliced into it lower through the ordinary `Bytes`
    // and boundary-codec paths. What those reach for is recorded here as they emit
    // and injected into the import line as a post-pass, the same shape
    // `emit_project` and `emit_worker_entry` use.
    let runtime_use = RuntimeUse::default();
    runtime_use.set_json_codec_qual(json_codec_qual_for_target(
        target_name,
        unit_tables,
        unit_uses,
    ));
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

    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::comment("Generated by bynkc — do not edit by hand.", None),
        0,
    ));
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::comment(format!("test target: {target_name}"), None),
        0,
    ));
    out.push('\n');

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
    let mut runtime_names: Vec<String> = ["Ok", "Err", "Some", "None"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if has_agents {
        runtime_names.push("makeTestState".to_string());
    }
    runtime_names.push("type Result".to_string());
    runtime_names.push("type Option".to_string());
    runtime_names.push("type ValidationError".to_string());
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::decl(
            TsDecl::Import {
                type_only: false,
                names: runtime_names,
                from: runtime_import.clone(),
            },
            None,
        ),
        0,
    ));

    // Compute relative import path from tests/ to the target's output dir.
    let import_target = relative_import_for_test(&target_dir);
    out.push_str(&bynk_ts::print_stmt(
        &TsStmt::decl(
            TsDecl::ImportNamespace {
                type_only: false,
                alias: target_ns.clone(),
                from: format!("./{import_target}.{ext}"),
            },
            None,
        ),
        0,
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
        out.push_str(&bynk_ts::print_stmt(
            &TsStmt::decl(
                TsDecl::ImportNamespace {
                    type_only: false,
                    alias: ns.clone(),
                    from: format!("./{path}.{ext}"),
                },
                None,
            ),
            0,
        ));
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
        out.push_str(&bynk_ts::print_stmt(
            &TsStmt::decl(
                TsDecl::ImportNamespace {
                    type_only: false,
                    alias: ns.clone(),
                    from: format!("./{path}.{ext}"),
                },
                None,
            ),
            0,
        ));
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
            tys,
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
        crate::emitter::extend_printed_at(
            &mut out,
            observation_call_record_types(target_name, unit_tables),
            0,
        );
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
            &runtime_use,
            tys,
        ));
        out.push('\n');
    }

    // Emit the deps factory.
    out.push_str(&bynk_ts::print_stmt(
        &emit_test_deps(
            target_name,
            target_kind,
            stubs,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_flattened,
        ),
        0,
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
        let rel_path = parsed[i].identity_path();
        let rel_path = rel_path.to_string_lossy();
        for case in &test_decl.cases {
            discovered.push(DiscoveredCase {
                name: case.name.clone(),
                location: Some(discovered_location(
                    parsed[i].source(),
                    &rel_path,
                    case.name_span,
                )),
            });
            let runner_name = sanitise_case_name(&case.name, &mut case_runners.len());
            case_runners.push(runner_name.clone());
            // v0.118: record each case's effective tier (its own `as <tier>`, else
            // the suite default, else `unit`). unit and integration share this
            // in-process harness; the tier rides the emitted module for reporting.
            out.push_str(&bynk_ts::print_stmt(
                &TsStmt::comment(
                    format!(
                        "case tier: {}",
                        super::case_effective_tier(case, test_decl).as_str()
                    ),
                    None,
                ),
                0,
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
                parsed[i].source(),
                &rel_path,
                &runtime_use,
                tys,
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
                module_smb.add_source(parsed[i].map_source_name(), parsed[i].source().to_string());
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
        let rel_path = parsed[i].identity_path();
        let rel_path = rel_path.to_string_lossy();
        for prop in &test_decl.properties {
            discovered.push(DiscoveredCase {
                name: prop.name.clone(),
                location: Some(discovered_location(
                    parsed[i].source(),
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
                    parsed[i].source(),
                    &rel_path,
                    &runtime_use,
                    tys,
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
                    parsed[i].source(),
                    &rel_path,
                    &runtime_use,
                    tys,
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
            parsed[i]
                .test()
                .map(|_| parsed[i].identity_path().to_string_lossy().into_owned())
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
            &runtime_use,
            tys,
        );
        out.push_str(&attack_text);
        out.push('\n');
    }

    // #917: every case/property/stub/attack body above has now lowered, so every
    // `Json.decode[T]`/`Json.encode` root it reached for is in hand. The unit
    // this module targets (or one of its `uses`) exports `T` but no codec for
    // it — generate the caller's own closure here, the same pattern #661 uses
    // for a workers cross-context caller's consumed-boundary types. Plain
    // `function` declarations hoist, and this is a pure append after every
    // `module_smb` merge above, so it disturbs no earlier source-map offset.
    //
    // P6.28 (design/tracks/the-ir.md §6a): `take_json_codec_roots` now returns
    // the checker's own `TyId`s, not `TypeRef`s — converted back here, once,
    // right before the one remaining consumer (`collect_codec_closure`) that is
    // still genuinely `TypeRef`-driven, rather than at each of the two push
    // sites in `emitter/lower.rs`. A root `ty_to_type_ref` rejects (functions,
    // effects, type variables) is silently dropped, the same filtering the old
    // push-time conversion already did — moving where the filter runs, not what
    // it filters.
    let json_codec_roots: Vec<TypeRef> = runtime_use
        .take_json_codec_roots()
        .into_iter()
        .filter_map(|ty| crate::emitter::ty_to_type_ref(ty, tys))
        .collect();
    if !json_codec_roots.is_empty() {
        let synthetic =
            synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses, tys);
        let (codec_names, codec_insts) = crate::emitter::serialisation::collect_codec_closure(
            &json_codec_roots,
            &synthetic.types,
        );
        if !codec_names.is_empty() || !codec_insts.is_empty() {
            // The helpers below always name `JsonValue`/`BoundaryError` in their
            // own signatures, regardless of which arm (if any) the case bodies'
            // own `Json.decode`/`Json.encode` calls happened to trip.
            runtime_use.note_json_codec();
            runtime_use.note_boundary_codec();
            let qual = runtime_use.json_codec_qual();
            crate::emitter::extend_printed_at(
                &mut out,
                crate::emitter::serialisation::decls_as_stmts_block(
                    crate::emitter::serialisation::emit_helpers_for_owner_qualified(
                        &codec_names,
                        &synthetic.types,
                        target_name,
                        &qual,
                        &runtime_use,
                    ),
                ),
                0,
            );
            crate::emitter::extend_printed_at(
                &mut out,
                crate::emitter::serialisation::decls_as_stmts(
                    crate::emitter::serialisation::emit_generic_helpers_qualified(
                        &codec_insts,
                        &synthetic.types,
                        &qual,
                        &runtime_use,
                    ),
                ),
                0,
            );
        }
    }

    // Module-level runner. v0.127: `only` filters to a single case/property by
    // name (the per-case run lens); undefined runs every one.
    let mut dispatch_stmts: Vec<TsStmt> = Vec::new();
    let mut case_index = 0;
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            let runner_name = &case_runners[case_index];
            dispatch_stmts.push(run_dispatch_stmt(&case.name, runner_name));
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
            dispatch_stmts.push(run_dispatch_stmt(&prop.name, runner_name));
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
        dispatch_stmts.push(run_dispatch_stmt(
            &format!("contract {}", fname.name),
            runner_name,
        ));
        prop_index += 1;
    }
    out.push_str(&bynk_ts::print_stmt(&build_run_function(dispatch_stmts), 0));

    let module_file = module_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module.ts".to_string());
    let source_map = module_smb.to_v3(&out, &module_file);
    // #914: fold in whatever the spliced bodies actually reached for. After
    // `to_v3`, and into the existing import line rather than a new one, so the map
    // computed above stays valid — the same ordering `emit_project` relies on.
    // `inject_runtime_imports` drops any binding the fixed list already carries.
    if runtime_use.boundary_codec() {
        out = emitter::inject_runtime_imports(
            out,
            &runtime_import,
            emitter::BOUNDARY_CODEC_RUNTIME_IMPORTS,
        );
    }
    if runtime_use.json_codec() {
        out = emitter::inject_runtime_imports(
            out,
            &runtime_import,
            emitter::JSON_CODEC_RUNTIME_IMPORTS,
        );
    }
    if runtime_use.bytes() {
        out = emitter::inject_runtime_imports(out, &runtime_import, emitter::BYTES_RUNTIME_IMPORTS);
    }
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
    groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    import_ext: ImportExt,
    emitted: &mut HashSet<PathBuf>,
) -> Option<StagedFile> {
    let indices = groups.get(name)?;
    // Multi-file only: *every* file must sit under a `<name>/` directory, the
    // layout where no `out/<name>.ts` is otherwise produced. A unit with any file
    // at `<name>.bynk` already owns `out/<name>.ts`, so it must not get a barrel.
    if indices.is_empty()
        || !indices
            .iter()
            .all(|&i| is_multi_file_layout(&parsed[i].source_path(), name))
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
    let mut files: Vec<PathBuf> = indices.iter().map(|&i| parsed[i].source_path()).collect();
    files.sort();
    files.dedup();
    let mut stmts = vec![TsStmt::comment(
        "Generated by bynkc — do not edit by hand.",
        None,
    )];
    for file in &files {
        let spec = emitter::cross_commons_import_specifier_for_path(&barrel_loc, file, import_ext);
        stmts.push(TsStmt::decl(TsDecl::ReExportAll { from: spec }, None));
    }
    Some(StagedFile {
        output_path,
        document: Document::Ts(TsProgram { stmts }),
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

/// Finding #17: the source lives at `emitter/test_runtime/expectation.ts` —
/// a real, syntax-highlighted `.ts` file rather than a Rust string built
/// line-by-line via `push_str`. `ExpectationError`'s fields are declared and
/// assigned explicitly rather than via TS parameter properties: parameter
/// properties are a transform-only construct that Node's strip-only
/// type-stripping rejects (`ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`), and
/// `bynkc test --inspect` runs this `.ts` directly under strip-only Node
/// (slice 2, ADR 0104). The explicit form is equivalent and strip-clean.
fn expectation_runtime_helpers() -> String {
    include_str!("../emitter/test_runtime/expectation.ts").to_string()
}

/// v0.118: the runtime helper the `__Stub_<Cap>` stubs rely on — a
/// structural deep-equality over lowered argument patterns (bigint-safe, since
/// `Int` erases to `bigint` and `JSON.stringify` rejects it raw). Finding
/// #17: source lives at `emitter/test_runtime/stub.ts`.
fn stub_runtime_helpers() -> String {
    include_str!("../emitter/test_runtime/stub.ts").to_string()
}

/// v0.118: emit the `__Stub_<Cap>` stub class for a capability seam
/// overridden by `stub` clauses (testing track slice 6). One async method
/// per overridden operation renders its clauses as a first-match-wins if-chain
/// over the call's argument patterns; a matched clause returns its lowered
/// value, throws an injected fault, or advances a per-call sequence cursor.
#[allow(clippy::too_many_arguments)]
fn emit_stub_class(
    rp: &ResolvedStub,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
) -> String {
    let mut out = String::new();
    let cap = &rp.cap;
    // Value expressions are lowered in the target context's privileged view, so
    // its types, variants and `uses` vocabulary resolve unqualified.
    let owning_unit = target_name.to_string();
    let scope_ns = owning_unit.replace('.', "_");
    // Each in-scope type name's *owning* namespace — the target's own for a
    // locally-declared type, but the specific `uses`d commons' for one reached
    // only through it (never the target's), since `emit_context_rebrands`
    // only re-exports a `uses`-sourced type under the target's namespace when
    // the target's own lowered body references it by name — a stub class
    // implementing an adapter-flattened capability (e.g. `Locale`) generally
    // doesn't (Locale capability track, slice 1, #844).
    let mut type_ns: HashMap<String, String> = unit_tables
        .get(&owning_unit)
        .map(|t| {
            t.types
                .keys()
                .map(|n| (n.clone(), scope_ns.clone()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(used) = unit_uses.get(&owning_unit) {
        for u in used {
            if let Some(table) = unit_tables.get(u) {
                let uns = u.replace('.', "_");
                for n in table.types.keys() {
                    type_ns.entry(n.clone()).or_insert_with(|| uns.clone());
                }
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
    let scope_type_names: Vec<String> = unit_tables
        .get(&owning_unit)
        .map(|t| {
            let mut v: Vec<String> = t.types.keys().cloned().collect();
            v.sort();
            v.dedup();
            v
        })
        .unwrap_or_default();

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

    // The class wrapper (header/fields) stays hand-written text — Decision C
    // (#1359's own `emit_provider` precedent): each method's own body is one
    // opaque `TsStmt::raw` (it mixes real structure with `lower_stub_value_
    // block`'s own already-lowered opaque text, an `async () => { ... }`
    // IIFE whose own multi-statement block body no `TsArrowBody::Block`
    // renderer can flatten onto one line — the same "block-bodied arrow
    // stays opaque" call `emit_composition_root`'s own `__eventsDispatch`
    // closure already made), so each method is printed as
    // its own real `TsClassMethod` fragment via `bynk_ts::print_class_method`
    // at depth 0 — the only depth `render_class_method`'s own debug_assert
    // allows a `Raw`-bodied method to print correctly at — and spliced
    // directly into this still-hand-written wrapper, with no blank line
    // between fields/methods or between methods (matching this function's
    // own zero-blank-line output exactly).
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
        let params: Vec<TsParam> = op
            .params
            .iter()
            .map(|p| TsParam {
                name: p.name.name.clone(),
                ty: Some(emitter::ts_type_ref_qualified_multi_ts_type(
                    &p.type_ref,
                    &type_ns,
                )),
                optional: false,
            })
            .collect();
        let return_ty = emitter::ts_type_ref_qualified_multi_ts_type(&op.return_type, &type_ns);

        let mut body_text = String::new();
        crate::emitter::extend_printed_at(
            &mut body_text,
            emit_ns_destructure(&scope_ns, &scope_names, &scope_type_names),
            2,
        );
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
                        runtime_use,
                        tys,
                    );
                    let vname = format!("__pv_{idx}_{i}");
                    body_text.push_str(&format!("    const {vname} = await (async () => {{\n"));
                    for line in body.lines() {
                        body_text.push_str("      ");
                        body_text.push_str(line);
                        body_text.push('\n');
                    }
                    body_text.push_str("    })();\n");
                    cond_parts.push(format!("__bynkDeepEqual({}, {vname})", param.name.name));
                }
            }
            let cond = if cond_parts.is_empty() {
                "true".to_string()
            } else {
                cond_parts.join(" && ")
            };
            body_text.push_str(&format!("    if ({cond}) {{\n"));
            let rhs_body = emit_stub_rhs(
                clause,
                idx,
                op,
                target_name,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                runtime_use,
                tys,
            );
            for line in rhs_body.lines() {
                body_text.push_str("      ");
                body_text.push_str(line);
                body_text.push('\n');
            }
            body_text.push_str("    }\n");
        }
        body_text.push_str(&format!(
            "    throw new Error(\"bynk: no stub clause matched for {cap}.{method}\");\n"
        ));

        let method_node = TsClassMethod {
            name: method.clone(),
            private: false,
            is_async: true,
            params,
            return_type: Some(return_ty),
            doc: None,
            body: vec![TsStmt::raw(body_text, None)],
        };
        out.push_str(&bynk_ts::print_class_method(&method_node, 0));
    }
    out.push_str("}\n");
    out
}

/// `throw new Error("bynk: injected capability fault (stubs … fails)");` —
/// the one real site anywhere in this cluster injecting a synthetic
/// capability failure, shared by [`emit_stub_rhs`]'s own `Fails`/
/// `ReturnsEach`-with-a-failing-outcome arms.
fn stub_fault_stmt() -> TsStmt {
    TsStmt::throw_stmt(
        TsExpr::New {
            callee: Box::new(ident("Error")),
            args: vec![str_lit("bynk: injected capability fault (stubs … fails)")],
        },
        None,
    )
}

/// Prefix every line of `text` with `prefix` — [`TsStmtKind::Raw`]'s own
/// "pre-indented at a fixed absolute depth by the caller" contract (its own
/// doc), used here to carry [`lower_stub_value_block`]'s already-lowered
/// opaque body text as one switch-case's own statement.
fn reindent_by(text: &str, prefix: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        out.push_str(prefix);
        out.push_str(line);
        out.push('\n');
    }
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
) -> String {
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
            runtime_use,
            tys,
        )
    };
    match &clause.rhs {
        StubRhs::Returns(e) => lower(e),
        StubRhs::Fails(_) => bynk_ts::print_stmt(&stub_fault_stmt(), 0),
        StubRhs::ReturnsEach(outcomes, _) => {
            let n = outcomes.len();
            let mut stmts = vec![TsStmt::const_stmt(
                TsBindingName::Ident("__k".to_string()),
                None,
                member(ident("this"), format!("__seq_{clause_idx}")),
                None,
            )];
            if n > 1 {
                stmts.push(if_(
                    TsExpr::Binary {
                        op: TsBinaryOp::LessThan,
                        left: Box::new(member(ident("this"), format!("__seq_{clause_idx}"))),
                        right: Box::new(num_lit((n - 1).to_string())),
                    },
                    TsStmt::increment(member(ident("this"), format!("__seq_{clause_idx}")), None),
                ));
            }
            let outcome_body = |o: &SeqOutcome| match o {
                SeqOutcome::Value(e) => lower(e),
                SeqOutcome::Fails(_) => bynk_ts::print_stmt(&stub_fault_stmt(), 0),
            };
            let mut cases: Vec<TsSwitchCase> = outcomes
                .iter()
                .enumerate()
                .take(n.saturating_sub(1))
                .map(|(j, o)| TsSwitchCase {
                    test: Some(num_lit(j.to_string())),
                    body: vec![TsStmt::raw(reindent_by(&outcome_body(o), "    "), None)],
                    default_braced: false,
                    // Arc E slice 6 (#1445): unaffected by `case_braced`'s
                    // addition — this site's own non-`default` cases were
                    // already, and stay, `{ }`-blocked.
                    case_braced: true,
                })
                .collect();
            cases.push(TsSwitchCase {
                test: None,
                body: vec![TsStmt::raw(
                    reindent_by(&outcome_body(&outcomes[n - 1]), "    "),
                    None,
                )],
                default_braced: true,
                case_braced: true,
            });
            stmts.push(TsStmt::switch_stmt(ident("__k"), cases, None));
            stmts.iter().map(|s| bynk_ts::print_stmt(s, 0)).collect()
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
) -> String {
    let owning_unit = target_name.to_string();
    let mut typed = synthetic_typed_commons_for_target(&owning_unit, unit_tables, unit_uses, tys);
    let block = test_suites::value_block(e);
    if let Some((resolved, _)) = test_suites::build_privileged_resolved(
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
                tys,
                expr_types: &mut typed.expr_types,
                errors: &mut errs,
                refs: &mut RefSink::new(),
                hints: &mut HintSink::new(),
                locals: &mut LocalsSink::new(),
                requirements: &mut RequirementSink::new(),
                callees: &mut typed.callees,
            },
        );
    }
    let cross = bynk_check::resolver::CrossContextInfo::default();
    // v0.70: `stub` value scaffolding is not user test logic, so its source
    // map is discarded — it stays unmapped (a deliberate scope cut).
    emitter::lower_block_to_async_body(&block, ret_type, &mut typed, &cross, runtime_use).0
}

fn synthetic_typed_commons_for_target(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    tys: &Arc<checker::Types>,
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
        callees: HashMap::new(),
        warnings: vec![],
        // T3.6b (R4.1): the *caller's* table, not a fresh one. A synthesised
        // commons starts with an empty `expr_types`, but the case/property
        // checks below fill it in — interning into the table they were handed
        // — and the lowering then resolves those ids back through this field.
        // A table of its own would make every one of those reads a
        // wrong-table lookup.
        ty_intern: Arc::clone(tys),
        actor_bindings: HashMap::new(),
    }
}

/// `undefined as unknown as <ty>` — the placeholder value for an un-stubbed
/// capability/surface entry, shared by every branch below.
fn undefined_as_unknown_as(ty: impl Into<String>) -> TsExpr {
    TsExpr::As {
        expr: Box::new(TsExpr::As {
            expr: Box::new(ident("undefined")),
            ty: TsType::named("unknown"),
        }),
        ty: TsType::named(ty),
    }
}

/// #1479: returns the real [`TsStmt`] itself (was pre-printed `String`) — the
/// one real declaration this function ever built; its caller now prints it
/// directly via `bynk_ts::print_stmt`, the same shape it always used, just
/// one call further out.
fn emit_test_deps(
    target_name: &str,
    target_kind: UnitKind,
    stubs: &HashMap<String, ResolvedStub>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
) -> TsStmt {
    let mut entries: Vec<(String, TsExpr)> = Vec::new();
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
            let value = if stubs.contains_key(cap) {
                TsExpr::New {
                    callee: Box::new(ident(format!("__Stub_{cap}"))),
                    args: Vec::new(),
                }
            } else if let Some(provider) = table.providers.get(cap) {
                TsExpr::New {
                    callee: Box::new(member(
                        ident(ns.clone()),
                        provider.provider_name.name.clone(),
                    )),
                    args: Vec::new(),
                }
            } else {
                undefined_as_unknown_as(format!("{ns}.{cap}"))
            };
            entries.push((cap.clone(), value));
        }
        // v0.17 (Locale capability track, slice 1, #844): a capability
        // flattened in via `consumes U { Cap }` (e.g. an adapter's `Locale`)
        // is never in `table.capabilities` above — that holds only
        // capabilities this unit declares itself. Its real implementation is
        // wired by production `compose()` from a platform binding the test
        // module never imports, so an un-stubbed one is always the
        // placeholder, exactly like a locally-declared capability with no
        // provider.
        let mut flattened: Vec<(&String, &String)> = unit_flattened
            .get(target_name)
            .map(|m| m.iter().collect())
            .unwrap_or_default();
        flattened.sort_by_key(|(cap, _)| cap.as_str());
        for (cap, owner) in flattened {
            let owner_ns = owner.replace('.', "_");
            let value = if stubs.contains_key(cap) {
                TsExpr::New {
                    callee: Box::new(ident(format!("__Stub_{cap}"))),
                    args: Vec::new(),
                }
            } else {
                undefined_as_unknown_as(format!("{owner_ns}.{cap}"))
            };
            entries.push((cap.clone(), value));
        }
        // Cross-context surface: consumed contexts run with their real surface
        // (v0.118 `stub` is capability-only — a consumed-context capability
        // flattened via `consumes U { Cap }` is folded in via `unit_flattened`
        // above). An `adapter` target (e.g. `consumes bynk { Locale }`) has no
        // `makeSurface` at all — so it must not get a surface entry either
        // (Locale capability track, slice 1, #844).
        let consumed: Vec<String> = unit_consumes
            .get(target_name)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|q| {
                !matches!(
                    unit_tables.get(q).and_then(|t| t.kind),
                    Some(UnitKind::Adapter)
                )
            })
            .collect();
        let aliases = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let mut alias_for_target: HashMap<String, String> = HashMap::new();
        for (alias, q) in &aliases {
            alias_for_target.insert(q.clone(), alias.clone());
        }
        let mut surface_entries: Vec<(String, TsExpr)> = Vec::new();
        for q in &consumed {
            let key = alias_for_target
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            let other_ns = q.replace('.', "_");
            surface_entries.push((
                key,
                undefined_as_unknown_as(format!("ReturnType<typeof {other_ns}.makeSurface>")),
            ));
        }
        if !surface_entries.is_empty() {
            entries.push(("surface".to_string(), TsExpr::object(surface_entries)));
        }
    }
    // The same `"{  }"` double-space quirk `workers.rs`/`project.rs`/
    // `emit.rs`/`gen_ts_for_ty` (this file, Arc C slice 31) already carry as
    // opaque text: the pre-conversion `format!("  return {{ {} }};\n",
    // entries.join(", "))` template always has a space on each side of its
    // `{}` slot, so zero entries (a non-`Context` target, or a `Context`
    // with no capabilities/flattened caps/consumed surface) literally
    // produced a double space, not the tight `"{}"` `TsExpr::object`'s own
    // empty-entries shortcut renders — a real, reachable shape (an
    // integration target's own non-`Context` participants all hit it).
    let return_value = if entries.is_empty() {
        ident("{  }")
    } else {
        TsExpr::object(entries)
    };
    TsStmt::decl(
        TsDecl::Function {
            name: "makeTestDeps".to_string(),
            generics: Vec::new(),
            params: Vec::new(),
            return_type: None,
            body: vec![TsStmt::return_stmt(Some(return_value), None)],
            is_async: false,
            inline: false,
        },
        None,
    )
}

/// #18 (testing-track infra): a real value destructure plus a per-type alias,
/// instead of one `const { … } = ns as any`. Every bynk-declared type pairs an
/// `export type`/`export interface` with a companion runtime value
/// (`export const X = {...}`, a refinement's `of`/`unsafe`, a sum's variant
/// constructors, or simply `{}` for a plain record) — but destructuring does
/// not carry the *type* side of that merge into the new local binding, so
/// `const { X } = ns` alone leaves `X` usable only as a value. The blanket
/// `as any` papered over that by disabling type-checking for every
/// destructured name, not just the ones needing the workaround — which is
/// exactly why this generator's own output couldn't be gated by
/// `tsc --strict`: nothing inside a generated test body was ever really
/// checked. `type_names` (a subset of `value_names`) gets the alias;
/// `value_names` is unconditionally destructured with no cast.
/// #1479: returns real [`TsStmt`]s (was `out: &mut String`) — every caller
/// now appends via `crate::emitter::extend_printed_at(out, stmts, 2)`, the
/// same depth-2 this scaffold body's own statements always printed at.
fn emit_ns_destructure(ns: &str, value_names: &[String], type_names: &[String]) -> Vec<TsStmt> {
    let mut stmts = Vec::new();
    if !value_names.is_empty() {
        stmts.push(TsStmt::const_stmt(
            TsBindingName::ObjectPattern(value_names.to_vec()),
            None,
            ident(ns),
            None,
        ));
    }
    for t in type_names {
        stmts.push(TsStmt::decl(
            TsDecl::TypeAlias {
                name: t.clone(),
                type_params: Vec::new(),
                ty: TsType::named(format!("{ns}.{t}")),
            },
            None,
        ));
    }
    stmts
}

/// Emit the shared per-runner scope setup — agent reset, the `deps` factory, and
/// the destructurings (see [`emit_ns_destructure`]) that bring the target's,
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
        let stmt = TsStmt::expr_stmt(
            method_call(ident(target_ns.clone()), "__resetAgents", Vec::new()),
            None,
        );
        out.push_str(&bynk_ts::print_stmt(&stmt, 2));
    }
    // v0.117: the per-case recorded-call trace, and — for a context target with
    // capabilities — a `deps` wrapped so each capability operation records its
    // calls into `__obs`. Observations and `trace(Cap.op)` in the body read it.
    let obs_spec: Option<TsExpr> = if record_calls && target_kind == UnitKind::Context {
        unit_tables.get(target_name).and_then(|table| {
            if table.capabilities.is_empty() {
                return None;
            }
            let mut caps: Vec<&String> = table.capabilities.keys().collect();
            caps.sort();
            let entries: Vec<(String, TsExpr)> = caps
                .iter()
                .map(|c| {
                    let mut ops: Vec<&String> = table.capabilities[*c]
                        .ops
                        .iter()
                        .map(|o| &o.name.name)
                        .collect();
                    ops.sort();
                    (
                        (*c).clone(),
                        TsExpr::array(ops.into_iter().map(str_lit).collect()),
                    )
                })
                .collect();
            Some(TsExpr::object(entries))
        })
    } else {
        None
    };
    if target_kind == UnitKind::Context {
        if let Some(spec) = obs_spec {
            // P7.2: matches `emitter/lower.rs`'s own `{ args: unknown[] }` reads
            // of this exact shape (`Trace`/`Called`-with-predicate lowering).
            let obs_ty = TsType::named_with_args(
                "Record",
                vec![
                    TsType::named("string"),
                    TsType::array(TsType::Object(vec![
                        TsTypeMember::Prop {
                            name: "args".to_string(),
                            ty: TsType::array(TsType::named("unknown")),
                            optional: false,
                            readonly: false,
                        },
                        TsTypeMember::Prop {
                            name: "order".to_string(),
                            ty: TsType::named("number"),
                            optional: false,
                            readonly: false,
                        },
                    ])),
                ],
            );
            let obs_stmt = TsStmt::const_stmt(
                TsBindingName::Ident("__obs".to_string()),
                None,
                TsExpr::object(vec![
                    (
                        "log".to_string(),
                        TsExpr::As {
                            expr: Box::new(TsExpr::object(vec![])),
                            ty: obs_ty,
                        },
                    ),
                    ("n".to_string(), num_lit("0")),
                ]),
                None,
            );
            out.push_str(&bynk_ts::print_stmt(&obs_stmt, 2));
            let deps_stmt = TsStmt::const_stmt(
                TsBindingName::Ident("deps".to_string()),
                None,
                call(
                    ident("__bynkRecordDeps"),
                    vec![
                        call(ident("makeTestDeps"), Vec::new()),
                        spec,
                        ident("__obs"),
                    ],
                ),
                None,
            );
            out.push_str(&bynk_ts::print_stmt(&deps_stmt, 2));
        } else {
            let deps_stmt = TsStmt::const_stmt(
                TsBindingName::Ident("deps".to_string()),
                None,
                call(ident("makeTestDeps"), Vec::new()),
                None,
            );
            out.push_str(&bynk_ts::print_stmt(&deps_stmt, 2));
        }
    } else {
        let deps_stmt = TsStmt::const_stmt(
            TsBindingName::Ident("deps".to_string()),
            None,
            TsExpr::object(vec![]),
            None,
        );
        out.push_str(&bynk_ts::print_stmt(&deps_stmt, 2));
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
        let mut type_names: Vec<String> = table.types.keys().cloned().collect();
        type_names.sort();
        type_names.dedup();
        crate::emitter::extend_printed_at(
            out,
            emit_ns_destructure(&target_ns, &names, &type_names),
            2,
        );
    }
    // Bring in `uses` commons names too — the target's body can use them.
    // message-bundles slice 1 (#859): a name the target itself already
    // declares must be excluded here — the same local-shadows-`uses`
    // precedence `compose_unit_symbols` already applies to production
    // emission (project.rs's `combined_fns`/`combined_types`), which this
    // test-scaffold path builds independently and had never applied. Never
    // exercised before this slice: no prior commons declared a name also
    // present in something it `uses`. Without the filter, a target with its
    // own `render` (a messages block's synthetic one) *and* `uses
    // bynk.locale` (which also declares `render`) destructured both into one
    // scope — `Cannot redeclare block-scoped variable 'render'` under `tsc`.
    let target_local: std::collections::HashSet<&String> = unit_tables
        .get(target_name)
        .map(|t| t.types.keys().chain(t.fns.keys()).collect())
        .unwrap_or_default();
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                let mut names: Vec<String> = table
                    .types
                    .keys()
                    .chain(table.fns.keys())
                    .filter(|n| !target_local.contains(n))
                    .cloned()
                    .collect();
                names.sort();
                names.dedup();
                let mut type_names: Vec<String> = table
                    .types
                    .keys()
                    .filter(|n| !target_local.contains(n))
                    .cloned()
                    .collect();
                type_names.sort();
                type_names.dedup();
                crate::emitter::extend_printed_at(
                    out,
                    emit_ns_destructure(&ns, &names, &type_names),
                    2,
                );
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
            let is_adapter = matches!(
                unit_tables.get(q).and_then(|t| t.kind),
                Some(UnitKind::Adapter)
            );
            if let Some(table) = unit_tables.get(q) {
                let mut names: Vec<String> = table.types.keys().cloned().collect();
                names.sort();
                names.dedup();
                crate::emitter::extend_printed_at(out, emit_ns_destructure(&ns, &names, &names), 2);
            }
            // An `adapter` target has no `makeSurface`/`deps.surface` entry —
            // its capabilities are already flattened onto `deps` directly
            // (Locale capability track, slice 1, #844).
            if is_adapter {
                continue;
            }
            let key = alias_for
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            // P7.2: deferred, not narrowed. `deps.surface`'s own per-capability
            // mock shapes are constructed elsewhere in this file; typing `{key}`
            // correctly here needs cross-referencing that mock-construction code
            // rather than guessing a structural type, which risks a `tsc --strict`
            // mismatch against whatever it actually builds. The *node
            // representation* has no such gap, though — `TsExpr::OptionalMember`
            // already exists — so review of #1404 caught this site's own
            // `format!` construction as a real miss, not a documented carve-out.
            let alias_stmt = TsStmt::const_stmt(
                TsBindingName::Ident(key.clone()),
                None,
                TsExpr::OptionalMember {
                    object: Box::new(member(
                        TsExpr::As {
                            expr: Box::new(ident("deps")),
                            ty: TsType::named("any"),
                        },
                        "surface",
                    )),
                    property: key.clone(),
                },
                None,
            );
            out.push_str(&bynk_ts::print_stmt(&alias_stmt, 2));
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
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
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses, tys);
    // v0.117: re-type-check the case body (with the call-record types registered)
    // so the lowering has full expr types — collection kernels, notably a
    // `trace(Cap.op)` result's `List[…]` methods, dispatch on the checked type.
    if let Some((mut resolved, _)) = test_suites::build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        test_suites::register_call_record_types(&mut resolved, target_name, unit_tables);
        let mut throwaway_errors: Vec<CompileError> = Vec::new();
        let mut throwaway_refs = RefSink::new();
        // P6.21 review: `callees` is real output here now, not discarded —
        // see `typecheck_case_body`'s own doc comment for why a test-case
        // body's `Callee` classification was previously lost entirely.
        (typed.expr_types, typed.callees) = test_suites::typecheck_case_body(
            target_name,
            &case.body,
            case.span,
            unit_tables,
            &resolved,
            &mut throwaway_errors,
            &mut throwaway_refs,
            HashMap::new(),
            tys,
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
        target_service_handler_kinds(unit_tables.get(target_name)),
        test_agents,
        source,
        rel_path,
        runtime_use,
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
    let return_pass_true = TsStmt::return_stmt(
        Some(TsExpr::object(vec![(
            "pass".to_string(),
            TsExpr::Lit(TsLit::Bool(true)),
        )])),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&return_pass_true, 2));
    out.push_str("  } catch (e) {\n");
    let expectation_error_branch = if_(
        TsExpr::Binary {
            op: TsBinaryOp::InstanceOf,
            left: Box::new(ident("e")),
            right: Box::new(ident("ExpectationError")),
        },
        block(vec![TsStmt::return_stmt(
            Some(TsExpr::object(vec![
                ("pass".to_string(), TsExpr::Lit(TsLit::Bool(false))),
                (
                    "error".to_string(),
                    TsExpr::object(vec![
                        ("message".to_string(), member(ident("e"), "message")),
                        ("location".to_string(), member(ident("e"), "location")),
                    ]),
                ),
            ])),
            None,
        )]),
    );
    out.push_str(&bynk_ts::print_stmt(&expectation_error_branch, 2));
    let fallback_return = TsStmt::return_stmt(
        Some(TsExpr::object(vec![
            ("pass".to_string(), TsExpr::Lit(TsLit::Bool(false))),
            (
                "error".to_string(),
                TsExpr::object(vec![
                    (
                        "message".to_string(),
                        call(ident("String"), vec![ident("e")]),
                    ),
                    ("location".to_string(), str_lit("unknown")),
                ]),
            ),
        ])),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&fallback_return, 2));
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
/// #1479: returns real [`TsStmt`]s (was `String`) — its one caller now
/// appends via `crate::emitter::extend_printed_at(out, stmts, 0)`, the same
/// depth-0 top-level shape these type aliases always printed at.
fn observation_call_record_types(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
) -> Vec<TsStmt> {
    let Some(table) = unit_tables.get(target_name) else {
        return Vec::new();
    };
    // Named/opaque parameter types are re-exported under the target's namespace,
    // so qualify them (`AuthId` → `commerce_payment.AuthId`); base types are
    // unaffected. Matches the mock-signature qualification.
    let scope_ns = target_name.replace('.', "_");
    let scope_type_names: HashSet<String> = table.types.keys().cloned().collect();
    let mut caps: Vec<&String> = table.capabilities.keys().collect();
    caps.sort();
    let mut stmts = Vec::new();
    for cap in caps {
        for op in &table.capabilities[cap].ops {
            let name = checker::call_record_type_name(cap, &op.name.name);
            let fields: Vec<TsTypeMember> = op
                .params
                .iter()
                .map(|p| TsTypeMember::Prop {
                    name: p.name.name.clone(),
                    ty: emitter::ts_type_ref_qualified_ts_type(
                        &p.type_ref,
                        &scope_type_names,
                        &scope_ns,
                    ),
                    optional: false,
                    readonly: false,
                })
                .collect();
            // The same `"{  }"` double-space quirk as elsewhere in this file
            // (`gen_ts_for_ty`/`canon_ts_for_ty`, Arc C slice 31; `emit_test_deps`
            // above) — the pre-conversion `format!("type {name} = {{ {} }};\n",
            // fields.join("; "))` template always has a space on each side of its
            // `{}` slot, so a zero-param capability op (real and reachable)
            // literally produced a double space, not the tight `"{}"`
            // `TsType::Object`'s own empty-members shortcut renders.
            let ty = if fields.is_empty() {
                TsType::named("{  }")
            } else {
                TsType::Object(fields)
            };
            stmts.push(TsStmt::decl(
                TsDecl::TypeAlias {
                    name,
                    type_params: Vec::new(),
                    ty,
                },
                None,
            ));
        }
    }
    stmts
}

/// v0.117: the observation runtime — wraps each observed capability operation on
/// the test `deps` so every call records its arguments and a monotonic order
/// index into the per-case trace `__obs`. Emitted once per module that observes.
/// Finding #17: source lives at `emitter/test_runtime/observation.ts`.
fn observation_runtime_helpers() -> String {
    include_str!("../emitter/test_runtime/observation.ts").to_string()
}

fn property_runtime_helpers() -> String {
    include_str!("../emitter/test_runtime/property.ts").to_string()
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
    include_str!("../emitter/test_runtime/history.ts").to_string()
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
) -> TsExpr {
    let draw = match base {
        BaseType::Int => {
            let (lo, hi, _) = int_bounds(refinement);
            method_call(
                ident("rng"),
                "int",
                vec![num_lit(format!("{lo}n")), num_lit(format!("{hi}n"))],
            )
        }
        BaseType::Float => {
            let (lo, hi) = float_bounds(refinement);
            method_call(
                ident("rng"),
                "float",
                vec![num_lit(lo.to_string()), num_lit(hi.to_string())],
            )
        }
        BaseType::String => {
            let min = str_min(refinement);
            method_call(
                ident("rng"),
                "str",
                vec![num_lit(min.to_string()), num_lit((min + 8).to_string())],
            )
        }
        BaseType::Bool => method_call(ident("rng"), "bool", vec![]),
        _ => ident(base_canon(base)),
    };
    // `unchecked_construct_test` is also called from `lower.rs`'s own
    // permanently-excluded general expression lowering (Third correction) and
    // must keep its exact `-> String` signature — its own branching (a real
    // `.unsafe(...)` call vs a real `(... as any)` cast) is not duplicated here
    // as a second node-building path; its already-formed text is carried as an
    // opaque `Ident` instead, the same "an unconverted shared sibling's own
    // text stays opaque" posture this track already uses (P7.9's `ts_base`,
    // #1369's `Cell` zero value).
    ident(emitter::unchecked_construct_test(
        name,
        &bynk_ts::print_expr(&draw),
        is_opaque,
    ))
}

/// Coerce a Sum-variant-payload/Record-field value to `number` when its own
/// resolved type draws/emits as `bigint` — a bare `Int`, or a refined/opaque
/// `Int` (#1398). `gen_ts_for_ty`'s/`canon_ts_for_ty`'s own top-level
/// `Base(Int)` arm draws/emits `bigint` — correct there, since a top-level
/// `for all` binding's own boundaries/shrink machinery is bigint-typed
/// throughout (the "P7.2: deferred, not narrowed" representation gap, #1426
/// tracks its own further consequences). But a Sum variant's real compiled
/// constructor and a Record's real compiled object shape both type an `Int`
/// field `number` (`ts_type_ref_to_ts_type`'s own mapping) — including a
/// refined/opaque one, which erases to `number & { __brand }`, not `bigint`
/// — so a bigint-drawing payload/field value recursed into from *inside* a
/// Sum/Record arm needs this coercion the top-level arm itself must not
/// have. A refined/opaque `Int` (`Percent`, etc.) routes through
/// `refined_gen_ts`'s/its `canon_ts_for_ty` sibling's own
/// `unchecked_construct_test`-wrapped cast (`(rng.int(...) as any)` /
/// `Pct.unsafe(rng.int(...))`) — that cast is compile-time only and does
/// nothing to the runtime value, so it still needs this same coercion
/// (review of #1428): confirmed the exact `TypeError: Cannot mix BigInt and
/// other types` this issue names still reproduces for a refined-Int record
/// field without it. `Number(...)` around either wrapper form is safe:
/// `.unsafe`/`as any` are both identity at runtime, never anything more than
/// a re-tag. Every other type's own recursive result is already
/// number/string/bool/object-shaped, so this stays a no-op there. And the
/// depth-exhaustion `undefined` sentinel (`canon_ts_for_ty`'s own `depth ==
/// 0` early return, reachable through a self-recursive Sum whose *first*
/// variant carries an `Int` payload — `prop_binding_generable` validates only
/// that variant, `bynk-check/src/test_suites.rs`) is passed through
/// unwrapped rather than turned into a silent `Number(undefined)` (`NaN`) —
/// also review of #1428.
fn coerce_int_field(
    t: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
    value: TsExpr,
) -> TsExpr {
    if matches!(&value, TsExpr::Ident(s) if s == "undefined") {
        return value;
    }
    if ty_draws_bigint(t, types, tys) {
        call(ident("Number"), vec![value])
    } else {
        value
    }
}

/// Does `t` draw/emit as a JS `bigint` — a bare `Int`, or a refined/opaque
/// type over one? The shared predicate behind [`coerce_int_field`]'s own
/// `Number(…)`-wrap decision and [`destructure_vals`]'s "does this binding
/// set need coercion at all" check (#1426).
fn ty_draws_bigint(
    t: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
) -> bool {
    match &*tys.get(t) {
        checker::Ty::Base(BaseType::Int) => true,
        checker::Ty::Named { name, .. } => matches!(
            types.get(name).map(|d| &d.body),
            Some(TypeBody::Refined {
                base: BaseType::Int,
                ..
            }) | Some(TypeBody::Opaque {
                base: BaseType::Int,
                ..
            })
        ),
        _ => false,
    }
}

/// Coerce a top-level `for all`/attack-parameter binding's own drawn value to
/// `number` (#1426) — like [`coerce_int_field`], but for the bind site rather
/// than a nested Sum/Record field, which needs a genuinely different result
/// shape for a *named* refined/opaque `Int`, not just the same `Number(…)`
/// wrap. Review of #1432, finding 1: a bare `Int` really does compile to
/// plain `number` everywhere, so `Number(value)` alone is correct and
/// sufficient there. But a refined/opaque `Int` (`Percent`, etc.) compiles to
/// a *branded* type (`number & { readonly __brand: "Percent" }`,
/// `emit_refined_type`) — a plain `Number(value)` local's own inferred type
/// is bare `number`, not the brand, so passing it to a real function
/// expecting the named type is a genuine new `tsc --strict` `TS2345` error
/// this fix would otherwise introduce (confirmed: reverting to a bare
/// `Number(…)` wrap here reproduces it for a minimal `fn label(p: Percent)`
/// repro, which compiled and ran fine pre-#1426 since the pre-fix local was
/// `any`, not a concrete type). Closed the same way `refined_gen_ts` already
/// keeps a branded draw compiling — wrap the coerced `Number(…)` in `(… as
/// any)`, restoring the escape hatch a plain array-destructured `any[]`
/// element always had for this shape, while still fixing the *runtime* value
/// (`as any` is compile-time only; the `Number(…)` underneath still runs).
/// Every other type's own value (including a bare `Int`) is unaffected —
/// only a *named* refined/opaque `Int` binding gets the extra wrap.
fn coerce_top_level_int_binding(
    t: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
    value: TsExpr,
) -> TsExpr {
    let is_named_int = matches!(
        &*tys.get(t),
        checker::Ty::Named { .. } if ty_draws_bigint(t, types, tys)
    );
    let coerced = coerce_int_field(t, types, tys, value);
    if is_named_int {
        TsExpr::As {
            expr: Box::new(coerced),
            ty: TsType::named("any"),
        }
    } else {
        coerced
    }
}

/// Destructure `__vals` into its named locals, coercing each `Int`-drawing
/// (bare, refined, or opaque) binding's own runtime value to `number` at the
/// bind site (#1426) via [`coerce_top_level_int_binding`]. `binding_gen`'s
/// own boundaries/shrink machinery for a top-level `for all`/attack-parameter
/// `Int` binding is deliberately `bigint`-typed throughout —
/// `coerce_int_field`'s own doc comment names this as correct, since the
/// shrink loop's `current[i]`/`vals[i]` elements must keep matching what
/// `gens[i].shrink(v)` expects — but the *local* a predicate/attacked-function
/// body evaluates ordinary Bynk `Int` arithmetic against must not stay
/// `bigint`, or `n + 1` throws `TypeError: Cannot mix BigInt and other types`
/// the instant it meets a `number` literal.  Never previously hit: every
/// existing fixture's predicate either avoided arithmetic on a drawn `Int`
/// entirely, or routed through a refined type's own compile-time-only `as
/// any` cast (`refined_gen_ts`), which suppresses the *static* `tsc` error,
/// not the *runtime* one. Reuses `coerce_int_field`'s own "does this type
/// draw bigint" check — the same representation gap #1398/#1428 already
/// closed for record/sum-field values, just at the binding site itself
/// rather than a nested field; `binding_types[i]` is `None` when the
/// binding's own type didn't resolve (mirrors `gens`' own
/// `unwrap_or`/`undefined`-sentinel fallback above — `__vals[i]` is
/// `undefined` there regardless of what type it "should" have been, so it
/// passes through unwrapped either way).
///
/// Falls back to the original plain `const [a, b, ...] = __vals;` array
/// destructure when *no* binding actually needs coercion — a record/sum/
/// string/bool binding (the common case) draws its own top-level value
/// already `number`/`string`/`boolean`/object-shaped, only a bare or
/// refined/opaque `Int` binding draws `bigint` — so the common case emits
/// byte-identical text to before this fix, and only a fixture with a real
/// top-level `Int`-drawing binding sees its own destructure change shape.
fn destructure_vals(
    names: &[String],
    binding_types: &[Option<checker::TyId>],
    types: Option<&HashMap<String, Arc<TypeDecl>>>,
    tys: &Arc<Types>,
) -> String {
    let needs_coercion = binding_types
        .iter()
        .any(|ty| matches!((ty, types), (Some(t), Some(types)) if ty_draws_bigint(*t, types, tys)));
    if !needs_coercion {
        return format!("const [{}] = __vals;", names.join(", "));
    }
    names
        .iter()
        .zip(binding_types)
        .enumerate()
        .map(|(i, (name, ty))| {
            let raw = TsExpr::Index {
                object: Box::new(ident("__vals")),
                index: Box::new(num_lit(i.to_string())),
            };
            let value = match (ty, types) {
                (Some(t), Some(types)) => coerce_top_level_int_binding(*t, types, tys, raw),
                _ => raw,
            };
            format!("const {name} = {};", bynk_ts::print_expr(&value))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A TypeScript expression drawing a random inhabitant of a resolved type using
/// the in-scope `rng` — the property generator (DECISION P: a type is its own
/// inhabitant space). Sums pick a random variant; records generate every field.
fn gen_ts_for_ty(
    ty: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    depth: u32,
    tys: &Arc<Types>,
) -> TsExpr {
    if depth == 0 {
        // #1429: a bare `1` here starved `canon_ts_for_ty`'s own recursion
        // one level short of what `prop_binding_generable` (`bynk-check`)
        // already promised was reachable *for the top-level bound type*.
        // Both functions share the same recursion shape — `depth == 0`
        // checked unconditionally at entry, one unit spent per `Sum`/
        // `Record` step down into a field's own type, `Ty::Base` never
        // itself gating on `depth` — so a type `prop_binding_generable`
        // accepts within `PROP_GEN_DEPTH` is guaranteed to bottom out in
        // `canon_ts_for_ty` within that same budget: a self-recursive type
        // like `T = Base(n: Int) | Cons(tail: T)` previously exhausted
        // this fallback's own budget one field short of `Base`'s terminal
        // `n: Int`, landing a bare `undefined` inside a real, `number`-
        // typed constructor argument (`T.Base(undefined)`, `tsc TS2345`).
        //
        // Review of #1434, finding 1: `prop_binding_generable` only ever
        // descends a `Sum`'s *first* variant, but this function's own Sum
        // branch (below) builds a thunk for *every* variant — so `ty` here
        // can be a type the checker never validated at all (reached only
        // through a non-first variant, e.g. `Nope(b: Bad)` on
        // `T = Plain(n: Int) | Nope(b: Bad)`, where `Bad = Node(l: Bad, r:
        // Bad) | Leaf(n: Int)` is itself first-variant-unterminating). For
        // such a type hand it the full `PROP_GEN_DEPTH` budget regardless,
        // and `canon_ts_for_ty`'s own "always expand the first variant"
        // strategy walks a full branching tree before ever reaching the
        // `Leaf` escape — confirmed to reproduce a real multi-second,
        // 170MB+ emit for exactly this shape (a plausible emit-time
        // hang/OOM, not merely an invalid file) once this fallback started
        // handing out 12 levels of budget instead of 1.
        //
        // So: only spend the full budget when the checker actually
        // promised `ty` itself bottoms out within it: that's exactly what
        // `prop_binding_generable` decides, and — same "first variant
        // only, `.all` short-circuits on the first failing field" shape as
        // `canon_ts_for_ty` — costs at most `PROP_GEN_DEPTH` recursive
        // calls to answer, not a tree walk. Any other type falls back to
        // the original, deliberately conservative `1` — the same "produces
        // a small invalid file fast" behaviour this fallback always had
        // for a type the checker never vouched for, not a regression.
        if test_suites::prop_binding_generable(ty, types, test_suites::PROP_GEN_DEPTH, tys) {
            return canon_ts_for_ty(ty, types, test_suites::PROP_GEN_DEPTH, tys);
        }
        return canon_ts_for_ty(ty, types, 1, tys);
    }
    match &*tys.get(ty) {
        checker::Ty::Base(BaseType::Int) => {
            let (lo, hi, _) = int_bounds(None);
            method_call(
                ident("rng"),
                "int",
                vec![num_lit(format!("{lo}n")), num_lit(format!("{hi}n"))],
            )
        }
        checker::Ty::Base(BaseType::String) => {
            method_call(ident("rng"), "str", vec![num_lit("0"), num_lit("8")])
        }
        checker::Ty::Base(BaseType::Bool) => method_call(ident("rng"), "bool", vec![]),
        checker::Ty::Base(BaseType::Float) => method_call(
            ident("rng"),
            "float",
            vec![num_lit("-1000"), num_lit("1000")],
        ),
        checker::Ty::Base(b) => ident(base_canon(*b)),
        checker::Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return ident("undefined");
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
                    let thunks: Vec<TsExpr> = s
                        .variants
                        .iter()
                        .map(|v| {
                            let body = if v.payload.is_empty() {
                                member(ident(name), &v.name.name)
                            } else {
                                let args: Vec<TsExpr> = v
                                    .payload
                                    .iter()
                                    .map(|f| {
                                        checker::resolve_type_ref(&f.type_ref, types, tys)
                                            .map(|t| {
                                                coerce_int_field(
                                                    t,
                                                    types,
                                                    tys,
                                                    gen_ts_for_ty(t, types, depth - 1, tys),
                                                )
                                            })
                                            .unwrap_or_else(|| ident("undefined"))
                                    })
                                    .collect();
                                call(member(ident(name), &v.name.name), args)
                            };
                            TsExpr::Arrow {
                                params: Vec::new(),
                                is_async: false,
                                generics: Vec::new(),
                                return_type: None,
                                body: Box::new(TsArrowBody::Expr(Box::new(body))),
                            }
                        })
                        .collect();
                    if thunks.is_empty() {
                        ident("undefined")
                    } else {
                        method_call(ident("rng"), "pick", vec![TsExpr::array(thunks)])
                    }
                }
                TypeBody::Record(r) => {
                    let fields: Vec<(String, TsExpr)> = r
                        .fields
                        .iter()
                        .map(|f| {
                            let g = checker::resolve_type_ref(&f.type_ref, types, tys)
                                .map(|t| {
                                    coerce_int_field(
                                        t,
                                        types,
                                        tys,
                                        gen_ts_for_ty(t, types, depth - 1, tys),
                                    )
                                })
                                .unwrap_or_else(|| ident("undefined"));
                            (f.name.name.clone(), g)
                        })
                        .collect();
                    // The same `"{  }"` double-space quirk `workers.rs`/
                    // `project.rs`/`emit.rs` already carry as opaque text
                    // (#1321/#1327/#1390): the pre-conversion `format!("{{
                    // {} }}", fields.join(", "))` template always has a space
                    // on each side of its `{}` slot, so zero fields literally
                    // produced a double space, not the tight `"{}"`
                    // `TsExpr::object`'s own empty-entries shortcut renders.
                    // A zero-field record is exotic but not provably
                    // unreachable — guarded rather than assumed.
                    if fields.is_empty() {
                        ident("{  }")
                    } else {
                        TsExpr::object(fields)
                    }
                }
            }
        }
        _ => ident("undefined"),
    }
}

/// A canonical (deterministic, boundary) inhabitant of a resolved type — the
/// boundary value the runner draws first (refinement floor / minimum length /
/// first variant), and the shrink target for sums.
fn canon_ts_for_ty(
    ty: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    depth: u32,
    tys: &Arc<Types>,
) -> TsExpr {
    if depth == 0 {
        return ident("undefined");
    }
    match &*tys.get(ty) {
        checker::Ty::Base(BaseType::Int) => num_lit("0n"),
        checker::Ty::Base(BaseType::String) => str_lit(""),
        checker::Ty::Base(BaseType::Bool) => TsExpr::Lit(TsLit::Bool(true)),
        checker::Ty::Base(BaseType::Float) => num_lit("0"),
        checker::Ty::Base(b) => ident(base_canon(*b)),
        checker::Ty::Named { name, .. } => {
            let Some(decl) = types.get(name) else {
                return ident("undefined");
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
                            num_lit(format!("{lo}n"))
                        }
                        BaseType::Float => {
                            let (lo, _) = float_bounds(refinement.as_ref());
                            num_lit(lo.to_string())
                        }
                        BaseType::String => {
                            let min = str_min(refinement.as_ref());
                            str_lit("x".repeat(min.max(0) as usize))
                        }
                        BaseType::Bool => TsExpr::Lit(TsLit::Bool(true)),
                        other => ident(base_canon(*other)),
                    };
                    ident(emitter::unchecked_construct_test(
                        name,
                        &bynk_ts::print_expr(&lit),
                        matches!(decl.body, TypeBody::Opaque { .. }),
                    ))
                }
                TypeBody::Sum(s) => match s.variants.first() {
                    None => ident("undefined"),
                    Some(v) if v.payload.is_empty() => member(ident(name), &v.name.name),
                    Some(v) => {
                        let args: Vec<TsExpr> = v
                            .payload
                            .iter()
                            .map(|f| {
                                checker::resolve_type_ref(&f.type_ref, types, tys)
                                    .map(|t| {
                                        coerce_int_field(
                                            t,
                                            types,
                                            tys,
                                            canon_ts_for_ty(t, types, depth - 1, tys),
                                        )
                                    })
                                    .unwrap_or_else(|| ident("undefined"))
                            })
                            .collect();
                        call(member(ident(name), &v.name.name), args)
                    }
                },
                TypeBody::Record(r) => {
                    let fields: Vec<(String, TsExpr)> = r
                        .fields
                        .iter()
                        .map(|f| {
                            let g = checker::resolve_type_ref(&f.type_ref, types, tys)
                                .map(|t| {
                                    coerce_int_field(
                                        t,
                                        types,
                                        tys,
                                        canon_ts_for_ty(t, types, depth - 1, tys),
                                    )
                                })
                                .unwrap_or_else(|| ident("undefined"));
                            (f.name.name.clone(), g)
                        })
                        .collect();
                    // Same `"{  }"` double-space quirk as `gen_ts_for_ty`'s
                    // own Record branch above — see its comment for why.
                    if fields.is_empty() {
                        ident("{  }")
                    } else {
                        TsExpr::object(fields)
                    }
                }
            }
        }
        _ => ident("undefined"),
    }
}

/// The generator descriptor for one `for all` binding: boundary values, a random
/// generator, and a shrinker (all real TypeScript expressions, evaluated in the
/// runner's scope where the type constructors are in scope).
struct BindingGen {
    boundaries: Vec<TsExpr>,
    gen_ts: TsExpr,
    shrink: TsExpr,
}

/// Build the generator descriptor for a binding whose resolved type is `ty`.
fn binding_gen(
    ty: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
) -> BindingGen {
    let gen_ts = gen_ts_for_ty(ty, types, test_suites::PROP_GEN_DEPTH, tys);
    let (boundaries, shrink) = match &*tys.get(ty) {
        checker::Ty::Base(BaseType::Int) => {
            let (lo, hi, floor) = int_bounds(None);
            (
                vec![
                    num_lit(format!("{floor}n")),
                    num_lit(format!("{hi}n")),
                    num_lit(format!("{lo}n")),
                ],
                call(
                    ident("__bynkShrinkInt"),
                    vec![ident("v"), num_lit(format!("{floor}n"))],
                ),
            )
        }
        checker::Ty::Base(BaseType::String) => (
            vec![str_lit("")],
            call(ident("__bynkShrinkString"), vec![ident("v"), num_lit("0")]),
        ),
        checker::Ty::Base(BaseType::Bool) => (
            vec![
                TsExpr::Lit(TsLit::Bool(true)),
                TsExpr::Lit(TsLit::Bool(false)),
            ],
            TsExpr::Paren(Box::new(cond_expr(
                ident("v"),
                TsExpr::array(vec![TsExpr::Lit(TsLit::Bool(false))]),
                TsExpr::array(vec![]),
            ))),
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
                                ident(emitter::unchecked_construct_test(
                                    name,
                                    &format!("{lo}n"),
                                    is_opaque,
                                )),
                                ident(emitter::unchecked_construct_test(
                                    name,
                                    &format!("{hi}n"),
                                    is_opaque,
                                )),
                            ],
                            method_call(
                                call(
                                    ident("__bynkShrinkInt"),
                                    vec![ident("v"), num_lit(format!("{floor}n"))],
                                ),
                                "map",
                                vec![TsExpr::Arrow {
                                    params: vec![TsParam {
                                        name: "__n".to_string(),
                                        ty: Some(TsType::named("bigint")),
                                        optional: false,
                                    }],
                                    is_async: false,
                                    generics: Vec::new(),
                                    return_type: None,
                                    body: Box::new(TsArrowBody::Expr(Box::new(ident(shrunk)))),
                                }],
                            ),
                        )
                    }
                    BaseType::String => {
                        let min = str_min(refinement.as_ref());
                        let lit = format!("\"{}\"", "x".repeat(min.max(0) as usize));
                        let shrunk = emitter::unchecked_construct_test(name, "__s", is_opaque);
                        (
                            vec![ident(emitter::unchecked_construct_test(
                                name, &lit, is_opaque,
                            ))],
                            method_call(
                                call(
                                    ident("__bynkShrinkString"),
                                    vec![ident("v"), num_lit(min.to_string())],
                                ),
                                "map",
                                vec![TsExpr::Arrow {
                                    params: vec![TsParam {
                                        name: "__s".to_string(),
                                        ty: Some(TsType::named("string")),
                                        optional: false,
                                    }],
                                    is_async: false,
                                    generics: Vec::new(),
                                    return_type: None,
                                    body: Box::new(TsArrowBody::Expr(Box::new(ident(shrunk)))),
                                }],
                            ),
                        )
                    }
                    _ => (
                        vec![canon_ts_for_ty(ty, types, test_suites::PROP_GEN_DEPTH, tys)],
                        TsExpr::array(vec![]),
                    ),
                }
            }
            Some(TypeBody::Sum(_)) => {
                let canon = canon_ts_for_ty(ty, types, test_suites::PROP_GEN_DEPTH, tys);
                (vec![canon.clone()], TsExpr::array(vec![canon]))
            }
            _ => (
                vec![canon_ts_for_ty(ty, types, test_suites::PROP_GEN_DEPTH, tys)],
                TsExpr::array(vec![]),
            ),
        },
        _ => (Vec::new(), TsExpr::array(vec![])),
    };
    BindingGen {
        boundaries,
        gen_ts,
        shrink,
    }
}

/// `{ [name: "…",] boundaries: […], gen: (rng: any) => …, shrink: (v: any) => …,
/// show: (v: any) => __bynkShow(v) }` — the one generator-descriptor object
/// shape [`emit_test_property_function`]/[`emit_test_history_property_function`]/
/// [`emit_contract_attack_function`] each build once per binding/param, now
/// directly from `binding_gen`'s own real `TsExpr` fields (Arc C slice A,
/// #1395) instead of printing them back to text and re-embedding via
/// `format!` — the seam those three functions' own doc comments named as
/// "not yet converted" is closed here. `name` is absent for a history
/// handler's own per-param generator (no `name` key at all in that shape).
fn gen_descriptor_entry(name: Option<TsExpr>, bg: &BindingGen) -> TsExpr {
    let mut entries: Vec<(String, TsExpr)> = Vec::new();
    if let Some(n) = name {
        entries.push(("name".to_string(), n));
    }
    entries.push((
        "boundaries".to_string(),
        TsExpr::array(bg.boundaries.clone()),
    ));
    entries.push((
        "gen".to_string(),
        TsExpr::Arrow {
            params: vec![TsParam {
                name: "rng".to_string(),
                ty: Some(TsType::named("any")),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            // #1397: an unparenthesised object-literal arrow body parses as
            // a block (`{ a: … }` reads `a:` as a statement label, and a
            // bare `{  }` reads as an empty block) — for a record-typed
            // binding, `bg.gen_ts` is `TsExpr::Object` for a non-empty
            // record, or `gen_ts_for_ty`'s own `"{  }"` sentinel `TsExpr::
            // Ident` (its Record arm's double-space-quirk comment) for a
            // zero-field one. Wrap either in `TsExpr::Paren` here, the one
            // choke point all three call sites share. The `Ident` arm
            // matches by leading `{` rather than the sentinel's exact
            // double-space spelling (review of #1425): a valid identifier
            // can never start with `{`, so this is still a no-op for every
            // other `gen_ts` shape, but it keeps matching if that
            // formatting quirk is ever normalised away (the same cleanup
            // #1321/#1327/#1390 already invite for its other two copies).
            body: Box::new(TsArrowBody::Expr(Box::new(match &bg.gen_ts {
                o @ TsExpr::Object { .. } => TsExpr::Paren(Box::new(o.clone())),
                o @ TsExpr::Ident(s) if s.trim_start().starts_with('{') => {
                    TsExpr::Paren(Box::new(o.clone()))
                }
                e => e.clone(),
            }))),
        },
    ));
    entries.push((
        "shrink".to_string(),
        TsExpr::Arrow {
            params: vec![TsParam {
                name: "v".to_string(),
                ty: Some(TsType::named("any")),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            body: Box::new(TsArrowBody::Expr(Box::new(bg.shrink.clone()))),
        },
    ));
    entries.push((
        "show".to_string(),
        TsExpr::Arrow {
            params: vec![TsParam {
                name: "v".to_string(),
                ty: Some(TsType::named("any")),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            body: Box::new(TsArrowBody::Expr(Box::new(call(
                ident("__bynkShow"),
                vec![ident("v")],
            )))),
        },
    ));
    TsExpr::object(entries)
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
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
    let resolved = test_suites::build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    )
    .map(|(r, _)| r);
    // P7.2: deferred, not narrowed. `v`'s real type varies per binding, and
    // for `Int` (and refined-`Int`) bindings the *internal* property-test
    // representation is `bigint` (see `rng.int(...)`'s own `n`-suffixed
    // literals and `__bynkShrinkInt(v: bigint, ...)` above) — not `number`,
    // which is what a general `ts_type_ref`/`ts_ty` rendering of the same
    // declared type would produce. Narrowing correctly needs threading the
    // resolved type (or its internal representation) out of `BindingGen`
    // itself, not a same-line text change, and a wrong guess here risks a
    // real `tsc --strict` failure specifically on the shrink helpers' own
    // typed parameters.
    let gens: Vec<TsExpr> = prop
        .forall
        .bindings
        .iter()
        .map(|b| {
            let bg = resolved
                .as_ref()
                .and_then(|r| checker::resolve_type_ref(&b.type_ref, &r.types, tys).map(|t| (t, r)))
                .map(|(t, r)| binding_gen(t, &r.types, tys))
                .unwrap_or(BindingGen {
                    boundaries: Vec::new(),
                    gen_ts: ident("undefined"),
                    shrink: TsExpr::array(vec![]),
                });
            gen_descriptor_entry(Some(str_lit(b.name.name.clone())), &bg)
        })
        .collect();
    let gens_stmt = TsStmt::const_stmt(
        TsBindingName::Ident("__gens".to_string()),
        None,
        TsExpr::multiline_array(gens),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&gens_stmt, 2));

    // The `where` filter and the predicate body, as closures over the tuple.
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses, tys);
    let cross = bynk_check::resolver::CrossContextInfo::default();
    let binding_names: Vec<String> = prop
        .forall
        .bindings
        .iter()
        .map(|b| b.name.name.clone())
        .collect();
    let binding_types: Vec<Option<checker::TyId>> = prop
        .forall
        .bindings
        .iter()
        .map(|b| {
            resolved
                .as_ref()
                .and_then(|r| checker::resolve_type_ref(&b.type_ref, &r.types, tys))
        })
        .collect();
    let destructure = destructure_vals(
        &binding_names,
        &binding_types,
        resolved.as_ref().map(|r| &r.types),
        tys,
    );
    // #1427: the `for all` bindings' own resolved types, in scope for both
    // the `where`-clause and body re-type-check passes below — mirrors
    // `emit_test_history_property_function`'s own `run: List[Step]` scope
    // entry, generalised to every binding a property declares.
    let initial_scope: HashMap<String, checker::TyId> = binding_names
        .iter()
        .cloned()
        .zip(binding_types.iter().copied())
        .filter_map(|(name, t)| t.map(|t| (name, t)))
        .collect();

    if let Some(w) = &prop.forall.where_pred {
        let synth = Block {
            statements: Vec::new(),
            tail: Box::new(w.clone()),
            span: w.span,
            tail_leading_comments: Vec::new(),
            implicit_tail: false,
        };
        // #1427: re-type-check the `where` clause, the same pass
        // `emit_test_case_function` already runs for a plain `case` body —
        // without it, `typed.expr_types` has no entry for anything here, so
        // a match's own discriminant type is unresolvable and payload-
        // binding lowering (`positional_field_name`) silently falls back to
        // the generic single-field `"value"` name instead of a variant's
        // own real field name.
        if let Some(r) = resolved.as_ref() {
            let mut throwaway_errors: Vec<CompileError> = Vec::new();
            let mut throwaway_refs = RefSink::new();
            let (where_types, where_callees) = test_suites::typecheck_case_body(
                target_name,
                &synth,
                w.span,
                unit_tables,
                r,
                &mut throwaway_errors,
                &mut throwaway_refs,
                initial_scope.clone(),
                tys,
            );
            typed.expr_types.extend(where_types);
            typed.callees.extend(where_callees);
        }
        let (src, _) = emitter::lower_block_to_async_body(
            &synth,
            &TypeRef::Base(BaseType::Bool, w.span),
            &mut typed,
            &cross,
            runtime_use,
        );
        // P7.2: deferred — `__vals`'s elements are heterogeneous, one per binding,
        // some internally `bigint`-represented; same reason as `__gens`'s own
        // construction elsewhere in this function.
        out.push_str("    const __where = (__vals: any[]) => {\n");
        out.push_str(&format!("      {destructure}\n"));
        for line in src.lines() {
            out.push_str("      ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    };\n");
    } else {
        let where_null = TsStmt::const_stmt(
            TsBindingName::Ident("__where".to_string()),
            None,
            TsExpr::Lit(TsLit::Null),
            None,
        );
        out.push_str(&bynk_ts::print_stmt(&where_null, 2));
    }

    let test_services: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.services.keys().cloned().collect())
        .unwrap_or_default();
    let test_agents: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.agents.keys().cloned().collect())
        .unwrap_or_default();
    // #1427: re-type-check the property body itself — same reason and same
    // pass as the `where` clause above.
    if let Some(r) = resolved.as_ref() {
        let mut throwaway_errors: Vec<CompileError> = Vec::new();
        let mut throwaway_refs = RefSink::new();
        let (body_types, body_callees) = test_suites::typecheck_case_body(
            target_name,
            &prop.forall.body,
            prop.span,
            unit_tables,
            r,
            &mut throwaway_errors,
            &mut throwaway_refs,
            initial_scope,
            tys,
        );
        typed.expr_types.extend(body_types);
        typed.callees.extend(body_callees);
    }
    // Property bodies are collaborator-free predicate scaffolding; like mock op
    // bodies, their source map is a deliberate scope cut (the `expect` location
    // still binds through `assert_loc`).
    let (body_src, _body_smb) = emitter::lower_test_case_body(
        &prop.forall.body,
        &mut typed,
        &cross,
        test_services,
        target_service_handler_kinds(unit_tables.get(target_name)),
        test_agents,
        source,
        rel_path,
        runtime_use,
    );
    // P7.2: deferred, same reason as `__where`'s own `__vals` above.
    out.push_str("    const __body = async (__vals: any[]) => {\n");
    out.push_str(&format!("      {destructure}\n"));
    for line in body_src.lines() {
        out.push_str("      ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    };\n");

    // Forward slashes so the emitted location/reproduce path is portable — on
    // Windows a `PathBuf` join yields `\`, which must not leak into the golden
    // `.ts` (mirrors `discovered_location`'s normalisation).
    let rel_path_fwd = rel_path.replace('\\', "/");
    let return_run = TsStmt::return_stmt(
        Some(await_expr(call(
            ident("__bynkRunProperty"),
            vec![TsExpr::object(vec![
                (
                    "seed".to_string(),
                    call(
                        ident("__bynkMix"),
                        vec![ident("__bynkSeed"), num_lit(prop_ordinal.to_string())],
                    ),
                ),
                ("cases".to_string(), num_lit("100")),
                ("gens".to_string(), ident("__gens")),
                ("where".to_string(), ident("__where")),
                ("body".to_string(), ident("__body")),
                ("name".to_string(), str_lit(prop.name.clone())),
                ("location".to_string(), str_lit(rel_path_fwd.clone())),
                ("file".to_string(), str_lit(rel_path_fwd)),
            ])],
        ))),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&return_run, 2));
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
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
        let return_pass_true = TsStmt::return_stmt(
            Some(TsExpr::object(vec![(
                "pass".to_string(),
                TsExpr::Lit(TsLit::Bool(true)),
            )])),
            None,
        );
        out.push_str(&bynk_ts::print_stmt(&return_pass_true, 2));
        out.push_str("}\n");
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
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses, tys);
    let mut handler_descs: Vec<TsExpr> = Vec::new();
    if let Some((mut resolved, _)) = test_suites::build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        test_suites::register_call_record_types(&mut resolved, target_name, unit_tables);
        let inner = TypeRef::Named(Ident {
            name: agent_name.to_string(),
            span: prop.span,
        });
        let mut throwaway_refs = RefSink::new();
        let step_ty = test_suites::check_history_binding(
            &inner,
            prop.span,
            &mut resolved,
            &mut throwaway_refs,
            tys,
        )
        .ok();

        // Per-handler argument generators (the slice-2 value generator over each
        // handler parameter), in declaration order — the sequence generator picks a
        // handler uniformly and draws its arguments here.
        if let Some(agent) = resolved.agents.get(agent_name) {
            for h in test_suites::history_handlers(agent) {
                let tag = test_suites::history_variant_name(&h.method_name.as_ref().unwrap().name);
                // P7.2: deferred — same reason as `__gens`'s own construction
                // above (`Int`'s internal `bigint` representation).
                let gens: Vec<TsExpr> = h
                    .params
                    .iter()
                    .map(|p| {
                        let bg = checker::resolve_type_ref(&p.type_ref, &resolved.types, tys)
                            .map(|t| binding_gen(t, &resolved.types, tys))
                            .unwrap_or(BindingGen {
                                boundaries: Vec::new(),
                                gen_ts: ident("undefined"),
                                shrink: TsExpr::array(vec![]),
                            });
                        gen_descriptor_entry(None, &bg)
                    })
                    .collect();
                handler_descs.push(TsExpr::object(vec![
                    ("tag".to_string(), str_lit(tag)),
                    ("gens".to_string(), TsExpr::array(gens)),
                ]));
            }
        }

        if let Some(step_ty) = step_ty {
            let mut scope: HashMap<String, checker::TyId> = HashMap::new();
            scope.insert(run_var.to_string(), tys.intern(step_ty));
            let mut throwaway_errors: Vec<CompileError> = Vec::new();
            let mut throwaway_refs2 = RefSink::new();
            // P6.21 review: `callees` is real output here now — see
            // `typecheck_case_body`'s own doc comment.
            (typed.expr_types, typed.callees) = test_suites::typecheck_case_body(
                target_name,
                &prop.forall.body,
                prop.span,
                unit_tables,
                &resolved,
                &mut throwaway_errors,
                &mut throwaway_refs2,
                scope,
                tys,
            );
        }
        // Carry the synthetic call/step/state types into the lowering commons so
        // `is` on the call sum and field access on the step resolve.
        for (n, d) in &resolved.types {
            typed.types.entry(n.clone()).or_insert_with(|| d.clone());
        }
    }

    let handlers_stmt = TsStmt::const_stmt(
        TsBindingName::Ident("__handlers".to_string()),
        None,
        TsExpr::multiline_array(handler_descs),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&handlers_stmt, 2));

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
        target_service_handler_kinds(unit_tables.get(target_name)),
        test_agents,
        source,
        rel_path,
        runtime_use,
    );
    // P7.2: matches `__bynkDriveHistory_*`'s own real signature — see
    // `emit.rs`'s own driver-signature narrowing and the matching `__drive`
    // call site below (`{target_ns}.__bynkDriveHistory_{agent_name}`).
    out.push_str("    const __body = async (__run: Array<{ h: number, args: unknown[] }>) => {\n");
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
    // P7.2: `seq` matches `__bynkDriveHistory_*`'s own real param type
    // (`emit.rs`'s own driver-signature narrowing). `(target_ns as any)` and
    // `deps` stay deferred — the callee's own `deps: any` param is itself
    // deferred (per-handler `given` sets can differ; see `emit.rs`), and
    // `target_ns`'s own generated namespace shape wasn't traced here. The
    // `Array<{ h: number, args: unknown[] }>` param type stays one opaque
    // `TsType::named` string — its own inline object type's real member
    // separator is `, ` (a comma), not `TsType::Object`'s own established
    // `; ` (semicolon) convention, so building it as a real `TsType::Object`
    // would silently change the emitted bytes; the same "odd, one-off shape
    // stays opaque text" posture this track already uses for `Query[T]`/
    // `this.state.storage.get<T>`/`ReturnType<typeof X>`.
    let drive_stmt = TsStmt::const_stmt(
        TsBindingName::Ident("__drive".to_string()),
        None,
        TsExpr::Arrow {
            params: vec![TsParam {
                name: "seq".to_string(),
                ty: Some(TsType::named("Array<{ h: number, args: unknown[] }>")),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            body: Box::new(TsArrowBody::Expr(Box::new(method_call(
                TsExpr::As {
                    expr: Box::new(ident(target_ns)),
                    ty: TsType::named("any"),
                },
                &format!("__bynkDriveHistory_{agent_name}"),
                vec![ident("seq"), ident("deps")],
            )))),
        },
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&drive_stmt, 2));

    let rel_path_fwd = rel_path.replace('\\', "/");
    let return_run = TsStmt::return_stmt(
        Some(await_expr(call(
            ident("__bynkRunHistory"),
            vec![TsExpr::object(vec![
                (
                    "seed".to_string(),
                    call(
                        ident("__bynkMix"),
                        vec![ident("__bynkSeed"), num_lit(prop_ordinal.to_string())],
                    ),
                ),
                ("cases".to_string(), num_lit("60")),
                ("maxLen".to_string(), num_lit("16")),
                ("handlers".to_string(), ident("__handlers")),
                ("drive".to_string(), ident("__drive")),
                ("body".to_string(), ident("__body")),
                ("name".to_string(), str_lit(prop.name.clone())),
                ("location".to_string(), str_lit(rel_path_fwd.clone())),
                ("file".to_string(), str_lit(rel_path_fwd)),
            ])],
        ))),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&return_run, 2));
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
    runtime_use: &RuntimeUse,
    tys: &Arc<Types>,
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
    //
    // P7.2: deferred, not narrowed. `v`'s real type varies per binding, and
    // for `Int` (and refined-`Int`) bindings the *internal* property-test
    // representation is `bigint` (see `rng.int(...)`'s own `n`-suffixed
    // literals and `__bynkShrinkInt(v: bigint, ...)` above) — not `number`,
    // which is what a general `ts_type_ref`/`ts_ty` rendering of the same
    // declared type would produce. Narrowing correctly needs threading the
    // resolved type (or its internal representation) out of `BindingGen`
    // itself, not a same-line text change, and a wrong guess here risks a
    // real `tsc --strict` failure specifically on the shrink helpers' own
    // typed parameters.
    let gens: Vec<TsExpr> = f
        .params
        .iter()
        .map(|p| {
            let bg = checker::resolve_type_ref(&p.type_ref, &resolved.types, tys)
                .map(|t| binding_gen(t, &resolved.types, tys))
                .unwrap_or(BindingGen {
                    boundaries: Vec::new(),
                    gen_ts: ident("undefined"),
                    shrink: TsExpr::array(vec![]),
                });
            gen_descriptor_entry(Some(str_lit(p.name.name.clone())), &bg)
        })
        .collect();
    let gens_stmt = TsStmt::const_stmt(
        TsBindingName::Ident("__gens".to_string()),
        None,
        TsExpr::multiline_array(gens),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&gens_stmt, 2));

    let param_names: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
    let param_types: Vec<Option<checker::TyId>> = f
        .params
        .iter()
        .map(|p| checker::resolve_type_ref(&p.type_ref, &resolved.types, tys))
        .collect();
    // #1426: the same top-level Int-binding representation gap `emit_test_
    // property_function`'s own `for all` bindings have — an attacked
    // function's own fuzzed `Int` parameter draws `bigint` exactly the same
    // way, and its `requires`/body can do ordinary arithmetic on it.
    let destructure = destructure_vals(&param_names, &param_types, Some(&resolved.types), tys);

    // `__where` — the conjunction of `requires`, lowered over the parameter
    // tuple. #1426: `destructure` above already coerces every `Int`-drawing
    // param to `number` before this closure's own body runs, so a `requires`
    // clause doing real arithmetic (not just comparison) on a param is safe
    // too — comparisons alone tolerated the pre-fix `bigint`/`number` split,
    // but arithmetic never did.
    let where_pred = f.requires.iter().rev().fold(None, |acc: Option<Expr>, c| {
        Some(match acc {
            None => c.predicate.clone(),
            Some(rest) => Expr {
                id: ExprId::SYNTHETIC,
                kind: ExprKind::BinOp(BinOp::And, Box::new(c.predicate.clone()), Box::new(rest)),
                span: f.span,
            },
        })
    });
    if let Some(w) = where_pred {
        let mut typed =
            synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses, tys);
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
            runtime_use,
        );
        // P7.2: deferred — `__vals`'s elements are heterogeneous, one per binding,
        // some internally `bigint`-represented; same reason as `__gens`'s own
        // construction elsewhere in this function.
        out.push_str("    const __where = (__vals: any[]) => {\n");
        out.push_str(&format!("      {destructure}\n"));
        for line in src.lines() {
            out.push_str("      ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    };\n");
    } else {
        let where_null = TsStmt::const_stmt(
            TsBindingName::Ident("__where".to_string()),
            None,
            TsExpr::Lit(TsLit::Null),
            None,
        );
        out.push_str(&bynk_ts::print_stmt(&where_null, 2));
    }

    // `__body` — call the (guarded) function. #1426: every param is already
    // coerced to `number` by `destructure_vals` at the bind site above (an
    // `Int`-drawing local is never left `bigint`), so the call passes each
    // param straight through — no separate per-argument coercion needed here
    // any more (this used to `Number(…)`-wrap `Int` args itself; redundant
    // now that the bind site handles it, and would double-wrap). The guard
    // asserts the `ensures`.
    let call_args: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
    // P7.2: deferred, same reason as `__where`'s own `__vals` above.
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
    let return_run = TsStmt::return_stmt(
        Some(await_expr(call(
            ident("__bynkRunProperty"),
            vec![TsExpr::object(vec![
                (
                    "seed".to_string(),
                    call(
                        ident("__bynkMix"),
                        vec![ident("__bynkSeed"), num_lit(prop_ordinal.to_string())],
                    ),
                ),
                ("cases".to_string(), num_lit("100")),
                ("gens".to_string(), ident("__gens")),
                ("where".to_string(), ident("__where")),
                ("body".to_string(), ident("__body")),
                ("name".to_string(), str_lit(name)),
                ("location".to_string(), str_lit(rel_path_fwd.clone())),
                ("file".to_string(), str_lit(rel_path_fwd)),
            ])],
        ))),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&return_run, 2));
    out.push_str("}\n");
    out
}

// -- Small tree-construction helpers (#1325) ------------------------------
//
// Mirrors `workers.rs`'s/`workers_entry.rs`'s own local helper sets (#1321/
// #1323) — this file's own private set, not shared, matching this track's
// own established per-file scoping.

fn ident(s: impl Into<String>) -> TsExpr {
    TsExpr::Ident(s.into())
}

fn str_lit(s: impl Into<String>) -> TsExpr {
    TsExpr::Lit(TsLit::Str(s.into()))
}

fn num_lit(n: impl Into<String>) -> TsExpr {
    TsExpr::Lit(TsLit::Num(n.into()))
}

fn member(object: TsExpr, property: impl Into<String>) -> TsExpr {
    TsExpr::Member {
        object: Box::new(object),
        property: property.into(),
    }
}

fn call(callee: TsExpr, args: Vec<TsExpr>) -> TsExpr {
    TsExpr::Call {
        callee: Box::new(callee),
        args,
    }
}

fn method_call(object: TsExpr, method: &str, args: Vec<TsExpr>) -> TsExpr {
    call(member(object, method), args)
}

fn await_expr(expr: TsExpr) -> TsExpr {
    TsExpr::Await(Box::new(expr))
}

/// A zero-substitution template literal — `` `<text>` `` with `text` already
/// fully assembled (any `${...}` it contains is baked-in literal text, not a
/// real substitution `bynk_ts` tracks). Slice F (#1407): `emit_system_http_
/// support`'s own `concrete_path` (every `:name` path segment already
/// resolved to `${paramIdent}` text at Rust-`format!` time, per its own doc)
/// is the real site. Routes through the validated `TsExpr::template_lit`
/// constructor rather than the bare `TsExpr::TemplateLit { .. }` variant, the
/// same guard this file's own not-yet-refactored call sites already use
/// (review of #1408, finding 1).
fn template(text: impl Into<String>) -> TsExpr {
    TsExpr::template_lit(vec![text.into()], vec![])
}

/// One `async function __sysdrive_*` driver: an optional lead statement
/// (`const __body = JSON.stringify(...)`, typed/no-auth drivers only), `const
/// __h = makeHarness();`, `const __req = new Request(<url>, <options>);`,
/// `const __res = await __h.env.<binding>.fetch(__req);`, then `return
/// <decode_fn>(__res, <payload>);`. Slice F (#1407): the one real shape
/// shared by all four per-route `__sysdrive_{,raw_,noauth_,rawnoauth_}*`
/// drivers in `emit_system_http_support`, differing only in name (built here
/// from `kind_prefix`/`sname`/`key`, one shared spot rather than one
/// `format!` call per driver kind), params, the optional body statement, the
/// options object (Decision A, see its own call sites), and the decode
/// function — every real difference is a parameter here, not a reason to
/// hand-write four near-identical bodies.
#[allow(clippy::too_many_arguments)]
fn sysdrive_driver(
    kind_prefix: &str,
    sname: &str,
    key: &str,
    mut params: Vec<TsParam>,
    body_stmt: Option<TsStmt>,
    url: TsExpr,
    options: TsExpr,
    binding: &str,
    decode_fn: &str,
    payload: &str,
) -> TsStmt {
    params.push(TsParam {
        name: "__sub".to_string(),
        ty: Some(TsType::named("string")),
        optional: false,
    });
    let mut body = Vec::new();
    if let Some(stmt) = body_stmt {
        body.push(stmt);
    }
    body.push(TsStmt::const_stmt(
        TsBindingName::Ident("__h".to_string()),
        None,
        call(ident("makeHarness"), vec![]),
        None,
    ));
    body.push(TsStmt::const_stmt(
        TsBindingName::Ident("__req".to_string()),
        None,
        TsExpr::New {
            callee: Box::new(ident("Request")),
            args: vec![url, options],
        },
        None,
    ));
    body.push(TsStmt::const_stmt(
        TsBindingName::Ident("__res".to_string()),
        None,
        await_expr(method_call(
            member(member(ident("__h"), "env"), binding),
            "fetch",
            vec![ident("__req")],
        )),
        None,
    ));
    body.push(TsStmt::return_stmt(
        Some(call(ident(decode_fn), vec![ident("__res"), ident(payload)])),
        None,
    ));
    TsStmt::decl(
        TsDecl::Function {
            name: format!("__sysdrive_{kind_prefix}{sname}_{key}"),
            generics: Vec::new(),
            params,
            return_type: None,
            body,
            is_async: true,
            inline: false,
        },
        None,
    )
}

fn and_expr(left: TsExpr, right: TsExpr) -> TsExpr {
    TsExpr::Binary {
        op: bynk_ts::TsBinaryOp::And,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn gt(left: TsExpr, right: TsExpr) -> TsExpr {
    TsExpr::Binary {
        op: bynk_ts::TsBinaryOp::GreaterThan,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn strict_eq(left: TsExpr, right: TsExpr) -> TsExpr {
    TsExpr::Binary {
        op: bynk_ts::TsBinaryOp::StrictEq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn or_expr(left: TsExpr, right: TsExpr) -> TsExpr {
    TsExpr::Binary {
        op: bynk_ts::TsBinaryOp::Or,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `if (want("{name}")) results.push({ name: "{name}", ...(await
/// {runner_name}()) });` — one dispatch line of a module's own `run(only)`
/// runner. Slice G (#1409): the identical shape `emit_integration_module`'s
/// own per-case loop and `emit_test_module`'s own per-case/per-property/
/// per-attack loops all build, factored into one shared helper rather than
/// three near-identical call sites. `name` is the raw, decoded case/property
/// name — `str_lit` routes it through `TsLit::Str`'s own printer escaping,
/// the same convention this file's own other `str_lit(prop.name.clone())`
/// sites already use; review of #1410, finding 1: the first draft passed an
/// already-`emitter::escape_ts_string`-escaped name, which `TsLit::Str`'s
/// own renderer (deliberately byte-identical to `escape_ts_string`, per its
/// own doc) then escaped a second time, corrupting any name containing a
/// quote/backslash/tab/newline — under this algebra, the printer alone owns
/// escaping. The `...(await …())` spread wraps its
/// `Await` in an explicit `TsExpr::Paren` — `TsObjectEntry::Spread` renders
/// its inner expression via the depth-unaware `render_expr` (no
/// `render_operand` parenthesisation, since `Await` isn't in
/// `needs_parens_as_operand`'s own set) — to match the existing hand-written
/// text's own explicit parens byte-for-byte.
fn run_dispatch_stmt(name: &str, runner_name: &str) -> TsStmt {
    TsStmt::if_stmt(
        call(ident("want"), vec![str_lit(name.to_string())]),
        expr_stmt(method_call(
            ident("results"),
            "push",
            vec![TsExpr::object_entries(vec![
                TsObjectEntry::Prop("name".to_string(), str_lit(name.to_string())),
                TsObjectEntry::Spread(TsExpr::Paren(Box::new(await_expr(call(
                    ident(runner_name),
                    vec![],
                ))))),
            ])],
        )),
        None,
    )
}

/// `export async function run(only?: string) { const results = []; const
/// want = (n: string): boolean => only === undefined || only === n;
/// <dispatch_stmts> return results; }` — a module's own runner, built from
/// its own real per-case/per-property/per-attack [`run_dispatch_stmt`]
/// sequence. Slice G (#1409): carries no opaque lowered content and no
/// source-map splice sensitivity (unlike the per-case bodies each dispatch
/// line calls out to), so it converts fully, no carve-out at all — shared by
/// both `emit_integration_module` and `emit_test_module`, which build the
/// identical shape over different case/property/attack lists.
fn build_run_function(dispatch_stmts: Vec<TsStmt>) -> TsStmt {
    let mut body = vec![
        const_("results", TsExpr::array(vec![])),
        const_(
            "want",
            TsExpr::Arrow {
                params: vec![TsParam {
                    name: "n".to_string(),
                    ty: Some(TsType::named("string")),
                    optional: false,
                }],
                is_async: false,
                generics: Vec::new(),
                return_type: Some(TsType::named("boolean")),
                body: Box::new(TsArrowBody::Expr(Box::new(or_expr(
                    strict_eq(ident("only"), ident("undefined")),
                    strict_eq(ident("only"), ident("n")),
                )))),
            },
        ),
    ];
    body.extend(dispatch_stmts);
    body.push(TsStmt::return_stmt(Some(ident("results")), None));
    TsStmt::decl(
        TsDecl::Export(Box::new(TsDecl::Function {
            name: "run".to_string(),
            generics: Vec::new(),
            params: vec![TsParam {
                name: "only".to_string(),
                ty: Some(TsType::named("string")),
                optional: true,
            }],
            return_type: None,
            body,
            is_async: true,
            inline: false,
        })),
        None,
    )
}

/// `test ? consequent : alternate`.
fn cond_expr(test: TsExpr, consequent: TsExpr, alternate: TsExpr) -> TsExpr {
    TsExpr::Conditional {
        test: Box::new(test),
        consequent: Box::new(consequent),
        alternate: Box::new(alternate),
    }
}

fn console_log(arg: TsExpr) -> TsStmt {
    expr_stmt(method_call(ident("console"), "log", vec![arg]))
}

fn const_(name: impl Into<String>, init: TsExpr) -> TsStmt {
    TsStmt::const_stmt(TsBindingName::Ident(name.into()), None, init, None)
}

fn expr_stmt(expr: TsExpr) -> TsStmt {
    TsStmt::expr_stmt(expr, None)
}

fn block(stmts: Vec<TsStmt>) -> TsStmt {
    TsStmt::block(stmts, None)
}

fn if_(cond: TsExpr, then_branch: TsStmt) -> TsStmt {
    TsStmt::if_stmt(cond, then_branch, None)
}

pub(crate) fn emit_test_main(tests: &[RunnableTest], import_ext: ImportExt) -> TsProgram {
    let ext = import_ext.as_str();
    let mut program = TsProgram::new();
    program.push(TsStmt::comment(
        "Generated by bynkc — do not edit by hand.",
        None,
    ));
    program.push(TsStmt::comment("top-level test runner", None));
    // Node's `process` global isn't declared without @types/node. The runner
    // uses `process.exit` and reads `process.env.BYNK_TEST_FORMAT` (v0.59: set
    // to `ndjson` by `bynkc test --format json`), so we narrow the global with a
    // minimal ambient declaration rather than pulling in a dependency.
    program.push(TsStmt::decl(
        TsDecl::DeclareConst {
            name: "process".to_string(),
            ty: TsType::Object(vec![
                TsTypeMember::method(
                    "exit",
                    vec![TsParam {
                        name: "code".to_string(),
                        ty: Some(TsType::named("number")),
                        optional: false,
                    }],
                    TsType::named("never"),
                ),
                TsTypeMember::prop(
                    "env",
                    TsType::Object(vec![TsTypeMember::index(
                        "k",
                        TsType::named("string"),
                        TsType::union(vec![TsType::named("string"), TsType::named("undefined")]),
                    )]),
                ),
            ]),
        },
        None,
    ));
    let mut sorted: Vec<&RunnableTest> = tests.iter().collect();
    sorted.sort_by(|a, b| a.target_name.cmp(&b.target_name));
    for (i, t) in sorted.iter().enumerate() {
        let module_stem = t
            .module_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("test");
        program.push(TsStmt::decl(
            TsDecl::ImportNamespace {
                type_only: false,
                alias: format!("test_{i}"),
                from: format!("./{module_stem}.{ext}"),
            },
            None,
        ));
    }

    // v0.127 (editor-currency slice 6): `bynkc test --case <name>` sets
    // BYNK_TEST_CASE, threaded here as an `only` filter into every suite's
    // `run(only)`. Unset runs the whole project unchanged.
    let mut main_body: Vec<TsStmt> = vec![const_(
        "only",
        member(member(ident("process"), "env"), "BYNK_TEST_CASE"),
    )];
    let modules_items: Vec<TsExpr> = sorted
        .iter()
        .enumerate()
        .map(|(i, t)| {
            TsExpr::object(vec![
                ("name".to_string(), str_lit(t.target_name.clone())),
                ("run".to_string(), member(ident(format!("test_{i}")), "run")),
            ])
        })
        .collect();
    main_body.push(const_("modules", TsExpr::multiline_array(modules_items)));
    main_body.push(TsStmt::let_stmt(
        TsBindingName::Ident("passed".to_string()),
        None,
        Some(num_lit("0")),
        None,
    ));
    main_body.push(TsStmt::let_stmt(
        TsBindingName::Ident("failed".to_string()),
        None,
        Some(num_lit("0")),
        None,
    ));
    // v0.59: `--format json` sets BYNK_TEST_FORMAT=ndjson and captures stdout;
    // the runner then emits one JSON event per line (an internal protocol the
    // driver re-renders into the pinned document). Otherwise the human ✓ / ✗
    // output is byte-for-byte unchanged.
    //
    // `TsLit::Raw`, not `TsLit::Str` — see its own doc: the literal already
    // spells the JS unicode escape as six ASCII characters, which `Str`'s own
    // escaper would double the backslash of.
    main_body.push(const_(
        "PREFIX",
        TsExpr::Lit(TsLit::Raw("\"integration \\u00b7 \"".to_string())),
    ));

    let emit_call = |entries: Vec<TsObjectEntry>| {
        expr_stmt(call(ident("emit"), vec![TsExpr::object_entries(entries)]))
    };

    let ndjson_branch = block(vec![
        const_(
            "emit",
            TsExpr::Arrow {
                params: vec![TsParam {
                    name: "o".to_string(),
                    ty: Some(TsType::named("unknown")),
                    optional: false,
                }],
                is_async: false,
                generics: Vec::new(),
                return_type: None,
                body: Box::new(TsArrowBody::Expr(Box::new(method_call(
                    ident("console"),
                    "log",
                    vec![method_call(ident("JSON"), "stringify", vec![ident("o")])],
                )))),
            },
        ),
        emit_call(vec![
            TsObjectEntry::Prop("type".to_string(), str_lit("run-begin")),
            TsObjectEntry::Prop("suites".to_string(), member(ident("modules"), "length")),
        ]),
        TsStmt::for_of(
            "m",
            ident("modules"),
            block(vec![
                const_(
                    "integration",
                    method_call(
                        member(ident("m"), "name"),
                        "startsWith",
                        vec![ident("PREFIX")],
                    ),
                ),
                const_(
                    "suite",
                    cond_expr(
                        ident("integration"),
                        method_call(
                            member(ident("m"), "name"),
                            "slice",
                            vec![member(ident("PREFIX"), "length")],
                        ),
                        member(ident("m"), "name"),
                    ),
                ),
                const_(
                    "kind",
                    cond_expr(
                        ident("integration"),
                        str_lit("integration"),
                        str_lit("unit"),
                    ),
                ),
                const_(
                    "results",
                    await_expr(method_call(ident("m"), "run", vec![ident("only")])),
                ),
                emit_call(vec![
                    TsObjectEntry::Prop("type".to_string(), str_lit("suite-begin")),
                    TsObjectEntry::Prop("name".to_string(), ident("suite")),
                    TsObjectEntry::Shorthand("kind".to_string()),
                    TsObjectEntry::Prop("tests".to_string(), member(ident("results"), "length")),
                ]),
                TsStmt::for_of(
                    "r",
                    ident("results"),
                    block(vec![TsStmt::if_else_same_line_stmt(
                        member(ident("r"), "pass"),
                        block(vec![
                            TsStmt::increment(ident("passed"), None),
                            emit_call(vec![
                                TsObjectEntry::Prop("type".to_string(), str_lit("case")),
                                TsObjectEntry::Shorthand("suite".to_string()),
                                TsObjectEntry::Prop("name".to_string(), member(ident("r"), "name")),
                                TsObjectEntry::Prop("outcome".to_string(), str_lit("pass")),
                            ]),
                        ]),
                        block(vec![
                            TsStmt::increment(ident("failed"), None),
                            emit_call(vec![
                                TsObjectEntry::Prop("type".to_string(), str_lit("case")),
                                TsObjectEntry::Shorthand("suite".to_string()),
                                TsObjectEntry::Prop("name".to_string(), member(ident("r"), "name")),
                                TsObjectEntry::Prop("outcome".to_string(), str_lit("fail")),
                                TsObjectEntry::Prop(
                                    "message".to_string(),
                                    and_expr(
                                        member(ident("r"), "error"),
                                        member(member(ident("r"), "error"), "message"),
                                    ),
                                ),
                                TsObjectEntry::Prop(
                                    "location".to_string(),
                                    and_expr(
                                        member(ident("r"), "error"),
                                        member(member(ident("r"), "error"), "location"),
                                    ),
                                ),
                            ]),
                        ]),
                        None,
                    )]),
                    None,
                ),
                emit_call(vec![
                    TsObjectEntry::Prop("type".to_string(), str_lit("suite-end")),
                    TsObjectEntry::Prop("name".to_string(), ident("suite")),
                ]),
            ]),
            None,
        ),
        emit_call(vec![
            TsObjectEntry::Prop("type".to_string(), str_lit("run-end")),
            TsObjectEntry::Shorthand("passed".to_string()),
            TsObjectEntry::Shorthand("failed".to_string()),
        ]),
    ]);

    let human_branch = block(vec![
        console_log(str_lit("Running tests...\n")),
        TsStmt::for_of(
            "m",
            ident("modules"),
            block(vec![
                console_log(TsExpr::template_lit(
                    vec![String::new(), ":".to_string()],
                    vec![member(ident("m"), "name")],
                )),
                const_(
                    "results",
                    await_expr(method_call(ident("m"), "run", vec![ident("only")])),
                ),
                TsStmt::for_of(
                    "r",
                    ident("results"),
                    block(vec![TsStmt::if_else_same_line_stmt(
                        member(ident("r"), "pass"),
                        TsStmt::inline_block(
                            vec![
                                TsStmt::increment(ident("passed"), None),
                                console_log(TsExpr::template_lit(
                                    vec!["  \\u2713 ".to_string(), String::new()],
                                    vec![member(ident("r"), "name")],
                                )),
                            ],
                            None,
                        ),
                        TsStmt::inline_block(
                            vec![
                                TsStmt::increment(ident("failed"), None),
                                console_log(TsExpr::template_lit(
                                    vec!["  \\u2717 ".to_string(), String::new()],
                                    vec![member(ident("r"), "name")],
                                )),
                                if_(
                                    member(ident("r"), "error"),
                                    console_log(TsExpr::template_lit(
                                        vec!["    ".to_string(), String::new()],
                                        vec![member(member(ident("r"), "error"), "message")],
                                    )),
                                ),
                            ],
                            None,
                        ),
                        None,
                    )]),
                    None,
                ),
                console_log(str_lit("")),
            ]),
            None,
        ),
        console_log(TsExpr::template_lit(
            vec![
                String::new(),
                " passed, ".to_string(),
                " failed.".to_string(),
            ],
            vec![ident("passed"), ident("failed")],
        )),
    ]);

    main_body.push(TsStmt::if_else_same_line_stmt(
        strict_eq(
            member(member(ident("process"), "env"), "BYNK_TEST_FORMAT"),
            str_lit("ndjson"),
        ),
        ndjson_branch,
        human_branch,
        None,
    ));
    main_body.push(if_(
        gt(ident("failed"), num_lit("0")),
        expr_stmt(call(member(ident("process"), "exit"), vec![num_lit("1")])),
    ));

    program.push(TsStmt::decl(
        TsDecl::Function {
            name: "main".to_string(),
            generics: Vec::new(),
            params: vec![],
            return_type: None,
            body: main_body,
            is_async: true,
            inline: false,
        },
        None,
    ));
    program.push(expr_stmt(call(ident("main"), vec![])));
    program
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

    /// Review of #1410, finding 1: `run_dispatch_stmt` must escape a raw
    /// case/property name exactly once. Pins a name containing both a
    /// double quote and a backslash — the exact shape a double-escape
    /// (passing an already-`emitter::escape_ts_string`-escaped name into
    /// `str_lit`, which escapes again) would corrupt, and which no `.bynk`
    /// fixture exercises today.
    #[test]
    fn run_dispatch_stmt_escapes_a_quoted_backslashed_name_exactly_once() {
        let stmt = run_dispatch_stmt(r#"say "hi" \ ok"#, "test_a");
        let printed = bynk_ts::print_stmt(&stmt, 0);
        assert_eq!(
            printed,
            "if (want(\"say \\\"hi\\\" \\\\ ok\")) results.push({ name: \"say \\\"hi\\\" \\\\ ok\", ...(await test_a()) });\n"
        );
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
        let program = emit_test_main(&tests, ImportExt::Js);
        let out = bynk_ts::print(&program, "", "", "").text;
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

    /// #1325's own direct fixture test — `emit_test_main`'s signature
    /// (`&[RunnableTest]`/`ImportExt`) is simple enough to hand-construct
    /// directly, unlike `emit_worker_compose`'s/`emit_worker_entry`'s own
    /// whole-project `UnitTable`-derived data (#1321/#1323's own reason for
    /// relying on the full fixture corpus instead). Calls the real,
    /// converted function with real-shaped data and asserts the printed
    /// output is byte-identical to a real fixture's own `tests/main.ts`, not
    /// a hand-rebuilt approximation of it.
    #[test]
    fn matches_the_real_fixtures_own_tests_main_ts_byte_for_byte() {
        let expected = include_str!(
            "../../../bynkc/tests/fixtures/positive/109_test_with_opaque_construction/\
             expected/tests/main.ts"
        );
        let tests = vec![RunnableTest {
            target_name: "commerce.payment".to_string(),
            module_path: std::path::PathBuf::from("commerce_payment.test.js"),
            kind: "unit",
            suite_name: "commerce.payment".to_string(),
            cases: vec![DiscoveredCase {
                name: "opaque".to_string(),
                location: None,
            }],
        }];
        let program = emit_test_main(&tests, ImportExt::Js);
        let printed = bynk_ts::print(&program, "", "", "");
        assert_eq!(printed.text, expected);
    }

    /// #1329's own direct fixture test — `emit_commons_barrel`'s inputs
    /// (`groups`/`parsed`/`import_ext`/`emitted`) are hand-constructible via
    /// `ParsedFile::new`'s own documented "general constructor — `bynk-emit`'s
    /// own tests use this" purpose, unlike `emit_worker_compose`'s/
    /// `emit_worker_entry`'s/`emit_composition_root`'s own whole-project
    /// `UnitTable`-derived data (#1321/#1323/#1327's own reason for relying
    /// on the full fixture corpus instead). Calls the real, converted
    /// function with real-shaped data and asserts the printed output is
    /// byte-identical to `251_multi_file_commons_test`'s own real
    /// `expected/thing.ts`, not a hand-rebuilt approximation of it.
    #[test]
    fn matches_the_real_fixtures_own_commons_barrel_ts_byte_for_byte() {
        use bynk_syntax::ast::SourceUnit;

        let expected = include_str!(
            "../../../bynkc/tests/fixtures/positive/251_multi_file_commons_test/\
             expected/thing.ts"
        );
        let commons = |name: &str| {
            SourceUnit::Commons(Commons {
                name: QualifiedName {
                    parts: vec![Ident {
                        name: name.to_string(),
                        span: Span::new(0, 0),
                    }],
                    span: Span::new(0, 0),
                },
                items: vec![],
                uses: vec![],
                documentation: None,
                form: CommonsForm::Fragment,
                span: Span::new(0, 0),
                trivia: Trivia {
                    leading: vec![],
                    trailing: None,
                },
                trailing_comments: vec![],
            })
        };
        let parsed = vec![
            ParsedFile::new(
                PathBuf::from("thing/make.bynk"),
                PathBuf::from("thing/make.bynk"),
                None,
                String::new(),
                commons("thing"),
                UnitKind::Commons,
                false,
            ),
            ParsedFile::new(
                PathBuf::from("thing/widget.bynk"),
                PathBuf::from("thing/widget.bynk"),
                None,
                String::new(),
                commons("thing"),
                UnitKind::Commons,
                false,
            ),
        ];
        let mut groups = BTreeMap::new();
        groups.insert("thing".to_string(), vec![0, 1]);
        let mut emitted = HashSet::new();
        let staged = emit_commons_barrel("thing", &groups, &parsed, ImportExt::Js, &mut emitted)
            .expect("a multi-file commons must produce a barrel");
        let Document::Ts(program) = staged.document else {
            panic!("emit_commons_barrel must build a TsProgram");
        };
        let printed = bynk_ts::print(&program, "", "", "");
        assert_eq!(printed.text, expected);
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

    // -- coerce_int_field (#1398, review of #1428) ---------------------------

    fn refined_int_decl(name: &str) -> TypeDecl {
        TypeDecl {
            name: Ident {
                name: name.to_string(),
                span: Span::default(),
            },
            type_params: Vec::new(),
            body: TypeBody::Refined {
                base: BaseType::Int,
                base_span: Span::default(),
                refinement: None,
            },
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        }
    }

    fn opaque_int_decl(name: &str) -> TypeDecl {
        TypeDecl {
            name: Ident {
                name: name.to_string(),
                span: Span::default(),
            },
            type_params: Vec::new(),
            body: TypeBody::Opaque {
                base: BaseType::Int,
                base_span: Span::default(),
                refinement: None,
            },
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        }
    }

    #[test]
    fn coerce_int_field_wraps_a_bare_int_draw_in_number() {
        let tys = Arc::new(Types::new());
        let int_ty = tys.intern(checker::Ty::Base(BaseType::Int));
        let types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        let draw = ident("rng.int(-1000n, 1000n)");

        let coerced = coerce_int_field(int_ty, &types, &tys, draw);
        assert_eq!(
            bynk_ts::print_expr(&coerced),
            "Number(rng.int(-1000n, 1000n))"
        );
    }

    #[test]
    fn coerce_int_field_wraps_a_refined_int_field_too() {
        // Review of #1428: `refined_gen_ts`'s own `(value as any)` cast is
        // compile-time only — the runtime value underneath is still a real
        // bigint, so a refined-Int payload/field needs the same coercion a
        // bare Int does, or the exact `TypeError: Cannot mix BigInt and
        // other types` #1398 names still reproduces one type-alias away.
        let tys = Arc::new(Types::new());
        let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        types.insert("Pct".to_string(), Arc::new(refined_int_decl("Pct")));
        let pct_ty = tys.intern(checker::Ty::Named {
            name: "Pct".to_string(),
            kind: checker::NamedKind::Refined(BaseType::Int),
            args: Vec::new(),
        });
        let draw = ident("(rng.int(0n, 100n) as any)");

        let coerced = coerce_int_field(pct_ty, &types, &tys, draw);
        assert_eq!(
            bynk_ts::print_expr(&coerced),
            "Number((rng.int(0n, 100n) as any))"
        );
    }

    #[test]
    fn coerce_int_field_wraps_an_opaque_int_field_too() {
        let tys = Arc::new(Types::new());
        let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        types.insert("Pct".to_string(), Arc::new(opaque_int_decl("Pct")));
        let pct_ty = tys.intern(checker::Ty::Named {
            name: "Pct".to_string(),
            kind: checker::NamedKind::Opaque(BaseType::Int),
            args: Vec::new(),
        });
        let draw = ident("Pct.unsafe(rng.int(0n, 100n))");

        let coerced = coerce_int_field(pct_ty, &types, &tys, draw);
        assert_eq!(
            bynk_ts::print_expr(&coerced),
            "Number(Pct.unsafe(rng.int(0n, 100n)))"
        );
    }

    #[test]
    fn coerce_int_field_passes_the_undefined_sentinel_through_unwrapped() {
        // Review of #1428: the depth-exhaustion `undefined` sentinel
        // (`canon_ts_for_ty`'s own `depth == 0` early return) must not
        // become `Number(undefined)` — a silent `NaN` where the unwrapped
        // sentinel at least fails loudly (a separate, pre-existing gap
        // tracked as #1429, not fixed here).
        let tys = Arc::new(Types::new());
        let int_ty = tys.intern(checker::Ty::Base(BaseType::Int));
        let types: HashMap<String, Arc<TypeDecl>> = HashMap::new();

        let coerced = coerce_int_field(int_ty, &types, &tys, ident("undefined"));
        assert_eq!(bynk_ts::print_expr(&coerced), "undefined");
    }

    #[test]
    fn coerce_int_field_leaves_a_non_int_value_untouched() {
        let tys = Arc::new(Types::new());
        let string_ty = tys.intern(checker::Ty::Base(BaseType::String));
        let types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        let draw = ident("rng.str(0, 8)");

        let coerced = coerce_int_field(string_ty, &types, &tys, draw);
        assert_eq!(bynk_ts::print_expr(&coerced), "rng.str(0, 8)");
    }

    // -- gen_ts_for_ty's depth-exhaustion fallback (#1429, review of #1434) --

    fn sum_decl(name: &str, variants: Vec<(&str, Vec<(&str, TypeRef)>)>) -> TypeDecl {
        TypeDecl {
            name: Ident {
                name: name.to_string(),
                span: Span::default(),
            },
            type_params: Vec::new(),
            body: TypeBody::Sum(bynk_syntax::ast::SumBody {
                variants: variants
                    .into_iter()
                    .map(|(vname, fields)| bynk_syntax::ast::Variant {
                        name: Ident {
                            name: vname.to_string(),
                            span: Span::default(),
                        },
                        payload: fields
                            .into_iter()
                            .map(|(fname, type_ref)| bynk_syntax::ast::VariantField {
                                name: Ident {
                                    name: fname.to_string(),
                                    span: Span::default(),
                                },
                                type_ref,
                                span: Span::default(),
                            })
                            .collect(),
                        span: Span::default(),
                    })
                    .collect(),
                embeds: Vec::new(),
                span: Span::default(),
            }),
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        }
    }

    #[test]
    fn gen_ts_for_ty_bottoms_out_a_first_variant_recursive_sum_the_checker_validated() {
        // #1429's own repro: `T = Base(n: Int) | Cons(tail: T)`. The checker
        // accepts `T` as a `for all` binding (`prop_binding_generable` only
        // needs the first variant, `Base`, to be generable) — the fallback
        // must be able to actually build a `Base(n: Int)` once generation
        // depth is exhausted deep inside a `Cons` chain, not `undefined`.
        let tys = Arc::new(Types::new());
        let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        types.insert(
            "T".to_string(),
            Arc::new(sum_decl(
                "T",
                vec![
                    ("Base", vec![("n", base(BaseType::Int))]),
                    ("Cons", vec![("tail", named("T"))]),
                ],
            )),
        );
        let t_ty = tys.intern(checker::Ty::Named {
            name: "T".to_string(),
            kind: checker::NamedKind::Sum,
            args: Vec::new(),
        });

        // depth: 0 forces the fallback immediately, the same state
        // `gen_ts_for_ty`'s own recursion reaches once its budget runs out.
        let generated = gen_ts_for_ty(t_ty, &types, 0, &tys);
        let printed = bynk_ts::print_expr(&generated);
        assert!(
            !printed.contains("undefined"),
            "a checker-validated recursive sum must not fall back to `undefined`, got: {printed}"
        );
        assert!(
            printed.contains("T.Base("),
            "expected the fallback to build T's own first (validated) variant, got: {printed}"
        );
    }

    #[test]
    fn gen_ts_for_ty_stays_bounded_for_a_branching_variant_the_checker_never_validated() {
        // Review of #1434, finding 1: `prop_binding_generable` only ever
        // descends a sum's *first* variant, but `gen_ts_for_ty`'s own Sum
        // branch builds a thunk for *every* variant — so a non-first
        // variant's field type can reach the depth-0 fallback fully
        // unvalidated. `Bad`'s own first variant (`Node`) is branching and
        // never bottoms out on its own; handing the fallback the full
        // `PROP_GEN_DEPTH` budget for `Bad` (not just `T`, the type the
        // checker actually validated) would walk a full binary tree before
        // ever reaching `Leaf`'s escape — confirmed to reproduce a real
        // ~176MB, 50+ second emit for this exact shape before the guard
        // below. The fallback must recognise `Bad` itself was never
        // validated and fall back to the original, small budget for it.
        let tys = Arc::new(Types::new());
        let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        types.insert(
            "Bad".to_string(),
            Arc::new(sum_decl(
                "Bad",
                vec![
                    ("Node", vec![("l", named("Bad")), ("r", named("Bad"))]),
                    ("Leaf", vec![("n", base(BaseType::Int))]),
                ],
            )),
        );
        types.insert(
            "T".to_string(),
            Arc::new(sum_decl(
                "T",
                vec![
                    ("Plain", vec![("n", base(BaseType::Int))]),
                    ("Nope", vec![("b", named("Bad"))]),
                ],
            )),
        );
        let t_ty = tys.intern(checker::Ty::Named {
            name: "T".to_string(),
            kind: checker::NamedKind::Sum,
            args: Vec::new(),
        });

        // depth: 1 reaches `Nope`'s own `b: Bad` field at depth 0 — `T`
        // itself is still checker-validated (via `Plain`), so this isn't
        // testing the depth-0-at-the-top-level case above, only what
        // happens once the *nested*, unvalidated `Bad` field is reached.
        let generated = gen_ts_for_ty(t_ty, &types, 1, &tys);
        let printed = bynk_ts::print_expr(&generated);
        // A full `PROP_GEN_DEPTH`-deep expansion of `Bad`'s own branching
        // first variant is tens of thousands of characters at minimum
        // (confirmed empirically: ~200KB unguarded vs. ~200 bytes guarded,
        // for the equivalent whole-fixture emit); a generous bound well
        // below that catches a regression without pinning an exact byte
        // count against unrelated formatting changes.
        assert!(
            printed.len() < 2_000,
            "expected a bounded fallback for Bad's own unvalidated branching \
             variant, got {} chars (a regression here likely means the depth-0 \
             fallback stopped checking prop_binding_generable before spending \
             the full PROP_GEN_DEPTH budget): {printed}",
            printed.len()
        );
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
            bynk_ts::print_type(&emitter::ts_type_ref_qualified_ts_type(
                &named("Order"),
                &scope,
                "Ns"
            )),
            "Ns.Order"
        );
        // A named type outside the scope is left bare.
        assert_eq!(
            bynk_ts::print_type(&emitter::ts_type_ref_qualified_ts_type(
                &named("Other"),
                &scope,
                "Ns"
            )),
            "Other"
        );
        // Qualification recurses through generic arguments.
        assert_eq!(
            bynk_ts::print_type(&emitter::ts_type_ref_qualified_ts_type(
                &TypeRef::List(Box::new(named("Order")), Span::default()),
                &scope,
                "Ns"
            )),
            "readonly Ns.Order[]"
        );
        // Base types are unaffected by qualification.
        assert_eq!(
            bynk_ts::print_type(&emitter::ts_type_ref_qualified_ts_type(
                &base(BaseType::Int),
                &scope,
                "Ns"
            )),
            "number"
        );
    }
}
