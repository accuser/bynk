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
#[derive(Debug, Default, PartialEq, Eq)]
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
}

/// Read everything `deploy` acts on out of one worker's generated config.
fn read_resources(config: &Path) -> Result<Resources, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    let parsed: WranglerConfig = toml::from_str(&text).map_err(|e| e.to_string())?;
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
) -> Vec<String> {
    graph
        .get(worker)
        .into_iter()
        .flatten()
        .filter(|target| !lock.is_deployed("default", target))
        .cloned()
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct DeployLock {
    #[serde(default = "lock_version")]
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
}

impl DeployLock {
    fn is_deployed(&self, environment: &str, worker: &str) -> bool {
        self.environments
            .get(environment)
            .and_then(|env| env.workers.get(worker))
            .is_some_and(|record| record.deployed)
    }

    fn record_deployed(&mut self, environment: &str, worker: &str) {
        self.environments
            .entry(environment.to_string())
            .or_default()
            .workers
            .insert(worker.to_string(), WorkerRecord { deployed: true });
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
    environment: &'static str,
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

#[derive(Debug, Serialize)]
struct PlanMigration<'a> {
    tag: &'a str,
    /// Always `wrangler deploy`, and that is the point: the field names an
    /// owner other than `bynk`, which is the whole content of the advisory
    /// (ADR 0194 D1). A consumer reading the plan learns that this line is not
    /// a claim about the account's state, without having to know the ADR.
    applied_by: &'static str,
}

/// Run the slice-0 single-context deployment pipeline.
pub fn run(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    project_root: &Path,
    node_floor: u32,
    opts: &DeployOptions,
) -> ExitCode {
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
        let absent = absent_dependencies(worker, &graph, &lock);
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

    let plan = derive_plan(&order, &resources, &lock);
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
        let recorded = recorded_kv(&lock, worker);
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
            &opts.wrangler_args,
        ) {
            Ok(Pushed::Ok) => {
                lock.record_deployed("default", worker);
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
    wrangler_args: &[String],
) -> Result<Pushed, DeployFailure> {
    let worker_dir = workers_dir.join(worker);
    let config = worker_dir.join("wrangler.toml");
    if declared.needs_kv {
        let kv_id = match recorded_kv(lock, worker) {
            Some(id) => id.to_owned(),
            None => {
                let id = create_kv(provenance, worker, project_root).map_err(|e| {
                    DeployFailure::driver(format!(
                        "could not create KV namespace for `{worker}`: {e}"
                    ))
                })?;
                lock.environments
                    .entry("default".into())
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
        if !materialise_kv_id(&config, &kv_id) {
            return Err(DeployFailure::driver(format!(
                "could not materialise the KV namespace id into `{worker}`'s generated configuration"
            )));
        }
    }
    // Reconciled against the account on every run, never against the ledger:
    // the ledger's queue set is a planning aid, so trusting it to skip would
    // leave a queue deleted out-of-band un-recreated and the push failing
    // against a binding with nothing behind it (ADR 0194 D2). Once per run,
    // not once per consuming context — a queue two contexts consume is one
    // queue. `wrangler deploy` will not create it for us: it checks and fails
    // with "To create it, run: wrangler queues create", so this step is the one
    // that makes such a project deployable at all.
    for queue in unattempted_queues(declared, attempted_queues) {
        if !queue_exists(provenance, &queue, project_root) {
            create_queue(provenance, &queue, project_root).map_err(|e| {
                DeployFailure::driver(format!("could not create the queue `{queue}`: {e}"))
            })?;
        }
        // Recorded before the push, as KV is: what the ledger claims is only
        // ever what it watched succeed. Recorded for a queue that was already
        // there, too — the set's use is the plan's `create`/`reuse` wording, and
        // "we confirmed this exists" is exactly what makes `reuse` the true word.
        if lock.record_queue("default", &queue) {
            write_lock(lock_path, lock).map_err(|e| {
                DeployFailure::driver(format!(
                    "provisioned the queue `{queue}` but could not record it in {}: {e}",
                    lock_path.display()
                ))
            })?;
        }
    }
    let Some(mut command) = dev::wrangler_command(provenance, "deploy") else {
        return Err(DeployFailure::driver("wrangler not found".into()));
    };
    command.current_dir(&worker_dir).args(wrangler_args);
    match command.status() {
        Ok(status) => wrangler_outcome(worker, &status),
        Err(e) => Err(DeployFailure::driver(format!(
            "could not run wrangler deploy for `{worker}`: {e}"
        ))),
    }
}

fn recorded_kv<'a>(lock: &'a DeployLock, worker: &str) -> Option<&'a str> {
    lock.environments
        .get("default")
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
    let lock: DeployLock =
        toml::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if lock.version != lock_version() {
        return Err(format!("unsupported deploy lock version {}", lock.version));
    }
    Ok(lock)
}

fn write_lock(path: &Path, lock: &DeployLock) -> Result<(), String> {
    std::fs::write(
        path,
        toml::to_string_pretty(lock).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
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

/// Fill a generated worker configuration from the committed deploy ledger.
/// This is shared with `bynk dev -- --remote`; local dev leaves placeholders
/// alone because Miniflare does not read the Cloudflare namespace id.
pub fn materialise_deploy_state(
    project_root: &Path,
    worker: &str,
    config: &Path,
) -> Result<bool, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    if !text.contains(KV_NAMESPACE_ID_PLACEHOLDER) {
        return Ok(false);
    }
    let lock = read_lock(&project_root.join(LOCK_FILE))?;
    let Some(id) = lock
        .environments
        .get("default")
        .and_then(|environment| environment.kv.get(worker))
        .map(|namespace| namespace.id.as_str())
    else {
        return Err(format!(
            "remote KV for `{worker}` has not been provisioned; run `bynk deploy` first"
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
) -> Plan<'a> {
    Plan {
        environment: "default",
        order: order.iter().map(String::as_str).collect(),
        contexts: order
            .iter()
            .map(|worker| {
                let declared = &resources[worker];
                ContextPlan {
                    worker,
                    kv: declared.needs_kv.then(|| PlanKv {
                        action: if recorded_kv(lock, worker).is_some() {
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
                            action: if lock.has_queue("default", queue) {
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
                    action: if lock.is_deployed("default", worker) {
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

    fn lock_with_deployed(workers: &[&str]) -> DeployLock {
        let mut lock = DeployLock::default();
        for worker in workers {
            lock.record_deployed("default", worker);
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
            &derive_plan(
                &names(&["api"]),
                &project(vec![("api", Resources::default().needs_kv())]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        // The guide's worked example: payment first, because orders binds to it.
        out.push_str("\n# several contexts, first deploy\n");
        out.push_str(&plan_report(
            &derive_plan(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Short,
        ));

        // A re-run re-pushes rather than skipping, so the word is `redeploy`
        // and the namespace is reused. The ledger records the KV *before* the
        // push (ADR 0180), so a deployed context always has its namespace
        // recorded too — depict that state, not an unreachable one.
        out.push_str("\n# several contexts, already live — a re-run re-pushes\n");
        out.push_str(&plan_report(
            &derive_plan(
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
            &derive_plan(
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
            &derive_plan(
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

        out.push_str("\n# --format json\n");
        out.push_str(&plan_report(
            &derive_plan(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Json,
        ));

        // The JSON shape of slice 1's kinds — the surface a CI job reads to
        // learn that the migration is not ours to claim.
        out.push_str("\n# --format json, with a queue and a migration\n");
        out.push_str(&plan_report(
            &derive_plan(
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
                    workers: BTreeMap::from([("api".into(), WorkerRecord { deployed: true })]),
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
        assert_eq!(recorded_kv(&lock, "api"), Some("abc"));
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
        lock.record_deployed("default", "api");
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

    #[test]
    fn plan_creates_or_reuses_kv_from_the_ledger() {
        let order = names(&["api"]);
        let declared = project(vec![("api", Resources::default().needs_kv())]);
        let fresh = DeployLock::default();
        assert_eq!(
            derive_plan(&order, &declared, &fresh).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "create"
        );
        assert_eq!(
            derive_plan(&order, &declared, &with_kv(DeployLock::default(), "api")).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "reuse"
        );
        assert!(
            derive_plan(
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
            |lock: &DeployLock| derive_plan(&order, &declared, lock).contexts[0].queues[0].action;
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
            derive_plan(
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
            let plan = derive_plan(&order, &declared, &lock);
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
            derive_plan(
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
            absent_dependencies("orders", &g, &DeployLock::default()),
            names(&["payment"]),
            "deploying orders alone would fail at upload — say which target is missing"
        );
        // Once payment is in the ledger, orders alone is fine.
        assert!(absent_dependencies("orders", &g, &lock_with_deployed(&["payment"])).is_empty());
        // A worker with no bindings never has an absent dependency.
        assert!(absent_dependencies("payment", &g, &DeployLock::default()).is_empty());
    }

    #[test]
    fn the_plan_distinguishes_a_first_deploy_from_a_redeploy() {
        let order = names(&["api"]);
        let declared = project(vec![("api", Resources::default())]);
        assert_eq!(
            derive_plan(&order, &declared, &DeployLock::default()).contexts[0].action,
            "deploy"
        );
        assert_eq!(
            derive_plan(&order, &declared, &lock_with_deployed(&["api"])).contexts[0].action,
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
        let plan = derive_plan(&order, &declared, &DeployLock::default());
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
}
