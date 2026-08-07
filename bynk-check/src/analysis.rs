//! The project-level analysis entry point (P4.1, #1115): discovery
//! (`bynk-project`) → parse → resolve → check, returning the `bynk-ide`-facing
//! analogue of `bynk-emit`'s `ProjectAnalysis` — without ever emitting.
//!
//! `bynk-ide` is **not** repointed at [`analyse_project`] by this slice (that
//! is P4.2's job) — `bynk_emit::project::analyse_project_with` is still what
//! every real caller uses today. This entry point exists so P4.2 has
//! something to repoint at, and so the differential fixture
//! (`bynk-check/tests/differential_analysis.rs`) can pin the two paths
//! together while both exist.
//!
//! ## The residual gap
//!
//! This entry point is diagnostically faithful to
//! `bynk-emit`'s `run_checks`'s `Mode::Analyse` arm **minus** seven
//! categories of whole-project checking, deliberately not ported here
//! (recorded on the tracking issue's own scope-correction comments, not
//! silently assumed):
//!
//! 1. **Schema-registry reconciliation** (`schema_registry::reconcile`) —
//!    unreachable from the `Mode::Analyse` path today anyway (it only runs
//!    under `SchemaLock::On`, and `analyse_project_with` always passes
//!    `SchemaLock::Off`), so this is a gap in name only.
//! 2. **`messages` bundle validation** (`check_messages_bundles`).
//! 3. **Locale bundle ambiguity** (`check_locale_bundle_ambiguity`).
//! 4. **Event-subscription validation** (`check_event_subscriptions`).
//! 5. **Platform-lock enforcement** (`check_platform_lock`).
//! 6. **Function-type-boundary checks** (`check_function_type_boundaries`) —
//!    reached, in `bynk-emit`, from inside `phase_group`; this entry point
//!    calls [`crate::project_model::phase_group`] with `None` for that
//!    phase's optional boundary-check hook, so the check genuinely doesn't
//!    run rather than being duplicated with a different diagnostic order.
//! 7. **Test/integration-suite processing**
//!    (`process_tests`/`process_integration_tests`) — unlike categories 2-6,
//!    these run *unconditionally* in `run_checks`, in `Mode::Analyse` too,
//!    and push into the same shared error sink (`bynk-emit`'s own
//!    `check_project_reports_a_test_body_error_past_an_earlier_structural_error`
//!    pins a `bynk.types.let_annotation_mismatch` originating inside a
//!    `suite`/`test integration` body). This entry point reports none of it:
//!    the two functions are emission-coupled (`CompiledFile`, `RunnableTest`,
//!    `ImportExt`, `contracts`, a shared `emitted_barrels` set) deeply enough
//!    that porting them here would mean porting a slice of emission into an
//!    entry point defined by never emitting — out of proportion with this
//!    slice. A second, non-diagnostic consequence rides along: both
//!    functions take `&mut RefSink`, so every binding edge inside a `.bynk`
//!    suite file is also absent from [`ProjectAnalysis::index`] here. After
//!    P4.2 repoints `bynk-ide` at this entry point, go-to-definition inside a
//!    test file stops working with no diagnostic to explain why, until this
//!    category closes.
//!
//! Emission itself is orthogonal rather than a gap: this entry point never
//! emits, by construction (it has no `BuildTarget`/`ImportExt`/`contracts`
//! concept at all), so there is no diagnostic-agreement question to ask of it.
//!
//! A fixture that exercises none of the seven categories above sees identical
//! diagnostics from this entry point and from `analyse_project_with` — that
//! is what the differential fixture's two clean/broken cases assert. A third
//! case (`new_entry_point_omits_test_body_diagnostics`) pins category 7's
//! divergence itself, so it is visible in the fixture rather than merely
//! documented here.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::check_pipeline::{check_file_core, prepare_unit_check_ctx, record_analyse_types};
use crate::checker::Types;
use crate::expr_types::{ExprTypeSink, FileExprTypes};
use crate::firstparty::Platform;
use crate::hints::{FileHints, HintSink};
use crate::index::{ProjectIndex, RefSink};
use crate::locals::{FileLocals, LocalsSink};
use crate::project_model::{
    self, ErrorSink, assemble_unit_info, collect_unit_methods, compose_unit_symbols,
    merge_consumed_exports, normalize_service_defaults,
};
use crate::requirements::{FileRequirements, RequirementSink};
use crate::symbols::{assemble_index, build_cross_context_info, combined_types_for};
use bynk_project::{AttributedError, Roots, UnitKind};
use bynk_syntax::ast::{AgentDecl, ServiceDecl, TypeDecl};

/// #846: the per-unit slice of resolution the sequence-diagram classifier
/// needs — see [`ProjectAnalysis::sequence_info`]. Moved verbatim (Decision
/// C, #1115) from `bynk-emit/src/project/diagnostics.rs`.
#[derive(Debug, Clone, Default)]
pub struct ContextSequenceInfo {
    pub cross_context: crate::resolver::CrossContextInfo,
    pub agents: HashMap<String, AgentDecl>,
}

/// #855: the per-unit slice of resolution the wire-contract peek needs —
/// see [`ProjectAnalysis::boundary_info`]. A sibling of
/// [`ContextSequenceInfo`], not a field on it: that struct is named and
/// documented for #846, and this is a separate retained table serving a
/// separate query (hover/panel over a single handler's boundary, not the
/// sequence-diagram classifier). Moved verbatim (Decision C, #1115).
#[derive(Debug, Clone, Default)]
pub struct ContextBoundaryInfo {
    /// `combined_types_for`: the unit's own declared types plus the types of
    /// every commons it `uses` — the same table `own_contract_hashes` hashes
    /// through, so the peek's hash and the emitted `X-Bynk-Contract` constant
    /// cannot disagree.
    pub types: HashMap<String, Arc<TypeDecl>>,
    pub services: HashMap<String, ServiceDecl>,
    pub agents: HashMap<String, AgentDecl>,
}

/// v0.24: the analyse-mode result — every discovered file's analysed text
/// snapshot (positions must convert against the text that was analysed, not
/// a newer buffer) plus the attributed diagnostics. Moved verbatim (Decision
/// C, #1115) from `bynk-emit/src/project/diagnostics.rs`; `bynk-emit`
/// re-exports this type at its old path (`bynk_emit::project::ProjectAnalysis`)
/// so `bynk-ide`'s existing destructuring needs no field-by-field rewrite.
pub struct ProjectAnalysis {
    /// `(project-relative source path, analysed text)` for every file read,
    /// including clean files (the LSP needs them to clear diagnostics).
    pub snapshots: Vec<(PathBuf, String)>,
    pub errors: Vec<AttributedError>,
    /// v0.25 (ADR 0053): the project-wide binding index. Empty when the
    /// pipeline bails before resolution (discovery/parse failures).
    pub index: ProjectIndex,
    /// v0.27 (ADR 0056): per-file inferred-type inlay hints — `(binding-name
    /// span, label)`, span-ordered, harvested from the checker's binding
    /// sites. Empty for files the pipeline never type-checked.
    pub hints: FileHints,
    /// v0.30.2 (ADR 0063): per-file expression types — `(expr span, Ty)`,
    /// captured on the Ok path (a file that checks clean), for `.`-member
    /// completion's receiver typing. Empty for files with errors (the
    /// clean-file ceiling) and for synthetic files.
    pub expr_types: FileExprTypes,
    /// T3.6b (R4.1): the intern table every `TyId` in `expr_types` resolves
    /// against — one table shared across the whole analysis.
    pub ty_intern: Arc<Types>,
    /// v0.31 (ADR 0064): per-file local bindings with their scope ranges —
    /// `let`/`let <-`, fn/handler/lambda params — for the scope-at-offset
    /// query backing locals completion + navigation. Synthetic files muted.
    pub locals: FileLocals,
    /// v0.99: per-file capability-requirement ledger — every
    /// capability-consuming site (direct call, store op), covered or not,
    /// with its provenance. Empty for files the pipeline never type-checked,
    /// and for synthetic/test files (muted).
    pub requirements: FileRequirements,
    /// Slice 6b (ADR 0095): qualified unit name → the project source file(s)
    /// that comprise it, in discovery order. Excludes synthetic
    /// (toolchain-injected) units; empty when the pipeline bails before the
    /// checker.
    pub unit_sources: HashMap<String, Vec<PathBuf>>,
    /// #846: qualified context/adapter unit name → the cross-context and
    /// agent tables needed to classify a handler call as a lifeline for the
    /// sequence-diagram query. Only contexts/adapters have an entry; empty
    /// when the pipeline bails before the checker.
    pub sequence_info: HashMap<String, ContextSequenceInfo>,
    /// #855: qualified context/adapter unit name → the combined type table
    /// and per-context service/agent tables the wire-contract peek needs.
    /// Only contexts/adapters have an entry; empty when the pipeline bails
    /// before the checker.
    pub boundary_info: HashMap<String, ContextBoundaryInfo>,
    /// #848: qualified unit name → its doc-comment intra-doc-link search
    /// order — itself first, then its `uses` targets, then its `consumes`
    /// targets, in that order. Empty when the pipeline bails before the
    /// checker.
    pub doc_scope: HashMap<String, Vec<String>>,
}

/// The `ProjectAnalysis` shape for every "bailed before the checker ran"
/// exit — discovery/file-conflict/parse failures. Factored out so the three
/// early-return sites in [`analyse_project`] agree by construction, the same
/// role `RunChecks::Bailed` plays in `bynk-emit`'s `run_checks`.
fn bailed(
    errors: ErrorSink,
    snapshots: Vec<(PathBuf, String)>,
    mut hints: HintSink,
    mut locals: LocalsSink,
    mut exprs: ExprTypeSink,
    mut requirements: RequirementSink,
    tys: &Arc<Types>,
) -> ProjectAnalysis {
    ProjectAnalysis {
        snapshots,
        // ADR 0117: the LSP renders warnings alongside errors (severity is
        // applied downstream), so analyse surfaces the full diagnostic list.
        errors: errors.into_all(),
        index: ProjectIndex::default(),
        hints: hints.take_files(),
        locals: locals.take_files(),
        expr_types: exprs.take_files(),
        ty_intern: Arc::clone(tys),
        requirements: requirements.take_files(),
        unit_sources: HashMap::new(),
        sequence_info: HashMap::new(),
        boundary_info: HashMap::new(),
        doc_scope: HashMap::new(),
    }
}

/// The `bynk-check`-native discovery→parse→resolve→check entry point (P4.1,
/// #1115) — see this module's own doc comment for the documented residual
/// gap against `bynk-emit`'s `analyse_project_with`. Mirrors
/// `analyse_project_with`'s own call shape exactly where the two overlap:
/// `BuildTarget::Bundle`-equivalent (this entry point has no build target at
/// all — it never emits), `Platform::default()`, no schema-registry lock.
///
/// Identity is project-relative (ADR 0198): a file's `source_path` here is
/// unique across `include` roots, same as `analyse_project_with`.
pub fn analyse_project(roots: &Roots, overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis {
    let tys = &Arc::new(Types::new());
    let trees = roots.trees();
    let excludes = roots.excludes();

    let mut errors = ErrorSink::new();
    let mut refs = RefSink::new();
    let mut hints = HintSink::new();
    let mut locals = LocalsSink::new();
    let mut requirements = RequirementSink::new();
    let mut exprs = ExprTypeSink::new();
    let mut snapshots: Vec<(PathBuf, String)> = Vec::new();

    // -- 1. Discovery. --
    let file_lists = match project_model::phase_discovery(&trees, &excludes, &mut errors) {
        Ok(files) => files,
        Err(()) => return bailed(errors, snapshots, hints, locals, exprs, requirements, tys),
    };
    if project_model::check_discovered_files(&trees, &file_lists, &mut errors).is_err() {
        return bailed(errors, snapshots, hints, locals, exprs, requirements, tys);
    }

    // -- 2. Parse. --
    let (mut parsed, consumes_bynk, consumes_cloudflare) =
        match project_model::phase_parse(&trees, &file_lists, overlay, &mut errors, &mut snapshots)
        {
            Ok(out) => out,
            Err(()) => return bailed(errors, snapshots, hints, locals, exprs, requirements, tys),
        };

    // -- 2b. Normalize service-level `by`/`given` defaults (v0.155). --
    normalize_service_defaults(&mut parsed);
    let parsed = parsed;

    // -- 3. Group. The `None` here is the residual gap's category 6 (see
    //       this module's own doc comment) — `phase_group` never calls
    //       `check_function_type_boundaries`. --
    let (groups, kinds, _test_groups, _integration_groups, _adapter_bindings, _npm_deps) =
        project_model::phase_group(
            &parsed,
            &trees,
            Platform::default(),
            consumes_bynk,
            consumes_cloudflare,
            overlay,
            None,
            &mut errors,
        );

    // -- 4. Per-unit combined symbol tables. --
    let unit_tables = project_model::phase_symbol_tables(&groups, &kinds, &parsed, &mut errors);

    // -- 5. `uses` resolution. --
    let unit_uses =
        project_model::phase_resolve_uses(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5b. `consumes` resolution. --
    let (unit_consumes, unit_flattened) = project_model::phase_resolve_consumes(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 5b'. `consumes` aliases. --
    let unit_consumes_aliases =
        project_model::phase_consumes_aliases(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5c. `consumes` cycles. --
    project_model::phase_detect_consumes_cycles(&groups, &parsed, &unit_consumes, &mut errors);

    // -- 6. `uses` name-conflict detection. --
    project_model::phase_uses_name_conflicts(
        &unit_uses,
        &unit_tables,
        &parsed,
        &groups,
        &mut errors,
    );

    // -- 6b. Type exports. Categories 2-4 (messages/locale/event checks) are
    //        the residual gap between here and `phase_validate_providers` —
    //        see this module's own doc comment. --
    let exports_visibility = project_model::phase_validate_type_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6b'. Capability exports. --
    project_model::phase_validate_capability_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6c. Provider matching. --
    project_model::phase_validate_providers(&unit_tables, &groups, &parsed, &mut errors, tys);

    // Category 1 (schema-registry reconciliation) is the residual gap here —
    // unreachable from this path in `bynk-emit` too (always `SchemaLock::Off`).

    // No bail gate: this entry point never bails after discovery (mirrors
    // `Mode::Analyse` — independent unit groups resolve/check past another
    // group's errors).

    // -- 7. Per-unit file index. --
    let unit_file_index = project_model::phase_file_index(&groups, &parsed);

    // -- 7b. Assemble per-unit info. --
    let unit_info = assemble_unit_info(
        &groups,
        &kinds,
        &unit_tables,
        &unit_uses,
        &unit_consumes,
        &unit_flattened,
        &unit_consumes_aliases,
        &exports_visibility,
        &unit_file_index,
    );

    // -- 8. For each unit, compose the symbol space and resolve+check every
    //       file. Categories 5 (platform-lock) and test/integration
    //       processing are the residual gap after this loop — see this
    //       module's own doc comment. --
    for (name, info) in &unit_info {
        let kind = info.kind;
        let indices = info.files.as_slice();
        let local_table = &info.table;
        let group_error_baseline = errors.len();

        let (
            mut combined_types,
            combined_fns,
            mut combined_methods,
            mut imported_from,
            mut imported_from_kind,
        ) = compose_unit_symbols(name, local_table, &unit_info);
        let consumed_types = merge_consumed_exports(
            name,
            &parsed,
            &unit_info,
            &mut combined_types,
            &mut combined_methods,
            &mut imported_from,
            &mut imported_from_kind,
            &mut errors,
        );

        if errors.len() > group_error_baseline {
            continue;
        }

        let local_names: HashSet<String> = local_table.types.keys().cloned().collect();
        let local_methods_for_type = collect_unit_methods(indices, &parsed);
        let ctx = prepare_unit_check_ctx(kind, &unit_info, &combined_types, &imported_from_kind);

        for &i in indices {
            let pf = &parsed[i];
            if let Some(crate::check_pipeline::FileCheckResult { typed, .. }) = check_file_core(
                name,
                kind,
                pf,
                &unit_info,
                &combined_types,
                &combined_fns,
                &combined_methods,
                &local_names,
                &local_methods_for_type,
                &consumed_types,
                &imported_from,
                &ctx,
                &mut errors,
                &mut refs,
                &mut hints,
                &mut locals,
                &mut exprs,
                &mut requirements,
                tys,
            ) {
                // This entry point never emits — every clean file's exit is
                // the Analyse-mode one (record best-effort/final types, move
                // on to the next file). Mirrors `check_unit_files`'s own
                // `mode == Mode::Analyse` clean-path branch.
                record_analyse_types(
                    &mut exprs,
                    &pf.identity_path(),
                    pf.is_synthetic(),
                    &typed.expr_types,
                );
            }
        }
    }

    // -- Assemble the `ProjectAnalysis`. Mirrors `analyse_project_with`'s own
    //    `RunChecks::Checked` arm exactly. --
    let index = assemble_index(
        &parsed,
        &unit_uses,
        &unit_consumes,
        std::mem::take(&mut refs),
    );

    let mut unit_sources: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for pf in &parsed {
        if pf.is_synthetic() {
            continue;
        }
        unit_sources
            .entry(pf.unit().name().joined())
            .or_default()
            .push(pf.identity_path());
    }

    let mut sequence_info: HashMap<String, ContextSequenceInfo> = HashMap::new();
    let mut boundary_info: HashMap<String, ContextBoundaryInfo> = HashMap::new();
    for (name, kind) in &kinds {
        if !matches!(kind, UnitKind::Context | UnitKind::Adapter) {
            continue;
        }
        let Some(table) = unit_tables.get(name) else {
            continue;
        };
        let mut cross_context = build_cross_context_info(
            name,
            &unit_consumes,
            &unit_consumes_aliases,
            &unit_uses,
            &unit_tables,
        );
        cross_context.flattened_caps = unit_flattened.get(name).cloned().unwrap_or_default();
        let agents: HashMap<String, AgentDecl> = table.agents.clone();
        sequence_info.insert(
            name.clone(),
            ContextSequenceInfo {
                cross_context,
                agents: agents.clone(),
            },
        );
        boundary_info.insert(
            name.clone(),
            ContextBoundaryInfo {
                types: combined_types_for(name, &unit_tables, &unit_uses),
                services: table.services.clone(),
                agents,
            },
        );
    }

    let mut doc_scope: HashMap<String, Vec<String>> = HashMap::new();
    for name in unit_sources.keys() {
        let mut scope = vec![name.clone()];
        scope.extend(unit_uses.get(name).cloned().unwrap_or_default());
        scope.extend(unit_consumes.get(name).cloned().unwrap_or_default());
        doc_scope.insert(name.clone(), scope);
    }

    ProjectAnalysis {
        snapshots,
        errors: errors.into_all(),
        index,
        hints: hints.take_files(),
        locals: locals.take_files(),
        expr_types: exprs.take_files(),
        ty_intern: Arc::clone(tys),
        requirements: requirements.take_files(),
        unit_sources,
        sequence_info,
        boundary_info,
        doc_scope,
    }
}
