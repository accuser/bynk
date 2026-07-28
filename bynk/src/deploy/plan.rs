use super::*;

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
    /// `--prune` (slice 5) — delete every reported KV/queue orphan, behind
    /// its own confirmation. Never deletes a Worker (DECISION C). Defaults
    /// to report-only, matching the track's "report or prevent, never
    /// silently share/destroy" posture (§6).
    pub prune: bool,
    pub wrangler_args: Vec<String>,
}

/// The whole-project plan (slice 2). `order` is the upload order, dependencies
/// first; `contexts` carries it with each context's own actions. Slice 0's
/// single-worker `Plan` is the one-element case of this.
#[derive(Debug, Serialize)]
pub(crate) struct Plan<'a> {
    environment: &'a str,
    /// Slice 5: every ledger entry this environment no longer declares —
    /// printed before the per-context breakdown, so an orphan is seen before
    /// what's actually being pushed.
    orphans: Orphans,
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
    if let Err(e) = workers::prepare_build_dir(project_root, &build_dir) {
        eprintln!("bynk: could not prepare build directory: {e}");
        return ExitCode::FAILURE;
    }
    if !workers::compile_once(compiler, project_root, &build_dir) {
        return ExitCode::FAILURE;
    }
    // Slice 2: every context, ordered — not the one context slice 0 demanded.
    let workers_dir = build_dir.join("workers");
    let available = workers::discover_workers(&workers_dir);
    let selected = match workers::select_contexts(&available, opts.context.as_slice()) {
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

    // Slice 5, DECISION B: once per run, not once per context — an
    // account-wide list, fetched only when it could actually change an
    // outcome (a context needs KV and already has a recorded id worth
    // checking; a first deploy has nothing to check drift against). `None`
    // (no wrangler, or the call failed) falls back to trusting the ledger
    // unconditionally, exactly as every deploy before this slice did.
    let live_kv_ids = if order
        .iter()
        .any(|w| resources[w].needs_kv && recorded_kv(&lock, w, &opts.environment).is_some())
    {
        live_kv_namespace_ids(&probe.provenance, project_root)
    } else {
        None
    };

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
            live_kv_ids.as_ref(),
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

    // Slice 5: pruning is project-wide, independent of `--context` — the
    // orphan report already is (DECISION A), so pruning follows it. Runs
    // only after every selected context has pushed cleanly: a failed deploy
    // above already returned, so a mid-flight ledger never reaches this.
    if opts.prune && plan.orphans.has_prunable() {
        if !confirm_prune(opts.yes, &plan.orphans) {
            return ExitCode::FAILURE;
        }
        if let Err(e) = prune_orphans(
            &probe.provenance,
            project_root,
            &mut lock,
            &lock_path,
            &opts.environment,
            &plan.orphans,
        ) {
            eprintln!("bynk: {}", e.message);
            return ExitCode::from(e.code);
        }
    }
    ExitCode::SUCCESS
}

pub fn preflight_failure_message(report: &Report) -> String {
    format!(
        "bynk: environment not ready for `deploy` — see below.\n\n{}",
        report::render(report, Format::Human)
    )
}

/// What the run did **not** get to, once a context failed. `rest` is the order
/// *beyond* the failure, so the last context failing reports nothing — there was
/// nothing left to withhold, and the failure itself has already been named.
///
/// Pure, so the wording — and the count's agreement with the list — is goldened
/// rather than described.
pub(crate) fn stopped_report(rest: &[String]) -> String {
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
pub(crate) fn plan_report(plan: &Plan<'_>, format: DeployFormat) -> String {
    match format {
        DeployFormat::Short => {
            let mut out = String::new();
            // Before the per-context section, deliberately: an orphan is a
            // fact about the account regardless of what this run is about to
            // do, and a reader should see it before the noise of what's being
            // pushed (slice 5).
            for kv in &plan.orphans.kv {
                out.push_str(&format!("orphan kv {kv}\n"));
            }
            for worker in &plan.orphans.workers {
                out.push_str(&format!("orphan worker {worker}\n"));
            }
            for queue in &plan.orphans.queues {
                out.push_str(&format!("orphan queue {queue}\n"));
            }
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
pub(crate) fn derive_plan<'a>(
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
        orphans: find_orphans(lock, environment, resources),
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

pub(crate) fn requires_interactive_confirmation(yes: bool, stdin_is_terminal: bool) -> bool {
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
pub(crate) mod tests {
    use super::*;
    use crate::deploy::config::tests::project;
    use crate::deploy::graph::tests::graph;
    use crate::deploy::ledger::tests::{lock_with_deployed, with_kv, with_queue};

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// `derive_plan` with no secret input and no `--force` — the shape every
    /// test that predates slice 3 wants, so those tests keep saying what they
    /// are about rather than restating two arguments they do not exercise.
    pub(crate) fn plan_of<'a>(
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
