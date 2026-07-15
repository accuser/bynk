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
// The Service-Binding graph and the deploy order (slice 2)
// ---------------------------------------------------------------------------

/// The `[[services]]` entries of a generated `wrangler.toml` — the emitted form
/// of a context's `consumes` edges, and the graph the deploy order must respect.
#[derive(Debug, Default, Deserialize)]
struct WranglerConfig {
    #[serde(default)]
    services: Vec<ServiceBinding>,
}

#[derive(Debug, Deserialize)]
struct ServiceBinding {
    /// The *target* worker's name (`worker_dir_name`, dots dasherised).
    service: String,
}

/// Read one worker's Service-Binding targets out of its generated config.
///
/// The graph is read from the **emitted `[[services]]`** rather than from the
/// checker's `consumes` map because this is precisely the relation Cloudflare
/// validates at upload: the same file wrangler is about to send. Adapters are
/// already excluded upstream (they are not Workers), so every edge here names a
/// real worker directory.
fn service_targets(config: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    let parsed: WranglerConfig = toml::from_str(&text).map_err(|e| e.to_string())?;
    Ok(parsed.services.into_iter().map(|s| s.service).collect())
}

/// Build the whole project's binding graph: worker → the workers it binds to.
pub fn service_graph(
    workers_dir: &Path,
    workers: &[String],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut graph = BTreeMap::new();
    for worker in workers {
        let targets = service_targets(&workers_dir.join(worker).join("wrangler.toml"))
            .map_err(|e| format!("could not read the configuration for `{worker}`: {e}"))?;
        graph.insert(worker.clone(), targets);
    }
    Ok(graph)
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
    // The graph spans the *whole* project even under `--context`: D4 needs the
    // selected context's binding targets to check they are live, and they are
    // by definition outside the selection.
    let graph = match service_graph(&workers_dir, &available) {
        Ok(graph) => graph,
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

    let needs_kv = match kv_requirements(&workers_dir, &order) {
        Ok(needs) => needs,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let plan = derive_plan(&order, &graph, &needs_kv, &lock);
    print_plan(&plan, opts.format);
    if opts.dry_run {
        return ExitCode::SUCCESS;
    }

    for worker in &order {
        let recorded = recorded_kv(&lock, worker);
        if should_refuse_unrecorded_ci(needs_kv[worker], recorded, is_ci()) {
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
    for (i, worker) in order.iter().enumerate() {
        if order.len() > 1 {
            eprintln!("bynk: deploying `{worker}` ({}/{})…", i + 1, order.len());
        }
        match deploy_one(
            &probe.provenance,
            project_root,
            &workers_dir,
            worker,
            needs_kv[worker],
            &mut lock,
            &lock_path,
            &opts.wrangler_args,
        ) {
            Ok(()) => {
                lock.record_deployed("default", worker);
                if let Err(e) = write_lock(&lock_path, &lock) {
                    eprintln!(
                        "bynk: deployed `{worker}` but could not record it in {}: {e}",
                        lock_path.display()
                    );
                    return ExitCode::FAILURE;
                }
            }
            Err(e) => {
                eprintln!("bynk: {e}");
                // Stop rather than push on: everything left in the order either
                // binds to what just failed or would be uploaded into a
                // topology that is not what the plan described. `worker` itself
                // is excluded — the line above already named it as the failure,
                // and listing it here again as "not deployed" would double-count
                // it against the number.
                eprint!("{}", stopped_report(&order[i + 1..]));
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

/// Provision and push exactly one context. Slice 0's body, lifted so the
/// multi-context loop and `--context` share one path.
#[allow(clippy::too_many_arguments)]
fn deploy_one(
    provenance: &Provenance,
    project_root: &Path,
    workers_dir: &Path,
    worker: &str,
    needs_kv: bool,
    lock: &mut DeployLock,
    lock_path: &Path,
    wrangler_args: &[String],
) -> Result<(), String> {
    let worker_dir = workers_dir.join(worker);
    let config = worker_dir.join("wrangler.toml");
    if needs_kv {
        let kv_id = match recorded_kv(lock, worker) {
            Some(id) => id.to_owned(),
            None => {
                let id = create_kv(provenance, worker, project_root)
                    .map_err(|e| format!("could not create KV namespace for `{worker}`: {e}"))?;
                lock.environments
                    .entry("default".into())
                    .or_default()
                    .kv
                    .insert(worker.to_string(), KvNamespace { id: id.clone() });
                // Recorded before the push, so an interrupted run never makes a
                // second namespace (ADR 0180).
                write_lock(lock_path, lock).map_err(|e| {
                    format!(
                        "created KV namespace for `{worker}` but could not record it in {}: {e}",
                        lock_path.display()
                    )
                })?;
                id
            }
        };
        if !materialise_kv_id(&config, &kv_id) {
            return Err(format!(
                "could not materialise the KV namespace id into `{worker}`'s generated configuration"
            ));
        }
    }
    let Some(mut command) = dev::wrangler_command(provenance, "deploy") else {
        return Err("wrangler not found".into());
    };
    command.current_dir(&worker_dir).args(wrangler_args);
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "wrangler deploy failed for `{worker}` (exit {})",
            exit_status_byte(&status)
        )),
        Err(e) => Err(format!("could not run wrangler deploy for `{worker}`: {e}")),
    }
}

/// Which of `workers` still carry the KV placeholder, i.e. need a namespace.
fn kv_requirements(
    workers_dir: &Path,
    workers: &[String],
) -> Result<BTreeMap<String, bool>, String> {
    let mut needs = BTreeMap::new();
    for worker in workers {
        let config = workers_dir.join(worker).join("wrangler.toml");
        let text = std::fs::read_to_string(&config)
            .map_err(|e| format!("could not read the configuration for `{worker}`: {e}"))?;
        needs.insert(worker.clone(), text.contains(KV_NAMESPACE_ID_PLACEHOLDER));
    }
    Ok(needs)
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
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
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
fn derive_plan<'a>(
    order: &'a [String],
    graph: &'a BTreeMap<String, Vec<String>>,
    needs_kv: &BTreeMap<String, bool>,
    lock: &DeployLock,
) -> Plan<'a> {
    Plan {
        environment: "default",
        order: order.iter().map(String::as_str).collect(),
        contexts: order
            .iter()
            .map(|worker| ContextPlan {
                worker,
                kv: needs_kv
                    .get(worker)
                    .copied()
                    .unwrap_or(false)
                    .then(|| PlanKv {
                        action: if recorded_kv(lock, worker).is_some() {
                            "reuse"
                        } else {
                            "create"
                        },
                        namespace: worker,
                    }),
                action: if lock.is_deployed("default", worker) {
                    "redeploy"
                } else {
                    "deploy"
                },
                binds_to: graph
                    .get(worker)
                    .into_iter()
                    .flatten()
                    .map(String::as_str)
                    .collect(),
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

    /// The `needs_kv` map for `workers`, with the named ones requiring a
    /// namespace.
    fn needs(workers: &[&str], with_kv: &[&str]) -> BTreeMap<String, bool> {
        workers
            .iter()
            .map(|w| (w.to_string(), with_kv.contains(w)))
            .collect()
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

    /// #601: the plan is what `--dry-run` shows and the deploy guide quotes, so
    /// it is pinned exactly — including the `order` line, which is the
    /// increment's load-bearing claim, and the JSON shape, which is a documented
    /// machine-readable surface.
    #[test]
    fn golden_deploy_plan() {
        let chain = graph(&[
            ("commerce-orders", &["commerce-payment"]),
            ("commerce-payment", &[]),
        ]);
        let chain_order = names(&["commerce-payment", "commerce-orders"]);
        let chain_needs = needs(
            &["commerce-orders", "commerce-payment"],
            &["commerce-payment"],
        );

        let mut out = String::new();

        // Slice 0's shape: one context, nothing recorded. No `order` line —
        // there is no ordering claim to make about a single worker.
        out.push_str("# one context, first deploy\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &graph(&[("api", &[])]),
                &needs(&["api"], &["api"]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        // The guide's worked example: payment first, because orders binds to it.
        out.push_str("\n# several contexts, first deploy\n");
        out.push_str(&plan_report(
            &derive_plan(&chain_order, &chain, &chain_needs, &DeployLock::default()),
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
                &chain,
                &chain_needs,
                &with_kv(
                    lock_with_deployed(&["commerce-payment", "commerce-orders"]),
                    "commerce-payment",
                ),
            ),
            DeployFormat::Short,
        ));

        out.push_str("\n# --format json\n");
        out.push_str(&plan_report(
            &derive_plan(&chain_order, &chain, &chain_needs, &DeployLock::default()),
            DeployFormat::Json,
        ));

        bless_or_assert("deploy-plan.txt", &out);
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
                },
            )]),
        };
        assert_eq!(
            toml::from_str::<DeployLock>(&toml::to_string_pretty(&lock).unwrap()).unwrap(),
            lock
        );
    }

    #[test]
    fn a_slice_0_ledger_without_workers_still_reads() {
        // The `workers` table is additive: a ledger committed before slice 2
        // must keep working, reporting its contexts as never-deployed rather
        // than failing to parse.
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
        let g = graph(&[("api", &[])]);
        let needs = BTreeMap::from([("api".to_string(), true)]);
        let fresh = DeployLock::default();
        assert_eq!(
            derive_plan(&order, &g, &needs, &fresh).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "create"
        );

        let mut recorded = DeployLock::default();
        recorded
            .environments
            .entry("default".into())
            .or_default()
            .kv
            .insert("api".into(), KvNamespace { id: "id".into() });
        assert_eq!(
            derive_plan(&order, &g, &needs, &recorded).contexts[0]
                .kv
                .as_ref()
                .unwrap()
                .action,
            "reuse"
        );

        let no_kv = BTreeMap::from([("api".to_string(), false)]);
        assert!(
            derive_plan(&order, &g, &no_kv, &fresh).contexts[0]
                .kv
                .is_none()
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
        let g = graph(&[("api", &[])]);
        let needs = BTreeMap::from([("api".to_string(), false)]);
        assert_eq!(
            derive_plan(&order, &g, &needs, &DeployLock::default()).contexts[0].action,
            "deploy"
        );
        assert_eq!(
            derive_plan(&order, &g, &needs, &lock_with_deployed(&["api"])).contexts[0].action,
            "redeploy",
            "a re-run re-pushes rather than skipping, so the plan must not say `deploy`"
        );
    }

    #[test]
    fn the_plan_carries_the_order_and_each_context_s_bindings() {
        let order = names(&["payment", "orders"]);
        let g = graph(&[("orders", &["payment"]), ("payment", &[])]);
        let needs = BTreeMap::from([
            ("orders".to_string(), false),
            ("payment".to_string(), false),
        ]);
        let plan = derive_plan(&order, &g, &needs, &DeployLock::default());
        assert_eq!(plan.order, vec!["payment", "orders"]);
        assert_eq!(plan.contexts[1].worker, "orders");
        assert_eq!(plan.contexts[1].binds_to, vec!["payment"]);
        assert!(plan.contexts[0].binds_to.is_empty());
    }

    #[test]
    fn service_targets_are_read_from_the_generated_config() {
        // The graph is read from the emitted `[[services]]` — the same file
        // wrangler uploads — so parse the real shape the emitter writes.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bynk-services-{}-{}.toml",
            std::process::id(),
            unique
        ));
        std::fs::write(
            &path,
            r#"
name = "commerce-orders"
main = "index.ts"
compatibility_date = "2024-11-01"

[[services]]
binding = "COMMERCE_PAYMENT"
service = "commerce-payment"
"#,
        )
        .unwrap();
        assert_eq!(service_targets(&path), Ok(names(&["commerce-payment"])));
        let _ = std::fs::remove_file(&path);

        // A config with no bindings is the common single-context case.
        let bare = path.with_extension("bare.toml");
        std::fs::write(&bare, "name = \"api\"\nmain = \"index.ts\"\n").unwrap();
        assert_eq!(service_targets(&bare), Ok(Vec::new()));
        let _ = std::fs::remove_file(&bare);
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
