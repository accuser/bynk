use super::*;

/// How one context's push ended, short of failing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pushed {
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
pub(crate) struct DeployFailure {
    pub(crate) message: String,
    pub(crate) code: u8,
}

impl DeployFailure {
    pub(crate) fn driver(message: String) -> Self {
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
pub(crate) fn deploy_one(
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
    // Slice 5, DECISION B: `Some` when `run()` fetched the account's live KV
    // namespace ids once, up front. A recorded id absent from this set is
    // exactly as untrustworthy as no record at all — re-provision rather than
    // inject a dead id. `None` means the fetch was skipped or failed; the
    // recorded id is trusted unconditionally, as it always was pre-slice-5,
    // rather than blocking a deploy on a fetch that didn't happen.
    live_kv_ids: Option<&BTreeSet<String>>,
) -> Result<Pushed, DeployFailure> {
    let worker_dir = workers_dir.join(worker);
    let config = worker_dir.join("wrangler.toml");
    let mut kv_id = None;
    if declared.needs_kv {
        let recorded = recorded_kv(lock, worker, environment).map(str::to_owned);
        let trust_recorded = should_trust_recorded_kv(recorded.as_deref(), live_kv_ids);
        let id = if trust_recorded {
            recorded.expect("trust_recorded is only true when recorded is Some")
        } else {
            let id = create_kv(provenance, worker, project_root).map_err(|e| {
                DeployFailure::driver(format!("could not create KV namespace for `{worker}`: {e}"))
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
    let Some(mut command) = workers::wrangler_command(provenance, "deploy") else {
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

pub(crate) fn whoami(provenance: &Provenance) -> bool {
    let Some(mut command) = workers::wrangler_command(provenance, "whoami") else {
        return false;
    };
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn create_kv(provenance: &Provenance, name: &str, project_root: &Path) -> Result<String, String> {
    let Some(mut command) = workers::wrangler_command(provenance, "kv") else {
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

/// Every KV namespace id currently on the account — slice 5, DECISION B.
///
/// Fetched **once per `bynk deploy` run**, not once per context: this is an
/// account-wide list, so calling it from inside `deploy_one` would pay one
/// identical round-trip per KV-bearing context on every redeploy. `run()`
/// calls this once, before the per-context loop, and threads the result
/// through — see `deploy_one`'s `live_kv_ids` parameter.
///
/// `None` means "could not ask" (no wrangler, network/auth failure, or an
/// unparseable response) — read the same way [`list_secrets`]'s `None` is:
/// the caller falls back to trusting whatever the ledger already says, rather
/// than blocking every deploy on this call succeeding.
///
/// **Completeness (review, #840):** Cloudflare's list-namespaces endpoint is
/// itself paginated, but `wrangler`'s `listKVNamespaces` (`workers-sdk`
/// `packages/wrangler/src/kv/helpers.ts`) loops `page`/`per_page` until a
/// short page comes back, aggregating every page before this command prints
/// — confirmed against wrangler's own source, not assumed. A truncated
/// response is not a live risk on the CLI path this calls.
pub(crate) fn live_kv_namespace_ids(
    provenance: &Provenance,
    project_root: &Path,
) -> Option<BTreeSet<String>> {
    let mut command = workers::wrangler_command(provenance, "kv")?;
    // No `--format json`: unlike `secret list`/`queues`, this command has no
    // such flag at all — confirmed against wrangler 4.103 (`Unknown argument:
    // format`, exit 1). Its *default* output is already a raw JSON array, so
    // this is not a missing-flag gap, just a different default per command.
    let output = command
        .arg("namespace")
        .arg("list")
        .current_dir(project_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_kv_namespace_ids(&String::from_utf8_lossy(&output.stdout))
}

/// The ids in `wrangler kv namespace list`'s (default, already-JSON) output —
/// the same structural shape [`parse_secret_list`] reads, `"id"` instead of
/// `"name"`.
fn parse_kv_namespace_ids(stdout: &str) -> Option<BTreeSet<String>> {
    let parsed: serde_json::Value = serde_json::from_str(stdout).ok()?;
    Some(
        parsed
            .as_array()?
            .iter()
            .filter_map(|entry| entry.get("id")?.as_str().map(str::to_owned))
            .collect(),
    )
}

/// Create `name`, or accept that it is already there.
///
/// Unlike KV there is no id to scrape: Cloudflare addresses a queue by the name
/// `from queue("n")` already gave it, so a successful create and an
/// already-existing queue are the same end state and both return `Ok`
/// (ADR 0194 D2).
fn create_queue(provenance: &Provenance, name: &str, project_root: &Path) -> Result<(), String> {
    let Some(mut command) = workers::wrangler_command(provenance, "queues") else {
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
    let Some(mut command) = workers::wrangler_command(provenance, "queues") else {
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

/// Delete a KV namespace by id — `--prune`'s one KV-side mutation.
///
/// `--skip-confirmation`: wrangler's own prompt is redundant with
/// [`confirm_prune`], which already named every resource about to go and
/// asked once for the whole batch: a second, per-resource prompt would be
/// noise, not safety. Idempotent against a namespace already gone (matched
/// via [`kv_namespace_already_deleted`]) — confirmed empirically (#839):
/// Cloudflare answers a not-found delete with an error, never silent success,
/// so this driver-side match is what makes re-running `--prune` safe.
pub(crate) fn delete_kv_namespace(
    provenance: &Provenance,
    id: &str,
    project_root: &Path,
) -> Result<(), String> {
    let Some(mut command) = workers::wrangler_command(provenance, "kv") else {
        return Err("wrangler not found".into());
    };
    let output = command
        .arg("namespace")
        .arg("delete")
        .arg("--namespace-id")
        .arg(id)
        .arg("--skip-confirmation")
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let said = wrangler_said(&output);
    if kv_namespace_already_deleted(&said) {
        return Ok(());
    }
    Err(said)
}

/// Does this `wrangler kv namespace delete` failure just mean the namespace
/// is already gone? Confirmed empirically (#839) against a real account:
/// `namespace not found [code: 10013]`. Matching the stable error code rather
/// than the prose, unlike [`queue_already_exists`]'s text match — Cloudflare
/// structures its API errors with a `code`, and `10013` is the more durable
/// half of the message if the wording is ever revised. Pure, so the rule is
/// tested without an account.
fn kv_namespace_already_deleted(stderr: &str) -> bool {
    stderr.contains("code: 10013") || stderr.to_ascii_lowercase().contains("namespace not found")
}

/// Delete a queue by name — `--prune`'s other mutation.
///
/// No confirmation flag to pass: confirmed empirically (#839) that
/// `wrangler queues delete` has no prompt or force flag at all (`--help`
/// shows only global flags) — it is already non-interactive by default.
/// Idempotent against a queue already gone, matched the same way
/// [`queue_already_exists`] matches the create side's inverse case.
pub(crate) fn delete_queue(
    provenance: &Provenance,
    name: &str,
    project_root: &Path,
) -> Result<(), String> {
    let Some(mut command) = workers::wrangler_command(provenance, "queues") else {
        return Err("wrangler not found".into());
    };
    let output = command
        .arg("delete")
        .arg(name)
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let said = wrangler_said(&output);
    if queue_already_deleted(&said) {
        return Ok(());
    }
    Err(said)
}

/// Does this `wrangler queues delete` failure just mean the queue is already
/// gone? Confirmed empirically (#839): `Queue "<name>" does not exist. To
/// create it, run: wrangler queues create <name>`. Pure, so the rule is
/// tested without an account.
fn queue_already_deleted(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("does not exist")
}

/// The names already set on this Worker, from `wrangler secret list`.
///
/// `None` means "could not ask" — see [`secret_action`]. Unlike `secret put`,
/// `secret list` has no draft-Worker path: it simply fails for a Worker that
/// does not exist, which is exactly the answer we want on a first deploy.
pub(crate) fn list_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    environment: &str,
) -> Option<BTreeSet<String>> {
    let mut command = workers::wrangler_command(provenance, "secret")?;
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
pub(crate) fn parse_secret_list(stdout: &str) -> Option<BTreeSet<String>> {
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
pub(crate) fn set_secret(
    provenance: &Provenance,
    worker_dir: &Path,
    name: &str,
    value: &str,
    environment: &str,
) -> Result<(), String> {
    let Some(mut command) = workers::wrangler_command(provenance, "secret") else {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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

    #[test]
    fn parses_wrangler_namespace_json() {
        assert_eq!(
            parse_kv_id(r#"{"id":"abc","title":"api"}"#),
            Some("abc".into())
        );
    }

    #[test]
    fn should_trust_recorded_kv_only_when_the_id_is_live_or_unchecked() {
        let live = BTreeSet::from(["kv-abc".to_string()]);
        assert!(
            should_trust_recorded_kv(Some("kv-abc"), Some(&live)),
            "recorded and present in the live set"
        );
        assert!(
            !should_trust_recorded_kv(Some("kv-deleted"), Some(&live)),
            "recorded but Cloudflare no longer has it — treat as unrecorded"
        );
        assert!(
            should_trust_recorded_kv(Some("kv-abc"), None),
            "the fetch never ran or failed — trust the ledger, as pre-slice-5"
        );
        assert!(
            !should_trust_recorded_kv(None, Some(&live)),
            "nothing recorded at all"
        );
        assert!(!should_trust_recorded_kv(None, None));
    }

    #[test]
    fn kv_namespace_already_deleted_matches_the_confirmed_error_shape() {
        // Confirmed empirically against a real account (#839): `namespace not
        // found [code: 10013]`. The code is matched, not just the prose — the
        // more durable half if Cloudflare ever reword the message.
        assert!(kv_namespace_already_deleted(
            "✘ [ERROR] A request to the Cloudflare API (...) failed.\n  namespace not found [code: 10013]"
        ));
        assert!(kv_namespace_already_deleted("code: 10013"));
        assert!(!kv_namespace_already_deleted(
            "✘ [ERROR] Authentication error [10000]"
        ));
        assert!(!kv_namespace_already_deleted(""));
    }

    #[test]
    fn queue_already_deleted_matches_the_confirmed_error_shape() {
        // Confirmed empirically (#839): `Queue "<name>" does not exist. To
        // create it, run: wrangler queues create <name>`.
        assert!(queue_already_deleted(
            "✘ [ERROR] Queue \"jobs\" does not exist. To create it, run: wrangler queues create jobs"
        ));
        assert!(!queue_already_deleted(
            "✘ [ERROR] A queue with this name already exists"
        ));
        assert!(!queue_already_deleted(""));
    }

    #[test]
    fn confirm_prune_short_circuits_on_yes_without_touching_stdin() {
        // The one path of confirm_prune testable without a terminal: --yes
        // must return true immediately, exactly as `confirm` does.
        assert!(confirm_prune(true, &Orphans::default()));
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
}
