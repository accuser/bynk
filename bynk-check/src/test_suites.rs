//! Test/integration-suite checking (P5.4,
//! `design/tracks/semantics-in-the-checker.md` §6) — closes category 7 of
//! `bynk-check/src/analysis.rs`'s own residual-gap accounting, the last of
//! the seven. `bynk-emit/src/project/tests_emit.rs` held
//! `process_tests`/`process_integration_tests`, real production code (not
//! fixture noise, despite the filename) checking + emitting `suite`/`test
//! integration` bodies — but it ran only inside `bynk-emit::run_checks`
//! (`Mode::Analyse` included), never inside `bynk_check::analysis::analyse_project`,
//! the entry point the LSP now uses. That gap meant no diagnostics *and* no
//! `RefSink` bindings (go-to-definition/find-references) for anything inside
//! a test file, in the editor — see `analysis.rs`'s own module doc for the
//! full accounting this closes.
//!
//! **What moved here** — every check-only helper `process_tests`/
//! `process_integration_tests` used, plus every function that is genuinely
//! **dual-use**: called both by this module's own [`phase_test_bodies`]/
//! [`phase_integration_bodies`] (the checking half, real diagnostic/`RefSink`
//! sinks) *and* by `bynk-emit`'s TypeScript lowering (throwaway sinks, needed
//! only for the resolved-type view a body's emission depends on). Dual-use
//! functions are `pub`, and `bynk-emit` calls them qualified
//! (`bynk_check::test_suites::foo(...)`) rather than duplicating them — see
//! [`build_privileged_resolved`], [`typecheck_case_body`],
//! [`check_history_binding`], [`register_call_record_types`],
//! [`history_handlers`], [`history_variant_name`], [`prop_binding_generable`]
//! and [`infer_participants`] for which and why (each names its own emit-side
//! call sites). Duplicating a dual-use function instead of relocating it is
//! exactly the drift risk this whole design track exists to close (§9,
//! "Relocating checks risks a quiet R4.6/R4.11 regression").
//!
//! **What stayed in `bynk-emit`** (pure TypeScript emission, or verified
//! emit-only by call-site count): `block_uses_observation`,
//! `target_service_handler_kinds`, `is_attackable_contract`,
//! `numeric_or_scalar_base`, `attackable_contracts`,
//! `json_codec_qual_for_target`, `prop_history_binding`, `prop_is_history`,
//! `SystemCaseInput`, `RunnableTest`, `discovered_location`,
//! `discovery_manifest`, `sanitise_suite`, `emit_integration_module` and its
//! http-driver/harness helpers, and the ~2,600-line TypeScript-codegen tail
//! starting at `emit_test_module` (`emit_stub_class`, `gen_ts_for_ty`,
//! `emit_test_property_function`, `emit_test_history_property_function`, and
//! the rest).
//!
//! `bynk-emit/src/project/tests_emit.rs`'s own `process_tests`/
//! `process_integration_tests` keep their exact signatures (`run_checks`'s
//! callers need no change) — their bodies now call
//! [`phase_test_bodies`]/[`phase_integration_bodies`] for the checking half,
//! then proceed to their existing, unmoved Phase-5 emission logic using the
//! "ready for emission" data these return.
//!
//! `bynk-emit` depends on `bynk-check` (a production dependency, never the
//! reverse), so this move has no circular-dependency subtlety to solve —
//! unlike P5.3's `phase_platform_lock`, which needed a from-scratch pure
//! reimplementation because its old home reached into a `bynk-emit`
//! TypeScript-codegen helper. This is a plain code-motion job, just a large
//! one.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::checker::{self, Types};
use crate::context_checks::{build_capability_op_info, ts_type_ref_display};
use crate::hints::HintSink;
use crate::index::{RefSink, SymbolKind};
use crate::locals::LocalsSink;
use crate::requirements::RequirementSink;
use crate::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use crate::symbols::{UnitTable, build_cross_context_info};
use bynk_project::ParsedFile;
use bynk_project::UnitKind;
use bynk_project::discovery::case_effective_tier;
use bynk_syntax::ast::*;
use bynk_syntax::error::CompileError;
use bynk_syntax::span::Span;

/// v0.118: a capability seam with one or more `stub` overrides applied
/// (testing track slice 6). Groups every `stub Cap.method(…)` clause — both
/// suite-scoped and case-scoped — targeting the same capability `cap`. The
/// resolved [`CapabilityDecl`] supplies each overridden method's parameter names
/// and return type for stub emission.
#[derive(Debug, Clone)]
pub struct ResolvedStub {
    /// The capability being overridden (a declared/consumed seam of the target).
    pub cap: String,
    /// The capability declaration, for op parameter names and return types.
    pub cap_decl: CapabilityDecl,
    /// The `stub` clauses for this capability, in match order (case-scoped
    /// first so they take precedence over suite-scoped in the emitted if-chain).
    pub clauses: Vec<StubClause>,
    /// The test file declaring the first clause — the recording context for
    /// edges in its value expressions (v0.25).
    ///
    /// ADR 0198/0201: a *recording context* is an index key, so this is the
    /// file's **identity** (project-relative), not its `include`-root-relative
    /// unit path. Everything the index keys must name a file the round
    /// analysed.
    pub identity_path: PathBuf,
}

/// P5.4 (`design/tracks/semantics-in-the-checker.md` §6): the checking half
/// of `test <target>` suite processing — target resolution, duplicate-case-
/// name detection, `stub`-clause resolution, and case/property body
/// type-checking. Formerly Phases 2-4 of `bynk-emit`'s own `process_tests`;
/// Phase 5 (TypeScript emission) stays in
/// `bynk-emit::project::tests_emit::process_tests`, which calls this
/// function for its checking half and then emits only for the targets this
/// returns — every target this function resolves, has no duplicate case
/// names, and whose bodies type-check clean is exactly "ready for
/// emission". `bynk_check::analysis::analyse_project` calls this too and
/// discards the returned map — it never emits, so only the diagnostic/
/// `RefSink` side effects matter there. Closes category 7 of
/// `bynk-check/src/analysis.rs`'s own residual-gap accounting, alongside
/// [`phase_integration_bodies`].
#[allow(clippy::too_many_arguments)]
pub fn phase_test_bodies(
    test_groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    tys: &Arc<Types>,
) -> HashMap<String, HashMap<String, ResolvedStub>> {
    let mut ready: HashMap<String, HashMap<String, ResolvedStub>> = HashMap::new();

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
        // case-scoped clause is not yet re-scoped to its own case). Runs
        // unconditionally, even when `had_dup` — its own diagnostics still
        // fire, matching `process_tests`'s original Phase 2/3 ordering.
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
            tys,
        );
        let bodies_failed = !bodies_errs.is_empty();
        errors.extend(bodies_errs);

        if bodies_failed {
            continue;
        }

        ready.insert(target_name.clone(), target_stubs);
    }

    ready
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
                collected.push((pc.clone(), parsed[i].identity_path()));
            }
        }
    }
    for &i in indices {
        let Some(t) = parsed[i].test() else { continue };
        for pc in &t.stubs {
            collected.push((pc.clone(), parsed[i].identity_path()));
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
    for (pc, identity_path) in collected {
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
        let Some(op_decl) = cap_decl.ops.iter().find(|o| o.name.name == pc.method.name) else {
            errors.push(CompileError::new(
                "bynk.stub.unknown_op",
                pc.method.span,
                format!(
                    "`{}` is not an operation of capability `{cap_name}`",
                    pc.method.name
                ),
            ));
            continue;
        };
        // #926 (Decision F): a generic capability operation cannot be stubbed
        // — `__Stub_Cap`'s per-op method body has no way to construct a
        // value of the op's unconstrained `T`. Deferred rather than
        // supported: the stub class carries no `implements` clause (its
        // members are duck-typed through an untyped `deps` seam), so
        // stubbing another, non-generic op of the same capability keeps
        // type-checking.
        if !op_decl.type_params.is_empty() {
            errors.push(
                CompileError::new(
                    "bynk.stub.generic_op",
                    pc.method.span,
                    format!(
                        "`{cap_name}.{}` declares its own type parameter — a generic capability operation cannot be stubbed at v1",
                        pc.method.name
                    ),
                )
                .with_note(
                    "test through the capability's real (external) provider instead, or restructure the test to avoid stubbing this operation",
                ),
            );
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
            identity_path: identity_path.clone(),
        });
        entry.clauses.push(pc);
    }
    out
}

/// v0.118: infer a `system`-tier suite's wired participants — the target's
/// transitive `consumes` closure (testing track slice 6). A BFS from the target
/// following `consumes` edges; the returned list starts with the target and
/// includes every context reachable through it (deterministic breadth order).
pub fn infer_participants(
    target: &str,
    unit_consumes: &HashMap<String, Vec<String>>,
) -> Vec<String> {
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

/// P5.4 (`design/tracks/semantics-in-the-checker.md` §6): the checking half
/// of `test integration "name"` suite processing — participant inference,
/// the `system`-needs-a-serialisation-edge gate, duplicate-case-name
/// detection, the harness-root cross-context view, and per-case body
/// type-checking (including the `Wire`/`by Nobody` tier gates). Formerly the
/// pre-emission logic of `bynk-emit`'s own `process_integration_tests`;
/// emission stays in `bynk-emit::project::tests_emit::process_integration_tests`,
/// which calls this function for its checking half and then emits only for
/// the groups this returns. Unlike [`phase_test_bodies`]'s `ResolvedStub`
/// map, the only thing worth handing back here is the harness's
/// [`resolver::CrossContextInfo`] — it's built from clone-heavy maps
/// (`harness_consumes`/`harness_uses`), so recomputing it a second time on
/// the emit side would be wasted work. `participants`/`uses_targets`/
/// `case_inputs` are cheap and pure (a BFS, a linear scan), so the emit-side
/// loop recomputes those itself from `parsed`/`unit_consumes`, using the
/// now-relocated [`infer_participants`]. `bynk_check::analysis::analyse_project`
/// calls this too and discards the returned map — it never emits. Closes
/// category 7 of `bynk-check/src/analysis.rs`'s own residual-gap accounting,
/// alongside [`phase_test_bodies`].
#[allow(clippy::too_many_arguments)]
pub fn phase_integration_bodies(
    integration_groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    tys: &Arc<Types>,
) -> HashMap<String, resolver::CrossContextInfo> {
    let mut ready: HashMap<String, resolver::CrossContextInfo> = HashMap::new();

    let mut sorted: Vec<&String> = integration_groups.keys().collect();
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
        let participants = infer_participants(&suite_target, unit_consumes);

        let mut bad = false;

        // v0.118 / testing-the-boundary Slice B: a `system` suite needs a real
        // **serialisation edge** — not merely ≥ 2 participants. The original rule
        // (`participants.len() < 2`) was a proxy for "nothing to serialise
        // across", exact only when the sole edge was cross-context. A single
        // context that exposes an `http` service has a real edge (the public
        // boundary: deserialise → handler → serialise), so it qualifies.
        //
        // Only `http` is admitted here, because only http-at-system is *wired*
        // (`emit_system_http_support` drives `worker.fetch`). A `queue` service
        // does serialise its message, but driving a queue over a real wire at
        // `system` is not built this slice — admitting it would let a queue-only
        // target compile as `system` while `q.message(...)` silently fell through
        // to the unit-tier direct call (no wire). `cron` never qualifies —
        // `scheduled` serialises nothing. Queue-at-system is a noted follow-on.
        let has_serialisation_edge = unit_tables.get(&suite_target).is_some_and(|t| {
            t.services
                .values()
                .any(|s| matches!(s.protocol, bynk_syntax::ast::ServiceProtocol::Http))
        });
        if participants.len() < 2 && !has_serialisation_edge {
            errors.push(
                CompileError::new(
                    "bynk.tier.system_needs_wire",
                    decl.target.span,
                    format!(
                        "`system`-tier suite for `{suite_target}` has no serialisation edge — the target consumes no other context and exposes no `http` service",
                    ),
                )
                .with_note(
                    "a `system` case crosses a real serialise → JSON → deserialise boundary; this target has none to cross, so `unit` already covers it",
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
            refs.enter_file(
                &parsed[i].identity_path(),
                &harness_name,
                parsed[i].is_synthetic(),
            );
            for case in &d.cases {
                check_integration_case_body(
                    &participants,
                    &uses_targets,
                    case,
                    &cross_context,
                    unit_tables,
                    &mut body_errs,
                    refs,
                    tys,
                );
                // Slice C: `Wire(…)` is a `system`-only raw argument (it drives the
                // real wire); in a non-`system` case it has no wire to be raw
                // about, so lowering it would silently pass raw text to a direct
                // in-process handler call. Reject it at the tier where it is known.
                if !matches!(
                    case_effective_tier(case, d),
                    bynk_syntax::ast::TestTier::System
                ) && block_uses_wire(&case.body)
                {
                    body_errs.push(CompileError::new(
                        "bynk.test.wire_needs_system",
                        case.name_span,
                        format!(
                            "case `\"{}\"` uses `Wire(...)` but is not a `system`-tier case",
                            case.name
                        ),
                    ).with_note(
                        "`Wire` hands raw, pre-validation input to the real boundary; promote the case with `as system`, or pass a typed argument",
                    ));
                }
                // #706: `by Nobody` presents no credential to the real auth seam
                // (the 401 path), which exists only at `system`; at a lower tier
                // the handler just runs with no identity, silently not a 401.
                if !matches!(
                    case_effective_tier(case, d),
                    bynk_syntax::ast::TestTier::System
                ) && block_uses_nobody(&case.body)
                {
                    body_errs.push(CompileError::new(
                        "bynk.test.credential_needs_system",
                        case.name_span,
                        format!(
                            "case `\"{}\"` drives `by Nobody` but is not a `system`-tier case",
                            case.name
                        ),
                    ).with_note(
                        "`by Nobody` presents no credential to the real auth seam (the 401 path), which exists only at `system`; promote the case with `as system`, or supply `by <Actor>(<identity>)`",
                    ));
                }
            }
        }
        let bodies_failed = !body_errs.is_empty();
        errors.extend(body_errs);
        if bodies_failed {
            continue;
        }

        ready.insert(group_name.clone(), cross_context);
    }

    ready
}

/// Type-check one integration test case body. The body lives in a synthetic
/// harness root that consumes every participant; entry calls
/// (`ctx.service(args)`) are therefore ordinary cross-context calls. The body
/// has type `Effect[Result[(), ExpectationError]]` (modelled as
/// `Effect[Result[(), ValidationError]]`, as in unit tests).
#[allow(clippy::too_many_arguments)]
fn check_integration_case_body(
    participants: &[String],
    uses_targets: &[String],
    case: &Case,
    cross_context: &resolver::CrossContextInfo,
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    tys: &Arc<Types>,
) {
    // Names in scope: types/fns/methods from `uses` commons (for constructing
    // arguments) plus each participant's types/methods (so return types rebrand
    // and variant patterns resolve).
    let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
    let mut fns: HashMap<String, Arc<FnDecl>> = HashMap::new();
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
    // `synthetic_commons` declares nothing of its own (`items: Vec::new()`
    // above) — every entry in `types`/`fns`/`methods` was merged in from a
    // `uses`/participant unit, so an empty local table (no local types, no
    // local events) is the correct answer here, not a stand-in for one.
    let no_local_types = HashMap::new();
    let no_local_events = HashMap::new();
    let resolved = ResolvedCommons::new(
        synthetic_commons,
        types,
        &no_local_types,
        fns,
        methods,
        HashMap::new(),
        &no_local_events,
        cross_context.clone(),
        HashMap::new(),
        // Test-scaffold body, not a real context emission — never rebranded.
        false,
        HashSet::new(),
    );

    let unit_span = case.span;
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );
    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types, tys).unwrap();
    let mut expr_types: HashMap<ExprId, checker::TypedExpr> = HashMap::new();
    let mut callees: HashMap<ExprId, checker::Callee> = HashMap::new();
    // Test bodies record no hints (out of v0.27 scope) — a throwaway sink.
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    // Test bodies record no capability requirements either — muted sink.
    let mut no_requirements = RequirementSink::new();
    let _ = checker::check_body(
        &resolved,
        &case.body,
        return_ty,
        case.span,
        HashMap::new(),
        checker::CapabilityCtx::default(),
        // Slice B: a `system` case addresses the target's own service (`api.POST`)
        // and names a principal (`by User(...)`), so the checker needs the
        // target's services and actors — the same resolution the unit tier does.
        target_test_services(participants.first().and_then(|t| unit_tables.get(t))),
        target_test_actors(participants.first().and_then(|t| unit_tables.get(t))),
        None,
        checker::CheckSinks {
            tys,
            expr_types: &mut expr_types,
            errors,
            refs,
            hints: &mut no_hints,
            locals: &mut no_locals,
            requirements: &mut no_requirements,
            callees: &mut callees,
        },
    );
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
    tys: &Arc<Types>,
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
            refs.enter_file(&rp.identity_path, target_name, false);
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
                    if !stub_value_typechecks(e, op, &resolved, tys) {
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
        refs.enter_file(
            &parsed[i].identity_path(),
            target_name,
            parsed[i].is_synthetic(),
        );
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
                tys,
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
                tys,
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
///
/// Dual-use (found during P5.4's move, not in the original slice plan):
/// `stub_value_typechecks` (in this module) uses it for the checking path;
/// `bynk-emit`'s `lower_stub_value_block` also calls it, qualified, to lower
/// a `stub` RHS value in the same op-body tail position. `pub` for that
/// second caller, same as every other dual-use function in this module.
pub fn value_block(e: &Expr) -> Block {
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
    resolved: &ResolvedCommons,
    tys: &Arc<Types>,
) -> bool {
    let block = value_block(e);
    let mut expr_types: HashMap<ExprId, checker::TypedExpr> = HashMap::new();
    let mut callees: HashMap<ExprId, checker::Callee> = HashMap::new();
    let mut errs: Vec<CompileError> = Vec::new();
    checker::check_handler_body(
        resolved,
        checker::HandlerBodyCheck::new(&block, &op.return_type, &op.params, &[]),
        checker::CheckSinks {
            tys,
            expr_types: &mut expr_types,
            errors: &mut errs,
            refs: &mut RefSink::new(),
            hints: &mut HintSink::new(),
            locals: &mut LocalsSink::new(),
            requirements: &mut RequirementSink::new(),
            callees: &mut callees,
        },
    );
    errs.is_empty()
}

/// Slice C: whether a `case` body uses a `Wire(…)` raw argument anywhere. A
/// `Wire` is only meaningful at `system` (it hands pre-validation input to the
/// real boundary); used at any other tier it is `bynk.test.wire_needs_system`.
fn block_uses_wire(block: &Block) -> bool {
    // Ported onto `bynk_syntax::ast::expr_children` (P5.4) — `bynk-emit`'s
    // `crate::emitter::walk_exprs` this used before the move is emission-
    // private and unreachable from `bynk-check`. `expr_children` is the
    // exhaustive total child iterator the checker already walks the same way
    // (see `context_checks.rs`/`checker.rs`); this reimplements the original
    // statement-value + tail walk faithfully, not a rewrite of its behaviour.
    fn contains_wire(e: &Expr) -> bool {
        matches!(e.kind, ExprKind::Wire(_))
            || bynk_syntax::ast::expr_children(e)
                .into_iter()
                .any(contains_wire)
    }
    for s in &block.statements {
        let e = match s {
            Statement::Let(l) => &l.value,
            Statement::EffectLet(l) => &l.value,
            Statement::Expect(x) => &x.value,
            Statement::Send(x) => &x.value,
            Statement::Do(d) => &d.value,
            Statement::Assign(a) => &a.value,
        };
        if contains_wire(e) {
            return true;
        }
    }
    contains_wire(&block.tail)
}

/// #706: whether a `case` body drives an effect-let `by Nobody` — the "no
/// credential" principal. It is only meaningful at `system` (there is no auth
/// seam to reject a missing credential at `unit`), so a non-`system` case using
/// it is `bynk.test.credential_needs_system`.
fn block_uses_nobody(block: &Block) -> bool {
    block.statements.iter().any(|s| {
        matches!(s, Statement::EffectLet(l)
            if l.principal.as_ref().is_some_and(|p| p.actor.name == "Nobody"))
    })
}

/// Register a synthetic call-record type per capability operation of the target
/// context (v0.117, testing track slice 5), so `trace(Cap.op)` — typed
/// `List[<CallRecord>]` — supports field access on its records. The record's
/// fields are the operation's parameters.
pub fn register_call_record_types(
    resolved: &mut ResolvedCommons,
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
                Arc::new(TypeDecl {
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
                }),
            );
        }
    }
}

fn target_test_actors(table: Option<&UnitTable>) -> HashMap<String, bynk_syntax::ast::ActorDecl> {
    table.map(|t| t.actors.clone()).unwrap_or_default()
}

fn target_test_services(table: Option<&UnitTable>) -> HashMap<String, checker::TestServiceSig> {
    use bynk_syntax::ast::ServiceProtocol;
    let Some(t) = table else {
        return HashMap::new();
    };
    t.services
        .iter()
        .map(|(name, decl)| {
            let protocol = match &decl.protocol {
                ServiceProtocol::Call => None,
                ServiceProtocol::Http => Some("http".to_string()),
                ServiceProtocol::Cron => Some("cron".to_string()),
                ServiceProtocol::Queue { .. } => Some("queue".to_string()),
                ServiceProtocol::WebSocket { .. } => Some("websocket".to_string()),
                ServiceProtocol::Events { .. } => Some("events".to_string()),
            };
            let handlers = decl
                .handlers
                .iter()
                .map(|h| checker::TestHandler {
                    kind: h.kind.clone(),
                    params: h.params.clone(),
                    by_clause: h.by_clause.clone(),
                    span: h.span,
                })
                .collect();
            (name.clone(), checker::TestServiceSig { protocol, handlers })
        })
        .collect()
}

/// Type-check a test `case`/`property` body against the target unit's privileges,
/// returning the inferred `expr_types` map. The **check** path feeds real
/// diagnostic/ref sinks; the **emit** path reuses it with throwaway sinks to give
/// the case-body lowering full type information (so collection kernels — notably
/// `trace(Cap.op)`'s `List[…]` methods — dispatch on the receiver's checked type).
#[allow(clippy::too_many_arguments)]
pub fn typecheck_case_body(
    target_name: &str,
    body: &Block,
    unit_span: Span,
    unit_tables: &HashMap<String, UnitTable>,
    resolved: &ResolvedCommons,
    errors: &mut Vec<CompileError>,
    refs: &mut RefSink,
    // v0.119: bindings already in scope for the body — empty for a `case`, the
    // `run: List[Step]` binding for a history property.
    initial_scope: HashMap<String, checker::TyId>,
    tys: &Arc<Types>,
) -> HashMap<ExprId, checker::TypedExpr> {
    let mut expr_types: HashMap<ExprId, checker::TypedExpr> = HashMap::new();
    let mut callees: HashMap<ExprId, checker::Callee> = HashMap::new();
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
                .map(|op| build_capability_op_info(op, &resolved.types, tys))
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

    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types, tys).unwrap();
    let return_ty_span = unit_span;
    // Test bodies record no hints (out of v0.27 scope) — a throwaway sink.
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    // Test bodies record no capability requirements either — muted sink.
    let mut no_requirements = RequirementSink::new();
    let _ = checker::check_body(
        resolved,
        body,
        return_ty,
        return_ty_span,
        initial_scope,
        checker::CapabilityCtx {
            capabilities: capability_info_map.clone(),
            declared_capabilities: capability_info_map,
            given_remaining: given_declared.iter().cloned().collect(),
            given_used: HashSet::new(),
            given_entries: Vec::new(),
            given_anchor: None,
        },
        target_test_services(unit_tables.get(target_name)),
        target_test_actors(unit_tables.get(target_name)),
        None,
        checker::CheckSinks {
            tys,
            expr_types: &mut expr_types,
            errors,
            refs,
            hints: &mut no_hints,
            locals: &mut no_locals,
            requirements: &mut no_requirements,
            callees: &mut callees,
        },
    );
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
    tys: &Arc<Types>,
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
        tys,
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
                id: ExprId::SYNTHETIC,
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
pub const PROP_GEN_DEPTH: u32 = 12;

/// Whether a `for all x: T` binding's type is refinement-generable: refined
/// types must not carry a `Matches` predicate (no refinement-driven generator),
/// and sums/records must have every component recursively generable within the
/// depth cap. Mirrors the checker's `can_mock_bare`.
pub fn prop_binding_generable(
    ty: checker::TyId,
    types: &HashMap<String, Arc<TypeDecl>>,
    depth: u32,
    tys: &Arc<Types>,
) -> bool {
    if depth == 0 {
        return false;
    }
    match &*tys.get(ty) {
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
                        checker::resolve_type_ref(&f.type_ref, types, tys)
                            .is_some_and(|t| prop_binding_generable(t, types, depth - 1, tys))
                    })
                }),
                TypeBody::Record(r) => r.fields.iter().all(|f| {
                    checker::resolve_type_ref(&f.type_ref, types, tys)
                        .is_some_and(|t| prop_binding_generable(t, types, depth - 1, tys))
                }),
            }
        }
        _ => false,
    }
}

/// The refinement of a resolved refined/opaque named type, if any — used by the
/// conservative restates-refinement check.
fn named_refinement<'a>(
    ty: checker::TyId,
    types: &'a HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
) -> Option<&'a Refinement> {
    let node = tys.get(ty);
    let checker::Ty::Named { name, .. } = &*node else {
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
pub fn history_variant_name(handler: &str) -> String {
    let mut chars = handler.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => handler.to_string(),
    }
}

/// The agent's drivable `on call` handlers — the ones a history sequences. Other
/// handler kinds (`http`/`cron`/`message`/`open`/`close`) are not RPC entry points
/// and are never part of a generated call-history.
pub fn history_handlers(agent: &AgentDecl) -> Vec<&Handler> {
    agent
        .handlers
        .iter()
        .filter(|h| matches!(h.kind, HandlerKind::Call) && h.method_name.is_some())
        .collect()
}

/// v0.119 (testing track slice 7, ADR 0155): type-check a `for all run:
/// History[Agent]` binding. The subject is a *run* of the agent — a generated,
/// driven call-history — bound as an ordinary `List[Step]`. Validates the
/// DECISION-B rules (agent-only, every handler parameter generable), registers the
/// synthetic call-sum / step / state record types into `resolved.types` so the
/// predicate's `List` + value surface (`.call is …`, `.old`/`.new`, `.accepted`)
/// type-checks, and returns the bound `List[Step]` type.
pub fn check_history_binding(
    inner: &TypeRef,
    span: Span,
    resolved: &mut ResolvedCommons,
    refs: &mut RefSink,
    tys: &Arc<Types>,
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
            let generable = checker::resolve_type_ref(&p.type_ref, &resolved.types, tys)
                .is_some_and(|t| prop_binding_generable(t, &resolved.types, PROP_GEN_DEPTH, tys));
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
        Arc::new(TypeDecl {
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
        }),
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
        Arc::new(TypeDecl {
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
        }),
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
        Arc::new(TypeDecl {
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
        }),
    );

    Ok(checker::Ty::List(tys.intern(checker::Ty::Named {
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
    tys: &Arc<Types>,
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
    let mut binding_scope: HashMap<String, checker::TyId> = HashMap::new();
    let mut binding_types: Vec<(String, Option<checker::TyId>)> = Vec::new();
    // v0.119: the single `History[Agent]` binding (run-var, agent), for the
    // post-body `restates_invariant` check (DECISION D).
    let mut history_binding: Option<(String, AgentDecl)> = None;
    for b in &prop.forall.bindings {
        // v0.119 (ADR 0155): `for all run: History[Agent]` — the history rung. A
        // driven call-history, bound as an ordinary `List[Step]`.
        if let TypeRef::History(inner, hspan) = &b.type_ref {
            match check_history_binding(inner, *hspan, &mut resolved, refs, tys) {
                Ok(step_ty) => {
                    if let TypeRef::Named(agent_id) = &**inner
                        && let Some(agent) = resolved.agents.get(&agent_id.name)
                    {
                        history_binding = Some((b.name.name.clone(), agent.clone()));
                    }
                    binding_scope.insert(b.name.name.clone(), tys.intern(step_ty.clone()));
                    binding_types.push((b.name.name.clone(), Some(tys.intern(step_ty))));
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
        let ty = match checker::resolve_type_ref(&b.type_ref, &resolved.types, tys) {
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
        if !prop_binding_generable(ty, &resolved.types, PROP_GEN_DEPTH, tys) {
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
        binding_scope.insert(b.name.name.clone(), ty);
        binding_types.push((b.name.name.clone(), Some(ty)));
    }

    // Type the `where`/body predicates in the target's privileged view with the
    // bindings in scope — mirroring the `case` body context.
    let mut expr_types: HashMap<ExprId, checker::TypedExpr> = HashMap::new();
    let mut callees: HashMap<ExprId, checker::Callee> = HashMap::new();
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
                .map(|op| build_capability_op_info(op, &resolved.types, tys))
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
    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types, tys).unwrap();
    let return_ty_span = prop.span;
    let mut no_hints = HintSink::new();
    let mut no_locals = LocalsSink::new();
    let mut no_requirements = RequirementSink::new();
    // The optional `where` filter is checked first (against `Bool`), sharing
    // `check_body`'s `Ctx` with the body below; the body is the one predicate
    // surface: `expect`s self-check as `Bool`.
    let _ = checker::check_body(
        &resolved,
        &prop.forall.body,
        return_ty,
        return_ty_span,
        binding_scope,
        checker::CapabilityCtx {
            capabilities: capability_info_map.clone(),
            declared_capabilities: capability_info_map,
            given_remaining: given_declared.iter().cloned().collect(),
            given_used: HashSet::new(),
            given_entries: Vec::new(),
            given_anchor: None,
        },
        target_test_services(unit_tables.get(target_name)),
        target_test_actors(unit_tables.get(target_name)),
        prop.forall.where_pred.as_ref(),
        checker::CheckSinks {
            tys,
            expr_types: &mut expr_types,
            errors,
            refs,
            hints: &mut no_hints,
            locals: &mut no_locals,
            requirements: &mut no_requirements,
            callees: &mut callees,
        },
    );

    // Conservative restates-refinement flag: a single-binding property whose
    // body is exactly `expect <pred>` restating the bound var's refinement.
    if let [(var, Some(ty))] = binding_types.as_slice()
        && let Some(refinement) = named_refinement(*ty, &resolved.types, tys)
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
                    ty.display(tys)
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
    resolved: &ResolvedCommons,
    refs: &mut RefSink,
) {
    checker::record_type_refs(type_ref, &resolved.types, &HashSet::new(), refs);
}

/// Build a [`resolver::ResolvedCommons`] backed by `owning_unit`'s privileged
/// view: its types, fns, methods, plus types/fns from every commons it
/// `uses`, plus exported types from every consumed context. The same
/// shape used by the production pipeline. Returns the [`ResolvedCommons`]
/// plus a synthetic commons span for the test.
pub fn build_privileged_resolved(
    owning_unit: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> Option<(ResolvedCommons, ())> {
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
    let no_local_events = HashMap::new();
    let resolved = ResolvedCommons::new(
        synthetic_commons,
        types,
        &local.types,
        fns,
        methods,
        agents_for_resolved,
        // "Privileged" test/stub-body resolved — deliberately relaxed, not a
        // real context emission subject to the rebrand — so events stay
        // empty rather than reading `local`'s.
        &no_local_events,
        cross_context,
        HashMap::new(),
        false,
        HashSet::new(),
    );
    Some((resolved, ()))
}
