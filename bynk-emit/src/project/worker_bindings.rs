//! Per-Worker binding inputs, and the check that a Worker's binding names are
//! unique (`bynk.context.duplicate_binding`).
//!
//! A Worker's bindings come from three derivations that know nothing of each
//! other: the KV namespace's fixed [`firstparty::KV_BINDING_NAME`], a
//! consumed context's [`consumed_binding_name`](crate::emitter::wrangler::consumed_binding_name)
//! (`a.b` → `A_B`) and an agent's [`agent_binding_name`](crate::emitter::wrangler::agent_binding_name)
//! (`OrderEntity` → `ORDER_ENTITY`). Two of them can meet: an agent `Kv` in a
//! context that consumes `bynk.cloudflare { Kv }` is `KV` twice. Wrangler takes
//! such a config without complaint and one binding hides the other, so the
//! clash is a compile error. The names themselves are never adjusted to
//! avoid it, because deployed Workers depend on them.
//!
//! The inputs here are the ones the Workers build passes to
//! `emit_wrangler_toml`, built by the same helpers, so the check sees exactly
//! the bindings the build would write.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use bynk_check::checker::{Callee, ExprId};
use bynk_check::firstparty::{self, Platform};
use bynk_syntax::ast::{CommonsItem, SourceUnit};
use bynk_syntax::error::CompileError;
use bynk_syntax::span::Span;

use super::{
    AdapterBinding, BuildTarget, ErrorSink, ParsedFile, UnitKind, UnitTable,
    native_platforms_of_context, unit_table_uses_emit,
};
use crate::emitter::wrangler::{BindingSource, WorkerBinding, duplicate_bindings, worker_bindings};

/// v0.19 (decision 0025): each context whose in-process closure reaches a
/// platform-native unit, with those platforms. Drives env threading (bundle)
/// and the per-Worker Env/`wrangler.toml` resource derivation (workers).
pub(super) fn contexts_native(
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
) -> HashMap<String, BTreeMap<Platform, String>> {
    kinds
        .iter()
        .filter(|(_, k)| **k == UnitKind::Context)
        .filter_map(|(name, _)| {
            let table = unit_tables.get(name)?;
            let native = native_platforms_of_context(
                name,
                table,
                unit_tables,
                unit_consumes,
                unit_consumes_aliases,
                unit_flattened,
            );
            (!native.is_empty()).then(|| (name.clone(), native))
        })
        .collect()
}

/// v0.19 (C1): a Worker needs the KV namespace binding when its in-process
/// closure reaches the cloudflare adapter.
pub(super) fn needs_kv(
    context_native: &HashMap<String, BTreeMap<Platform, String>>,
    ctx: &str,
) -> bool {
    context_native
        .get(ctx)
        .is_some_and(|n| n.values().any(|u| u == firstparty::CLOUDFLARE_UNIT))
}

/// The contexts a Worker holds a Service Binding to: those it consumes,
/// less adapters (not Workers, so no binding), plus every context subscribing
/// to an event it publishes (Events track, slice 0, spine #936, ADR 0284). A
/// subscriber `consumes` the publisher for the event *type*; nothing upstream
/// gives the publisher a binding back to the subscriber, so its Worker needs
/// one added here: the reverse direction of an ordinary `consumes` edge.
pub(super) fn service_consumes(
    consumes_targets: &[String],
    adapter_bindings: &HashMap<String, AdapterBinding>,
    own_event_routes: &BTreeMap<String, Vec<(String, String)>>,
) -> Vec<String> {
    let mut targets: BTreeSet<String> = consumes_targets
        .iter()
        .filter(|t| !adapter_bindings.contains_key(*t))
        .cloned()
        .collect();
    targets.extend(
        own_event_routes
            .values()
            .flatten()
            .map(|(sub_ctx, _)| sub_ctx.clone()),
    );
    targets.into_iter().collect()
}

/// This context's own published events → their subscribers, sliced from the
/// project-wide table.
pub(super) fn own_event_routes(
    ctx: &str,
    event_subscribers: &BTreeMap<(String, String), Vec<(String, String)>>,
) -> BTreeMap<String, Vec<(String, String)>> {
    event_subscribers
        .iter()
        .filter(|((owner, _), _)| owner == ctx)
        .map(|((_, name), subs)| (name.clone(), subs.clone()))
        .collect()
}

/// `bynk.context.duplicate_binding`: on a Workers build, each context's
/// Worker must declare every binding name once. One error per clash, at the
/// declaration that brought in the second binding (an agent, or a `consumes`
/// clause), naming both sources. Run on an otherwise clean program: the KV
/// input walks the provider graph, whose acyclicity the earlier checks
/// establish.
#[allow(clippy::too_many_arguments)]
pub(super) fn phase_duplicate_bindings(
    target: BuildTarget,
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    unit_callees: &HashMap<String, HashMap<ExprId, Callee>>,
    adapter_bindings: &HashMap<String, AdapterBinding>,
    errors: &mut ErrorSink,
) {
    if target != BuildTarget::Workers {
        return;
    }
    let context_native = contexts_native(
        kinds,
        unit_tables,
        unit_consumes,
        unit_consumes_aliases,
        unit_flattened,
    );
    let event_subscribers =
        bynk_check::symbols::discover_event_subscribers(unit_tables, unit_consumes);
    for (ctx, kind) in kinds {
        if *kind != UnitKind::Context {
            continue;
        }
        let Some(table) = unit_tables.get(ctx) else {
            continue;
        };
        let consumes = service_consumes(
            unit_consumes
                .get(ctx)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            adapter_bindings,
            &own_event_routes(ctx, &event_subscribers),
        );
        let bindings = worker_bindings(
            table,
            &consumes,
            needs_kv(&context_native, ctx),
            unit_table_uses_emit(table, unit_callees.get(ctx)),
        );
        let indices = groups.get(ctx).map(Vec::as_slice).unwrap_or_default();
        for (earlier, later) in duplicate_bindings(&bindings) {
            let Some((file, span)) = declared_at(parsed, indices, later)
                .or_else(|| declared_at(parsed, indices, earlier))
                .or_else(|| context_header(parsed, indices))
            else {
                continue;
            };
            errors.push_for(
                Some(&file),
                CompileError::new(
                    "bynk.context.duplicate_binding",
                    span,
                    format!(
                        "the Worker for `{ctx}` would declare two bindings named `{}`: {} and {}",
                        later.name,
                        earlier.source.describe(),
                        later.source.describe(),
                    ),
                )
                .with_note(
                    "a Worker's bindings share one `env`, so each name must be unique; binding names follow agent and context names, so rename the agent or context behind one of them",
                ),
            );
        }
    }
}

/// Where the context declares the source of `binding`: an agent's name, or
/// the `consumes` clause naming a context (or `bynk.cloudflare`, for the KV
/// namespace). `None` for a source with no declaration of its own: the events
/// fan-out, or a Service Binding added for an event subscriber.
fn declared_at(
    parsed: &[ParsedFile],
    indices: &[usize],
    binding: &WorkerBinding,
) -> Option<(PathBuf, Span)> {
    indices.iter().find_map(|&i| {
        let SourceUnit::Context(c) = parsed[i].unit() else {
            return None;
        };
        let consumed = |target: &str| {
            c.consumes
                .iter()
                .find(|d| d.target.joined() == target)
                .map(|d| d.span)
        };
        let span = match &binding.source {
            BindingSource::Agent { class_name } => c.items.iter().find_map(|item| match item {
                CommonsItem::Agent(a) if a.name.name == *class_name => Some(a.name.span),
                _ => None,
            }),
            BindingSource::Service { context } => consumed(context),
            BindingSource::Kv => consumed(firstparty::CLOUDFLARE_UNIT),
            BindingSource::EventsFanout { .. } => None,
        }?;
        Some((parsed[i].identity_path(), span))
    })
}

/// The context's name in its first file: where a clash between two sources
/// with no declaration of their own is reported.
fn context_header(parsed: &[ParsedFile], indices: &[usize]) -> Option<(PathBuf, Span)> {
    indices.iter().find_map(|&i| match parsed[i].unit() {
        SourceUnit::Context(c) => Some((parsed[i].identity_path(), c.name.span)),
        _ => None,
    })
}
