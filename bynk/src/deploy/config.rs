use super::*;

// ---------------------------------------------------------------------------
// What one context declares: the resources to provision and the bindings to
// order against (slices 1 and 2)
// ---------------------------------------------------------------------------

/// The stanzas of a generated `wrangler.toml` that `deploy` acts on.
///
/// Everything here is read from the **emitted config** rather than from the
/// checker's project model because this is precisely the file wrangler is about
/// to send and Cloudflare is about to validate — so the plan and the upload can
/// never describe different projects (ADR 0193 D3, extended to slice 1's kinds).
#[derive(Debug, Default, Deserialize)]
struct WranglerConfig {
    #[serde(default)]
    services: Vec<ServiceBinding>,
    #[serde(default)]
    queues: QueueBindings,
    #[serde(default)]
    migrations: Vec<Migration>,
    // P7.4 (#1305): read structurally alongside everything else this struct
    // already deserialises, replacing `needs_kv`'s own former
    // `text.contains(KV_NAMESPACE_ID_PLACEHOLDER)` whole-file substring
    // search (R8.20 — deploy-time placeholders are typed, not textual).
    #[serde(default)]
    kv_namespaces: Vec<KvNamespaceBinding>,
}

#[derive(Debug, Deserialize)]
struct ServiceBinding {
    /// The *target* worker's name (`worker_dir_name`, dots dasherised).
    service: String,
}

#[derive(Debug, Deserialize)]
struct KvNamespaceBinding {
    // Presence of the stanza, not its `id`, is what `needs_kv` asks — the
    // `id` itself is only ever read/written structurally by
    // `bynk_emit::emitter::wrangler::materialise_kv_namespace_id`.
}

#[derive(Debug, Default, Deserialize)]
struct QueueBindings {
    #[serde(default)]
    consumers: Vec<QueueConsumer>,
}

#[derive(Debug, Deserialize)]
struct QueueConsumer {
    /// The queue's user-given name, straight from `from queue("n")`.
    queue: String,
}

#[derive(Debug, Deserialize)]
struct Migration {
    tag: String,
}

/// The provisioning surface one context's closure locks it to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Resources {
    /// The workers this one binds to — the edges the deploy order respects.
    /// Adapters are already excluded upstream (they are not Workers), so every
    /// edge here names a real worker directory.
    pub(crate) binds_to: Vec<String>,
    /// The queues this context consumes, by name. Created before the push
    /// (ADR 0194 D3): a `[[queues.consumers]]` binding whose queue does not
    /// exist fails the deploy.
    pub(crate) queues: Vec<String>,
    /// The migration tag `wrangler deploy` will apply, if the context has an
    /// agent. **Advisory** — Cloudflare owns the applied-migration record, so
    /// this says what will be asked for, never what is already true
    /// (ADR 0194 D1).
    pub(crate) migration: Option<String>,
    /// Still carries the KV placeholder, i.e. needs a namespace.
    pub(crate) needs_kv: bool,
    /// Slice 3: the secret names this context's handlers will read from `env` —
    /// an `actor`'s `auth` secret, read from the emitted `bynk-secrets.json`.
    ///
    /// A **floor, not a census** (ADR 0195 D2): an empty set means "the compiler
    /// proved nothing required", never "this context needs no secret".
    pub(crate) declared_secrets: Vec<String>,
    /// Slice 3 follow-up (ADR 0196): the literal `bynk.Secrets` names this
    /// context reads. **Advisory** — `Secrets.get` returns `Option`, so absence
    /// is a legitimate handled outcome and a missing value warns rather than
    /// failing.
    pub(crate) read_secrets: Vec<String>,
    /// False when the context names a secret with a computed expression, so
    /// `read_secrets` is not everything. The plan must say so rather than
    /// presenting its list as whole.
    pub(crate) reads_complete: bool,
}

impl Default for Resources {
    /// Hand-written for one field: `reads_complete` is **true** by vacuity — a
    /// context that names no secret has not computed one — whereas `bool`'s
    /// default is false, which would say the opposite. A derived `Default` here
    /// would make every resource-less context claim an incomplete list.
    fn default() -> Self {
        Self {
            binds_to: Vec::new(),
            queues: Vec::new(),
            migration: None,
            needs_kv: false,
            declared_secrets: Vec::new(),
            read_secrets: Vec::new(),
            reads_complete: true,
        }
    }
}

/// The emitted `bynk-secrets.json` (ADR 0195 D5) — the seam that carries the
/// compiler's knowledge to the driver across both compile paths, including the
/// shelled-`bynkc` one where there is no in-memory model to consult.
#[derive(Debug, Deserialize)]
struct SecretsManifest {
    version: u32,
    #[serde(default)]
    declared: Vec<String>,
    /// v0.173 (ADR 0196): literal `bynk.Secrets` names this context reads.
    /// **Advisory** — `Secrets.get` returns `Option`, so absence is a legitimate
    /// handled outcome and a missing value warns rather than failing.
    #[serde(default)]
    read: Vec<String>,
    /// False when the context names at least one secret with a computed
    /// expression, so `read` is not everything. Defaulted `true` **only**
    /// because the version guard already refuses a manifest that predates the
    /// field — no v1 file reaches this.
    #[serde(default = "read_complete_default")]
    read_complete: bool,
}

fn read_complete_default() -> bool {
    true
}

/// The manifest schema this driver understands.
///
/// **2** (v0.173): `read` + `read_complete`. Refusing v1 rather than reading it
/// with defaults is the point — a v1 manifest carries no evidence about computed
/// names, and assuming `read_complete: true` for it would make the driver claim
/// a completeness nothing established. A stale build tree is re-compiled by the
/// same command that reads it, so refusing costs nothing real.
const SECRETS_MANIFEST_VERSION: u32 = 2;

/// What the emitted manifest says about one context's secrets.
///
/// An absent file is the empty answer, not an error: a context with no secrets
/// of any kind emits none. An *unreadable* or *unparseable* one is an error — it
/// is emitted alongside the config we just read, so a failure there means the
/// build tree is damaged rather than merely old, and guessing would risk
/// skipping a secret the Worker fail-closes without.
fn read_secrets_manifest(worker_dir: &Path) -> Result<SecretsManifest, String> {
    let path = worker_dir.join(bynk_emit::emitter::secrets::SECRETS_MANIFEST);
    if !path.exists() {
        return Ok(SecretsManifest {
            version: SECRETS_MANIFEST_VERSION,
            declared: Vec::new(),
            read: Vec::new(),
            // Vacuously: a context that emits no manifest names no secret at
            // all, computed or otherwise.
            read_complete: true,
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let manifest: SecretsManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if manifest.version != SECRETS_MANIFEST_VERSION {
        return Err(format!(
            "unsupported {} version {}",
            bynk_emit::emitter::secrets::SECRETS_MANIFEST,
            manifest.version
        ));
    }
    Ok(manifest)
}

/// Read everything `deploy` acts on out of one worker's generated build output.
fn read_resources(config: &Path) -> Result<Resources, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    let parsed: WranglerConfig = toml::from_str(&text).map_err(|e| e.to_string())?;
    // Beside the config, not in it: secrets are a runtime store rather than
    // configuration, so `wrangler.toml` has no stanza to carry them.
    let secrets = read_secrets_manifest(
        config
            .parent()
            .ok_or_else(|| "configuration has no directory".to_string())?,
    )?;
    let mut queues: Vec<String> = parsed
        .queues
        .consumers
        .into_iter()
        .map(|c| c.queue)
        .collect();
    // The emitter already sorts and dedups, but a queue is created by name and
    // creating one twice is pure noise — so make it this reader's property
    // rather than an emitter detail deploy happens to inherit.
    queues.sort();
    queues.dedup();
    Ok(Resources {
        binds_to: parsed.services.into_iter().map(|s| s.service).collect(),
        queues,
        // Wrangler applies a config's migrations in order, so the *last* tag is
        // the state a successful push leaves behind. v1 emits exactly one block
        // (`tag = "v1"`), which makes this the same answer by a rule that still
        // holds if that ever changes.
        migration: parsed.migrations.into_iter().next_back().map(|m| m.tag),
        needs_kv: !parsed.kv_namespaces.is_empty(),
        declared_secrets: secrets.declared,
        read_secrets: secrets.read,
        reads_complete: secrets.read_complete,
    })
}

/// Read the whole project's declared resources, one config read per worker.
pub(crate) fn project_resources(
    workers_dir: &Path,
    workers: &[String],
) -> Result<BTreeMap<String, Resources>, String> {
    let mut all = BTreeMap::new();
    for worker in workers {
        let resources = read_resources(&workers_dir.join(worker).join("wrangler.toml"))
            .map_err(|e| format!("could not read the configuration for `{worker}`: {e}"))?;
        all.insert(worker.clone(), resources);
    }
    Ok(all)
}

/// v0.177 (#643): the contract manifest a Worker's build emitted.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ContractsManifest {
    #[serde(default)]
    version: u32,
    /// This context's own hash per `on call` service — what its entry enforces.
    #[serde(default)]
    pub(crate) provides: BTreeMap<String, String>,
    /// This context's compiled view of each dependency's contract — the hash it
    /// stamps at each call site, keyed by the dependency's qualified name.
    #[serde(default)]
    pub(crate) expects: BTreeMap<String, BTreeMap<String, String>>,
}

const CONTRACTS_MANIFEST_VERSION: u32 = 1;

/// Read one Worker's contract manifest.
///
/// An absent file is the empty answer, not an error: a context that neither
/// exposes nor calls an `on call` service emits none, and a build tree from a
/// compiler predating this file has none either. An *unreadable* or
/// *unparseable* one is an error — it sits beside the config we just read, so a
/// failure there means the tree is damaged rather than merely old.
pub(crate) fn read_contracts_manifest(worker_dir: &Path) -> Result<ContractsManifest, String> {
    let path = worker_dir.join(bynk_emit::emitter::contracts::CONTRACTS_MANIFEST);
    if !path.exists() {
        return Ok(ContractsManifest::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let manifest: ContractsManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if manifest.version != CONTRACTS_MANIFEST_VERSION {
        return Err(format!(
            "unsupported {} version {}",
            bynk_emit::emitter::contracts::CONTRACTS_MANIFEST,
            manifest.version
        ));
    }
    Ok(manifest)
}

// P7.4 (#1305): a thin wrapper over `bynk_emit::emitter::wrangler::
// materialise_kv_namespace_id` — the structural read-modify-write closing
// R7.6/R8.20. `-> bool` and the read-then-write shape are kept unchanged:
// both callers (`deploy/provisioning.rs`, `deploy/ledger.rs`) already treat
// `false` uniformly as "could not write" and don't need to distinguish read
// failure from a malformed file from a write failure.
pub(crate) fn materialise_kv_id(path: &Path, id: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(patched) = bynk_emit::emitter::wrangler::materialise_kv_namespace_id(&text, id) else {
        return false;
    };
    // Review of #1305, finding 3: `materialise_kv_namespace_id` returning
    // `text` unchanged (no `[[kv_namespaces]]` at all, or already
    // materialised) is a no-op by design — skip the write rather than
    // reopening the file to write back what's already there, restoring the
    // old `materialise_kv_id`'s own contract (a guaranteed `true`, no write
    // attempted, for a project that needed nothing materialised).
    if patched == text {
        return true;
    }
    std::fs::write(path, patched).is_ok()
}

/// The environment-qualified physical name for a resource whose logical
/// identity is chosen by the source (`on queue "n"`) or the compiler (a
/// target context's `worker_dir_name`) — slice 4, DECISION C.
///
/// `"default"` is unqualified, which is the property that keeps a plain
/// `bynk deploy` byte-for-byte unchanged from before this slice. Any other
/// environment gets Cloudflare's own suffix shape (`<name>-<env>`) — not a
/// free choice for a Service Binding target: Cloudflare auto-suffixes a
/// Worker's *own* deployed name the same way under `--env`, so a binding must
/// match it exactly to resolve. Queues are not Cloudflare-mandated to follow
/// this shape, but using the same one keeps the two resource kinds — and the
/// mental model — consistent.
pub(crate) fn env_qualify(environment: &str, name: &str) -> String {
    if environment == "default" {
        name.to_string()
    } else {
        format!("{name}-{environment}")
    }
}

/// Synthesise a `[env.<name>]` table covering every binding Wrangler does not
/// inherit into a named environment, and append it to the generated config —
/// slice 4, DECISIONs B and C combined.
///
/// **Why this exists at all.** Confirmed against Cloudflare's own docs:
/// bindings (`kv_namespaces`, `queues.consumers`, `durable_objects`,
/// `services`) are *non-inheritable* — a bare `--env staging` against the
/// flat config `emit_wrangler_toml` writes would deploy with **zero**
/// bindings. `emit_wrangler_toml` cannot fix this itself: it runs at compile
/// time, before any `--env` is known (there is no `environment` concept
/// anywhere in `bynk-syntax`/`bynk-check`), so the driver is the only place
/// this can live.
///
/// **Why this appends rather than edits.** The top-level stanza must stay
/// exactly as emitted — it continues to serve the plain, no-`--env`
/// `bynk deploy`. So this parses the config only to *read* the values it
/// needs (via `toml::Table`, not the narrow read-only `WranglerConfig`/
/// `ServiceBinding`/`QueueConsumer` structs above, which drop fields — e.g.
/// `ServiceBinding` has no `binding`, `Migration` has no `new_classes` — that
/// must be copied byte-for-byte), builds a *separate* `{ env: { <name>: … } }`
/// table, serialises only that fragment (so TOML string-escaping is the
/// `toml` crate's job, not a hand-rolled duplicate of
/// `bynk-emit`'s private `escape_toml_basic_string`), and appends the result
/// as text. The original bytes are never touched.
///
/// Queue names and Service Binding targets are environment-qualified
/// ([`env_qualify`]); KV gets the resolved id for this environment; Durable
/// Object bindings, migrations, and cron triggers carry no per-environment
/// identity and are copied verbatim.
pub(crate) fn synthesise_environment_block(
    config_text: &str,
    environment: &str,
    kv_id: Option<&str>,
) -> Result<String, String> {
    let doc: toml::Table = config_text
        .parse()
        .map_err(|e| format!("generated configuration is not valid TOML: {e}"))?;

    let mut env_block = toml::Table::new();

    if let Some(id) = kv_id
        && let Some(toml::Value::Array(namespaces)) = doc.get("kv_namespaces")
    {
        let mut namespaces = namespaces.clone();
        for ns in &mut namespaces {
            if let toml::Value::Table(t) = ns {
                t.insert("id".to_string(), toml::Value::String(id.to_string()));
            }
        }
        env_block.insert("kv_namespaces".to_string(), toml::Value::Array(namespaces));
    }

    if let Some(mut queues) = doc.get("queues").cloned() {
        if let toml::Value::Table(q) = &mut queues
            && let Some(toml::Value::Array(consumers)) = q.get_mut("consumers")
        {
            for consumer in consumers.iter_mut() {
                if let toml::Value::Table(t) = consumer
                    && let Some(name) = t
                        .get("queue")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned)
                {
                    t.insert(
                        "queue".to_string(),
                        toml::Value::String(env_qualify(environment, &name)),
                    );
                }
            }
        }
        env_block.insert("queues".to_string(), queues);
    }

    if let Some(toml::Value::Array(services)) = doc.get("services") {
        let mut services = services.clone();
        for service in &mut services {
            if let toml::Value::Table(t) = service
                && let Some(target) = t
                    .get("service")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            {
                t.insert(
                    "service".to_string(),
                    toml::Value::String(env_qualify(environment, &target)),
                );
            }
        }
        env_block.insert("services".to_string(), toml::Value::Array(services));
    }

    for key in ["durable_objects", "migrations", "triggers"] {
        if let Some(value) = doc.get(key) {
            env_block.insert(key.to_string(), value.clone());
        }
    }

    if env_block.is_empty() {
        // Nothing this context needs bound under this environment — leave the
        // config exactly as emitted rather than appending an empty table.
        return Ok(config_text.to_string());
    }

    let mut env_table = toml::Table::new();
    env_table.insert(environment.to_string(), toml::Value::Table(env_block));
    let mut wrapper = toml::Table::new();
    wrapper.insert("env".to_string(), toml::Value::Table(env_table));

    let appended = toml::to_string_pretty(&wrapper)
        .map_err(|e| format!("could not serialise the `[env.{environment}]` block: {e}"))?;
    Ok(format!("{config_text}\n{appended}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A fluent literal for what one context declares, so a test names only the
    /// resources it is about: `Resources::default().needs_kv().migrates("v1")`.
    impl Resources {
        pub(crate) fn binds(mut self, targets: &[&str]) -> Self {
            self.binds_to = names(targets);
            self
        }
        pub(crate) fn consumes(mut self, queues: &[&str]) -> Self {
            self.queues = names(queues);
            self
        }
        pub(crate) fn migrates(mut self, tag: &str) -> Self {
            self.migration = Some(tag.to_string());
            self
        }
        pub(crate) fn needs_kv(mut self) -> Self {
            self.needs_kv = true;
            self
        }
        /// The secret names the emitted manifest carries — an `actor`'s `auth`
        /// secret, which the compiler proved this Worker reads.
        pub(crate) fn declares(mut self, secrets: &[&str]) -> Self {
            self.declared_secrets = names(secrets);
            self
        }
        /// Literal `bynk.Secrets` names the compiler saw this Worker read.
        pub(crate) fn reads(mut self, secrets: &[&str]) -> Self {
            self.read_secrets = names(secrets);
            self
        }
        /// This context names at least one secret with a computed expression.
        pub(crate) fn reads_incompletely(mut self) -> Self {
            self.reads_complete = false;
            self
        }
    }

    pub(crate) fn project(specs: Vec<(&str, Resources)>) -> BTreeMap<String, Resources> {
        specs
            .into_iter()
            .map(|(worker, r)| (worker.to_string(), r))
            .collect()
    }

    /// A temp path unique to this process and call site.
    fn temp_config(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("bynk-{label}-{}-{unique}.toml", std::process::id()))
    }

    /// Parse `config` as `deploy` does. The literals in these tests are the
    /// emitter's real output (`bynk-emit/src/emitter/wrangler.rs`), pinned
    /// fixture-side by `bynkc/tests/fixtures/positive/` — 121 (an agent), 151
    /// (a queue) and 372 (KV + agent + queue in one context). Deploy reads what
    /// the emitter writes, so the two must depict the same file.
    fn parse_config(label: &str, text: &str) -> Resources {
        let path = temp_config(label);
        std::fs::write(&path, text).unwrap();
        let resources = read_resources(&path).expect("the emitted config parses");
        let _ = std::fs::remove_file(&path);
        resources
    }

    /// A config carrying every v1 binding kind — mirrors what
    /// `emit_wrangler_toml` actually writes (`bynk-emit/src/emitter/
    /// wrangler.rs`), so a gap here is a gap the real emitter output would
    /// hit too.
    const FULL_CONFIG: &str = r#"
# Generated by bynkc — do not edit by hand.
name = "api"
main = "index.ts"
compatibility_date = "2024-11-01"

[[services]]
binding = "PAYMENT"
service = "payment"

[[kv_namespaces]]
binding = "BYNK_KV"
id = "<KV_NAMESPACE_ID>" # set at deploy time

[[durable_objects.bindings]]
name = "ORDER_ENTITY"
class_name = "OrderEntity"

[[migrations]]
tag = "v1"
new_classes = ["OrderEntity"]

[triggers]
crons = ["*/5 * * * *"]

[[queues.consumers]]
queue = "jobs"
max_batch_size = 10
"#;

    // ---- #835 slice 4: environments -------------------------------------

    #[test]
    fn env_qualify_is_a_no_op_for_default_and_suffixes_otherwise() {
        assert_eq!(env_qualify("default", "jobs"), "jobs");
        assert_eq!(env_qualify("staging", "jobs"), "jobs-staging");
        assert_eq!(env_qualify("staging", "payment"), "payment-staging");
    }

    #[test]
    fn synthesise_environment_block_covers_every_binding_kind() {
        let out = synthesise_environment_block(FULL_CONFIG, "staging", Some("kv-staging-id"))
            .expect("valid TOML in, valid TOML out");

        // The top level is untouched — it still serves a plain `bynk deploy`.
        assert!(
            out.starts_with(FULL_CONFIG),
            "the original bytes must survive verbatim: {out}"
        );

        // Parsed back, not string-matched: proves the appended block is valid
        // TOML with the right shape, not merely text that looks right.
        let doc: toml::Table = out.parse().expect("synthesised output is valid TOML");
        let env = doc["env"]["staging"].as_table().expect("env.staging");

        assert_eq!(env["kv_namespaces"][0]["binding"].as_str(), Some("BYNK_KV"));
        assert_eq!(
            env["kv_namespaces"][0]["id"].as_str(),
            Some("kv-staging-id"),
            "the KV id is this environment's resolved id, not the placeholder"
        );

        assert_eq!(
            env["services"][0]["binding"].as_str(),
            Some("PAYMENT"),
            "the binding name carries over unqualified"
        );
        assert_eq!(
            env["services"][0]["service"].as_str(),
            Some("payment-staging"),
            "the target is environment-qualified — DECISION C's Service Binding half"
        );

        assert_eq!(
            env["queues"]["consumers"][0]["queue"].as_str(),
            Some("jobs-staging"),
            "the physical queue name is environment-qualified — DECISION C's queue half"
        );
        assert_eq!(
            env["queues"]["consumers"][0]["max_batch_size"].as_integer(),
            Some(10)
        );

        assert_eq!(
            env["durable_objects"]["bindings"][0]["class_name"].as_str(),
            Some("OrderEntity"),
            "DO bindings carry no per-environment identity — copied verbatim"
        );
        assert_eq!(env["migrations"][0]["tag"].as_str(), Some("v1"));
        assert_eq!(
            env["migrations"][0]["new_classes"][0].as_str(),
            Some("OrderEntity"),
            "new_classes is exactly the field the narrow Migration struct drops"
        );
        assert_eq!(
            env["triggers"]["crons"][0].as_str(),
            Some("*/5 * * * *"),
            "crons carry no per-environment identity — copied verbatim"
        );
    }

    #[test]
    fn synthesise_environment_block_is_unchanged_for_the_default_environment() {
        // `deploy_one` only calls this for a non-default environment, but the
        // function itself staying a faithful no-op at the boundary is worth
        // pinning: "default" must never gain a synthesised block.
        let out = synthesise_environment_block(FULL_CONFIG, "default", Some("kv-id")).unwrap();
        let doc: toml::Table = out.parse().unwrap();
        assert_eq!(
            doc["env"]["default"]["queues"]["consumers"][0]["queue"].as_str(),
            Some("jobs"),
            "\"default\" qualifies to itself, so this is still a no-op in effect"
        );
    }

    #[test]
    fn synthesise_environment_block_is_a_true_no_op_when_nothing_needs_binding() {
        let bare = "name = \"api\"\nmain = \"index.ts\"\n";
        let out = synthesise_environment_block(bare, "staging", None).unwrap();
        assert_eq!(
            out, bare,
            "a context with no bindable resource appends nothing, not an empty [env.staging]"
        );
    }

    #[test]
    fn synthesise_environment_block_rejects_unparseable_config() {
        assert!(synthesise_environment_block("not = valid = toml = =", "staging", None).is_err());
    }

    #[test]
    fn service_targets_are_read_from_the_generated_config() {
        // The graph is read from the emitted `[[services]]` — the same file
        // wrangler uploads — so parse the real shape the emitter writes.
        assert_eq!(
            parse_config(
                "services",
                r#"
name = "commerce-orders"
main = "index.ts"
compatibility_date = "2024-11-01"

[[services]]
binding = "COMMERCE_PAYMENT"
service = "commerce-payment"
"#,
            ),
            Resources::default().binds(&["commerce-payment"]),
        );

        // A config with no bindings is the common single-context case, and
        // declares nothing else either.
        assert_eq!(
            parse_config("bare", "name = \"api\"\nmain = \"index.ts\"\n"),
            Resources::default(),
        );
    }

    #[test]
    fn a_queue_only_context_declares_its_queues_by_name() {
        // `bynkc/tests/fixtures/positive/153_queue_multiple` — two
        // `from queue(...)` services in one context.
        assert_eq!(
            parse_config(
                "queues",
                r#"
name = "jobs"
main = "index.ts"
compatibility_date = "2024-11-01"

[[queues.consumers]]
queue = "high-priority"
max_batch_size = 10

[[queues.consumers]]
queue = "low-priority"
max_batch_size = 10
"#,
            ),
            Resources::default().consumes(&["high-priority", "low-priority"]),
        );
    }

    #[test]
    fn a_do_only_context_declares_the_tag_the_push_will_apply() {
        // `bynkc/tests/fixtures/positive/121_workers_with_agent` — an agent, so
        // a DO binding and the migration that registers its class.
        assert_eq!(
            parse_config(
                "durable-objects",
                r#"
name = "cart"
main = "index.ts"
compatibility_date = "2024-11-01"

[[durable_objects.bindings]]
name = "CART_ENTITY"
class_name = "CartEntity"

[[migrations]]
tag = "v1"
new_classes = ["CartEntity"]
"#,
            ),
            Resources::default().migrates("v1"),
        );
    }

    #[test]
    fn a_context_declaring_every_v1_resource_is_read_whole() {
        // `bynkc/tests/fixtures/positive/372_kv_agent_queue_workers` — the
        // combination slice 1 completes: KV (slice 0), an agent's migration and
        // a queue, in one context. Each kind is read independently, so one
        // present must not mask another.
        assert_eq!(
            parse_config(
                "everything",
                r#"
name = "ops-hub"
main = "index.ts"
compatibility_date = "2024-11-01"

[[kv_namespaces]]
binding = "KV"
id = "<KV_NAMESPACE_ID>" # set at deploy time

[[durable_objects.bindings]]
name = "JOB_LEDGER"
class_name = "JobLedger"

[[migrations]]
tag = "v1"
new_classes = ["JobLedger"]

[[queues.consumers]]
queue = "job-intake"
max_batch_size = 10
"#,
            ),
            Resources::default()
                .needs_kv()
                .migrates("v1")
                .consumes(&["job-intake"]),
        );
    }

    #[test]
    fn the_migration_read_is_the_state_a_push_leaves_behind() {
        // Wrangler applies a config's migrations in order, so the last tag is
        // what the account ends at. v1 emits one block; the rule is written for
        // the file, not for the emitter's current habit.
        assert_eq!(
            parse_config(
                "migration-chain",
                r#"
name = "cart"
main = "index.ts"

[[migrations]]
tag = "v1"
new_classes = ["CartEntity"]

[[migrations]]
tag = "v2"
new_classes = ["BasketEntity"]
"#,
            )
            .migration,
            Some("v2".to_string()),
        );
    }

    #[test]
    fn the_manifest_is_absent_empty_or_versioned() {
        let dir = std::env::temp_dir().join(format!(
            "bynk-secrets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(bynk_emit::emitter::secrets::SECRETS_MANIFEST);

        // Absent is the empty answer, not an error: a context with no secrets of
        // any kind emits no file. `read_complete` is true *vacuously* — such a
        // context names nothing, computed or otherwise.
        let absent = read_secrets_manifest(&dir).expect("an absent manifest is not an error");
        assert!(absent.declared.is_empty() && absent.read.is_empty());
        assert!(absent.read_complete);

        std::fs::write(
            &path,
            r#"{"version":2,"declared":["AUTH_JWT_SECRET"],"read":["API_KEY"],"read_complete":false}"#,
        )
        .unwrap();
        let read = read_secrets_manifest(&dir).expect("a v2 manifest parses");
        assert_eq!(read.declared, vec!["AUTH_JWT_SECRET".to_string()]);
        assert_eq!(read.read, vec!["API_KEY".to_string()]);
        assert!(!read.read_complete);

        // v1 is refused rather than read with defaults, and that is the point:
        // a v1 file carries no evidence either way about computed names, so
        // defaulting `read_complete` to true for it would make the driver claim
        // a completeness nothing established (ADR 0196 D2). The two constants
        // are independent — the emitter's `MANIFEST_VERSION` and this one — so
        // a stale build tree is exactly what this catches.
        std::fs::write(&path, r#"{"version":1,"declared":["AUTH_JWT_SECRET"]}"#).unwrap();
        assert!(read_secrets_manifest(&dir).is_err());

        // A version from the future is refused for the same reason.
        std::fs::write(&path, r#"{"version":3,"declared":[]}"#).unwrap();
        assert!(read_secrets_manifest(&dir).is_err());

        // Damaged rather than merely old — it sits beside a config we just read.
        std::fs::write(&path, "{not json").unwrap();
        assert!(read_secrets_manifest(&dir).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
