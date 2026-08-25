//! The shared per-unit/per-file resolve-check core, used by both
//! `bynk-emit`'s `check_unit_files` (`Mode::Build` and `Mode::Analyse`) and
//! this crate's own [`crate::analysis::analyse_project`].
//!
//! P4.1 (#1115), the same `extract, don't duplicate` move as
//! [`crate::project_model`]: `check_unit_files`'s per-file body is identical
//! for both modes except for four `record_analyse_types` call sites (the
//! error-path exits) and the final "Analyse mode always stops here, Build
//! mode falls through to `certify`+`emit_unit`" branch. This module owns
//! everything up to (not including) that branch — [`check_file_core`]
//! returns `Some(TypedCommons)` only on the fully-clean, non-blocked path, so
//! a caller that wants to emit knows exactly when it may. The four
//! error-path recordings are unconditional here now (previously gated on
//! `mode == Mode::Analyse`) — behaviour-preserving for the `Mode::Build`
//! caller, which never took that branch anyway (`mode == Mode::Analyse`
//! gated it), and whose `exprs` sink `compile_project`'s `ProjectOutput`
//! never exposes.
//!
//! What stayed in `bynk-emit`: `Mode` itself (meaningless here — this
//! crate's own entry point has exactly one behaviour), `certify`+`emit_unit`
//! (real emission), and the decision of *whether* to record the clean-path
//! types (each caller does that itself with the `Some(TypedCommons)` this
//! module hands back — `bynk-emit`'s `Mode::Build` caller skips it,
//! `Mode::Analyse` and this crate's own entry point both call
//! [`record_analyse_types`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::checker::{self, TypedCommons, Types};
use crate::context_checks::{check_context_constraints, check_context_declarations};
use crate::expr_types::ExprTypeSink;
use crate::hints::HintSink;
use crate::index::RefSink;
use crate::locals::LocalsSink;
use crate::project_model::{ErrorSink, UnitInfo};
use crate::requirements::RequirementSink;
use crate::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use crate::symbols::{ConsumedType, UnitTable, build_cross_context_info, combined_types_for};
use bynk_project::{ParsedFile, UnitKind};
use bynk_syntax::ast::{CommonsItem, ExprId, FnName, TypeDecl};

/// Record a file's (possibly partial) expression types into the Analyse-mode
/// sink. Called at every per-file exit in the check loop so `.`-member
/// completion and signature help get the receiver's type even when a later
/// check phase errors for the file (ADR 0094). A no-op-shaped wrapper,
/// factored out so the four error-path exits (now unconditional, see this
/// module's own doc comment) and every clean-path caller share one call.
pub fn record_analyse_types(
    exprs: &mut ExprTypeSink,
    source_path: &Path,
    synthetic: bool,
    types: &HashMap<ExprId, checker::TypedExpr>,
) {
    exprs.enter_file(source_path, synthetic);
    exprs.record_file(types);
}

/// The four parallel per-project maps `build_cross_context_info`/
/// `combined_types_for` need (their own general, map-based signature — see
/// [`UnitCheckCtx`]'s own doc comment for why the per-file core materialises
/// them from `unit_info` rather than changing that signature).
type CrossContextViews = (
    HashMap<String, UnitTable>,
    HashMap<String, Vec<String>>,
    HashMap<String, Vec<String>>,
    HashMap<String, HashMap<String, String>>,
);

/// v0.29.4: `build_cross_context_info` (and its `combined_types_for` helper)
/// is a general map-based function — the test-emission path calls it with
/// *synthetic* harness maps, not `unit_info` — so it keeps its parallel-map
/// signature. The per-file core only has `unit_info`, so this materialises
/// the four views that one call needs, once per unit ahead of the file loop
/// — but only for a context/adapter, `build_cross_context_info`'s only
/// caller. `UnitTable` owns every declaration body in the unit, so for every
/// other unit kind (including the seven injected first-party commons) this
/// would otherwise be a whole-project deep clone, performed and discarded,
/// once per unit.
pub struct UnitCheckCtx {
    cross_context_views: Option<CrossContextViews>,
    /// #907: the exact set of type names `emit_context_rebrands` rebrands for
    /// this unit — names brought in via `uses` of a *commons* specifically
    /// (not a local declaration, and not a type surfaced via `consumes`,
    /// which `imported_from_kind` tags `UnitKind::Context` in
    /// `merge_consumed_exports` and which the emitter never rebrands).
    pub uses_commons_type_names: HashSet<String>,
}

/// Build the per-unit prelude [`check_file_core`] shares across every file
/// in the unit — see [`UnitCheckCtx`]'s own doc comment.
pub fn prepare_unit_check_ctx(
    kind: UnitKind,
    unit_info: &BTreeMap<String, UnitInfo>,
    combined_types: &HashMap<String, Arc<TypeDecl>>,
    imported_from_kind: &HashMap<String, UnitKind>,
) -> UnitCheckCtx {
    let cross_context_views = if kind == UnitKind::Context || kind == UnitKind::Adapter {
        let unit_tables: HashMap<String, UnitTable> = unit_info
            .iter()
            .map(|(n, i)| (n.clone(), i.table.clone()))
            .collect();
        let unit_uses: HashMap<String, Vec<String>> = unit_info
            .iter()
            .map(|(n, i)| (n.clone(), i.uses.clone()))
            .collect();
        let unit_consumes: HashMap<String, Vec<String>> = unit_info
            .iter()
            .map(|(n, i)| (n.clone(), i.consumes.clone()))
            .collect();
        let unit_consumes_aliases: HashMap<String, HashMap<String, String>> = unit_info
            .iter()
            .map(|(n, i)| (n.clone(), i.aliases.clone()))
            .collect();
        Some((unit_tables, unit_uses, unit_consumes, unit_consumes_aliases))
    } else {
        None
    };
    let uses_commons_type_names: HashSet<String> = imported_from_kind
        .keys()
        .filter(|n| crate::resolver::is_uses_commons_type(imported_from_kind, combined_types, n))
        .cloned()
        .collect();
    UnitCheckCtx {
        cross_context_views,
        uses_commons_type_names,
    }
}

/// The clean-path output of [`check_file_core`]: the typed, fully-checked
/// unit plus the per-file cross-context info that produced it — a
/// `Mode::Build` caller needs both to reach `certify`+`emit_unit` (`emit_unit`
/// takes `cross_context_for_file` as its own argument, so this avoids making
/// the caller recompute it from `ctx`/`unit_info` a second time).
pub struct FileCheckResult {
    pub typed: TypedCommons,
    pub cross_context: resolver::CrossContextInfo,
}

/// The shared resolve+check+context-checks core for one file, factored out
/// of `check_unit_files` (see this module's own doc comment). Returns
/// `Some(FileCheckResult)` only on the fully-clean, non-blocked path — the
/// signal a `Mode::Build` caller uses to know it may proceed to
/// `certify`+`emit_unit`. Every error/blocked exit records best-effort
/// partial types unconditionally (see [`record_analyse_types`]) and returns
/// `None`.
#[allow(clippy::too_many_arguments)]
pub fn check_file_core(
    name: &str,
    kind: UnitKind,
    pf: &ParsedFile,
    unit_info: &BTreeMap<String, UnitInfo>,
    combined_types: &HashMap<String, Arc<TypeDecl>>,
    combined_fns: &HashMap<String, Arc<bynk_syntax::ast::FnDecl>>,
    combined_methods: &HashMap<String, ResolverMethodTable>,
    local_names: &HashSet<String>,
    local_methods_for_type: &HashMap<String, Vec<bynk_syntax::ast::FnDecl>>,
    consumed_types: &HashMap<String, ConsumedType>,
    imported_from: &HashMap<String, String>,
    ctx: &UnitCheckCtx,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    exprs: &mut ExprTypeSink,
    requirements: &mut RequirementSink,
    tys: &Arc<Types>,
) -> Option<FileCheckResult> {
    let mut emit_items: Vec<CommonsItem> = Vec::new();
    let types_in_this_file: HashSet<String> = pf
        .items()
        .iter()
        .filter_map(|it| match it {
            CommonsItem::Type(t) => Some(t.name.name.clone()),
            // Events track, slice 0 (spine #936): an `event` shares the
            // `types` namespace, so a multi-file context's method dispatch
            // treats its name the same as a `type`'s.
            CommonsItem::Event(e) => Some(e.name.name.clone()),
            _ => None,
        })
        .collect();
    for item in pf.items() {
        match item {
            CommonsItem::Type(t) => {
                emit_items.push(CommonsItem::Type(t.clone()));
            }
            CommonsItem::Fn(f) => match &f.name {
                FnName::Free(_) => emit_items.push(CommonsItem::Fn(f.clone())),
                FnName::Method { type_name, .. } => {
                    if types_in_this_file.contains(&type_name.name) {
                        emit_items.push(CommonsItem::Fn(f.clone()));
                    }
                }
            },
            CommonsItem::Capability(c) => {
                emit_items.push(CommonsItem::Capability(c.clone()));
            }
            CommonsItem::Provider(p) => {
                emit_items.push(CommonsItem::Provider(p.clone()));
            }
            CommonsItem::Service(s) => {
                emit_items.push(CommonsItem::Service(s.clone()));
            }
            CommonsItem::Agent(a) => {
                emit_items.push(CommonsItem::Agent(a.clone()));
            }
            CommonsItem::Actor(a) => {
                // Actors emit no standalone TS, but are carried so the
                // emitter can read their schemes for the verification seam.
                emit_items.push(CommonsItem::Actor(a.clone()));
            }
            CommonsItem::Messages(m) => {
                emit_items.push(CommonsItem::Messages(m.clone()));
            }
            CommonsItem::Event(e) => {
                emit_items.push(CommonsItem::Event(e.clone()));
            }
        }
    }
    for type_name in &types_in_this_file {
        if let Some(methods) = local_methods_for_type.get(type_name) {
            for m in methods {
                let already = emit_items.iter().any(|it| match it {
                    CommonsItem::Fn(existing) => match &existing.name {
                        FnName::Method {
                            type_name: t,
                            method_name: n,
                        } => match &m.name {
                            FnName::Method {
                                type_name: t2,
                                method_name: n2,
                            } => t.name == t2.name && n.name == n2.name,
                            _ => false,
                        },
                        _ => false,
                    },
                    _ => false,
                });
                if !already {
                    emit_items.push(CommonsItem::Fn(m.clone()));
                }
            }
        }
    }

    // Synthesize a "Commons-shaped" view of this file's items so we can
    // drive the existing resolver/checker without duplication.
    let synthetic_commons = pf.as_synthetic_commons(emit_items);

    // Cross-context info (v0.6) for contexts: consumed contexts, aliases,
    // services, and types. Computed once below; reused for the resolver,
    // checker, and (in `bynk-emit`) the emitter. v0.18: adapters get it too,
    // so an external provider's `given` resolves against the adapter's
    // flattened consumed capabilities (spec §4.5).
    let cross_context_for_file =
        if let Some((unit_tables, unit_uses, unit_consumes, unit_consumes_aliases)) =
            &ctx.cross_context_views
        {
            let mut cci = build_cross_context_info(
                name,
                unit_consumes,
                unit_consumes_aliases,
                unit_uses,
                unit_tables,
            );
            cci.flattened_caps = unit_info[name].flattened.clone();
            cci
        } else {
            resolver::CrossContextInfo::default()
        };

    // Events slice 3a (#972): this unit's own local + direct-`uses` types
    // (deliberately narrower than `combined_types`, which also merges
    // `consumes`) — the same view `emit_consumed_context_helpers` (#973)
    // builds for a *subscriber* regenerating this unit's own event codecs
    // cross-context. `check_context_declarations` uses it to validate an
    // event field default is constructible in that narrower view, not just
    // this unit's own wider one.
    let subscriber_visible_types: HashMap<String, Arc<TypeDecl>> =
        if let Some((unit_tables, unit_uses, _, _)) = &ctx.cross_context_views {
            combined_types_for(name, unit_tables, unit_uses)
        } else {
            HashMap::new()
        };

    // `ResolvedCommons::new` derives `local_type_names`/`event_type_names`
    // from this unit's own pre-merge table (`unit_info[name].table`), not
    // `combined_types` (already local+uses+consumes merged) — same
    // distinction the caller's `local_names` exists for. `Events.emit[E]`
    // additionally needs "is this specifically an event" on top of
    // owner-only emission (an ordinary local type must not pass as an emit
    // target just because it's locally declared), hence the separate
    // `events` table. Both are empty for a unit absent from `unit_info`.
    let empty_types = HashMap::new();
    let empty_events = HashMap::new();
    let local_table = unit_info.get(name).map(|i| &i.table);
    let local_types = local_table.map(|t| &t.types).unwrap_or(&empty_types);
    let local_events = local_table.map(|t| &t.events).unwrap_or(&empty_events);

    let resolved = ResolvedCommons::new(
        synthetic_commons,
        combined_types.clone(),
        local_types,
        combined_fns.clone(),
        combined_methods.clone(),
        HashMap::new(),
        local_events,
        cross_context_for_file.clone(),
        // ADR 0116 D6: provenance for the `bynk.list` deprecation lint.
        imported_from.clone(),
        kind == UnitKind::Context,
        ctx.uses_commons_type_names.clone(),
    );
    refs.enter_file(&pf.identity_path(), name, pf.is_synthetic());
    // v0.27: synthetic and test/integration files record no hints — neither
    // surfaces in an editor (the `assemble_index` rule).
    hints.enter_file(
        &pf.identity_path(),
        pf.is_synthetic() || matches!(pf.kind(), UnitKind::Test | UnitKind::Integration),
    );
    // v0.31: locals serve completion/navigation in test files too — only
    // synthetic (toolchain-injected) files are muted.
    locals.enter_file(&pf.identity_path(), pf.is_synthetic());
    // v0.99: capability requirements follow the inlay-hint muting rule —
    // synthetic and test/integration files surface none in an editor.
    requirements.enter_file(
        &pf.identity_path(),
        pf.is_synthetic() || matches!(pf.kind(), UnitKind::Test | UnitKind::Integration),
    );
    if let Err(errs) = resolver::resolve_file_record(&resolved, refs) {
        errors.extend_for(Some(&pf.identity_path()), errs);
        return None;
    }
    let rc = checker::check_record_in(resolved, tys, refs, hints, locals, requirements);
    let typed = match rc.result {
        Ok(t) => {
            // v0.89 (ADR 0117): a unit that checks clean may still carry
            // non-failing warnings — push them into the (severity-aware)
            // sink, where they are classified as warnings and never gate.
            if !t.warnings.is_empty() {
                errors.extend_for(Some(&pf.identity_path()), t.warnings.clone());
            }
            t
        }
        Err(errs) => {
            errors.extend_for(Some(&pf.identity_path()), errs);
            // ADR 0094: surface the best-effort partial types the checker
            // computed so `.`-member completion / signature help work on a
            // buffer with an unrelated error. Unconditional now (this
            // module's own doc comment) — a `Mode::Build` caller simply
            // never reads the sink this lands in.
            record_analyse_types(
                exprs,
                &pf.identity_path(),
                pf.is_synthetic(),
                &rc.partial_expr_types,
            );
            return None;
        }
    };

    // Run the context-specific checks: forbidden construction, private-type
    // references.
    if kind == UnitKind::Context {
        let context_check_errs =
            check_context_constraints(&typed, consumed_types, local_names, tys);
        if !context_check_errs.is_empty() {
            errors.extend_for(Some(&pf.identity_path()), context_check_errs);
            record_analyse_types(
                exprs,
                &pf.identity_path(),
                pf.is_synthetic(),
                &typed.expr_types,
            );
            return None;
        }
    }

    // v0.5: check capability/provider/service/agent declarations. v0.18:
    // adapters run these too — an external provider's `given` resolves
    // through the same path as a bodied provider's (the service/agent
    // checks are vacuous for adapters, which have none).
    let mut typed = typed;
    let unit_table_owned = unit_info.get(name).map(|i| i.table.clone());
    if (kind == UnitKind::Context || kind == UnitKind::Adapter)
        && let Some(table) = unit_table_owned.as_ref()
    {
        let decl_errs = check_context_declarations(
            &mut typed,
            table,
            &cross_context_for_file,
            kind == UnitKind::Context,
            &ctx.uses_commons_type_names,
            &subscriber_visible_types,
            refs,
            hints,
            locals,
            requirements,
            tys,
        );
        if !decl_errs.is_empty() {
            // ADR 0117: a warning-severity declaration diagnostic (e.g. the
            // `@indexed` hygiene hints) must not block emission — only an
            // error does. Partition first, then gate on error severity
            // alone.
            let blocks_emission = decl_errs.iter().any(|e| {
                matches!(
                    bynk_syntax::Severity::for_error(e),
                    bynk_syntax::Severity::Error
                )
            });
            errors.extend_for(Some(&pf.identity_path()), decl_errs);
            if blocks_emission {
                // ADR 0094: handler bodies are typed here — surface their
                // best-effort types even when a declaration check (e.g. a
                // service/agent wiring error) fails for the file.
                record_analyse_types(
                    exprs,
                    &pf.identity_path(),
                    pf.is_synthetic(),
                    &typed.expr_types,
                );
                return None;
            }
            // Warnings only: the declarations are valid — fall through.
        }
    }

    Some(FileCheckResult {
        typed,
        cross_context: cross_context_for_file,
    })
}
