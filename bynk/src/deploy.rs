//! `bynk deploy` — provision persistent Cloudflare identity, then publish.
//!
//! The generated `wrangler.toml` is deliberately disposable. This module owns
//! the small, committed `bynk.deploy.lock` ledger and materialises its KV id
//! into a freshly compiled worker immediately before Wrangler sees it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{ExitCode, Stdio};

use serde::{Deserialize, Serialize};

use crate::compiler::Compiler;
use crate::dev;
use crate::doctor::{self, Capability, Context, DoctorOptions, Report};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};
use crate::shell::exit_status_byte;

const LOCK_FILE: &str = "bynk.deploy.lock";
use bynk_emit::emitter::wrangler::KV_NAMESPACE_ID_PLACEHOLDER;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DeployFormat {
    #[default]
    Short,
    Json,
}

#[derive(Debug, Clone, Default)]
pub struct DeployOptions {
    pub dry_run: bool,
    pub format: DeployFormat,
    pub yes: bool,
    /// `--context NAME` — deploy this context alone, assuming the contexts it
    /// consumes are already live (slice 2, D4). Absent deploys the whole
    /// project in dependency order.
    pub context: Option<String>,
    /// `--env NAME` — slice 4. Selects the `bynk.deploy.lock` section; for any
    /// value other than `"default"` also drives synthesis of an environment-
    /// scoped Wrangler config section, since Cloudflare does not inherit
    /// bindings into a named environment (confirmed against Cloudflare's own
    /// docs — see #835). `DeployOptions::default()`'s empty string is a
    /// test-only artefact of `#[derive(Default)]`; every real invocation goes
    /// through the CLI, whose `default_value = "default"` guarantees this is
    /// never empty.
    pub environment: String,
    /// `--secrets-file` — a dotenv-style source of `NAME=value` pairs. Supplies
    /// **names and values** (slice 3, ADR 0195 D3).
    pub secrets_file: Option<std::path::PathBuf>,
    /// `--secret NAME` — a name whose *value* comes from the environment or a
    /// prompt. The environment is never scanned for names, so this is how a
    /// `bynk.Secrets` name reaches `deploy` without a file.
    pub secrets: Vec<String>,
    /// `--force` — overwrite a secret already set, rather than skipping it.
    pub force: bool,
    pub wrangler_args: Vec<String>,
}

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
}

#[derive(Debug, Deserialize)]
struct ServiceBinding {
    /// The *target* worker's name (`worker_dir_name`, dots dasherised).
    service: String,
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
struct Resources {
    /// The workers this one binds to — the edges the deploy order respects.
    /// Adapters are already excluded upstream (they are not Workers), so every
    /// edge here names a real worker directory.
    binds_to: Vec<String>,
    /// The queues this context consumes, by name. Created before the push
    /// (ADR 0194 D3): a `[[queues.consumers]]` binding whose queue does not
    /// exist fails the deploy.
    queues: Vec<String>,
    /// The migration tag `wrangler deploy` will apply, if the context has an
    /// agent. **Advisory** — Cloudflare owns the applied-migration record, so
    /// this says what will be asked for, never what is already true
    /// (ADR 0194 D1).
    migration: Option<String>,
    /// Still carries the KV placeholder, i.e. needs a namespace.
    needs_kv: bool,
    /// Slice 3: the secret names this context's handlers will read from `env` —
    /// an `actor`'s `auth` secret, read from the emitted `bynk-secrets.json`.
    ///
    /// A **floor, not a census** (ADR 0195 D2): an empty set means "the compiler
    /// proved nothing required", never "this context needs no secret".
    declared_secrets: Vec<String>,
    /// Slice 3 follow-up (ADR 0196): the literal `bynk.Secrets` names this
    /// context reads. **Advisory** — `Secrets.get` returns `Option`, so absence
    /// is a legitimate handled outcome and a missing value warns rather than
    /// failing.
    read_secrets: Vec<String>,
    /// False when the context names a secret with a computed expression, so
    /// `read_secrets` is not everything. The plan must say so rather than
    /// presenting its list as whole.
    reads_complete: bool,
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
        needs_kv: text.contains(KV_NAMESPACE_ID_PLACEHOLDER),
        declared_secrets: secrets.declared,
        read_secrets: secrets.read,
        reads_complete: secrets.read_complete,
    })
}

/// Read the whole project's declared resources, one config read per worker.
fn project_resources(
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

/// The project's binding graph: worker → the workers it binds to.
fn service_graph(resources: &BTreeMap<String, Resources>) -> BTreeMap<String, Vec<String>> {
    resources
        .iter()
        .map(|(worker, r)| (worker.clone(), r.binds_to.clone()))
        .collect()
}

/// Why an order could not be produced.
#[derive(Debug, PartialEq, Eq)]
pub enum OrderError {
    /// A `consumes` cycle. Unreachable through the compiler — `bynkc` rejects
    /// one as `bynk.context.consumes_cycle` before emit, and `deploy` compiles
    /// first — so this is defence in depth against a hand-edited build tree,
    /// not a user-facing path. Named rather than silently tolerated because the
    /// alternative under Cloudflare's upload-time resolution is a deploy that
    /// cannot be completed in one pass.
    Cycle(Vec<String>),
}

impl std::fmt::Display for OrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderError::Cycle(path) => write!(
                f,
                "the generated Service Bindings form a cycle ({}) — it cannot be uploaded in dependency order",
                path.join(" → ")
            ),
        }
    }
}

/// Order the workers so that **every binding target is uploaded before the
/// worker that binds to it** — dependencies first.
///
/// This is a correctness barrier, not a nicety: Cloudflare resolves a Service
/// Binding at **upload time**, and rejects a Worker whose bound target does not
/// yet exist ("deployment will fail, because Worker A declares a binding to
/// Worker B, which does not yet exist"). Uploading in a wrong order does not
/// merely open a transient half-wired window — it fails outright.
///
/// Depth-first post-order over the sorted worker list, so the result is
/// deterministic for a given project rather than dependent on map iteration
/// order. Pure, so the ordering contract is unit-tested without a build tree.
pub fn deploy_order(
    workers: &[String],
    graph: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>, OrderError> {
    let mut order = Vec::new();
    let mut done = BTreeSet::new();
    let mut path = Vec::new();
    for worker in workers {
        visit(worker, graph, &mut done, &mut path, &mut order)?;
    }
    Ok(order)
}

fn visit(
    worker: &str,
    graph: &BTreeMap<String, Vec<String>>,
    done: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<(), OrderError> {
    if done.contains(worker) {
        return Ok(());
    }
    if path.iter().any(|p| p == worker) {
        // Close the reported path onto the repeated node so the cycle reads as
        // a loop (`a → b → a`) rather than a bare list.
        let start = path.iter().position(|p| p == worker).unwrap_or(0);
        let mut cycle: Vec<String> = path[start..].to_vec();
        cycle.push(worker.to_string());
        return Err(OrderError::Cycle(cycle));
    }
    path.push(worker.to_string());
    for target in graph.get(worker).into_iter().flatten() {
        // A binding to something outside this project's build (an
        // externally-managed Worker) has no node to order against; leave it to
        // Cloudflare to accept or reject.
        if graph.contains_key(target) {
            visit(target, graph, done, path, order)?;
        }
    }
    path.pop();
    done.insert(worker.to_string());
    order.push(worker.to_string());
    Ok(())
}

/// The contexts `worker` binds to that the ledger has never recorded as
/// deployed — the D4 gate for `--context`. Deploying a Worker whose binding
/// target does not exist fails at upload, so naming the absent dependency beats
/// letting Cloudflare reject the push with its own vocabulary.
fn absent_dependencies(
    worker: &str,
    graph: &BTreeMap<String, Vec<String>>,
    lock: &DeployLock,
    environment: &str,
) -> Vec<String> {
    graph
        .get(worker)
        .into_iter()
        .flatten()
        .filter(|target| !lock.is_deployed(environment, target))
        .cloned()
        .collect()
}

/// v0.177 (#643): the contract manifest a Worker's build emitted.
#[derive(Debug, Default, Deserialize)]
struct ContractsManifest {
    #[serde(default)]
    version: u32,
    /// This context's own hash per `on call` service — what its entry enforces.
    #[serde(default)]
    provides: BTreeMap<String, String>,
    /// This context's compiled view of each dependency's contract — the hash it
    /// stamps at each call site, keyed by the dependency's qualified name.
    #[serde(default)]
    expects: BTreeMap<String, BTreeMap<String, String>>,
}

const CONTRACTS_MANIFEST_VERSION: u32 = 1;

/// Read one Worker's contract manifest.
///
/// An absent file is the empty answer, not an error: a context that neither
/// exposes nor calls an `on call` service emits none, and a build tree from a
/// compiler predating this file has none either. An *unreadable* or
/// *unparseable* one is an error — it sits beside the config we just read, so a
/// failure there means the tree is damaged rather than merely old.
fn read_contracts_manifest(worker_dir: &Path) -> Result<ContractsManifest, String> {
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

/// One dependency service whose live contract is not the one `worker` was
/// compiled against.
#[derive(Debug, PartialEq, Eq)]
struct ContractSkew {
    dependency: String,
    service: String,
    expected: String,
    live: String,
}

/// v0.177 (#643): the D4 gate extended from *exists* to *matches*.
///
/// `deploy --context A` pushes A alone, against dependencies assumed already
/// live. D4 already refuses when a dependency has never been deployed. This adds
/// the other half: a dependency that exists but no longer provides the contract A
/// was compiled against. Without it, the push succeeds and the skew is
/// discovered by production traffic 409ing — which is better than the silent
/// misinterpretation that preceded this increment, but far worse than refusing.
///
/// **Silence is not a match.** A dependency the ledger has no contract record
/// for (deployed by a pre-v0.177 driver) yields no finding: the gate reports what
/// it *knows* is skewed, never what it merely cannot rule out. The runtime check
/// remains the backstop for exactly that case — and for a `wrangler` push behind
/// the driver's back, or a ledger that has drifted. This gate is an optimisation
/// over the runtime check, never a replacement for it.
fn contract_skews(
    expects: &BTreeMap<String, BTreeMap<String, String>>,
    lock: &DeployLock,
    worker_of: impl Fn(&str) -> String,
    environment: &str,
) -> Vec<ContractSkew> {
    let mut out = Vec::new();
    for (dependency, services) in expects {
        let worker = worker_of(dependency);
        // No record — a pre-v0.177 push, or never deployed. Silence is not a
        // match: report only what is *known* to be skewed. (An empty-but-present
        // record is different, and is checked: it means the callee is known to
        // provide nothing, so every expected service is absent.)
        let Some(live) = lock.live_contracts(environment, &worker) else {
            continue;
        };
        for (service, expected) in services {
            match live.get(service) {
                Some(actual) if actual != expected => out.push(ContractSkew {
                    dependency: dependency.clone(),
                    service: service.clone(),
                    expected: expected.clone(),
                    live: actual.clone(),
                }),
                // A service the live callee no longer provides at all: it was
                // removed or renamed, which is the most complete skew there is.
                None => out.push(ContractSkew {
                    dependency: dependency.clone(),
                    service: service.clone(),
                    expected: expected.clone(),
                    live: "<absent>".to_string(),
                }),
                Some(_) => {}
            }
        }
    }
    out
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct DeployLock {
    // No serde `default`: a ledger with no `version` is not a fresh project, it
    // is corruption (a truncated write), and must fail the read rather than
    // parse as an empty v1 ledger that re-mints every namespace (#736).
    version: u32,
    #[serde(default)]
    environments: BTreeMap<String, Environment>,
}

fn lock_version() -> u32 {
    1
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Environment {
    #[serde(default)]
    kv: BTreeMap<String, KvNamespace>,
    /// Slice 2: which Workers this project has ever pushed. Additive and
    /// `default`ed, so a slice-0 ledger still reads.
    ///
    /// KV state alone could not answer "does this Worker exist on the account?"
    /// — a context with no KV has no `kv` entry at all — and `--context` must
    /// know, because a Service Binding to an absent Worker fails at upload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    workers: BTreeMap<String, WorkerRecord>,
    /// Slice 1: the queue names this project has created at least once.
    ///
    /// Environment-wide rather than per-worker, because a queue is an account
    /// resource addressed by name, not something a Worker owns — two contexts
    /// consuming `"jobs"` mean the same queue.
    ///
    /// **Authoritative for nothing** (ADR 0194 D2). It exists so the plan can
    /// say `create` or `reuse` without a `wrangler queues list` call; the
    /// provision step attempts the create regardless, so a queue deleted
    /// out-of-band comes back rather than being skipped on this set's word.
    /// Additive and `default`ed, so a slice-0 or slice-2 ledger still reads.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    queues: BTreeSet<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct KvNamespace {
    id: String,
}

/// What the ledger remembers about one pushed Worker. A struct rather than a
/// bare bool so slice 3's secrets have somewhere to land.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkerRecord {
    deployed: bool,
    /// v0.177 (#643): the contract hash this Worker *provides* per `on call`
    /// service, as of the push that recorded it — what is **live**.
    ///
    /// This is what makes a skew visible before a request finds it: a later
    /// `deploy --context A` compares A's compiled `expects` against these, and
    /// refuses rather than shipping a caller that will 409 in production.
    ///
    /// `None` means **no record** — a Worker pushed by a pre-v0.177 driver, which
    /// has nothing to say about contracts either way. `Some({})` means the
    /// Worker is *known* to provide no `on call` service at all.
    ///
    /// The distinction is load-bearing, and an empty map cannot carry it: a
    /// callee that removes **all** its services emits no manifest, so a
    /// bare-`BTreeMap` field would record `{}` — indistinguishable from "old
    /// ledger" — and the gate's `continue` would let a total, real skew through.
    /// `Option` keeps "silence is not a match" while still catching removal.
    ///
    /// Additive and `default`ed, so a pre-v0.177 ledger still reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contracts: Option<BTreeMap<String, String>>,
}

impl DeployLock {
    fn is_deployed(&self, environment: &str, worker: &str) -> bool {
        self.environments
            .get(environment)
            .and_then(|env| env.workers.get(worker))
            .is_some_and(|record| record.deployed)
    }

    fn record_deployed(
        &mut self,
        environment: &str,
        worker: &str,
        contracts: Option<BTreeMap<String, String>>,
    ) {
        self.environments
            .entry(environment.to_string())
            .or_default()
            .workers
            .insert(
                worker.to_string(),
                WorkerRecord {
                    deployed: true,
                    contracts,
                },
            );
    }

    /// v0.177 (#643): what the ledger believes `worker` currently provides.
    ///
    /// `None` for both "never deployed" and "deployed before contracts were
    /// recorded" — in each case the ledger cannot speak, and the gate must not
    /// invent an answer.
    fn live_contracts(&self, environment: &str, worker: &str) -> Option<&BTreeMap<String, String>> {
        self.environments
            .get(environment)
            .and_then(|env| env.workers.get(worker))
            .and_then(|record| record.contracts.as_ref())
    }

    fn has_queue(&self, environment: &str, queue: &str) -> bool {
        self.environments
            .get(environment)
            .is_some_and(|env| env.queues.contains(queue))
    }

    /// Note that this project has created `queue`. Returns whether the ledger
    /// changed, so a re-run that provisions nothing also writes nothing.
    fn record_queue(&mut self, environment: &str, queue: &str) -> bool {
        self.environments
            .entry(environment.to_string())
            .or_default()
            .queues
            .insert(queue.to_string())
    }
}

/// The whole-project plan (slice 2). `order` is the upload order, dependencies
/// first; `contexts` carries it with each context's own actions. Slice 0's
/// single-worker `Plan` is the one-element case of this.
#[derive(Debug, Serialize)]
struct Plan<'a> {
    environment: &'a str,
    /// The resolved upload order — the plan's headline, since Cloudflare
    /// rejects a Worker uploaded before its binding target.
    order: Vec<&'a str>,
    contexts: Vec<ContextPlan<'a>>,
}

#[derive(Debug, Serialize)]
struct ContextPlan<'a> {
    worker: &'a str,
    kv: Option<PlanKv<'a>>,
    /// One line per queue this context consumes, in name order.
    queues: Vec<PlanQueue<'a>>,
    /// The migration the push will apply, if the context has an agent.
    migration: Option<PlanMigration<'a>>,
    /// One line per secret this run will set on this context, in name order.
    secrets: Vec<PlanSecret>,
    /// False when this context names at least one secret with a computed
    /// expression, so `secrets` is **not** everything it reads (ADR 0196 D2).
    ///
    /// Carried in the machine surface as well as the human one, because this is
    /// the field that stops a CI job trusting a short list — the failure the
    /// whole increment exists to prevent is a reader taking silence for absence.
    secrets_complete: bool,
    /// `deploy` first time, `redeploy` when the ledger has pushed it before —
    /// the honest word, since a re-run re-pushes rather than skipping.
    action: &'static str,
    /// The workers this one binds to, in the emitted config.
    binds_to: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct PlanKv<'a> {
    action: &'static str,
    namespace: &'a str,
}

#[derive(Debug, Serialize)]
struct PlanQueue<'a> {
    /// `create` when this project has never made the queue, `reuse` when the
    /// ledger has it. Either way the provision step attempts the create and
    /// treats an existing queue as success, so `reuse` is a forecast — "expect
    /// nothing new" — not a promise to stay silent (ADR 0194 D2).
    action: &'static str,
    queue: &'a str,
}

/// One secret the run intends to set on one context.
///
/// There is deliberately **no presence field**. Presence is a live question
/// (`wrangler secret list`), and the plan is derived before `deploy`
/// authenticates — which is what keeps `--dry-run` working offline. So the plan
/// says what it will *try*, and the run reports the skip when a secret turns out
/// to be there. The ledger cannot help: it records no secret at all, because a
/// recorded presence could only ever be a stale one (ADR 0195 D1/D4).
#[derive(Debug, Serialize)]
struct PlanSecret {
    /// Owned: the name set is derived (declared ∪ supplied) rather than
    /// borrowed from any one source.
    name: String,
    /// `declared` — the compiler proved a handler reads it. `supplied` — the
    /// user named it. The mark is the floor-not-census contract made legible:
    /// no `declared` line for a `bynk.Secrets` name does **not** mean the
    /// context needs none (ADR 0195 D2).
    origin: Origin,
    /// `set`, or `overwrite` under `--force`. A `set` line may still report a
    /// skip at wire time — see the type's note.
    action: &'static str,
}

#[derive(Debug, Serialize)]
struct PlanMigration<'a> {
    tag: &'a str,
    /// Always `wrangler deploy`, and that is the point: the field names an
    /// owner other than `bynk`, which is the whole content of the advisory
    /// (ADR 0194 D1). A consumer reading the plan learns that this line is not
    /// a claim about the account's state, without having to know the ADR.
    applied_by: &'static str,
}

/// The `--` passthrough argument that conflicts with the driver's own
/// `--env`, if any (slice 4, DECISION E) — bare or `=`-joined, mirroring
/// `dev.rs`'s `passthrough_has` matching rule for the same class of clash
/// (`--port`/`--inspector-port` there). Returns the matched literal, not just
/// a bool, so the error can name what it conflicts with. Pure, so the rule is
/// tested without touching `DeployOptions`.
///
/// `pub(crate)`: `dev.rs` reuses this for the identical clash between its own
/// `--env` (which environment's ledger section `--remote` reads) and a
/// `-- --env`/`-- --environment` passthrough to `wrangler dev` (which
/// environment Wrangler actually connects to) — the same "two explicit,
/// conflicting environment selections, one of them silent" shape, just
/// without a value `dev` forwards to wrangler itself.
pub(crate) fn conflicting_env_passthrough(wrangler_args: &[String]) -> Option<&str> {
    wrangler_args
        .iter()
        .find(|arg| {
            ["--env", "--environment"]
                .iter()
                .any(|flag| arg.as_str() == *flag || arg.starts_with(&format!("{flag}=")))
        })
        .map(String::as_str)
}

/// Run the slice-0 single-context deployment pipeline.
pub fn run(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    project_root: &Path,
    node_floor: u32,
    opts: &DeployOptions,
) -> ExitCode {
    // Slice 4 (DECISION E), and the first check of all: once `--env` is a
    // real, driver-curated concept, a conflicting `-- --env`/`-- --environment`
    // would otherwise reach `wrangler deploy` as a second, contradictory flag —
    // Wrangler's own last-wins parsing deciding silently which one actually
    // deploys, while the ledger records the driver's choice regardless. Reject
    // before any other work, rather than pick a winner between two explicit,
    // conflicting inputs.
    if let Some(conflict) = conflicting_env_passthrough(&opts.wrangler_args) {
        eprintln!(
            "bynk: `--env {}` conflicts with `{conflict}` after `--` — pass one or the other, not both",
            opts.environment
        );
        return ExitCode::FAILURE;
    }

    let preflight_opts = DoctorOptions {
        only: Some(Capability::Deploy),
        strict: false,
    };
    let report = doctor::diagnose(
        tb,
        compiler,
        &Context {
            project_root: Some(project_root.to_path_buf()),
            in_repo: false,
            node_floor,
        },
        &preflight_opts,
    );
    if report.exit_nonzero(&preflight_opts) {
        eprint!("{}", preflight_failure_message(&report));
        return ExitCode::FAILURE;
    }

    let build_dir = project_root.join(".bynk").join("deploy");
    if let Err(e) = dev::prepare_build_dir(project_root, &build_dir) {
        eprintln!("bynk: could not prepare build directory: {e}");
        return ExitCode::FAILURE;
    }
    if !dev::compile_once(compiler, project_root, &build_dir) {
        return ExitCode::FAILURE;
    }
    // Slice 2: every context, ordered — not the one context slice 0 demanded.
    let workers_dir = build_dir.join("workers");
    let available = dev::discover_workers(&workers_dir);
    let selected = match dev::select_contexts(&available, opts.context.as_slice()) {
        Ok(selected) => selected,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Read spans the *whole* project even under `--context`: D4 needs the
    // selected context's binding targets to check they are live, and they are
    // by definition outside the selection.
    let resources = match project_resources(&workers_dir, &available) {
        Ok(resources) => resources,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let graph = service_graph(&resources);
    // Read before the plan: a malformed `--secrets-file` is the user's typo, and
    // it should surface as one now rather than as a missing-secret failure
    // partway through a run that has already pushed a Worker.
    let secret_source = match SecretSource::read(opts) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lock_path = project_root.join(LOCK_FILE);
    let mut lock = match read_lock(&lock_path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("bynk: could not read {}: {e}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };

    // (D4) `--context` does not deploy a dependency closure. A binding to a
    // Worker that has never been pushed fails at upload, so say which one
    // rather than letting Cloudflare's own error carry it.
    if opts.context.is_some()
        && let [worker] = selected.as_slice()
    {
        let absent = absent_dependencies(worker, &graph, &lock, &opts.environment);
        if !absent.is_empty() {
            eprintln!(
                "bynk: `{worker}` binds to {}, which {} never been deployed — a Service Binding to a Worker that does not exist fails at upload.",
                absent
                    .iter()
                    .map(|a| format!("`{a}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if absent.len() == 1 { "has" } else { "have" }
            );
            eprintln!("  Deploy the whole project once (`bynk deploy`) to bring the topology up.");
            return ExitCode::FAILURE;
        }

        // v0.177 (#643): the other half of D4. The dependency exists — but does
        // it still provide the contract this worker was compiled against?
        // Without this, the push succeeds and production discovers the skew by
        // 409ing. `--context` is precisely the flag that makes this reachable.
        let expects = match read_contracts_manifest(&workers_dir.join(worker)) {
            Ok(m) => m.expects,
            Err(e) => {
                eprintln!(
                    "bynk: could not read `{worker}`'s {}: {e}",
                    bynk_emit::emitter::contracts::CONTRACTS_MANIFEST
                );
                return ExitCode::FAILURE;
            }
        };
        let skews = contract_skews(
            &expects,
            &lock,
            bynk_emit::project::worker_dir_name,
            &opts.environment,
        );
        if !skews.is_empty() {
            eprintln!(
                "bynk: `{worker}` was compiled against a contract its live dependencies no longer provide (bynk.deploy.contract_skew):"
            );
            for s in &skews {
                eprintln!(
                    "  {}.{} — compiled against {}, live is {}",
                    s.dependency, s.service, s.expected, s.live
                );
            }
            eprintln!(
                "  Deploying this would ship a caller its callee rejects (409 ContractMismatch) on every call."
            );
            eprintln!("  Deploy the whole project (`bynk deploy`) so both sides move together.");
            return ExitCode::FAILURE;
        }
    }

    let order = match deploy_order(&selected, &graph) {
        Ok(order) => order
            .into_iter()
            // A whole-project run orders every worker; `--context` orders only
            // the selection, but the DFS reaches its (already-live) targets —
            // drop them, D4 having already checked them.
            .filter(|worker| selected.contains(worker))
            .collect::<Vec<_>>(),
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };

    let plan = derive_plan(
        &order,
        &resources,
        &lock,
        &secret_source,
        opts.force,
        &opts.environment,
    );
    print_plan(&plan, opts.format);
    if opts.dry_run {
        return ExitCode::SUCCESS;
    }

    // The CI gate is KV's alone, deliberately. A namespace id is *minted* by
    // Cloudflare, so a CI job that creates one and cannot commit the result
    // leaves an orphan nobody can find again. A queue's name comes from the
    // source, so CI creating one loses nothing: the next run derives the same
    // name and finds the same queue (ADR 0194 D2).
    for worker in &order {
        let recorded = recorded_kv(&lock, worker, &opts.environment);
        if should_refuse_unrecorded_ci(resources[worker].needs_kv, recorded, is_ci()) {
            eprintln!(
                "bynk: KV namespace for `{worker}` is unrecorded; provision locally first and commit {LOCK_FILE}"
            );
            return ExitCode::FAILURE;
        }
    }
    let probe = probe::detect(
        tb,
        "wrangler",
        DetectOpts {
            project_root: Some(project_root),
            allow_npx: true,
        },
    );
    if !whoami(&probe.provenance) {
        eprintln!(
            "bynk: Cloudflare authentication is unavailable; run `wrangler login` or set CLOUDFLARE_API_TOKEN"
        );
        return ExitCode::FAILURE;
    }
    if !confirm(opts.yes) {
        return ExitCode::FAILURE;
    }

    // Provision → wire → push, per context, in dependency order. Each context's
    // state is written to the ledger as it lands (ADR 0180's incremental
    // posture), so an interrupted multi-context run is resumable rather than
    // restartable — and never rolled back (D2): a half-deployed project is a
    // real state the next plan will show, not an error to unwind.
    // Shared across the loop: a queue two contexts consume is one queue, so it
    // wants one create attempt per run, not one per consumer (ADR 0194 D2).
    let mut attempted_queues = BTreeSet::new();
    // Shared across the loop so two contexts wanting the same secret prompt
    // once. Dropped with the run — nothing here is ever written (ADR 0195 D1).
    let mut resolved_secrets = BTreeMap::new();
    for (i, worker) in order.iter().enumerate() {
        if order.len() > 1 {
            eprintln!("bynk: deploying `{worker}` ({}/{})…", i + 1, order.len());
        }
        match deploy_one(
            &probe.provenance,
            project_root,
            &workers_dir,
            worker,
            &resources[worker],
            &mut lock,
            &lock_path,
            &mut attempted_queues,
            &mut Secrets {
                source: &secret_source,
                force: opts.force,
                resolved: &mut resolved_secrets,
            },
            &opts.wrangler_args,
            &opts.environment,
        ) {
            Ok(Pushed::Ok) => {
                // v0.177 (#643): record what this Worker now *provides*, so a
                // later `--context` push of one of its callers can be refused
                // before it ships a caller that would 409.
                // `Some` even when empty: this build *knows* what the Worker
                // provides, and "knows it provides nothing" must not read as
                // "no record" at the next gate.
                let provided = read_contracts_manifest(&workers_dir.join(worker))
                    .map(|m| Some(m.provides))
                    .unwrap_or(None);
                lock.record_deployed(&opts.environment, worker, provided);
                if let Err(e) = write_lock(&lock_path, &lock) {
                    eprintln!(
                        "bynk: deployed `{worker}` but could not record it in {}: {e}",
                        lock_path.display()
                    );
                    return ExitCode::FAILURE;
                }
            }
            // A shared Ctrl-C. Stop, but report nothing and exit cleanly: the
            // user asked for this, and the terminal signalled us too. `worker`
            // is deliberately *not* recorded as deployed — the push was cut
            // short, so whether it landed is unknown, and the ledger only ever
            // claims what it watched succeed.
            Ok(Pushed::Interrupted) => return ExitCode::SUCCESS,
            Err(f) => {
                eprintln!("bynk: {}", f.message);
                // Stop rather than push on: everything left in the order either
                // binds to what just failed or would be uploaded into a
                // topology that is not what the plan described. `worker` itself
                // is excluded — the line above already named it as the failure,
                // and listing it here again as "not deployed" would double-count
                // it against the number.
                eprint!("{}", stopped_report(&order[i + 1..]));
                // Wrangler's own code, not a flat 1 (slice 0's contract).
                return ExitCode::from(f.code);
            }
        }
    }
    ExitCode::SUCCESS
}

/// How one context's push ended, short of failing.
#[derive(Debug, PartialEq, Eq)]
enum Pushed {
    /// `wrangler deploy` exited 0.
    Ok,
    /// `wrangler deploy` died of a shared Ctrl-C. **Not** a failure: the
    /// terminal delivered the SIGINT to us too, which is exactly why
    /// [`exit_status_byte`] maps it to 0. The run stops without reporting an
    /// error, and `bynk` exits cleanly — as slice 0 did, since it passed the
    /// same status straight through `ExitCode::from`.
    Interrupted,
}

/// Why one context's deploy failed, and what `bynk` should exit with.
///
/// The code rides with the message because `wrangler deploy`'s own exit code is
/// the signal a CI job reads, and slice 0 propagated it
/// (`ExitCode::from(exit_status_byte(&status))`). Flattening every failure to 1
/// would lose it — the multi-context loop reports the *first* failure's code,
/// since the run stops there. A driver-side failure (KV, materialisation, a
/// missing or unspawnable wrangler) has no child code to carry and is a plain 1.
#[derive(Debug, PartialEq, Eq)]
struct DeployFailure {
    message: String,
    code: u8,
}

impl DeployFailure {
    fn driver(message: String) -> Self {
        Self { message, code: 1 }
    }
}

/// Map a finished `wrangler deploy` to an outcome. Pure, so the propagation
/// rule is tested without spawning wrangler.
fn wrangler_outcome(
    worker: &str,
    status: &std::process::ExitStatus,
) -> Result<Pushed, DeployFailure> {
    if status.success() {
        return Ok(Pushed::Ok);
    }
    // `exit_status_byte` is the driver's one place for "what should a child's
    // status make us exit with?" — including the deliberate SIGINT → 0 (a
    // shared Ctrl-C is a clean stop) and the `128 + signal` convention for the
    // signals that are real deaths. Reuse it rather than re-deciding here.
    match exit_status_byte(status) {
        0 => Ok(Pushed::Interrupted),
        code => Err(DeployFailure {
            message: format!("wrangler deploy failed for `{worker}` (exit {code})"),
            code,
        }),
    }
}

/// Provision and push exactly one context. Slice 0's body, lifted so the
/// multi-context loop and `--context` share one path.
///
/// The phase order is the contract (ADR 0194 D3): everything the upload needs
/// to already exist — the KV namespace, the queues a `[[queues.consumers]]`
/// binding would otherwise fail against — is provisioned first; the DO
/// migration is not provisioned at all, because `wrangler deploy` applies it
/// from the same config it is reading.
#[allow(clippy::too_many_arguments)]
fn deploy_one(
    provenance: &Provenance,
    project_root: &Path,
    workers_dir: &Path,
    worker: &str,
    declared: &Resources,
    lock: &mut DeployLock,
    lock_path: &Path,
    attempted_queues: &mut BTreeSet<String>,
    secrets: &mut Secrets<'_>,
    wrangler_args: &[String],
    environment: &str,
) -> Result<Pushed, DeployFailure> {
    let worker_dir = workers_dir.join(worker);
    let config = worker_dir.join("wrangler.toml");
    let mut kv_id = None;
    if declared.needs_kv {
        let id = match recorded_kv(lock, worker, environment) {
            Some(id) => id.to_owned(),
            None => {
                let id = create_kv(provenance, worker, project_root).map_err(|e| {
                    DeployFailure::driver(format!(
                        "could not create KV namespace for `{worker}`: {e}"
                    ))
                })?;
                lock.environments
                    .entry(environment.to_string())
                    .or_default()
                    .kv
                    .insert(worker.to_string(), KvNamespace { id: id.clone() });
                // Recorded before the push, so an interrupted run never makes a
                // second namespace (ADR 0180).
                write_lock(lock_path, lock).map_err(|e| {
                    DeployFailure::driver(format!(
                        "created KV namespace for `{worker}` but could not record it in {}: {e}",
                        lock_path.display()
                    ))
                })?;
                id
            }
        };
        if !materialise_kv_id(&config, &id) {
            return Err(DeployFailure::driver(format!(
                "could not materialise the KV namespace id into `{worker}`'s generated configuration"
            )));
        }
        kv_id = Some(id);
    }
    // Reconciled against the account on every run, never against the ledger:
    // the ledger's queue set is a planning aid, so trusting it to skip would
    // leave a queue deleted out-of-band un-recreated and the push failing
    // against a binding with nothing behind it (ADR 0194 D2). Once per run,
    // not once per consuming context — a queue two contexts consume is one
    // queue. `wrangler deploy` will not create it for us: it checks and fails
    // with "To create it, run: wrangler queues create", so this step is the one
    // that makes such a project deployable at all.
    //
    // The account-facing name is environment-qualified (slice 4, DECISION C):
    // queues reconcile by bare name account-wide, so two environments sharing
    // an account would otherwise create-or-reuse the same physical queue. The
    // ledger still keys by the *logical* name — the outer `environments` map
    // already separates `staging` from `default`, so no schema change is
    // needed, only the wrangler-facing name changes.
    for queue in unattempted_queues(declared, attempted_queues) {
        let physical = env_qualify(environment, &queue);
        if !queue_exists(provenance, &physical, project_root) {
            create_queue(provenance, &physical, project_root).map_err(|e| {
                DeployFailure::driver(format!("could not create the queue `{physical}`: {e}"))
            })?;
        }
        // Recorded before the push, as KV is: what the ledger claims is only
        // ever what it watched succeed. Recorded for a queue that was already
        // there, too — the set's use is the plan's `create`/`reuse` wording, and
        // "we confirmed this exists" is exactly what makes `reuse` the true word.
        if lock.record_queue(environment, &queue) {
            write_lock(lock_path, lock).map_err(|e| {
                DeployFailure::driver(format!(
                    "provisioned the queue `{physical}` but could not record it in {}: {e}",
                    lock_path.display()
                ))
            })?;
        }
    }
    // Slice 4 (DECISION B+C): Cloudflare does not inherit bindings into a named
    // environment, so a non-default `--env` needs its own `[env.<name>]` table —
    // synthesised here, not by the emitter, since the environment name is a
    // deploy-time concept the compiler never sees. The top-level stanza is left
    // untouched; it continues to serve the plain, no-`--env` `bynk deploy`.
    if environment != "default" {
        let config_text = std::fs::read_to_string(&config).map_err(|e| {
            DeployFailure::driver(format!(
                "could not read `{worker}`'s generated configuration: {e}"
            ))
        })?;
        let synthesised =
            synthesise_environment_block(&config_text, environment, kv_id.as_deref()).map_err(
                |e| {
                    DeployFailure::driver(format!(
                        "could not synthesise the `[env.{environment}]` configuration for `{worker}`: {e}"
                    ))
                },
            )?;
        std::fs::write(&config, synthesised).map_err(|e| {
            DeployFailure::driver(format!(
                "could not write `{worker}`'s `[env.{environment}]` configuration: {e}"
            ))
        })?;
    }
    // Secrets straddle the push, and which side depends on whether the Worker
    // already exists (ADR 0195 D6).
    //
    // `wrangler secret put` against a Worker that is not on the account yet does
    // not fail — it creates a **stub draft Worker** (`export default { fetch()
    // {} }`) and puts the secret on that. Non-interactively it does so without
    // asking (its confirm falls back to yes), and interactively it prompts
    // mid-deploy — where a decline makes it exit **0** having set nothing, which
    // `deploy` would read as success and push behind. Neither is a thing to do
    // on a plan that said "deploy `api`".
    //
    // So: a Worker the ledger has pushed before exists, and its secrets are set
    // **before** the push, as the phase order intends — the running code never
    // sees a request without them. A first deploy pushes first and sets after.
    // That window is fail-closed by construction (a handler whose auth secret is
    // unset answers 401, it does not serve unauthenticated), and it is a Worker
    // that did not exist a moment earlier, so there is no traffic to lose.
    //
    // What does **not** straddle is the deciding and resolving: `prepare_secrets`
    // runs before the push on *both* paths. Only the `wrangler secret put` waits.
    // Otherwise the first-deploy path would discover a missing value after making
    // a live Worker — the very outcome the straddle is arranged to avoid.
    let first_deploy = !lock.is_deployed(environment, worker);
    let prepared = prepare_secrets(
        provenance,
        &worker_dir,
        worker,
        declared,
        secrets,
        first_deploy,
        environment,
    )?;
    if !first_deploy {
        apply_secrets(provenance, &worker_dir, worker, &prepared, environment)?;
    }
    let pushed = push(provenance, &worker_dir, worker, wrangler_args, environment)?;
    if first_deploy && pushed == Pushed::Ok {
        apply_secrets(provenance, &worker_dir, worker, &prepared, environment)?;
    }
    Ok(pushed)
}

/// `wrangler deploy` in one worker directory. `--env` is appended for a
/// non-default environment (slice 4) — the one place Wrangler needs telling
/// which of the synthesised `[env.<name>]` tables to read.
fn push(
    provenance: &Provenance,
    worker_dir: &Path,
    worker: &str,
    wrangler_args: &[String],
    environment: &str,
) -> Result<Pushed, DeployFailure> {
    let Some(mut command) = dev::wrangler_command(provenance, "deploy") else {
        return Err(DeployFailure::driver("wrangler not found".into()));
    };
    command.current_dir(worker_dir).args(wrangler_args);
    if environment != "default" {
        command.arg("--env").arg(environment);
    }
    match command.status() {
        Ok(status) => wrangler_outcome(worker, &status),
        Err(e) => Err(DeployFailure::driver(format!(
            "could not run wrangler deploy for `{worker}`: {e}"
        ))),
    }
}

fn recorded_kv<'a>(lock: &'a DeployLock, worker: &str, environment: &str) -> Option<&'a str> {
    lock.environments
        .get(environment)
        .and_then(|env| env.kv.get(worker))
        .map(|kv| kv.id.as_str())
}

pub fn preflight_failure_message(report: &Report) -> String {
    format!(
        "bynk: environment not ready for `deploy` — see below.\n\n{}",
        report::render(report, Format::Human)
    )
}

fn read_lock(path: &Path) -> Result<DeployLock, String> {
    if !path.exists() {
        return Ok(DeployLock {
            version: lock_version(),
            ..Default::default()
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // A zero-byte or whitespace-only ledger is a truncated write, not an empty
    // project. Accepting it as an empty v1 ledger would tell the planner that no
    // namespaces exist and re-mint every one — the exact orphaning the ledger
    // exists to prevent (#736, ADR 0180). Fail hard so the operator restores it.
    if text.trim().is_empty() {
        return Err(format!(
            "deploy ledger `{}` is empty or truncated (corrupt); refusing to \
             treat it as a fresh project — restore it from version control",
            path.display()
        ));
    }
    // A file that does not parse — including one truncated mid-table or missing
    // its now-required `version` — is corruption too, and gets the same
    // restore-it guidance rather than a bare toml diagnostic. A version we simply
    // do not support is a distinct case (a newer or older format), not corruption.
    let lock: DeployLock = toml::from_str(&text).map_err(|e| {
        format!(
            "deploy ledger `{}` is corrupt ({e}) — restore it from version control",
            path.display()
        )
    })?;
    if lock.version != lock_version() {
        return Err(format!("unsupported deploy lock version {}", lock.version));
    }
    Ok(lock)
}

/// Distinguishes concurrent temp ledgers written by the same process.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn write_lock(path: &Path, lock: &DeployLock) -> Result<(), String> {
    let body = toml::to_string_pretty(lock).map_err(|e| e.to_string())?;
    // Atomic, durable replace: write a sibling temp file, fsync it, then rename
    // it over the ledger. A power loss or kill can then only leave the intact old
    // file or the intact new one — never a truncated ledger that reads as empty
    // (#736). Atomicity-for-readers (the rename) is not enough on its own: after
    // a crash the rename can be journaled while the temp's data blocks are still
    // only in the page cache, so we `sync_all` the data before the rename and
    // fsync the directory after it to make the new name itself durable.
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(LOCK_FILE);

    // A per-process counter keeps the temp name unique within a process, and
    // `create_new` makes the create exclusive — a stale temp from a prior crash
    // or a pre-planted symlink at this path is refused rather than followed.
    let (tmp, mut file) = loop {
        let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = dir.join(format!(".{file_name}.{}.{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };

    // From here on, any failure must remove the temp so a full disk or crash
    // does not litter the project with `.bynk.deploy.lock.*.tmp` files.
    let write_then_sync = file
        .write_all(body.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = write_then_sync {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    drop(file);

    // Preserve the ledger's existing permissions across the replace.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    // Best-effort: make the rename itself durable. Directory fsync is a no-op or
    // unsupported on some platforms, so a failure here is not fatal.
    if let Ok(dir_file) = std::fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

fn materialise_kv_id(path: &Path, id: &str) -> bool {
    let Ok(config) = std::fs::read_to_string(path) else {
        return false;
    };
    if !config.contains(KV_NAMESPACE_ID_PLACEHOLDER) {
        return true;
    }
    std::fs::write(path, config.replace(KV_NAMESPACE_ID_PLACEHOLDER, id)).is_ok()
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
fn env_qualify(environment: &str, name: &str) -> String {
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
fn synthesise_environment_block(
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

/// Fill a generated worker configuration from the committed deploy ledger.
/// This is shared with `bynk dev -- --remote`; local dev leaves placeholders
/// alone because Miniflare does not read the Cloudflare namespace id.
///
/// `environment` (slice 4, #837 review): before `--env` existed every real
/// deploy recorded into `"default"` regardless, so hardcoding it here always
/// matched. A project deployed only under a non-default `--env` now has
/// nothing under `"default"` — reading the wrong section would misreport a
/// provisioned project as never deployed, so this reads whichever section
/// `bynk dev --env NAME -- --remote` names (default `"default"`, unchanged).
pub fn materialise_deploy_state(
    project_root: &Path,
    worker: &str,
    config: &Path,
    environment: &str,
) -> Result<bool, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    if !text.contains(KV_NAMESPACE_ID_PLACEHOLDER) {
        return Ok(false);
    }
    let lock = read_lock(&project_root.join(LOCK_FILE))?;
    let Some(id) = lock
        .environments
        .get(environment)
        .and_then(|env| env.kv.get(worker))
        .map(|namespace| namespace.id.as_str())
    else {
        return Err(format!(
            "remote KV for `{worker}` has not been provisioned under environment `{environment}`; run `bynk deploy --env {environment}` first"
        ));
    };
    if materialise_kv_id(config, id) {
        Ok(true)
    } else {
        Err("could not write generated configuration".into())
    }
}

fn whoami(provenance: &Provenance) -> bool {
    let Some(mut command) = dev::wrangler_command(provenance, "whoami") else {
        return false;
    };
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn create_kv(provenance: &Provenance, name: &str, project_root: &Path) -> Result<String, String> {
    let Some(mut command) = dev::wrangler_command(provenance, "kv") else {
        return Err("wrangler not found".into());
    };
    let output = command
        .arg("namespace")
        .arg("create")
        .arg(name)
        .arg("--json")
        // The generated worker config still carries the placeholder until the
        // namespace exists. Create from the project root so Wrangler cannot
        // load and validate that incomplete config.
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(wrangler_said(&output));
    }
    parse_kv_id(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "wrangler did not return a namespace id".into())
}

fn parse_kv_id(output: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

/// Create `name`, or accept that it is already there.
///
/// Unlike KV there is no id to scrape: Cloudflare addresses a queue by the name
/// `from queue("n")` already gave it, so a successful create and an
/// already-existing queue are the same end state and both return `Ok`
/// (ADR 0194 D2).
fn create_queue(provenance: &Provenance, name: &str, project_root: &Path) -> Result<(), String> {
    let Some(mut command) = dev::wrangler_command(provenance, "queues") else {
        return Err("wrangler not found".into());
    };
    let output = command
        .arg("create")
        .arg(name)
        // As for KV: the generated config may still carry the KV placeholder,
        // so run from the project root, where wrangler cannot load and reject
        // an incomplete config that has nothing to do with this call.
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    // The race-loser's path, not the common one: `queue_exists` has already
    // answered for a queue that was simply there. Someone else creating it
    // between that check and this call lands here, so the "already exists"
    // complaint is still read as the success it describes — on both streams,
    // wrangler being inconsistent about which carries one.
    //
    // This is the driver's one claim about another tool's prose, and it cannot
    // be pinned by a test (ADR 0194 D2). It is deliberately reached only by a
    // concurrent deploy, so being wrong about it costs a spurious failure on a
    // rare race that a re-run fixes — not, as it would on the create-every-time
    // shape, every re-deploy of every queue project.
    let said = wrangler_said(&output);
    if queue_already_exists(&said) {
        return Ok(());
    }
    Err(said)
}

/// Is `name` already on the account?
///
/// `wrangler queues info <name>` is a lookup by the same name the config binds,
/// and it answers with an **exit code** rather than prose — which is the whole
/// reason to ask it. Cloudflare's own deploy path reconciles queues this way
/// (`getQueue` in wrangler's queues client), rather than by creating and reading
/// the complaint.
///
/// Asking the account, not the ledger: the ledger's queue set is a planning aid,
/// and trusting it here would leave a queue deleted out-of-band un-recreated
/// (ADR 0194 D2).
///
/// A non-zero exit is read as "not there — try to create it". That is also what
/// an auth or network failure produces, and the honest consequence is the right
/// one: the create then fails too and surfaces wrangler's real complaint, rather
/// than this call inventing a diagnosis of its own.
fn queue_exists(provenance: &Provenance, name: &str, project_root: &Path) -> bool {
    let Some(mut command) = dev::wrangler_command(provenance, "queues") else {
        return false;
    };
    command
        .arg("info")
        .arg(name)
        // As for the create: run from the project root, where wrangler cannot
        // load and reject a config still carrying the KV placeholder.
        .current_dir(project_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Everything a failed wrangler call said, both streams, never empty — a
/// non-zero exit with nothing to say must still read as something rather than
/// as a dangling colon.
fn wrangler_said(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let said: Vec<&str> = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|stream| !stream.is_empty())
        .collect();
    if said.is_empty() {
        return format!("wrangler exited {}", exit_status_byte(&output.status));
    }
    said.join("\n")
}

/// Which of a context's queues still want a create attempt on this run, marking
/// them attempted as it goes.
///
/// A queue is an account resource, so two contexts consuming `"jobs"` mean one
/// queue (ADR 0194 D2) — and the emitter's duplicate-consumer check is scoped to
/// a single context, so that is a legal project rather than a hypothetical. But
/// provisioning runs per context, so without this a shared queue costs one
/// wrangler spawn per consumer, and the ADR's "one call per queue per deploy"
/// would be a claim the code did not honour.
///
/// Scoped to one run, deliberately: every queue is still attempted on every
/// fresh deploy, which is the property the self-healing rests on. Pure but for
/// the marker, so the rule is tested without spawning wrangler.
fn unattempted_queues(declared: &Resources, attempted: &mut BTreeSet<String>) -> Vec<String> {
    declared
        .queues
        .iter()
        .filter(|queue| attempted.insert((*queue).to_string()))
        .cloned()
        .collect()
}

/// Does this `wrangler queues create` failure just mean the queue is already
/// there?
///
/// Matching wrangler's message is the only seam available — it has no
/// `--if-not-exists`, and the create is the *only* call that would tell us
/// (checking with `queues list` first would be the same race, one call later).
/// The failure mode is benign and visible: an unrecognised wording surfaces as
/// a plain deploy failure carrying wrangler's own text, never as a silent
/// mis-provision. Pure, so the rule is tested without an account.
fn queue_already_exists(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("already exists")
}

// ---------------------------------------------------------------------------
// Slice 3: secrets at deploy time (ADR 0195)
// ---------------------------------------------------------------------------

/// Where a secret's name came from — the plan's `declared`/`supplied` mark.
///
/// The distinction is the **floor, not a census** contract made visible
/// (ADR 0195 D2): `declared` is a name the compiler proved this Worker reads,
/// `supplied` is one only the user knows about. A reader must be able to tell
/// which of the two they are looking at, because the compiler's silence about a
/// `bynk.Secrets` name is not evidence that no such name exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Origin {
    /// The compiler proved a handler reads it *and* that its absence is
    /// fail-closed — an `actor`'s auth secret. Required: no value is an error.
    Declared,
    /// The compiler proved a handler reads it, but `Secrets.get` returns
    /// `Option`, so absence is a legitimate handled outcome (ADR 0196 D3).
    /// Advisory: no value warns.
    Read,
    /// The user named it. The compiler knows nothing about it either way.
    Supplied,
}

impl Origin {
    /// Is a missing value fatal?
    ///
    /// Only for `Declared`. This is the whole of ADR 0196 D3, and the reason the
    /// increment does **not** promote a read into the required class: an unset
    /// auth secret 401s every request, while an unset `Secrets.get` name is a
    /// `None` the program may be entirely happy about — erroring on it would
    /// break a legal program.
    fn required(self) -> bool {
        matches!(self, Origin::Declared)
    }

    fn label(self) -> &'static str {
        match self {
            Origin::Declared => "declared",
            Origin::Read => "read",
            Origin::Supplied => "supplied",
        }
    }
}

/// One secret this run intends to set on one context.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WantedSecret {
    name: String,
    origin: Origin,
}

/// The user's secret input.
///
/// Names and values are separate questions (ADR 0195 D3). The file supplies
/// both; `--secret` supplies a name alone; the environment supplies values for
/// names **already known** and is never scanned for names — sweeping `env` into
/// Cloudflare would exfiltrate the user's whole shell.
#[derive(Debug, Default, Clone)]
struct SecretSource {
    /// `--secrets-file` — names and values.
    file: BTreeMap<String, String>,
    /// `--secret NAME` — names only.
    named: BTreeSet<String>,
}

impl SecretSource {
    /// Read the source the options describe. Failing to read a named file is an
    /// error rather than an empty source: the user pointed at it, so silently
    /// proceeding with no values would surface later as a missing-secret error
    /// naming the wrong cause.
    fn read(opts: &DeployOptions) -> Result<Self, String> {
        let file = match &opts.secrets_file {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                parse_secrets_file(&text)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?
            }
            None => BTreeMap::new(),
        };
        Ok(Self {
            file,
            named: opts.secrets.iter().cloned().collect(),
        })
    }
}

/// Parse a dotenv-style `NAME=value` file.
///
/// Deliberately thin — `deploy` moves values, it does not store them, and the
/// source is an input rather than a vault this track owns. `#` comments, blank
/// lines, an optional `export ` prefix, and one layer of matching quotes around
/// a value that would otherwise lose its spacing.
///
/// A malformed line is an **error naming its number**, never a skip: a silently
/// dropped line is a secret that does not get set, which surfaces in production
/// as a 401 rather than here as a typo.
fn parse_secrets_file(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `NAME=value`", i + 1));
        };
        let name = name.trim();
        // Not a full Cloudflare naming rule — that is Cloudflare's to enforce,
        // and inventing one here would reject a name the platform accepts. This
        // catches the shapes that are unambiguously a typo in *this* file.
        if name.is_empty() || name.split_whitespace().count() > 1 {
            return Err(format!(
                "line {}: `{name}` is not a usable secret name",
                i + 1
            ));
        }
        if out
            .insert(name.to_string(), unquote(value.trim()))
            .is_some()
        {
            return Err(format!("line {}: `{name}` is set twice", i + 1));
        }
    }
    Ok(out)
}

/// Strip one layer of matching quotes, so a value with meaningful spacing can
/// survive the line trim.
fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// Which secrets this context wants set, and where each name came from.
///
/// Declared names are this context's own — the compiler proved *this* Worker
/// reads them. Supplied names go to **every** context in the run: nothing tells
/// `deploy` which contexts read a `bynk.Secrets` name, so the only available
/// answers are "all of them" or "none", and none would make `--secrets-file`
/// useless. The plan lists them per context so that spread is visible rather
/// than implied (ADR 0195 D2).
///
/// A name that is both declared and supplied is marked `declared`: the
/// compiler's knowledge is the more informative label, and it is the reason a
/// missing value is an error rather than a shrug.
fn wanted_secrets(
    declared: &[String],
    read: &[String],
    source: &SecretSource,
) -> Vec<WantedSecret> {
    let mut marks: BTreeMap<String, Origin> = BTreeMap::new();
    for name in source.file.keys().chain(source.named.iter()) {
        marks.insert(name.clone(), Origin::Supplied);
    }
    // Read beats supplied, and declared beats both: the marks are ordered by how
    // much the compiler knows, and the strongest thing known about a name is the
    // most useful label for it. `declared` last because it is the only one that
    // makes a missing value fatal.
    for name in read {
        marks.insert(name.clone(), Origin::Read);
    }
    for name in declared {
        marks.insert(name.clone(), Origin::Declared);
    }
    marks
        .into_iter()
        .map(|(name, origin)| WantedSecret { name, origin })
        .collect()
}

/// What the run will do with one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretAction {
    Set,
    Overwrite,
    SkipPresent,
}

/// May a secret we cannot find a value for be left alone rather than failing the
/// deploy? Pure, so the rule is tested without an account.
///
/// Only where **both** hold: presence is unknown (the account could not be
/// asked), and the Worker has been live before. That combination is far more
/// likely to mean "already set, and the user had no reason to supply it again"
/// — the common CI redeploy, no `--secrets-file`, no TTY — than "genuinely
/// missing". Failing there would block a deploy that works, on the strength of a
/// *read* failure in a check that is advisory by design (ADR 0195 D4).
///
/// A first deploy gets no benefit of the doubt: its Worker is new, so an
/// unresolvable declared secret really is missing. And where presence *is*
/// known, the answer is authoritative — a secret we know to be absent, with no
/// value to set, is a real failure that must be named.
fn tolerate_unresolvable(present: Option<&BTreeSet<String>>, first_deploy: bool) -> bool {
    present.is_none() && !first_deploy
}

/// The set-if-absent rule (ADR 0195 D4). Pure, so the rule is tested without an
/// account.
///
/// `present` is `None` when the account could not be asked — a Worker that does
/// not exist yet, or an auth/network failure. Both are read as "assume nothing
/// is set and try": a first deploy genuinely has no secrets, and a real auth
/// failure then surfaces as the `secret put`'s own complaint rather than as a
/// diagnosis this function invented (the `queue_exists` posture).
fn secret_action(name: &str, present: Option<&BTreeSet<String>>, force: bool) -> SecretAction {
    match (present.is_some_and(|p| p.contains(name)), force) {
        (false, _) => SecretAction::Set,
        (true, true) => SecretAction::Overwrite,
        (true, false) => SecretAction::SkipPresent,
    }
}

/// The non-interactive half of D3's precedence: the file, else the environment.
/// `None` means only a prompt is left. Pure (the environment is read by the
/// caller), so the precedence is tested without touching the process env.
fn value_from(name: &str, source: &SecretSource, from_env: Option<String>) -> Option<String> {
    source.file.get(name).cloned().or(from_env)
}

/// The names already set on this Worker, from `wrangler secret list`.
///
/// `None` means "could not ask" — see [`secret_action`]. Unlike `secret put`,
/// `secret list` has no draft-Worker path: it simply fails for a Worker that
/// does not exist, which is exactly the answer we want on a first deploy.
fn list_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    environment: &str,
) -> Option<BTreeSet<String>> {
    let mut command = dev::wrangler_command(provenance, "secret")?;
    command
        .arg("list")
        .arg("--format")
        .arg("json")
        // In the worker directory: `secret list` reads the Worker's name from
        // the config beside it, and any KV id is materialised by now, so
        // wrangler can load a complete config.
        .current_dir(worker_dir);
    if environment != "default" {
        command.arg("--env").arg(environment);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_secret_list(&String::from_utf8_lossy(&output.stdout))
}

/// The names in `wrangler secret list --format json`'s output.
///
/// Wrangler's shape, so this is a claim about another tool's output rather than
/// a contract — but a structural one (a JSON array of objects with a `name`),
/// not a prose match.
///
/// A shape change reads as `None`, "could not tell", which is the same answer as
/// a network or auth failure and is handled once in [`prepare_secrets`]: where a
/// value is available, every secret is re-set (noisy and idempotent); where one
/// is not, a **redeploy leaves the secret alone and says so** rather than failing
/// on the strength of a read failure in an advisory check. Only a first deploy
/// treats an unresolvable declared secret as fatal, because there its Worker
/// really is new.
fn parse_secret_list(stdout: &str) -> Option<BTreeSet<String>> {
    let parsed: serde_json::Value = serde_json::from_str(stdout).ok()?;
    Some(
        parsed
            .as_array()?
            .iter()
            .filter_map(|entry| entry.get("name")?.as_str().map(str::to_owned))
            .collect(),
    )
}

/// Set one secret, feeding the value on **stdin**.
///
/// Never argv, and not merely by preference: `wrangler secret put` has no value
/// option at all — the value is stdin or an interactive prompt — so this is the
/// only interface, and it is the one that keeps the value out of the process
/// list (ADR 0195 D1). Wrangler takes the stdin path exactly when *its* stdin is
/// not a TTY, and a pipe never is, so this works whether or not `bynk` itself
/// has a terminal.
fn set_secret(
    provenance: &Provenance,
    worker_dir: &Path,
    name: &str,
    value: &str,
    environment: &str,
) -> Result<(), String> {
    let Some(mut command) = dev::wrangler_command(provenance, "secret") else {
        return Err("wrangler not found".into());
    };
    command.arg("put").arg(name).current_dir(worker_dir);
    if environment != "default" {
        command.arg("--env").arg(environment);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        // Taken and dropped inside the block: wrangler reads to EOF, so the
        // pipe must close before `wait_with_output`, or both ends block.
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "could not open a pipe for the secret value".to_string())?;
        // No trailing newline: wrangler trims trailing whitespace from what it
        // reads, so one would be dropped anyway — sending exactly the value
        // keeps this end honest about what it sent.
        stdin
            .write_all(value.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    Err(wrangler_said(&output))
}

/// Everything the secret step needs, threaded as one value rather than as four
/// more parameters on an already-long list.
struct Secrets<'a> {
    source: &'a SecretSource,
    force: bool,
    /// Values resolved earlier in this run, so two contexts wanting the same
    /// name prompt once rather than once each.
    ///
    /// In memory only, and dropped with the run: holding a value long enough to
    /// hand it to wrangler is unavoidable, but nothing here is ever written —
    /// the ledger records no secret, not even its presence (ADR 0195 D1).
    resolved: &'a mut BTreeMap<String, String>,
}

/// Decide what to set and resolve every value — the non-mutating half.
///
/// Returns the `(name, value)` pairs [`apply_secrets`] will put. Split from the
/// put so that **every failure a user can act on happens before anything is
/// pushed**, on both sides of D6's straddle: a first deploy pushes before it
/// sets, so resolving lazily there would surface a missing value only once a
/// live Worker existed, 401ing every request. Nothing here touches Cloudflare
/// except the advisory presence read.
#[allow(clippy::too_many_arguments)]
fn prepare_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    worker: &str,
    declared: &Resources,
    secrets: &mut Secrets<'_>,
    first_deploy: bool,
    environment: &str,
) -> Result<Vec<(String, String)>, DeployFailure> {
    let wanted = wanted_secrets(
        &declared.declared_secrets,
        &declared.read_secrets,
        secrets.source,
    );
    // Said once per context, before any of it: a reader who takes the lines
    // below for the whole story would be wrong, and this is the only place that
    // can tell them (ADR 0196 D2).
    if !declared.reads_complete {
        eprintln!(
            "bynk: `{worker}` names at least one secret with a computed expression, so the list below is not everything it reads."
        );
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    // Asked live, never recorded: presence is the only observable Cloudflare
    // offers (it does not return values), and a recorded answer could only ever
    // be a stale one (ADR 0195 D1/D4). Asked *before* the push on both paths —
    // our own `wrangler deploy` does not change which secrets are set, so one
    // answer serves the whole step, and `secret list` has no draft-Worker path
    // to trip over on a first deploy.
    let present = list_secrets(provenance, worker_dir, environment);
    let mut prepared = Vec::new();
    for want in &wanted {
        match secret_action(&want.name, present.as_ref(), secrets.force) {
            SecretAction::SkipPresent => {
                eprintln!(
                    "bynk: secret `{}` is already set on `{worker}`, skipping — use --force to overwrite",
                    want.name
                );
                continue;
            }
            SecretAction::Set | SecretAction::Overwrite => {}
        }
        match resolve_secret_value(&want.name, want.origin, worker, secrets) {
            Ok(value) => prepared.push((want.name.clone(), value)),
            // Advisory by type (ADR 0196 D3): `Secrets.get` returns `Option`, so
            // a read with no value is a `None` the program may be entirely happy
            // about. Say what will happen and carry on — failing here would
            // refuse to deploy a legal program over a secret it never needed.
            Err(_) if !want.origin.required() => {
                eprintln!(
                    "bynk: `{worker}` reads the secret `{}`, but no value was supplied — it will see None.",
                    want.name
                );
                eprintln!(
                    "  Supply it with --secrets-file or --secret {}, if it is meant to be set.",
                    want.name
                );
            }
            Err(_) if tolerate_unresolvable(present.as_ref(), first_deploy) => {
                eprintln!(
                    "bynk: could not ask Cloudflare which secrets `{worker}` has, and no value for `{}` was supplied — leaving it as it is.",
                    want.name
                );
                eprintln!(
                    "  If it is not set, `{worker}` will answer 401. Check with `wrangler secret list`, or supply a value to set it."
                );
            }
            Err(e) => return Err(e),
        }
    }
    Ok(prepared)
}

/// Put each prepared secret. The mutating half, and deliberately the whole of
/// it: everything that can fail for a reason the user can fix — a missing value,
/// a malformed file — has already happened by the time this runs.
fn apply_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    worker: &str,
    prepared: &[(String, String)],
    environment: &str,
) -> Result<(), DeployFailure> {
    for (name, value) in prepared {
        set_secret(provenance, worker_dir, name, value, environment).map_err(|e| {
            DeployFailure::driver(format!(
                "could not set the secret `{name}` on `{worker}`: {e}"
            ))
        })?;
    }
    Ok(())
}

/// The value for one secret: this run's cache, then the file, then the
/// environment, then a prompt (ADR 0195 D3).
///
/// A name with no value anywhere is a **hard error naming it** when there is no
/// terminal to ask — never a blank. A blank would be worse than the failure it
/// replaces: the deploy would report success and the Worker would 401 every
/// request, with nothing to read that says why.
fn resolve_secret_value(
    name: &str,
    origin: Origin,
    worker: &str,
    secrets: &mut Secrets<'_>,
) -> Result<String, DeployFailure> {
    if let Some(cached) = secrets.resolved.get(name) {
        return Ok(cached.clone());
    }
    let from_env = std::env::var(name).ok();
    let value = match value_from(name, secrets.source, from_env) {
        Some(value) => value,
        None => prompt_for_secret(name)
            .ok_or_else(|| DeployFailure::driver(missing_secret_message(name, origin, worker)))?,
    };
    secrets.resolved.insert(name.to_string(), value.clone());
    Ok(value)
}

/// Ask for a value, when there is a terminal to ask.
///
/// `None` when there is no TTY (CI, a piped session) — the caller turns that
/// into the named error. Read from the terminal without echoing would be better
/// still; v1 does not, so the prompt says so rather than implying secrecy it
/// does not provide.
fn prompt_for_secret(name: &str) -> Option<String> {
    if !io::stdin().is_terminal() {
        return None;
    }
    eprint!("Secret `{name}` (input is visible): ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return None;
    }
    let answer = answer.trim_end_matches(['\n', '\r']).to_string();
    (!answer.is_empty()).then_some(answer)
}

/// Why a secret could not be resolved, in the vocabulary of where its name came
/// from — the two origins are missing for different reasons and want different
/// remedies.
fn missing_secret_message(name: &str, origin: Origin, worker: &str) -> String {
    match origin {
        Origin::Declared => format!(
            "`{worker}` declares the secret `{name}` (an actor's `auth` secret) but no value was supplied — \
             pass it in --secrets-file, or set {name} in the environment. \
             Deploying without it would answer every request with 401."
        ),
        // Reached only when a read is *required*, which it never is — kept
        // total rather than unreachable!(), since the compiler cannot know that
        // and a panic here would be a poor way to learn otherwise.
        Origin::Read | Origin::Supplied => format!(
            "the secret `{name}` was named but no value was supplied — \
             set {name} in the environment, or give it a value in --secrets-file."
        ),
    }
}

/// What the run did **not** get to, once a context failed. `rest` is the order
/// *beyond* the failure, so the last context failing reports nothing — there was
/// nothing left to withhold, and the failure itself has already been named.
///
/// Pure, so the wording — and the count's agreement with the list — is goldened
/// rather than described.
fn stopped_report(rest: &[String]) -> String {
    if rest.is_empty() {
        return String::new();
    }
    format!(
        "bynk: stopping — {} not deployed: {}. Re-run `bynk deploy` to resume; what already landed is kept.\n",
        if rest.len() == 1 {
            "1 more context was".to_string()
        } else {
            format!("{} further contexts were", rest.len())
        },
        rest.join(", ")
    )
}

/// Render the plan exactly as the user sees it. Pure, so the output surface the
/// deploy guide documents is goldened rather than described — `print_plan` is
/// the transport.
fn plan_report(plan: &Plan<'_>, format: DeployFormat) -> String {
    match format {
        DeployFormat::Short => {
            let mut out = String::new();
            for context in &plan.contexts {
                if let Some(kv) = &context.kv {
                    out.push_str(&format!("kv {} {}\n", kv.action, kv.namespace));
                }
                for queue in &context.queues {
                    out.push_str(&format!("queue {} {}\n", queue.action, queue.queue));
                }
                // Between the provisioning lines and the push, because that is
                // where it happens: the migration rides the config `wrangler
                // deploy` reads rather than being a step of its own. Flagged
                // advisory in place — a reader must not take it for a claim
                // that the tag is not yet applied (ADR 0194 D1).
                if let Some(migration) = &context.migration {
                    out.push_str(&format!(
                        "migration {} (advisory — {} applies it)\n",
                        migration.tag, migration.applied_by
                    ));
                }
                // Before the lines it qualifies, not after: a reader who takes
                // the list for the whole story is the failure this increment
                // exists to prevent (ADR 0196 D2).
                if !context.secrets_complete {
                    out.push_str(&format!(
                        "secrets incomplete {} (computes at least one name)\n",
                        context.worker
                    ));
                }
                // Names only, never values (ADR 0195 D1). The origin rides each
                // line because the three are not equally known: `declared` is
                // required, `read` is advisory, `supplied` is the user's word.
                for secret in &context.secrets {
                    out.push_str(&format!(
                        "secret {} {} ({})\n",
                        secret.action,
                        secret.name,
                        secret.origin.label()
                    ));
                }
                out.push_str(&format!("{} {}\n", context.action, context.worker));
            }
            // The order is the plan's load-bearing claim once there is more
            // than one context, so state it rather than leaving it implied by
            // the line order above.
            if plan.order.len() > 1 {
                out.push_str(&format!("order {}\n", plan.order.join(" → ")));
            }
            out
        }
        DeployFormat::Json => {
            format!(
                "{}\n",
                serde_json::to_string_pretty(plan).expect("plan serialises")
            )
        }
    }
}

fn print_plan(plan: &Plan<'_>, format: DeployFormat) {
    print!("{}", plan_report(plan, format));
}

/// Derive the plan over the resolved order. Pure, so the per-context breakdown
/// and the ordering claim are unit-tested without a Cloudflare account.
///
/// Indexes `resources` rather than defending a miss: `order` ⊆ the workers it
/// was read for, and the deploy loop indexes the same map anyway — so a
/// tolerated miss here would only understate the plan a moment before the run
/// panicked on it regardless.
fn derive_plan<'a>(
    order: &'a [String],
    resources: &'a BTreeMap<String, Resources>,
    lock: &DeployLock,
    // Not borrowed into the plan: a `PlanSecret` owns its name, because the set
    // is derived (declared ∪ supplied) rather than taken from either source.
    secrets: &SecretSource,
    force: bool,
    environment: &'a str,
) -> Plan<'a> {
    Plan {
        environment,
        order: order.iter().map(String::as_str).collect(),
        contexts: order
            .iter()
            .map(|worker| {
                let declared = &resources[worker];
                ContextPlan {
                    worker,
                    kv: declared.needs_kv.then(|| PlanKv {
                        action: if recorded_kv(lock, worker, environment).is_some() {
                            "reuse"
                        } else {
                            "create"
                        },
                        namespace: worker,
                    }),
                    queues: declared
                        .queues
                        .iter()
                        .map(|queue| PlanQueue {
                            action: if lock.has_queue(environment, queue) {
                                "reuse"
                            } else {
                                "create"
                            },
                            queue,
                        })
                        .collect(),
                    migration: declared.migration.as_deref().map(|tag| PlanMigration {
                        tag,
                        applied_by: "wrangler deploy",
                    }),
                    secrets: wanted_secrets(
                        &declared.declared_secrets,
                        &declared.read_secrets,
                        secrets,
                    )
                    .into_iter()
                    .map(|want| PlanSecret {
                        name: want.name,
                        origin: want.origin,
                        // Presence is not knowable here — the plan runs
                        // before auth so `--dry-run` stays offline — so the
                        // action is what the run will attempt.
                        action: if force { "overwrite" } else { "set" },
                    })
                    .collect(),
                    secrets_complete: declared.reads_complete,
                    action: if lock.is_deployed(environment, worker) {
                        "redeploy"
                    } else {
                        "deploy"
                    },
                    binds_to: declared.binds_to.iter().map(String::as_str).collect(),
                }
            })
            .collect(),
    }
}

fn should_refuse_unrecorded_ci(needs_kv: bool, recorded: Option<&str>, ci: bool) -> bool {
    needs_kv && recorded.is_none() && ci
}

fn requires_interactive_confirmation(yes: bool, stdin_is_terminal: bool) -> bool {
    !yes && stdin_is_terminal
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some_and(|value| value != "false")
}

fn confirm(yes: bool) -> bool {
    if yes {
        return true;
    }
    if !requires_interactive_confirmation(yes, io::stdin().is_terminal()) {
        eprintln!("bynk: refusing to mutate in a non-interactive session without --yes");
        return false;
    }
    eprint!("Deploy to Cloudflare? [y/N] ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A binding graph literal: `[("a", &["b"])]` = a binds to b.
    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(from, to)| (from.to_string(), names(to)))
            .collect()
    }

    /// `derive_plan` with no secret input and no `--force` — the shape every
    /// test that predates slice 3 wants, so those tests keep saying what they
    /// are about rather than restating two arguments they do not exercise.
    fn plan_of<'a>(
        order: &'a [String],
        resources: &'a BTreeMap<String, Resources>,
        lock: &DeployLock,
    ) -> Plan<'a> {
        derive_plan(
            order,
            resources,
            lock,
            &SecretSource::default(),
            false,
            "default",
        )
    }

    fn lock_with_deployed(workers: &[&str]) -> DeployLock {
        let mut lock = DeployLock::default();
        for worker in workers {
            lock.record_deployed("default", worker, Some(Default::default()));
        }
        lock
    }

    /// Record `worker`'s KV namespace, as a real deploy does before it pushes.
    fn with_kv(mut lock: DeployLock, worker: &str) -> DeployLock {
        lock.environments
            .entry("default".into())
            .or_default()
            .kv
            .insert(worker.to_string(), KvNamespace { id: "ns-id".into() });
        lock
    }

    /// Mark `queue` as one this project has already created.
    fn with_queue(mut lock: DeployLock, queue: &str) -> DeployLock {
        lock.record_queue("default", queue);
        lock
    }

    /// A fluent literal for what one context declares, so a test names only the
    /// resources it is about: `Resources::default().needs_kv().migrates("v1")`.
    impl Resources {
        fn binds(mut self, targets: &[&str]) -> Self {
            self.binds_to = names(targets);
            self
        }
        fn consumes(mut self, queues: &[&str]) -> Self {
            self.queues = names(queues);
            self
        }
        fn migrates(mut self, tag: &str) -> Self {
            self.migration = Some(tag.to_string());
            self
        }
        fn needs_kv(mut self) -> Self {
            self.needs_kv = true;
            self
        }
        /// The secret names the emitted manifest carries — an `actor`'s `auth`
        /// secret, which the compiler proved this Worker reads.
        fn declares(mut self, secrets: &[&str]) -> Self {
            self.declared_secrets = names(secrets);
            self
        }
        /// Literal `bynk.Secrets` names the compiler saw this Worker read.
        fn reads(mut self, secrets: &[&str]) -> Self {
            self.read_secrets = names(secrets);
            self
        }
        /// This context names at least one secret with a computed expression.
        fn reads_incompletely(mut self) -> Self {
            self.reads_complete = false;
            self
        }
    }

    /// A user secret input literal: `source(&[("A", "v")], &["B"])` is a
    /// `--secrets-file` carrying `A=v` plus a `--secret B`.
    fn source(file: &[(&str, &str)], named: &[&str]) -> SecretSource {
        SecretSource {
            file: file
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            named: names(named).into_iter().collect(),
        }
    }

    fn project(specs: Vec<(&str, Resources)>) -> BTreeMap<String, Resources> {
        specs
            .into_iter()
            .map(|(worker, r)| (worker.to_string(), r))
            .collect()
    }

    /// The guide's worked example: `commerce-orders` binds to
    /// `commerce-payment`, which is the one with the KV namespace.
    fn chain() -> BTreeMap<String, Resources> {
        project(vec![
            (
                "commerce-orders",
                Resources::default().binds(&["commerce-payment"]),
            ),
            ("commerce-payment", Resources::default().needs_kv()),
        ])
    }

    /// The goldens live beside the integration ones (`tests/golden/`) and bless
    /// identically — `BYNK_BLESS=1 cargo test -p bynk`. They are driven from
    /// here rather than from `tests/` because `derive_plan` reads the ledger and
    /// the binding graph, which are this module's private types: goldening the
    /// output must not force them into the crate's public API.
    fn bless_or_assert(name: &str, actual: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        if std::env::var_os("BYNK_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; regenerate with BYNK_BLESS=1 cargo test -p bynk",
                path.display()
            )
        });
        assert_eq!(
            actual, expected,
            "golden {name} drifted; re-bless with BYNK_BLESS=1 cargo test -p bynk"
        );
    }

    /// #601/#600: the plan is what `--dry-run` shows and the deploy guide
    /// quotes, so it is pinned exactly — the `order` line (slice 2's
    /// load-bearing claim), the queue and migration lines (slice 1's), and the
    /// JSON shape, which is a documented machine-readable surface.
    #[test]
    fn golden_deploy_plan() {
        let chain_order = names(&["commerce-payment", "commerce-orders"]);

        let mut out = String::new();

        // Slice 0's shape: one context, nothing recorded. No `order` line —
        // there is no ordering claim to make about a single worker.
        out.push_str("# one context, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["api"]),
                &project(vec![("api", Resources::default().needs_kv())]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        // The guide's worked example: payment first, because orders binds to it.
        out.push_str("\n# several contexts, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Short,
        ));

        // A re-run re-pushes rather than skipping, so the word is `redeploy`
        // and the namespace is reused. The ledger records the KV *before* the
        // push (ADR 0180), so a deployed context always has its namespace
        // recorded too — depict that state, not an unreachable one.
        out.push_str("\n# several contexts, already live — a re-run re-pushes\n");
        out.push_str(&plan_report(
            &plan_of(
                &chain_order,
                &chain(),
                &with_kv(
                    lock_with_deployed(&["commerce-payment", "commerce-orders"]),
                    "commerce-payment",
                ),
            ),
            DeployFormat::Short,
        ));

        // Slice 1's kinds. The migration line is advisory in both states, so it
        // reads the same before and after — that sameness is the point, and the
        // golden is where it is visible.
        out.push_str("\n# slice 1: an agent and a queue, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .needs_kv()
                        .consumes(&["job-intake"])
                        .migrates("v1"),
                )]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        out.push_str("\n# slice 1: the same context, already provisioned\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .needs_kv()
                        .consumes(&["job-intake"])
                        .migrates("v1"),
                )]),
                &with_queue(with_kv(lock_with_deployed(&["jobs"]), "jobs"), "job-intake"),
            ),
            DeployFormat::Short,
        ));

        // Slice 3. The origin mark is the load-bearing part: `declared` is the
        // compiler's word, `supplied` is the user's, and a reader must not take
        // the absence of a `declared` line for "this context needs no secret".
        out.push_str("\n# slice 3: a declared auth secret, and one the user supplied\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET"]),
                )]),
                &DeployLock::default(),
                &source(&[("STRIPE_KEY", "sk_live_x")], &[]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        // `--force`: the action is `overwrite` rather than `set`. Presence is
        // absent from the plan by design — it is a live question, and the plan
        // is derived before auth so `--dry-run` stays offline.
        out.push_str("\n# slice 3: --force overwrites rather than setting if absent\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET"]),
                )]),
                &lock_with_deployed(&["api"]),
                &source(&[], &["PROBE_TOKEN"]),
                true,
                "default",
            ),
            DeployFormat::Short,
        ));

        // A supplied name goes to *every* context in the run: nothing says which
        // contexts read a `bynk.Secrets` name. The plan lists it per context so
        // that spread is visible rather than implied.
        out.push_str("\n# slice 3: a supplied secret reaches every context\n");
        out.push_str(&plan_report(
            &derive_plan(
                &chain_order,
                &chain(),
                &DeployLock::default(),
                &source(&[("SHARED_KEY", "v")], &[]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        // The three classes side by side — the increment's whole surface. A
        // reader must be able to tell the compiler's *required* knowledge
        // (`declared`) from its *advisory* knowledge (`read`) from the user's
        // word (`supplied`), because they fail differently.
        out.push_str("\n# all three classes: declared (required), read (advisory), supplied\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default()
                        .declares(&["AUTH_JWT_SECRET"])
                        .reads(&["STRIPE_KEY"]),
                )]),
                &DeployLock::default(),
                &source(&[], &["PROBE_TOKEN"]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        out.push_str("\n# --format json\n");
        out.push_str(&plan_report(
            &plan_of(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Json,
        ));

        // A computed name: the list is not a census, and the JSON is where a CI
        // job learns that rather than trusting a short list.
        out.push_str("\n# --format json, a context that computes a secret name\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default()
                        .reads(&["WELL_KNOWN"])
                        .reads_incompletely(),
                )]),
                &DeployLock::default(),
                &SecretSource::default(),
                false,
                "default",
            ),
            DeployFormat::Json,
        ));

        // The JSON shape of slice 3's kinds — the surface a CI job reads to
        // learn which names it must supply, and which the compiler already knows.
        out.push_str("\n# --format json, with declared and supplied secrets\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET", "WH_SECRET"]),
                )]),
                &DeployLock::default(),
                &source(&[("STRIPE_KEY", "sk_live_x")], &["PROBE_TOKEN"]),
                false,
                "default",
            ),
            DeployFormat::Json,
        ));

        // The JSON shape of slice 1's kinds — the surface a CI job reads to
        // learn that the migration is not ours to claim.
        out.push_str("\n# --format json, with a queue and a migration\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .consumes(&["job-intake"])
                        .migrates("v1"),
                )]),
                &DeployLock::default(),
            ),
            DeployFormat::Json,
        ));

        bless_or_assert("deploy-plan.txt", &out);
    }

    // ---- #601 slice 2: what `wrangler deploy`'s status makes us exit with ----
    //
    // Unix-only: `ExitStatus` is only constructible from a raw wait status
    // there, and the raw encoding (`code << 8`, or a bare signal number) is a
    // Unix concept. The rule itself is platform-independent.

    #[cfg(unix)]
    fn status(raw: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(raw)
    }

    #[cfg(unix)]
    #[test]
    fn a_clean_push_is_ok() {
        assert_eq!(wrangler_outcome("api", &status(0)), Ok(Pushed::Ok));
    }

    #[cfg(unix)]
    #[test]
    fn wranglers_exit_code_is_propagated_rather_than_flattened() {
        // Slice 0 exited with `exit_status_byte(&status)`; the multi-context
        // loop must not lose that. A CI job reads the code, so a wrangler exit
        // 2 must not surface as a generic 1.
        let Err(failure) = wrangler_outcome("api", &status(2 << 8)) else {
            panic!("a non-zero wrangler exit is a failure");
        };
        assert_eq!(failure.code, 2, "wrangler's own code reaches the caller");
        assert!(
            failure.message.contains("(exit 2)"),
            "the message states the code it exits with: {}",
            failure.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_ctrl_c_is_a_clean_stop_not_a_failure() {
        // SIGINT reaches us too (shared foreground process group), which is why
        // `exit_status_byte` maps it to 0. Reporting it as a failure would print
        // "wrangler deploy failed … (exit 0)" — an error whose own code says
        // success — and slice 0 exited 0 here.
        assert_eq!(wrangler_outcome("api", &status(2)), Ok(Pushed::Interrupted));
    }

    #[cfg(unix)]
    #[test]
    fn a_real_signal_death_is_a_failure_at_128_plus_the_signal() {
        // Not every signal is a clean stop: a SIGSEGV or the OOM killer's
        // SIGKILL is a genuine failure, and `exit_status_byte` says `128 + sig`.
        let Err(segv) = wrangler_outcome("api", &status(11)) else {
            panic!("a SIGSEGV is a failure, not a clean stop");
        };
        assert_eq!(segv.code, 139);
        let Err(kill) = wrangler_outcome("api", &status(9)) else {
            panic!("a SIGKILL is a failure, not a clean stop");
        };
        assert_eq!(kill.code, 137);
    }

    #[test]
    fn a_driver_side_failure_has_no_child_code_to_carry() {
        // KV, materialisation, a missing wrangler — nothing ran, so there is no
        // code to propagate and 1 is the honest answer.
        assert_eq!(DeployFailure::driver("nope".into()).code, 1);
    }

    /// #601 D4: a failure stops the run and names what did not land. The count
    /// and the list must agree, and the context that just failed — already
    /// reported on its own line — must not be listed again here.
    #[test]
    fn golden_deploy_stopped() {
        let mut out = String::new();
        out.push_str("# the last context failed: nothing was left to withhold\n");
        out.push_str(&stopped_report(&[]));
        out.push_str("# one context was left\n");
        out.push_str(&stopped_report(&names(&["commerce-orders"])));
        out.push_str("# several were left\n");
        out.push_str(&stopped_report(&names(&[
            "commerce-orders",
            "commerce-shipping",
        ])));
        bless_or_assert("deploy-stopped.txt", &out);
    }

    #[test]
    fn the_stop_report_counts_only_what_is_left_and_agrees_with_its_list() {
        // The regression: the slice reported `order[i..]`, which included the
        // context that had just failed — so a 3-context run failing at the 2nd
        // said "1 more context was not deployed: b, c", naming two.
        assert_eq!(
            stopped_report(&[]),
            "",
            "the failure itself is already reported"
        );
        for n in 1..5usize {
            let rest = names(&["c0", "c1", "c2", "c3"][..n]);
            let report = stopped_report(&rest);
            let listed = report
                .split(" not deployed: ")
                .nth(1)
                .and_then(|tail| tail.split(". Re-run").next())
                .expect("the list sits between the count and the remedy");
            assert_eq!(
                listed.split(", ").count(),
                n,
                "the list names every withheld context: {report}"
            );
            let count = if n == 1 {
                "1 more context was".to_string()
            } else {
                format!("{n} further contexts were")
            };
            assert!(
                report.contains(&count),
                "the count states the number it lists: {report}"
            );
        }
    }

    #[test]
    fn lock_round_trip_is_environment_keyed() {
        let lock = DeployLock {
            version: 1,
            environments: BTreeMap::from([(
                "default".into(),
                Environment {
                    kv: BTreeMap::from([("api".into(), KvNamespace { id: "abc".into() })]),
                    workers: BTreeMap::from([(
                        "api".into(),
                        WorkerRecord {
                            deployed: true,
                            contracts: Default::default(),
                        },
                    )]),
                    queues: BTreeSet::from(["intake".to_string()]),
                },
            )]),
        };
        assert_eq!(
            toml::from_str::<DeployLock>(&toml::to_string_pretty(&lock).unwrap()).unwrap(),
            lock
        );
    }

    #[test]
    fn a_slice_0_ledger_without_workers_or_queues_still_reads() {
        // Both tables are additive: a ledger committed before slice 2 (workers)
        // or slice 1 (queues) must keep working, reporting nothing recorded
        // rather than failing to parse. #600 D4: the version stays 1, so this
        // is the whole migration story.
        let lock: DeployLock = toml::from_str(
            r#"
            version = 1
            [environments.default.kv.api]
            id = "abc"
        "#,
        )
        .expect("a slice-0 ledger must still parse");
        assert_eq!(recorded_kv(&lock, "api", "default"), Some("abc"));
        assert!(!lock.is_deployed("default", "api"));
        assert!(!lock.has_queue("default", "intake"));
    }

    #[test]
    fn the_queue_set_serialises_as_names_under_the_environment() {
        // The committed shape is a documented surface — a reviewer reads this
        // file in a diff. Queues are environment-wide names, not a per-worker
        // table, and carry no id.
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.record_queue("default", "job-intake");
        lock.record_queue("default", "job-retry");
        let text = toml::to_string_pretty(&lock).unwrap();
        assert!(
            text.contains("[environments.default]") && text.contains("queues = ["),
            "the queue set is environment-wide, not a per-worker table: {text}"
        );
        for queue in ["job-intake", "job-retry"] {
            assert!(
                text.contains(&format!("\"{queue}\"")),
                "{queue} is recorded"
            );
        }
        assert!(
            !text.contains("id"),
            "a queue is addressed by name — the ledger has no id to record: {text}"
        );
        assert_eq!(toml::from_str::<DeployLock>(&text).unwrap(), lock);
    }

    #[test]
    fn an_empty_queue_set_is_not_written_at_all() {
        // A project with no queues must not grow an empty `queues = []` line in
        // a committed file for a slice it does not use.
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.record_deployed("default", "api", Some(Default::default()));
        assert!(!toml::to_string_pretty(&lock).unwrap().contains("queues"));
    }
    #[test]
    fn parses_wrangler_namespace_json() {
        assert_eq!(
            parse_kv_id(r#"{"id":"abc","title":"api"}"#),
            Some("abc".into())
        );
    }
    #[test]
    fn materialises_only_the_placeholder() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bynk-deploy-{}-{}", std::process::id(), unique));
        std::fs::write(&path, format!("id = \"{KV_NAMESPACE_ID_PLACEHOLDER}\"")).unwrap();
        assert!(materialise_kv_id(&path, "abc"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "id = \"abc\"");
        let _ = std::fs::remove_file(path);
    }

    // ---- #835 slice 4: environments -------------------------------------

    #[test]
    fn env_qualify_is_a_no_op_for_default_and_suffixes_otherwise() {
        assert_eq!(env_qualify("default", "jobs"), "jobs");
        assert_eq!(env_qualify("staging", "jobs"), "jobs-staging");
        assert_eq!(env_qualify("staging", "payment"), "payment-staging");
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
    fn conflicting_env_passthrough_finds_bare_and_equals_forms() {
        assert_eq!(
            conflicting_env_passthrough(&names(&["--env", "production"])),
            Some("--env")
        );
        assert_eq!(
            conflicting_env_passthrough(&names(&["--env=production"])),
            Some("--env=production")
        );
        assert_eq!(
            conflicting_env_passthrough(&names(&["--environment", "production"])),
            Some("--environment")
        );
        assert_eq!(
            conflicting_env_passthrough(&names(&["--minify"])),
            None,
            "an unrelated flag is not a conflict"
        );
        assert_eq!(conflicting_env_passthrough(&[]), None);
    }

    #[test]
    fn two_environments_do_not_cross_contaminate_the_ledger() {
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.record_deployed("staging", "api", None);
        lock.record_queue("staging", "jobs");
        lock.environments
            .entry("staging".into())
            .or_default()
            .kv
            .insert(
                "api".into(),
                KvNamespace {
                    id: "kv-staging".into(),
                },
            );

        // "default" was never touched — a `--env staging` run must not
        // fabricate or leak into the section a plain `bynk deploy` reads.
        assert!(!lock.is_deployed("default", "api"));
        assert!(!lock.has_queue("default", "jobs"));
        assert_eq!(recorded_kv(&lock, "api", "default"), None);

        assert!(lock.is_deployed("staging", "api"));
        assert!(lock.has_queue("staging", "jobs"));
        assert_eq!(recorded_kv(&lock, "api", "staging"), Some("kv-staging"));
    }

    /// #837 review: `materialise_deploy_state` (shared with `bynk dev --
    /// --remote`) hardcoded `"default"` even after `--env` shipped. Before
    /// `--env` existed every real deploy recorded into `"default"`
    /// regardless, so that always matched — but a project deployed *only*
    /// under `bynk deploy --env staging` now has nothing under `"default"`,
    /// and reading the wrong section misreports a provisioned project as
    /// never deployed.
    #[test]
    fn materialise_deploy_state_reads_the_named_environment_not_default() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bynk-materialise-state-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let project_root = dir.clone();
        let config = dir.join("wrangler.toml");
        std::fs::write(&config, format!("id = \"{KV_NAMESPACE_ID_PLACEHOLDER}\"")).unwrap();

        // Provisioned under "staging" alone — the scenario the review named:
        // a project that has never had a plain `bynk deploy` (no "default").
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.environments
            .entry("staging".into())
            .or_default()
            .kv
            .insert(
                "api".into(),
                KvNamespace {
                    id: "kv-staging".into(),
                },
            );
        std::fs::write(
            project_root.join(LOCK_FILE),
            toml::to_string_pretty(&lock).unwrap(),
        )
        .unwrap();

        // Reading "default" (what this function did unconditionally before
        // the fix) must fail helpfully, not silently mis-materialise.
        let err = materialise_deploy_state(&project_root, "api", &config, "default")
            .expect_err("nothing is recorded under \"default\" — this must not silently pass");
        assert!(
            err.contains("environment `default`"),
            "the error should name which environment it looked under: {err}"
        );

        // Reading "staging" — the environment it was actually deployed under
        // — must succeed and materialise that environment's id.
        assert!(materialise_deploy_state(&project_root, "api", &config, "staging").unwrap());
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "id = \"kv-staging\""
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    fn scratch_lock_path(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bynk-{label}-{}-{unique}.deploy.lock",
            std::process::id()
        ))
    }

    #[test]
    fn a_written_ledger_reads_back_identically() {
        // The floor the atomic write must not disturb: a real ledger survives a
        // write/read round-trip unchanged.
        let path = scratch_lock_path("roundtrip");
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.environments
            .entry("default".into())
            .or_default()
            .kv
            .insert(
                "api".into(),
                KvNamespace {
                    id: "kv-123".into(),
                },
            );
        write_lock(&path, &lock).unwrap();
        assert_eq!(read_lock(&path).unwrap(), lock);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_empty_ledger_is_corruption_not_a_fresh_project() {
        // #736: a truncated write leaves a zero-byte file. Reading it as an
        // empty v1 ledger would re-mint every namespace, so it must fail hard —
        // whereas a genuinely absent file is a fresh project and reads clean.
        let path = scratch_lock_path("empty");
        std::fs::write(&path, "").unwrap();
        assert!(
            read_lock(&path).is_err(),
            "a zero-byte ledger must be rejected, not treated as no environments"
        );
        std::fs::write(&path, "   \n\t\n").unwrap();
        assert!(
            read_lock(&path).is_err(),
            "a whitespace-only ledger is just as corrupt"
        );
        let _ = std::fs::remove_file(&path);
        assert!(
            read_lock(&path).is_ok(),
            "an absent ledger is a fresh project, not corruption"
        );
    }

    #[test]
    fn a_ledger_without_a_version_is_rejected() {
        // With the serde default gone, a file that parses but carries no
        // `version` is corruption rather than a silent empty v1 ledger.
        let path = scratch_lock_path("noversion");
        std::fs::write(&path, "[environments]\n").unwrap();
        assert!(read_lock(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    fn scratch_lock_dir(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("bynk-{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_litter(dir: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "tmp"))
            .collect()
    }

    #[test]
    fn a_successful_write_leaves_no_temp_litter() {
        // The atomic write renames its temp over the ledger; nothing sibling to
        // the ledger may survive the write.
        let dir = scratch_lock_dir("nolitter");
        let path = dir.join(LOCK_FILE);
        let lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        write_lock(&path, &lock).unwrap();
        write_lock(&path, &lock).unwrap(); // over an existing ledger, too
        assert!(
            temp_litter(&dir).is_empty(),
            "no `.tmp` sibling may outlive the rename"
        );
        assert_eq!(read_lock(&path).unwrap(), lock);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_rewrite_preserves_the_ledger_permissions() {
        use std::os::unix::fs::PermissionsExt;
        // A committed ledger's mode must survive a rewrite, or the atomic replace
        // would silently loosen or tighten it via the temp file's fresh mode.
        let dir = scratch_lock_dir("perms");
        let path = dir.join(LOCK_FILE);
        let lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        write_lock(&path, &lock).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_lock(&path, &lock).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "the rewrite must carry the ledger's own mode across the rename"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plan_creates_or_reuses_kv_from_the_ledger() {
        let order = names(&["api"]);
        let declared = project(vec![("api", Resources::default().needs_kv())]);
        let fresh = DeployLock::default();
        assert_eq!(
            plan_of(&order, &declared, &fresh).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "create"
        );
        assert_eq!(
            plan_of(&order, &declared, &with_kv(DeployLock::default(), "api")).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "reuse"
        );
        assert!(
            plan_of(
                &order,
                &project(vec![("api", Resources::default())]),
                &fresh
            )
            .contexts[0]
                .kv
                .is_none(),
            "a context declaring no KV gets no KV line"
        );
    }

    // ---- #600 slice 1: queues and DO migrations ------------------------

    #[test]
    fn plan_creates_or_reuses_a_queue_by_its_name() {
        // Queues reconcile on the name `from queue("n")` gave them — there is
        // no id — so the ledger's whole answer is "have we made this before?"
        let order = names(&["jobs"]);
        let declared = project(vec![("jobs", Resources::default().consumes(&["intake"]))]);
        let line =
            |lock: &DeployLock| plan_of(&order, &declared, lock).contexts[0].queues[0].action;
        assert_eq!(line(&DeployLock::default()), "create");
        assert_eq!(line(&with_queue(DeployLock::default(), "intake")), "reuse");
        // The name is keyed environment-wide, not per worker: a different
        // context consuming `intake` means the same queue.
        assert!(with_queue(DeployLock::default(), "intake").has_queue("default", "intake"));
        assert!(!with_queue(DeployLock::default(), "intake").has_queue("default", "other"));
    }

    #[test]
    fn a_context_with_no_queues_gets_no_queue_lines() {
        assert!(
            plan_of(
                &names(&["api"]),
                &project(vec![("api", Resources::default())]),
                &DeployLock::default(),
            )
            .contexts[0]
                .queues
                .is_empty()
        );
    }

    #[test]
    fn the_migration_line_is_advisory_in_every_ledger_state() {
        // D1: Cloudflare owns the applied-migration record, so the plan says
        // what the push will *ask for* and never what is already true. A ledger
        // that has deployed this context before must not change the line —
        // there is no state here for the ledger to have an opinion about.
        let order = names(&["jobs"]);
        let declared = project(vec![("jobs", Resources::default().migrates("v1"))]);
        for lock in [DeployLock::default(), lock_with_deployed(&["jobs"])] {
            let plan = plan_of(&order, &declared, &lock);
            let migration = plan.contexts[0]
                .migration
                .as_ref()
                .expect("a context with an agent has a migration line");
            assert_eq!(migration.tag, "v1");
            assert_eq!(
                migration.applied_by, "wrangler deploy",
                "the plan names an owner other than bynk — that is the advisory"
            );
        }
        // No agent, no migration line.
        assert!(
            plan_of(
                &names(&["api"]),
                &project(vec![("api", Resources::default())]),
                &DeployLock::default(),
            )
            .contexts[0]
                .migration
                .is_none()
        );
    }

    #[test]
    fn an_existing_queue_is_success_not_a_failure() {
        // D2: `wrangler queues create` has no `--if-not-exists` (verified
        // against wrangler 4.103's `queues create --help`), so a create that
        // loses the race against a concurrent deploy reads its "already there"
        // complaint as the success it describes.
        //
        // This match is the driver's one unpinnable claim about wrangler's
        // prose: the wording is Cloudflare's API text, which wrangler renders
        // verbatim as `{message} [code: {code}]` and has no queue-specific
        // handling for. `queue_exists` is what keeps it off the common path.
        assert!(queue_already_exists(
            "✘ [ERROR] A queue with this name already exists"
        ));
        assert!(
            queue_already_exists("queue already exists"),
            "the match is on the phrase, whatever wrangler wraps it in"
        );
        // Anything else is a real failure and must surface with wrangler's own
        // words rather than being swallowed as idempotency.
        assert!(!queue_already_exists(
            "✘ [ERROR] Authentication error [10000]"
        ));
        assert!(!queue_already_exists(
            "✘ [ERROR] A request to the Cloudflare API failed."
        ));
        assert!(!queue_already_exists(""));
    }

    #[test]
    fn a_queue_two_contexts_share_is_attempted_once_per_run() {
        // Provisioning runs per context, but a queue is an account resource and
        // two contexts consuming `"jobs"` mean one queue — and the emitter's
        // duplicate-consumer check is context-scoped, so that project is legal.
        // Without the dedup, a shared queue costs one wrangler spawn per
        // consumer and the ADR's "one call per queue per deploy" is a claim the
        // code does not honour.
        let orders = Resources::default().consumes(&["jobs", "orders-only"]);
        let billing = Resources::default().consumes(&["billing-only", "jobs"]);
        let mut run = BTreeSet::new();
        assert_eq!(
            unattempted_queues(&orders, &mut run),
            names(&["jobs", "orders-only"])
        );
        assert_eq!(
            unattempted_queues(&billing, &mut run),
            names(&["billing-only"]),
            "`jobs` was already attempted by the orders context this run"
        );

        // A *fresh* run attempts everything again — the dedup is per run, which
        // is what keeps a queue deleted out-of-band self-healing (ADR 0194 D2).
        let mut later = BTreeSet::new();
        assert_eq!(
            unattempted_queues(&billing, &mut later),
            names(&["billing-only", "jobs"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_wrangler_complaint_is_read_from_whichever_stream_carries_it() {
        let said = |stdout: &str, stderr: &str| {
            wrangler_said(&std::process::Output {
                status: status(1 << 8),
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
            })
        };
        assert_eq!(said("", "boom"), "boom");
        assert_eq!(
            said("boom", ""),
            "boom",
            "wrangler is not consistent about which stream carries a complaint"
        );
        assert!(
            said("", "").contains("exited 1"),
            "a silent failure still reads as something, not a dangling colon"
        );
        // The consequence that makes this worth doing: a create whose
        // "already exists" lands on stdout must still be read as success, or
        // every re-deploy of a queue project fails.
        assert!(queue_already_exists(&said(
            "A queue with this name already exists",
            ""
        )));
    }

    #[test]
    fn the_queue_ledger_records_once_and_reports_whether_it_changed() {
        // The provision step writes the ledger only when the set actually
        // gained a name, so a re-run that provisions nothing writes nothing.
        let mut lock = DeployLock::default();
        assert!(lock.record_queue("default", "intake"), "the first is new");
        assert!(
            !lock.record_queue("default", "intake"),
            "the second changes nothing, so the ledger must not be rewritten"
        );
    }

    // ---- #601 slice 2: the deploy order --------------------------------

    #[test]
    fn a_binding_target_is_deployed_before_the_worker_that_binds_to_it() {
        // The defining contract. Cloudflare resolves a Service Binding at
        // upload and rejects a Worker whose target does not exist yet, so
        // this order is a correctness barrier, not a nicety.
        let g = graph(&[
            ("commerce-orders", &["commerce-payment"]),
            ("commerce-payment", &[]),
        ]);
        assert_eq!(
            deploy_order(&names(&["commerce-orders", "commerce-payment"]), &g),
            Ok(names(&["commerce-payment", "commerce-orders"])),
            "payment must be uploaded before the orders worker that binds to it"
        );
    }

    #[test]
    fn a_chain_deploys_from_the_far_end() {
        // a → b → c: c has no dependencies, so it goes first.
        let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        assert_eq!(
            deploy_order(&names(&["a", "b", "c"]), &g),
            Ok(names(&["c", "b", "a"]))
        );
    }

    #[test]
    fn a_diamond_deploys_the_shared_dependency_once_and_first() {
        //   a → b → d
        //   a → c → d
        let g = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
        let order = deploy_order(&names(&["a", "b", "c", "d"]), &g).expect("a diamond is acyclic");
        assert_eq!(
            order.len(),
            4,
            "the shared dependency is deployed once: {order:?}"
        );
        let at = |w: &str| order.iter().position(|o| o == w).expect("present");
        assert!(at("d") < at("b") && at("d") < at("c"), "{order:?}");
        assert!(at("b") < at("a") && at("c") < at("a"), "{order:?}");
    }

    #[test]
    fn the_order_is_deterministic_for_a_given_project() {
        // Two independent workers keep the discovered (sorted) order, so a
        // plan a reviewer approved is the plan that runs.
        let g = graph(&[("a", &[]), ("b", &[])]);
        assert_eq!(
            deploy_order(&names(&["a", "b"]), &g),
            Ok(names(&["a", "b"]))
        );
    }

    #[test]
    fn a_binding_outside_the_project_is_left_to_cloudflare() {
        // An externally-managed Worker has no node to order against; it must
        // not be invented into the order.
        let g = graph(&[("a", &["someone-elses-worker"])]);
        assert_eq!(deploy_order(&names(&["a"]), &g), Ok(names(&["a"])));
    }

    #[test]
    fn a_cycle_is_reported_rather_than_looping_forever() {
        // Defence in depth: `bynkc` rejects a `consumes` cycle
        // (`bynk.context.consumes_cycle`) before emit and `deploy` compiles
        // first, so this is unreachable through the compiler — but a
        // hand-edited build tree must not hang or overflow the stack.
        let g = graph(&[("a", &["b"]), ("b", &["a"])]);
        let Err(OrderError::Cycle(path)) = deploy_order(&names(&["a", "b"]), &g) else {
            panic!("a cycle must be reported");
        };
        assert_eq!(
            path.first(),
            path.last(),
            "the path closes into a loop: {path:?}"
        );
        assert!(path.contains(&"a".to_string()) && path.contains(&"b".to_string()));
    }

    #[test]
    fn a_self_binding_is_reported_not_looped() {
        let g = graph(&[("a", &["a"])]);
        assert!(matches!(
            deploy_order(&names(&["a"]), &g),
            Err(OrderError::Cycle(_))
        ));
    }

    // ---- #601 slice 2: `--context` dependency liveness (D4) ------------

    #[test]
    fn context_flag_names_a_dependency_that_was_never_deployed() {
        let g = graph(&[("orders", &["payment"]), ("payment", &[])]);
        assert_eq!(
            absent_dependencies("orders", &g, &DeployLock::default(), "default"),
            names(&["payment"]),
            "deploying orders alone would fail at upload — say which target is missing"
        );
        // Once payment is in the ledger, orders alone is fine.
        assert!(
            absent_dependencies("orders", &g, &lock_with_deployed(&["payment"]), "default")
                .is_empty()
        );
        // A worker with no bindings never has an absent dependency.
        assert!(absent_dependencies("payment", &g, &DeployLock::default(), "default").is_empty());
    }

    #[test]
    fn the_plan_distinguishes_a_first_deploy_from_a_redeploy() {
        let order = names(&["api"]);
        let declared = project(vec![("api", Resources::default())]);
        assert_eq!(
            plan_of(&order, &declared, &DeployLock::default()).contexts[0].action,
            "deploy"
        );
        assert_eq!(
            plan_of(&order, &declared, &lock_with_deployed(&["api"])).contexts[0].action,
            "redeploy",
            "a re-run re-pushes rather than skipping, so the plan must not say `deploy`"
        );
    }

    #[test]
    fn the_plan_carries_the_order_and_each_context_s_bindings() {
        let order = names(&["payment", "orders"]);
        let declared = project(vec![
            ("orders", Resources::default().binds(&["payment"])),
            ("payment", Resources::default()),
        ]);
        let plan = plan_of(&order, &declared, &DeployLock::default());
        assert_eq!(plan.order, vec!["payment", "orders"]);
        assert_eq!(plan.contexts[1].worker, "orders");
        assert_eq!(plan.contexts[1].binds_to, vec!["payment"]);
        assert!(plan.contexts[0].binds_to.is_empty());
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
    fn the_graph_is_the_binding_edges_of_the_resources_read() {
        let resources = project(vec![
            (
                "orders",
                Resources::default().binds(&["payment"]).consumes(&["q"]),
            ),
            ("payment", Resources::default().needs_kv()),
        ]);
        assert_eq!(
            service_graph(&resources),
            graph(&[("orders", &["payment"]), ("payment", &[])]),
            "the graph carries the binding edges and nothing else"
        );
    }

    #[test]
    fn dry_run_and_ci_gates_do_not_reach_mutation() {
        assert!(
            DeployOptions {
                dry_run: true,
                ..Default::default()
            }
            .dry_run
        );
        assert!(should_refuse_unrecorded_ci(true, None, true));
        assert!(!should_refuse_unrecorded_ci(true, Some("id"), true));
        assert!(!should_refuse_unrecorded_ci(true, None, false));
    }

    #[test]
    fn non_interactive_deploy_requires_yes() {
        assert!(!requires_interactive_confirmation(false, false));
        assert!(requires_interactive_confirmation(false, true));
        assert!(!requires_interactive_confirmation(true, false));
    }

    // ---- #602 slice 3: secrets at deploy (ADR 0195) --------------------

    #[test]
    fn a_secrets_file_is_read_as_names_and_values() {
        let parsed = parse_secrets_file(
            r#"
# a comment
API_KEY=sk_live_abc

export EXPORTED=fine
QUOTED="  spaced  "
SINGLE='single'
EMPTY=
WITH_EQUALS=a=b
"#,
        )
        .expect("a well-formed file parses");
        assert_eq!(
            parsed.get("API_KEY").map(String::as_str),
            Some("sk_live_abc")
        );
        assert_eq!(
            parsed.get("EXPORTED").map(String::as_str),
            Some("fine"),
            "an `export ` prefix is the shape a shell-sourced file already has"
        );
        assert_eq!(
            parsed.get("QUOTED").map(String::as_str),
            Some("  spaced  "),
            "quotes are what let a value keep spacing the line trim would eat"
        );
        assert_eq!(parsed.get("SINGLE").map(String::as_str), Some("single"));
        assert_eq!(
            parsed.get("EMPTY").map(String::as_str),
            Some(""),
            "an empty value is the user's to mean; only a missing *name* is an error"
        );
        assert_eq!(
            parsed.get("WITH_EQUALS").map(String::as_str),
            Some("a=b"),
            "only the first `=` separates — a value may contain more"
        );
        assert!(!parsed.contains_key("# a comment"));
    }

    #[test]
    fn a_malformed_secrets_line_is_an_error_naming_its_number() {
        // The consequence that makes this worth failing on: a silently skipped
        // line is a secret that never gets set, which surfaces in production as
        // a 401 rather than here as a typo.
        let err = parse_secrets_file("GOOD=1\nnonsense\n").expect_err("a bare word is not a pair");
        assert!(err.contains("line 2"), "{err}");
        let err = parse_secrets_file("A B=1\n").expect_err("a spaced name is a typo");
        assert!(err.contains("line 1"), "{err}");
        let err = parse_secrets_file("=1\n").expect_err("no name at all");
        assert!(err.contains("line 1"), "{err}");
        let err = parse_secrets_file("A=1\nA=2\n").expect_err("set twice");
        assert!(err.contains("line 2") && err.contains('A'), "{err}");
    }

    #[test]
    fn the_wanted_set_is_declared_union_supplied_and_declared_wins_the_mark() {
        let src = source(&[("STRIPE_KEY", "v")], &["PROBE_TOKEN"]);
        let wanted = wanted_secrets(&names(&["AUTH_JWT_SECRET"]), &[], &src);
        assert_eq!(
            wanted,
            vec![
                WantedSecret {
                    name: "AUTH_JWT_SECRET".into(),
                    origin: Origin::Declared
                },
                WantedSecret {
                    name: "PROBE_TOKEN".into(),
                    origin: Origin::Supplied
                },
                WantedSecret {
                    name: "STRIPE_KEY".into(),
                    origin: Origin::Supplied
                },
            ]
        );

        // A name that is both is `declared`: the compiler's knowledge is the
        // more informative label, and it is why a missing value is an error.
        let both = wanted_secrets(&names(&["SHARED"]), &[], &source(&[("SHARED", "v")], &[]));
        assert_eq!(both[0].origin, Origin::Declared);

        // No input at all: nothing to set, and in particular no invented name.
        assert!(wanted_secrets(&[], &[], &SecretSource::default()).is_empty());
    }

    #[test]
    fn set_if_absent_skips_a_present_secret_unless_forced() {
        let present: BTreeSet<String> = ["THERE".to_string()].into_iter().collect();
        assert_eq!(
            secret_action("THERE", Some(&present), false),
            SecretAction::SkipPresent,
            "the default must not cut a fresh Cloudflare version every deploy"
        );
        assert_eq!(
            secret_action("THERE", Some(&present), true),
            SecretAction::Overwrite
        );
        assert_eq!(
            secret_action("ABSENT", Some(&present), false),
            SecretAction::Set
        );

        // `None` is "could not ask" — a Worker that does not exist yet, or an
        // auth failure. Both mean try: a first deploy genuinely has none, and a
        // real auth failure surfaces as the put's own complaint rather than as
        // a diagnosis invented here.
        assert_eq!(secret_action("THERE", None, false), SecretAction::Set);
    }

    #[test]
    fn an_unaskable_account_does_not_fail_a_redeploy_that_supplies_nothing() {
        // The regression this encodes: `secret_action` reads "could not ask" as
        // `Set`, so a redeploy whose secrets are all already on the account —
        // the common CI shape, no --secrets-file, no TTY — would try to resolve
        // a value it has no reason to have, find none, and fail. A *read*
        // failure in an advisory check would block a deploy that works, and both
        // the code's own comment and ADR 0195 D4 claimed it would not.
        //
        // `tolerate_unresolvable` is the rule: it holds only where presence is
        // unknown *and* the Worker has been live before.
        assert!(
            tolerate_unresolvable(None, false),
            "a redeploy with no presence answer leaves the secret alone"
        );
        // A first deploy gets no benefit of the doubt — its Worker is new, so an
        // unresolvable declared secret really is missing.
        assert!(!tolerate_unresolvable(None, true));
        // Presence known: the answer is authoritative either way, so an
        // unresolvable secret we know to be absent is a real failure.
        let none_present = BTreeSet::new();
        assert!(!tolerate_unresolvable(Some(&none_present), false));
        assert!(!tolerate_unresolvable(Some(&none_present), true));
    }

    #[test]
    fn a_value_comes_from_the_file_before_the_environment() {
        let src = source(&[("A", "from-file")], &[]);
        assert_eq!(
            value_from("A", &src, Some("from-env".into())).as_deref(),
            Some("from-file"),
            "the file is the more specific instruction, so it wins"
        );
        assert_eq!(
            value_from("B", &src, Some("from-env".into())).as_deref(),
            Some("from-env")
        );
        assert_eq!(
            value_from("C", &src, None),
            None,
            "nothing left but a prompt — and, with no terminal, a named error"
        );
    }

    #[test]
    fn wranglers_secret_list_is_read_as_names() {
        // Wrangler's shape (`secret list --format json`, wrangler 4.103).
        assert_eq!(
            parse_secret_list(
                r#"[{"name":"A","type":"secret_text"},{"name":"B","type":"secret_text"}]"#
            ),
            Some(["A".to_string(), "B".to_string()].into_iter().collect()),
        );
        assert_eq!(parse_secret_list("[]"), Some(BTreeSet::new()));
        // Anything unreadable is "could not tell" rather than "none present" —
        // and `secret_action` reads that as "try", which is idempotent.
        assert_eq!(parse_secret_list("not json"), None);
        assert_eq!(parse_secret_list(r#"{"unexpected":"shape"}"#), None);
    }

    #[test]
    fn a_missing_secret_names_itself_and_says_what_it_costs() {
        // The error is the whole mitigation for the silent-blank risk, so it
        // must name the secret and the remedy rather than saying "failed".
        let declared = missing_secret_message("AUTH_JWT_SECRET", Origin::Declared, "api");
        assert!(declared.contains("AUTH_JWT_SECRET"), "{declared}");
        assert!(declared.contains("api"), "{declared}");
        assert!(
            declared.contains("401"),
            "a declared secret's absence is fail-closed — say so: {declared}"
        );
        let supplied = missing_secret_message("STRIPE_KEY", Origin::Supplied, "api");
        assert!(supplied.contains("STRIPE_KEY"), "{supplied}");
        assert!(
            !supplied.contains("401"),
            "a supplied name is not known to gate auth, so do not claim it does: {supplied}"
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

    #[test]
    fn a_read_is_advisory_and_a_declared_secret_is_not() {
        // ADR 0196 D3, the increment's load-bearing distinction. `Secrets.get`
        // returns `Option`, so an unsupplied read is a `None` the program may be
        // happy about; an unset auth secret 401s every request. Erroring on the
        // first would refuse to deploy a legal program.
        assert!(Origin::Declared.required());
        assert!(!Origin::Read.required());
        assert!(!Origin::Supplied.required());
        assert_eq!(Origin::Read.label(), "read");
    }

    #[test]
    fn the_marks_are_ordered_by_how_much_the_compiler_knows() {
        // A name can be in more than one class. The strongest thing known about
        // it is the most useful label — and `declared` is the only one that
        // makes a missing value fatal, so it must win.
        let src = source(&[("BOTH", "v"), ("SUPPLIED_ONLY", "v")], &[]);
        let wanted = wanted_secrets(&names(&["BOTH"]), &names(&["BOTH", "READ_ONLY"]), &src);
        let mark = |n: &str| {
            wanted
                .iter()
                .find(|w| w.name == n)
                .unwrap_or_else(|| panic!("{n} is wanted"))
                .origin
        };
        assert_eq!(
            mark("BOTH"),
            Origin::Declared,
            "declared beats read and supplied"
        );
        assert_eq!(mark("READ_ONLY"), Origin::Read);
        assert_eq!(mark("SUPPLIED_ONLY"), Origin::Supplied);
    }

    #[test]
    fn the_plan_never_carries_a_secret_value() {
        // ADR 0195 D1's headline guarantee, asserted rather than described: the
        // plan is printed and piped to CI logs, so a value reaching it would be
        // a leak in the most-copied surface `deploy` has.
        const SENTINEL: &str = "sk_live_do_not_leak_me";
        let order = names(&["api"]);
        let declared = project(vec![(
            "api",
            Resources::default().declares(&["AUTH_JWT_SECRET"]),
        )]);
        let plan = derive_plan(
            &order,
            &declared,
            &DeployLock::default(),
            &source(&[("STRIPE_KEY", SENTINEL)], &[]),
            false,
            "default",
        );
        for format in [DeployFormat::Short, DeployFormat::Json] {
            let rendered = plan_report(&plan, format);
            assert!(
                !rendered.contains(SENTINEL),
                "a secret value reached the plan ({format:?}): {rendered}"
            );
            assert!(
                rendered.contains("STRIPE_KEY"),
                "the name is the whole point of the line: {rendered}"
            );
        }
    }

    #[test]
    fn the_ledger_never_carries_a_secret() {
        // The other half of D1: the ledger is a *committed* file, so a value —
        // or even a name — reaching it would be published, not merely logged.
        // The type has no field for one; this pins that as a property rather
        // than an observation about today's struct.
        const SENTINEL: &str = "sk_live_do_not_commit_me";
        let mut lock = DeployLock::default();
        lock.record_deployed("default", "api", Some(Default::default()));
        lock.record_queue("default", "intake");
        let text = toml::to_string_pretty(&lock).expect("the ledger serialises");
        assert!(!text.contains(SENTINEL));
        assert!(
            !text.to_ascii_lowercase().contains("secret"),
            "the ledger records no secret at all — not even its presence: {text}"
        );
    }

    // ---- v0.177 (#643): the deploy-time contract-skew gate ----

    fn lock_with(worker: &str, contracts: &[(&str, &str)]) -> DeployLock {
        let mut lock = DeployLock::default();
        lock.record_deployed(
            "default",
            worker,
            Some(
                contracts
                    .iter()
                    .map(|(s, h)| (s.to_string(), h.to_string()))
                    .collect(),
            ),
        );
        lock
    }

    fn expects(dep: &str, svc: &str, hash: &str) -> BTreeMap<String, BTreeMap<String, String>> {
        BTreeMap::from([(
            dep.to_string(),
            BTreeMap::from([(svc.to_string(), hash.to_string())]),
        )])
    }

    #[test]
    fn matching_contracts_are_not_a_skew() {
        let lock = lock_with("app-b", &[("whoami", "317bdd3de84d2176")]);
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &lock,
            |c| c.replace('.', "-"),
            "default",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_changed_contract_is_a_skew() {
        // The scenario the increment exists for: B was redeployed with a new
        // contract, and A still stamps the old one.
        let lock = lock_with("app-b", &[("whoami", "ffffffffffffffff")]);
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &lock,
            |c| c.replace('.', "-"),
            "default",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].dependency, "app.b");
        assert_eq!(found[0].service, "whoami");
        assert_eq!(found[0].expected, "317bdd3de84d2176");
        assert_eq!(found[0].live, "ffffffffffffffff");
    }

    #[test]
    fn a_service_the_live_callee_no_longer_provides_is_a_skew() {
        let lock = lock_with("app-b", &[("somethingElse", "317bdd3de84d2176")]);
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &lock,
            |c| c.replace('.', "-"),
            "default",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].live, "<absent>");
    }

    #[test]
    fn a_ledger_with_no_contract_record_yields_no_finding() {
        // Silence is not a match. A dependency deployed by a pre-v0.177 driver
        // has no contract record, and the gate must report only what it *knows*
        // is skewed — never what it merely cannot rule out. The runtime check is
        // the backstop for exactly this case, so a false accusation here would
        // block a legitimate deploy for no gain.
        let mut lock = DeployLock::default();
        lock.record_deployed("default", "app-b", None);
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &lock,
            |c| c.replace('.', "-"),
            "default",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_callee_that_now_provides_nothing_is_a_total_skew() {
        // The counterpart to the rule above, and why the sentinel is `Option`
        // rather than an empty map: a callee that removed *all* its `on call`
        // services emits no manifest, so a bare-map field would record `{}` —
        // indistinguishable from "old ledger" — and the gate would wave through
        // the most complete skew there is. `Some({})` says "known to provide
        // nothing", which is a finding.
        let lock = lock_with("app-b", &[]);
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &lock,
            |c| c.replace('.', "-"),
            "default",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].live, "<absent>");
    }

    #[test]
    fn a_never_deployed_dependency_yields_no_finding_here() {
        // That is D4's existing job (`absent_dependencies`), and it runs first.
        // Reporting it twice, in two vocabularies, would only confuse.
        let found = contract_skews(
            &expects("app.b", "whoami", "317bdd3de84d2176"),
            &DeployLock::default(),
            |c| c.replace('.', "-"),
            "default",
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
