//! `wrangler.toml` generation per Worker (v0.8 §4.4 / §4.5).
//!
//! Each context becomes a Cloudflare Worker with its own wrangler config.
//! Service Bindings are declared for every consumed context. Durable
//! Object bindings + `exports` entries are declared for every agent.

use crate::emitter::toml_doc::{TomlBlock, TomlDocument, TomlEntry, TomlValue};
use crate::project::{UnitTable, worker_dir_name};
pub(crate) use bynk_check::worker_bindings::{BindingSource, WorkerBinding};

/// Compile-time pinned compatibility date. Cloudflare uses this to lock
/// Workers runtime behaviour. When to bump it, and the gate a bump must pass,
/// is the policy in `design/bynk-release-discipline.md` ("Part 3 — The Workers
/// compatibility date", #1677): reviewed at each release, and moved only after
/// the workerd smokes pass on the new date.
pub const COMPATIBILITY_DATE: &str = "2026-07-01";

/// #1890: the compatibility flags every emitted Worker carries, ahead of the
/// project's own (`bynk.toml`'s `[workers] compatibility_flags`).
///
/// `global_fetch_strictly_public` makes `fetch()` to a URL go out over the
/// public internet even when the URL is another Worker on the same account
/// or zone. Without it Cloudflare refuses such a fetch (error 1042, or a
/// fast `404` from a `workers.dev` sibling), so a Bynk `Fetch` to a sibling
/// service's public URL failed where the same URL worked from outside. In
/// Bynk, `Fetch` to a URL means over the public internet, which is exactly
/// this flag's behaviour. Worker-to-Worker calls inside one project use
/// Service Bindings, which the flag leaves alone.
///
/// Reviewed with [`COMPATIBILITY_DATE`] (`design/bynk-release-discipline.md`,
/// Part 3): a date bump can turn a flag on by default, which makes it
/// redundant here.
pub const DEFAULT_COMPATIBILITY_FLAGS: &[&str] = &["global_fetch_strictly_public"];

/// #1890: the flags a Worker's `wrangler.toml` carries, and the duplicates
/// dropped on the way. [`DEFAULT_COMPATIBILITY_FLAGS`] come first, then
/// `extra` (the manifest's list) in order. A name already present, whether a
/// default or earlier in `extra`, is dropped and returned in the second list,
/// once per extra occurrence, for the caller to warn about. Names are not
/// otherwise checked.
pub fn compatibility_flags(extra: &[String]) -> (Vec<String>, Vec<String>) {
    let mut flags: Vec<String> = DEFAULT_COMPATIBILITY_FLAGS
        .iter()
        .map(|f| f.to_string())
        .collect();
    let mut duplicates = Vec::new();
    for flag in extra {
        if flags.contains(flag) {
            duplicates.push(flag.clone());
        } else {
            flags.push(flag.clone());
        }
    }
    (flags, duplicates)
}

/// #1732: the oldest wrangler whose bundled `workerd` serves
/// [`COMPATIBILITY_DATE`]. An older `workerd` refuses a newer date outright
/// (`wrangler dev` exits: "This Worker requires compatibility date …"), so
/// `bynk doctor` warns about a wrangler below this. Set by the same
/// compatibility-date review that sets the date (Part 3's lag rule keeps it at
/// least three months old). `bynk-emit/tests/compat_date_policy.rs` checks the
/// policy doc names both.
pub const WRANGLER_MIN: &str = "4.107.0";

/// Events track, slice 0 (spine #936, ADR 0284): the class name of a
/// publishing context's fan-out Durable Object (`emitter::events_fanout`).
/// Shared with `emitter::workers` (the `Env` field name + `deps.
/// __eventsDispatch` call it drives) and `emitter::events_fanout` (the class
/// this name must actually export) so the three can never drift apart.
///
/// Double-underscore-prefixed, matching every other compiler-synthesised
/// identifier in emitted output (`__events`, `__eventsDispatch`,
/// `__makeLedger`, …) — a Bynk `agent` name can never start with `_` (a
/// parse error, checked directly: `agent _Foo { … }` fails with
/// `expected identifier after \`agent\`, found \`_\``), so an agent
/// coincidentally named the same as this synthetic class is structurally
/// impossible, not merely unlikely.
pub(crate) const EVENTS_FANOUT_CLASS_NAME: &str = "__EventsFanout";

/// Deploy-time sentinel in generated Worker configuration. The driver replaces
/// this with the persistent Cloudflare KV namespace id immediately before a
/// remote Wrangler command runs.
pub const KV_NAMESPACE_ID_PLACEHOLDER: &str = "<KV_NAMESPACE_ID>";

/// #1883, slice 1b: the `[assets]` block for a Worker that ships static files.
/// `directory` is the Worker's own assets directory
/// ([`crate::project::WORKER_ASSETS_DIR`]), beside — never containing — its
/// compiled code. The three routing settings are set explicitly rather than
/// inherited (#1883 [DECISION C], checked against the wrangler reference):
///
/// - `run_worker_first = false`: a matching asset is served without running
///   the Worker, which is what makes an asset public by construction.
/// - `not_found_handling = "none"`: a request no asset matches falls through
///   to the Worker, so every handler stays reachable.
/// - `html_handling = "none"`: an asset is served only at its exact path (the
///   default would also serve `foo.html` at `/foo`), so the route-conflict
///   check can compare paths exactly.
pub(crate) fn assets_block() -> TomlBlock {
    TomlBlock::table(
        "assets",
        vec![
            TomlEntry::kv(
                "directory",
                TomlValue::str(format!("./{}", crate::project::WORKER_ASSETS_DIR)),
            ),
            TomlEntry::kv("run_worker_first", TomlValue::Bool(false)),
            TomlEntry::kv("not_found_handling", TomlValue::str("none")),
            TomlEntry::kv("html_handling", TomlValue::str("none")),
        ],
    )
}

/// #1883, slice 1b: the `_headers` file written into a Worker's assets
/// directory beside its files. Every served path carries its content hash, so
/// a file under [`bynk_check::assets::SERVED_PREFIX`] never changes and may
/// be cached for a year. Without it the platform's default is `max-age=0,
/// must-revalidate`. The glob is built from the prefix, so the two cannot
/// drift (review of #1904).
pub(crate) fn asset_headers() -> String {
    format!(
        "{}*\n  Cache-Control: public, max-age=31536000, immutable\n",
        bynk_check::assets::SERVED_PREFIX
    )
}

/// P6.x cutover slice 2 (#1191, narrowed from #1187): `crons`/`queues` arrive
/// pre-collected, sorted and deduped by the caller (`project.rs`'s own
/// `emit_wrangler_toml` call site) rather than being walked here off
/// `table.services`. Both used to match a handler's cron-schedule kind and a
/// service's queue-binding protocol directly, straight off the syntax tree —
/// this file's entire raw-syntax footprint, per #1191's own grounding.
/// Relocating just these two matches removed `wrangler.rs` from the
/// `ast_importers` probe outright — no `bynk_ir` equivalent exists to route
/// through instead: nothing builds a project-wide service IR at the call site
/// (#1191's Framing), and after #1542's Slice D2 nothing builds one anywhere.
///
/// P7.3 (#1303): builds a [`TomlDocument`] rather than writing text directly
/// — the caller (`project.rs`) prints it via `emitter::toml_doc::
/// print_toml_document`. What gets built, in what order, under what gates,
/// is unchanged; only the construction shape moved from direct `writeln!`
/// into pushing typed blocks. String-escaping moved with it: every value
/// below is escaped unconditionally by the printer now, not selectively by
/// this function (`emitter/toml_doc.rs`'s own module doc has the reasoning).
pub(crate) fn emit_wrangler_toml(
    context: &str,
    // #1944: every binding this Worker declares, [`worker_bindings`]' list —
    // the same one its `Env` interface and the duplicate check read.
    bindings: &[WorkerBinding],
    // v0.10a: every `on cron "expr"` schedule in the context, sorted+deduped.
    // Sorting is load-bearing, not cosmetic (review of #1192): the caller
    // walks `table.services`, a `HashMap`, so unsorted input makes
    // `wrangler.toml` non-reproducible across runs.
    crons: &[String],
    // v0.10b/v0.44: every `from queue("name")` service's bound queue name,
    // sorted+deduped (same reproducibility requirement as `crons`).
    queues: &[String],
    // #1890: the merged flag list, [`compatibility_flags`]' first result.
    // The same for every Worker in the project, so the caller merges (and
    // warns about duplicates) once, not per Worker.
    flags: &[String],
) -> TomlDocument {
    let name = worker_dir_name(context);
    let mut doc = TomlDocument::new(
        "Generated by bynkc — do not edit by hand.",
        vec![
            TomlEntry::kv("name", TomlValue::str(name)),
            TomlEntry::kv("main", TomlValue::str("index.ts")),
            TomlEntry::kv("compatibility_date", TomlValue::str(COMPATIBILITY_DATE)),
            TomlEntry::kv(
                "compatibility_flags",
                TomlValue::Array(flags.iter().map(TomlValue::str).collect()),
            ),
        ],
    );

    let mut class_names: Vec<&str> = Vec::new();
    for binding in bindings {
        let name = TomlValue::str(binding.name.clone());
        let block = match &binding.source {
            BindingSource::Service { context } => TomlBlock::array_table(
                "services",
                vec![
                    TomlEntry::kv("binding", name),
                    TomlEntry::kv("service", TomlValue::str(worker_dir_name(context))),
                ],
            ),
            BindingSource::Kv => TomlBlock::array_table(
                "kv_namespaces",
                vec![
                    TomlEntry::kv("binding", name),
                    TomlEntry::with_comment(
                        "id",
                        TomlValue::str(KV_NAMESPACE_ID_PLACEHOLDER),
                        "set at deploy time",
                    ),
                ],
            ),
            BindingSource::Agent { class_name } | BindingSource::EventsFanout { class_name } => {
                class_names.push(class_name);
                TomlBlock::array_table(
                    "durable_objects.bindings",
                    vec![
                        TomlEntry::kv("name", name),
                        TomlEntry::kv("class_name", TomlValue::str(class_name.clone())),
                    ],
                )
            }
        };
        doc.push_block(block);
    }
    // #1796: each class is declared in Cloudflare's declarative `exports`
    // map, not a `[[migrations]]` list. Migrations are picked by tag: Wrangler
    // uploads only the ones after the Worker's applied tag, so the single
    // fixed `v1` this used to emit registered the classes of the *first*
    // deploy and no class added after it (a new agent, or the fan-out class
    // arriving with a context's first `emit`). `exports` has no tag. On every
    // deploy Cloudflare compares the declared set with the Worker's
    // namespaces and creates what's missing, so the config stays a pure
    // function of the source and the ledger still records nothing (ADR 0194
    // D1's principle). Wrangler reads `exports` from 4.107.0, which is
    // [`WRANGLER_MIN`], and `wrangler dev` takes each class's backend from
    // it too.
    //
    // Every class is SQLite-backed (#1779, ADR 0438): the emitted agent uses
    // only `storage.get`/`storage.put` on its single `"state"` key, which a
    // SQLite-backed class supports unchanged, and the fan-out class uses no
    // storage at all. A Worker whose classes an older Bynk created
    // key-value-backed (`new_classes`, ≤ 0.309.10) is refused
    // (`storage_type_mismatch`, per Cloudflare's `exports` docs), because
    // Cloudflare can't change a backend in place. That is a documented
    // pre-1.0 break: such a Worker is torn down and redeployed. No
    // `deleted`/`renamed` tombstone is ever emitted — destroying or moving a
    // class's data is #539's decision. Per Cloudflare's docs, removing an
    // agent then fails the deploy (`orphaned_provisioned_namespace`), which
    // is the loud outcome wanted; that is not yet confirmed against a live
    // account (#1796).
    for class_name in class_names {
        doc.push_block(TomlBlock::keyed_table(
            "exports",
            class_name.to_string(),
            vec![
                TomlEntry::kv("type", TomlValue::str("durable-object")),
                TomlEntry::kv("storage", TomlValue::str("sqlite")),
            ],
        ));
    }

    // v0.10a: cron triggers. Cloudflare uses a single `[triggers]` table with a
    // `crons` array aggregating every `on cron` schedule in the context.
    // Already sorted+deduped by the caller (#1191).
    if !crons.is_empty() {
        let quoted = crons.iter().map(TomlValue::str).collect();
        doc.push_block(TomlBlock::table(
            "triggers",
            vec![TomlEntry::kv("crons", TomlValue::Array(quoted))],
        ));
    }

    // v0.10b: queue consumers. Each `on queue "name"` becomes a
    // `[[queues.consumers]]` binding. Already sorted+deduped by the caller
    // (#1191).
    for name in queues {
        doc.push_block(TomlBlock::array_table(
            "queues.consumers",
            vec![
                TomlEntry::kv("queue", TomlValue::str(name.clone())),
                TomlEntry::kv("max_batch_size", TomlValue::Int(10)),
            ],
        ));
    }

    doc
}

/// Every binding a Worker's `wrangler.toml` declares, in emission order:
/// Service Bindings by target, then the KV namespace, then the Durable
/// Objects by class name. [`emit_wrangler_toml`] writes exactly this list,
/// the Worker's `Env` interface (`emitter::workers`) types it, and the
/// duplicate check (`bynk_check::worker_bindings`) reads it, so a new kind of
/// binding is declared, typed and checked from one place (#1944). Each kind
/// derives its names independently, so two can meet (an agent `Kv` and the KV
/// namespace are both `KV`); names are never adjusted to avoid that, because
/// deployed Workers depend on them. Two members with one name would also be
/// invalid TypeScript in `Env`, so the duplicate check must run first.
pub(crate) fn worker_bindings(
    table: &UnitTable,
    consumes: &[String],
    needs_kv: bool,
    uses_emit: bool,
) -> Vec<WorkerBinding> {
    let mut bindings = Vec::new();
    let mut sorted_consumes: Vec<&String> = consumes.iter().collect();
    sorted_consumes.sort();
    bindings.extend(sorted_consumes.into_iter().map(|t| service_binding(t)));
    if needs_kv {
        bindings.push(kv_binding());
    }
    // Events track, slice 0 (spine #936, ADR 0284): a context whose handlers
    // emit gets its own fan-out DO among the agents' — Cloudflare only cares
    // that `index.ts` (this Worker's `main`) exports a class with this name,
    // not which generated file it came from. Sources by provenance, not by
    // name: an agent that happened to share the fan-out's class name is
    // still an agent.
    let mut objects: Vec<(&str, WorkerBinding)> = table
        .agents
        .keys()
        .map(|class_name| (class_name.as_str(), agent_binding(class_name)))
        .collect();
    if uses_emit {
        objects.push((EVENTS_FANOUT_CLASS_NAME, events_fanout_binding()));
    }
    objects.sort_by(|a, b| a.0.cmp(b.0));
    bindings.extend(objects.into_iter().map(|(_, binding)| binding));
    bindings
}

/// #1950: the bindings a handler's `deps.env` type exposes on Workers — the
/// projection of [`worker_bindings`] handlers reach directly, which
/// `BindingSource`'s variant docs state per kind. `consumed` is the consumed
/// contexts with services, `agents` every local agent when the handler
/// constructs one (else empty); order as in [`worker_bindings`].
///
/// Built by the same constructors, so each name and source is the Worker's
/// own: a consumed context with services is never an adapter (an adapter may
/// not declare a `service`, `bynk.adapter.disallowed_item`), so it has a
/// Service Binding, and every local agent has a Durable Object binding. The
/// KV namespace, the events fan-out and subscribers' Service Bindings never
/// reach handlers.
pub(crate) fn handler_bindings(consumed: &[String], agents: &[String]) -> Vec<WorkerBinding> {
    let mut consumed: Vec<&String> = consumed.iter().collect();
    consumed.sort();
    let mut agents: Vec<&String> = agents.iter().collect();
    agents.sort();
    consumed
        .into_iter()
        .map(|t| service_binding(t))
        .chain(agents.into_iter().map(|a| agent_binding(a)))
        .collect()
}

/// The TypeScript type a binding has on the Worker's `env`, for both its
/// `Env` interface and a handler's `deps.env`.
pub(crate) fn binding_ts_type(source: &BindingSource) -> &'static str {
    match source {
        BindingSource::Service { .. } => "__ServiceBinding",
        BindingSource::Kv => "__KVNamespace",
        BindingSource::Agent { .. } | BindingSource::EventsFanout { .. } => {
            "__DurableObjectNamespace"
        }
    }
}

/// The Service Binding to context `target`'s Worker.
fn service_binding(target: &str) -> WorkerBinding {
    WorkerBinding {
        name: consumed_binding_name(target),
        source: BindingSource::Service {
            context: target.to_string(),
        },
    }
}

/// The KV namespace binding the cloudflare adapter's `Kv` reads.
fn kv_binding() -> WorkerBinding {
    WorkerBinding {
        name: bynk_check::firstparty::KV_BINDING_NAME.to_string(),
        source: BindingSource::Kv,
    }
}

/// The Durable Object binding of agent `class_name`.
fn agent_binding(class_name: &str) -> WorkerBinding {
    WorkerBinding {
        name: agent_binding_name(class_name),
        source: BindingSource::Agent {
            class_name: class_name.to_string(),
        },
    }
}

/// The Durable Object binding of the events fan-out class.
fn events_fanout_binding() -> WorkerBinding {
    WorkerBinding {
        name: agent_binding_name(EVENTS_FANOUT_CLASS_NAME),
        source: BindingSource::EventsFanout {
            class_name: EVENTS_FANOUT_CLASS_NAME.to_string(),
        },
    }
}

/// Service Binding identifier for a consumed context: uppercase with
/// underscores. `commerce.payment` → `COMMERCE_PAYMENT`.
pub(crate) fn consumed_binding_name(target: &str) -> String {
    target.replace('.', "_").to_uppercase()
}

/// Durable Object binding identifier for an agent class. We use the
/// class name in screaming snake case so handlers can grab it by a
/// predictable name (`OrderEntity` → `ORDER_ENTITY`).
pub(crate) fn agent_binding_name(class_name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in class_name.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_uppercase());
    }
    out
}

// ---------------------------------------------------------------------------
// P7.4 (#1305): structural post-emission patches — closes R7.6 ("downstream
// consumers couple to nodes, never to emitted text") and R8.20 ("deploy-time
// placeholders are typed, not textual"). `bynk deploy`/`bynk dev --remote`
// and `--emit js` both need to rewrite one field of an *already-emitted*
// `wrangler.toml` after compilation — not build one from scratch (that's
// `emit_wrangler_toml`/`TomlDocument`, P7.3's own construction-side tree).
//
// This is the read-an-existing-document side, and needs a different tool
// than `TomlDocument`'s own from-scratch builder or a plain `toml::Table`
// parse: both discard everything they don't model (`toml::Table` round-trips
// through a `BTreeMap` with no comment/order trivia at all — a first attempt
// using it here silently dropped the "Generated by bynkc" banner and every
// inline comment, and re-sorted the whole document alphabetically, on every
// patch). `toml_edit::DocumentMut` is format-preserving: parse, mutate the
// one value that actually changed, re-serialise, and everything else survives
// byte-for-byte. That's the real shape "immune to a reformat, scoped to the
// actual field" needs — not just "found the right field," but "touched
// nothing else."
// ---------------------------------------------------------------------------

/// Set `item`'s value to `s`, preserving whatever leading/trailing
/// whitespace and comments already decorated it — e.g. the KV namespace
/// id's own `# set at deploy time` note survives materialisation (stale
/// once set, but that's the same text a caller would have seen under the
/// old substring-replace behaviour too; not this slice's concern to change).
fn set_string_preserving_decor(item: &mut toml_edit::Item, s: &str) {
    let decor = item
        .as_value()
        .map(|v| v.decor().clone())
        .unwrap_or_default();
    let mut new_value = toml_edit::Value::from(s);
    *new_value.decor_mut() = decor;
    *item = toml_edit::Item::Value(new_value);
}

/// Whether a generated `wrangler.toml`'s KV namespace id is still the
/// deploy-time placeholder — the same condition [`materialise_kv_namespace_id`]
/// itself checks internally before writing, exposed separately so a caller
/// can decide whether materialising is needed at all *before* doing the
/// (possibly fallible) work of finding the real id to substitute — e.g.
/// `bynk dev --remote` skipping a lock-file lookup entirely for a project
/// that either has no KV binding or is already materialised.
pub fn wrangler_needs_kv_materialisation(text: &str) -> Result<bool, String> {
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("invalid wrangler.toml: {e}"))?;
    Ok(current_kv_namespace_id(&doc) == Some(KV_NAMESPACE_ID_PLACEHOLDER))
}

/// Materialise the deploy-time KV namespace id placeholder in a generated
/// `wrangler.toml`'s text, structurally. Parses `text` and, if (and only
/// if) the first `[[kv_namespaces]]` entry's `id` is currently *exactly*
/// [`KV_NAMESPACE_ID_PLACEHOLDER`], replaces it with `id`.
///
/// Any other state — no `[[kv_namespaces]]` section at all, or an `id`
/// that's already been materialised to a real value — is a **no-op**,
/// returning `text` unchanged (byte-for-byte: the document is never
/// re-serialised in this branch, so there's nothing for a caller to
/// mistakenly treat as a real write). This mirrors the exact gate the old
/// `text.contains(KV_NAMESPACE_ID_PLACEHOLDER)` check enforced; it is not a
/// general "always overwrite with the given id" operation. Widening it into
/// a re-provisioning operation is a different feature, out of scope here
/// (#1305's own Decision B).
pub fn materialise_kv_namespace_id(text: &str, id: &str) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("invalid wrangler.toml: {e}"))?;
    if current_kv_namespace_id(&doc) != Some(KV_NAMESPACE_ID_PLACEHOLDER) {
        return Ok(text.to_string());
    }
    let item = doc
        .get_mut("kv_namespaces")
        .and_then(|i| i.as_array_of_tables_mut())
        .and_then(|arr| arr.get_mut(0))
        .and_then(|t| t.get_mut("id"))
        .expect("current_kv_namespace_id returned Some, so this navigation cannot fail");
    set_string_preserving_decor(item, id);
    Ok(doc.to_string())
}

/// The first `[[kv_namespaces]]` entry's `id`, if the stanza exists at all.
fn current_kv_namespace_id(doc: &toml_edit::DocumentMut) -> Option<&str> {
    doc.get("kv_namespaces")?
        .as_array_of_tables()?
        .get(0)?
        .get("id")?
        .as_str()
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    /// A full, realistic `wrangler.toml` — every stanza `emit_wrangler_toml`
    /// can produce, in its real order, with its real banner and inline
    /// comment (mirrors `bynkc/tests/fixtures/positive/
    /// 372_kv_agent_queue_workers/expected/workers/ops-hub/wrangler.toml`
    /// byte-for-byte). Review of #1305, finding 2: every test below asserts
    /// **exact** patched text against this fixture, not just "the changed
    /// field has the right value after a re-parse" — a re-parse is blind by
    /// construction to whatever the printer discarded, which is exactly how
    /// finding 1 (the whole-document alphabetical re-sort, dropped banner)
    /// survived a green suite the first time around.
    const REPRESENTATIVE: &str = "\
# Generated by bynkc — do not edit by hand.
name = \"ops-hub\"
main = \"index.ts\"
compatibility_date = \"2026-07-01\"
compatibility_flags = [\"global_fetch_strictly_public\"]

[[services]]
binding = \"PAYMENT\"
service = \"payment\"

[[kv_namespaces]]
binding = \"BYNK_KV\"
id = \"<KV_NAMESPACE_ID>\" # set at deploy time

[[durable_objects.bindings]]
name = \"JOB_LEDGER\"
class_name = \"JobLedger\"

[exports.JobLedger]
type = \"durable-object\"
storage = \"sqlite\"

[[queues.consumers]]
queue = \"job-intake\"
max_batch_size = 10
";

    #[test]
    fn materialise_kv_namespace_id_changes_only_the_id() {
        let expected = REPRESENTATIVE.replacen("<KV_NAMESPACE_ID>", "abc123", 1);
        assert_eq!(
            materialise_kv_namespace_id(REPRESENTATIVE, "abc123").unwrap(),
            expected
        );
        // The `# set at deploy time` note survives — the old text-replace
        // behaviour left it in place too (it only replaced the placeholder
        // substring, not the whole line), and this slice isn't the one
        // deciding whether that note should still be there post-materialisation.
        assert!(expected.contains("id = \"abc123\" # set at deploy time"));
    }

    #[test]
    fn materialise_kv_namespace_id_is_immune_to_reformatting() {
        let text = "[[kv_namespaces]]\nbinding = \"BYNK_KV\"\nid = \"<KV_NAMESPACE_ID>\" # set at deploy time\n";
        let patched = materialise_kv_namespace_id(text, "abc123").unwrap();
        assert_eq!(
            patched,
            "[[kv_namespaces]]\nbinding = \"BYNK_KV\"\nid = \"abc123\" # set at deploy time\n"
        );
    }

    #[test]
    fn materialise_kv_namespace_id_is_a_no_op_without_a_kv_namespaces_section() {
        let text = "name = \"api\"\nmain = \"index.ts\"\n";
        let patched = materialise_kv_namespace_id(text, "abc123").unwrap();
        assert_eq!(patched, text);
    }

    #[test]
    fn materialise_kv_namespace_id_is_a_no_op_once_already_materialised() {
        let text = "[[kv_namespaces]]\nbinding = \"BYNK_KV\"\nid = \"already-real\"\n";
        let patched = materialise_kv_namespace_id(text, "abc123").unwrap();
        assert_eq!(patched, text);
    }

    #[test]
    fn wrangler_needs_kv_materialisation_matches_materialise_kv_namespace_ids_own_gate() {
        let placeholder = "[[kv_namespaces]]\nid = \"<KV_NAMESPACE_ID>\"\n";
        assert!(wrangler_needs_kv_materialisation(placeholder).unwrap());

        let no_kv = "name = \"api\"\n";
        assert!(!wrangler_needs_kv_materialisation(no_kv).unwrap());

        let already_real = "[[kv_namespaces]]\nid = \"already-real\"\n";
        assert!(!wrangler_needs_kv_materialisation(already_real).unwrap());
    }

    #[test]
    fn wrangler_needs_kv_materialisation_rejects_invalid_toml() {
        assert!(wrangler_needs_kv_materialisation("not = valid = toml = at = all").is_err());
    }

    #[test]
    fn materialise_kv_namespace_id_rejects_invalid_toml() {
        assert!(materialise_kv_namespace_id("not = valid = toml = at = all", "abc123").is_err());
    }
}

#[cfg(test)]
mod compatibility_flags_tests {
    use super::*;

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// #1890: the default set alone, when the manifest adds nothing.
    #[test]
    fn the_default_is_global_fetch_strictly_public() {
        assert_eq!(
            DEFAULT_COMPATIBILITY_FLAGS,
            ["global_fetch_strictly_public"]
        );
        let (flags, duplicates) = compatibility_flags(&[]);
        assert_eq!(flags, ["global_fetch_strictly_public"]);
        assert!(duplicates.is_empty());
    }

    /// #1890: the manifest's flags follow the default, in manifest order;
    /// names are not checked.
    #[test]
    fn extra_flags_follow_the_default_in_order() {
        let (flags, duplicates) =
            compatibility_flags(&strings(&["nodejs_compat", "a_future_flag"]));
        assert_eq!(
            flags,
            [
                "global_fetch_strictly_public",
                "nodejs_compat",
                "a_future_flag"
            ]
        );
        assert!(duplicates.is_empty());
    }

    /// #1890: a repeat, of a default or of an earlier extra, is dropped and
    /// reported once per repeat.
    #[test]
    fn duplicates_are_dropped_and_reported() {
        let (flags, duplicates) = compatibility_flags(&strings(&[
            "nodejs_compat",
            "global_fetch_strictly_public",
            "nodejs_compat",
            "nodejs_compat",
        ]));
        assert_eq!(flags, ["global_fetch_strictly_public", "nodejs_compat"]);
        assert_eq!(
            duplicates,
            [
                "global_fetch_strictly_public",
                "nodejs_compat",
                "nodejs_compat"
            ]
        );
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use bynk_check::worker_bindings::duplicate_bindings;

    fn names(bindings: &[WorkerBinding]) -> Vec<&str> {
        bindings.iter().map(|b| b.name.as_str()).collect()
    }

    /// Emission order: Service Bindings by target, the KV namespace, then
    /// the Durable Objects by class name, the fan-out among them.
    #[test]
    fn worker_bindings_lists_every_kind_in_emission_order() {
        let table = UnitTable::default();
        let bindings = worker_bindings(
            &table,
            &["shop.payment".to_string(), "audit".to_string()],
            true,
            true,
        );
        assert_eq!(
            names(&bindings),
            ["AUDIT", "SHOP_PAYMENT", "KV", "___EVENTS_FANOUT"]
        );
        assert!(duplicate_bindings(&bindings).is_empty());
    }

    /// #1950: the handler projection is drawn from the Worker's own list —
    /// each binding identical, name and source — and leaves out the KV
    /// namespace and the events fan-out.
    #[test]
    fn handler_bindings_are_a_subset_of_the_worker_bindings() {
        let table = UnitTable::default();
        let worker = worker_bindings(
            &table,
            &["shop.payment".to_string(), "audit".to_string()],
            true,
            true,
        );
        let handler = handler_bindings(&["shop.payment".to_string()], &[]);
        assert_eq!(names(&handler), ["SHOP_PAYMENT"]);
        for binding in &handler {
            assert!(worker.contains(binding), "{binding:?} not in {worker:?}");
        }
        assert_eq!(
            handler_bindings(&[], &["OrderEntity".to_string()]),
            [WorkerBinding {
                name: "ORDER_ENTITY".to_string(),
                source: BindingSource::Agent {
                    class_name: "OrderEntity".to_string()
                },
            }]
        );
    }

    /// A consumed context `kv` and the KV namespace are both `KV`: the
    /// later one is paired with the earlier.
    #[test]
    fn a_shared_name_pairs_the_later_binding_with_the_earlier() {
        let table = UnitTable::default();
        let bindings = worker_bindings(&table, &["kv".to_string()], true, false);
        let duplicates = duplicate_bindings(&bindings);
        assert_eq!(duplicates.len(), 1);
        let (earlier, later) = duplicates[0];
        assert_eq!(
            earlier.source,
            BindingSource::Service {
                context: "kv".to_string()
            }
        );
        assert_eq!(later.source, BindingSource::Kv);
    }
}
