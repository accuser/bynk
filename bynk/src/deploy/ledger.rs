use super::*;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DeployLock {
    // No serde `default`: a ledger with no `version` is not a fresh project, it
    // is corruption (a truncated write), and must fail the read rather than
    // parse as an empty v1 ledger that re-mints every namespace (#736).
    version: u32,
    #[serde(default)]
    pub(crate) environments: BTreeMap<String, Environment>,
}

fn lock_version() -> u32 {
    1
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Environment {
    #[serde(default)]
    pub(crate) kv: BTreeMap<String, KvNamespace>,
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
pub(crate) struct KvNamespace {
    pub(crate) id: String,
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
    pub(crate) fn is_deployed(&self, environment: &str, worker: &str) -> bool {
        self.environments
            .get(environment)
            .and_then(|env| env.workers.get(worker))
            .is_some_and(|record| record.deployed)
    }

    pub(crate) fn record_deployed(
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
    pub(crate) fn live_contracts(
        &self,
        environment: &str,
        worker: &str,
    ) -> Option<&BTreeMap<String, String>> {
        self.environments
            .get(environment)
            .and_then(|env| env.workers.get(worker))
            .and_then(|record| record.contracts.as_ref())
    }

    pub(crate) fn has_queue(&self, environment: &str, queue: &str) -> bool {
        self.environments
            .get(environment)
            .is_some_and(|env| env.queues.contains(queue))
    }

    /// Note that this project has created `queue`. Returns whether the ledger
    /// changed, so a re-run that provisions nothing also writes nothing.
    pub(crate) fn record_queue(&mut self, environment: &str, queue: &str) -> bool {
        self.environments
            .entry(environment.to_string())
            .or_default()
            .queues
            .insert(queue.to_string())
    }
}

/// Slice 5: every ledger entry for this environment that the current build no
/// longer declares. Owned, not borrowed — ledger-derived names don't share
/// `Plan<'a>`'s lifetime over `order`/`resources` (the `PlanSecret.name:
/// String` precedent below).
///
/// `kv` and `workers` are independent checks, deliberately not merged: both
/// are keyed by worker name in the ledger (`Environment`, above), so a
/// context removed from source that had KV is reported as **two** orphans,
/// one per map, not one combined line. `--prune` (when it lands) treats them
/// independently too — the `kv` line is prunable, the `workers` line is
/// report-only (a whole-Worker delete is a materially larger blast radius
/// than a namespace or a queue, and out of this slice's scope).
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Orphans {
    pub(crate) kv: Vec<String>,
    pub(crate) workers: Vec<String>,
    pub(crate) queues: Vec<String>,
}

impl Orphans {
    /// Whether `--prune` would actually delete anything. **Not** the same as
    /// checking whether every field is empty (review, #840): `workers`
    /// orphans are report-only (DECISION C — never `wrangler delete`), so a
    /// project whose only orphan is an unprunable Worker must not trigger
    /// `confirm_prune`'s prompt at all — `Delete 0 resource(s)?` is a bug,
    /// not a valid state.
    pub(crate) fn has_prunable(&self) -> bool {
        !self.kv.is_empty() || !self.queues.is_empty()
    }
}

/// The orphan diff: ledger vs. the current build's full declared resource
/// set, regardless of `--context` — `resources` already spans the *whole*
/// project (`project_resources`, called from `run()` over `available` — every
/// worker `workers::discover_workers` found — not the `--context`-narrowed
/// `order`/`selected`), so `resources.keys()` alone is the full live-worker
/// set and this reuses data `run()` already has rather than reading anything
/// new. No live Cloudflare call: the report half of reconciliation costs
/// nothing and needs no auth, keeping `--dry-run`'s "never authenticates"
/// promise intact.
///
/// Pure, so the diff — including the shared-queue case (a queue two contexts
/// still consume must never appear orphaned because a *third* context that
/// used to consume it was removed) — is tested without a build tree.
pub(crate) fn find_orphans(
    lock: &DeployLock,
    environment: &str,
    resources: &BTreeMap<String, Resources>,
) -> Orphans {
    let Some(env) = lock.environments.get(environment) else {
        return Orphans::default();
    };
    let live_queues: BTreeSet<&str> = resources
        .values()
        .flat_map(|r| r.queues.iter().map(String::as_str))
        .collect();
    Orphans {
        kv: env
            .kv
            .keys()
            .filter(|worker| !resources.contains_key(worker.as_str()))
            .cloned()
            .collect(),
        workers: env
            .workers
            .keys()
            .filter(|worker| !resources.contains_key(worker.as_str()))
            .cloned()
            .collect(),
        queues: env
            .queues
            .iter()
            .filter(|queue| !live_queues.contains(queue.as_str()))
            .cloned()
            .collect(),
    }
}

pub(crate) fn recorded_kv<'a>(
    lock: &'a DeployLock,
    worker: &str,
    environment: &str,
) -> Option<&'a str> {
    lock.environments
        .get(environment)
        .and_then(|env| env.kv.get(worker))
        .map(|kv| kv.id.as_str())
}

/// Slice 5, DECISION B, extracted for direct testing (review, #840): should
/// `deploy_one` trust a recorded KV id, or treat it as though nothing were
/// recorded at all?
///
/// - No record at all → never trust (nothing to trust).
/// - A record, and the live fetch never ran or failed (`None`) → trust it,
///   unconditionally — exactly pre-slice-5 behaviour, so a fetch outage
///   never blocks a deploy that would have succeeded before this slice.
/// - A record, and a live id set — trust it only if the id is actually in
///   that set. Absent means Cloudflare no longer recognises it: re-provision,
///   the same as an unrecorded id would.
pub(crate) fn should_trust_recorded_kv(
    recorded: Option<&str>,
    live_kv_ids: Option<&BTreeSet<String>>,
) -> bool {
    match (recorded, live_kv_ids) {
        (Some(id), Some(live)) => live.contains(id),
        (Some(_), None) => true,
        (None, _) => false,
    }
}

pub(crate) fn read_lock(path: &Path) -> Result<DeployLock, String> {
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

pub(crate) fn write_lock(path: &Path, lock: &DeployLock) -> Result<(), String> {
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
    // P7.4 (#1305): structural, not a substring search — closes R7.6/R8.20.
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    if !bynk_emit::emitter::wrangler::wrangler_needs_kv_materialisation(&text)? {
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

// ---------------------------------------------------------------------------
// Slice 5: reconciliation — orphan pruning
// ---------------------------------------------------------------------------

/// Print exactly what `--prune` is about to delete, then ask once for the
/// whole batch — the "strictly stronger gate" the track doc (§6) calls for on
/// top of [`confirm`]'s creation gate. `--yes` alone does **not** imply this:
/// a CI job that wants unattended pruning must pass `--yes` **and**
/// `--prune` together, the same non-interactive-requires-`--yes` shape
/// [`confirm`] already uses, just with its own prompt so a script that only
/// meant to authorise *creation* cannot accidentally also authorise deletion.
pub(crate) fn confirm_prune(yes: bool, orphans: &Orphans) -> bool {
    for kv in &orphans.kv {
        eprintln!("bynk: will delete KV namespace for `{kv}`");
    }
    for queue in &orphans.queues {
        eprintln!("bynk: will delete queue `{queue}`");
    }
    if yes {
        return true;
    }
    if !requires_interactive_confirmation(yes, io::stdin().is_terminal()) {
        eprintln!("bynk: refusing to prune in a non-interactive session without --yes");
        return false;
    }
    let count = orphans.kv.len() + orphans.queues.len();
    eprint!("Delete {count} resource(s)? [y/N] ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Delete every KV and queue orphan `confirm_prune` just named — never a
/// worker (DECISION C: `wrangler delete`'s blast radius, routes/domains/crons
/// along with the script, is categorically larger than a namespace or a
/// queue, and pruning a whole Worker is explicitly out of this slice).
///
/// The ledger entry is stripped whether the delete found something to remove
/// or found it already gone (DECISION E) — both mean "this resource is not
/// there", and treating only a clean delete as ledger-worthy would wedge a
/// half-completed prune: crash between a successful Cloudflare delete and the
/// ledger write, and the next run would re-report the same orphan and
/// re-issue a delete Cloudflare now rejects as not-found.
pub(crate) fn prune_orphans(
    provenance: &Provenance,
    project_root: &Path,
    lock: &mut DeployLock,
    lock_path: &Path,
    environment: &str,
    orphans: &Orphans,
) -> Result<(), DeployFailure> {
    for worker in &orphans.kv {
        let Some(id) = lock
            .environments
            .get(environment)
            .and_then(|env| env.kv.get(worker))
            .map(|ns| ns.id.clone())
        else {
            continue;
        };
        delete_kv_namespace(provenance, &id, project_root).map_err(|e| {
            DeployFailure::driver(format!(
                "could not delete the orphaned KV namespace for `{worker}`: {e}"
            ))
        })?;
        if let Some(env) = lock.environments.get_mut(environment) {
            env.kv.remove(worker);
        }
        write_lock(lock_path, lock).map_err(|e| {
            DeployFailure::driver(format!(
                "deleted the orphaned KV namespace for `{worker}` but could not record it in {}: {e}",
                lock_path.display()
            ))
        })?;
    }
    for queue in &orphans.queues {
        let physical = env_qualify(environment, queue);
        delete_queue(provenance, &physical, project_root).map_err(|e| {
            DeployFailure::driver(format!(
                "could not delete the orphaned queue `{physical}`: {e}"
            ))
        })?;
        if let Some(env) = lock.environments.get_mut(environment) {
            env.queues.remove(queue);
        }
        write_lock(lock_path, lock).map_err(|e| {
            DeployFailure::driver(format!(
                "deleted the orphaned queue `{physical}` but could not record it in {}: {e}",
                lock_path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    pub(crate) fn lock_with_deployed(workers: &[&str]) -> DeployLock {
        let mut lock = DeployLock::default();
        for worker in workers {
            lock.record_deployed("default", worker, Some(Default::default()));
        }
        lock
    }

    /// Record `worker`'s KV namespace, as a real deploy does before it pushes.
    pub(crate) fn with_kv(mut lock: DeployLock, worker: &str) -> DeployLock {
        lock.environments
            .entry("default".into())
            .or_default()
            .kv
            .insert(worker.to_string(), KvNamespace { id: "ns-id".into() });
        lock
    }

    /// Mark `queue` as one this project has already created.
    pub(crate) fn with_queue(mut lock: DeployLock, queue: &str) -> DeployLock {
        lock.record_queue("default", queue);
        lock
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
    fn materialises_only_the_placeholder() {
        // P7.4 (#1305): the real shape `emit_wrangler_toml` produces — a
        // `[[kv_namespaces]]` stanza, not a bare root `id` key (the old
        // fixture's shortcut, which only worked because the old
        // implementation was a whole-file substring replace that didn't
        // care where the placeholder sat).
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bynk-deploy-{}-{}", std::process::id(), unique));
        std::fs::write(
            &path,
            format!(
                "[[kv_namespaces]]\nbinding = \"BYNK_KV\"\nid = \"{KV_NAMESPACE_ID_PLACEHOLDER}\"\n"
            ),
        )
        .unwrap();
        assert!(materialise_kv_id(&path, "abc"));
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Table = text.parse().unwrap();
        assert_eq!(
            parsed["kv_namespaces"].as_array().unwrap()[0]["id"].as_str(),
            Some("abc")
        );
        let _ = std::fs::remove_file(path);
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
        std::fs::write(
            &config,
            format!(
                "[[kv_namespaces]]\nbinding = \"BYNK_KV\"\nid = \"{KV_NAMESPACE_ID_PLACEHOLDER}\"\n"
            ),
        )
        .unwrap();

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
        let text = std::fs::read_to_string(&config).unwrap();
        let parsed: toml::Table = text.parse().unwrap();
        assert_eq!(
            parsed["kv_namespaces"].as_array().unwrap()[0]["id"].as_str(),
            Some("kv-staging")
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// P7.4 (#1305): a project with no KV binding at all must skip the lock
    /// lookup entirely and return `Ok(false)` — not error just because
    /// nothing is recorded for it (there's nothing to materialise). No lock
    /// file exists in this project root at all, proving the early exit
    /// really does short-circuit before `read_lock` runs.
    #[test]
    fn materialise_deploy_state_is_a_no_op_without_a_kv_binding() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bynk-materialise-state-no-kv-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("wrangler.toml");
        std::fs::write(&config, "name = \"api\"\nmain = \"index.ts\"\n").unwrap();

        assert_eq!(
            materialise_deploy_state(&dir, "api", &config, "default"),
            Ok(false)
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    // ---- #839 slice 5: reconciliation maturity + orphan reporting -------

    #[test]
    fn a_removed_context_with_kv_is_two_orphans_not_one() {
        // `kv` and `workers` are both keyed by worker name — a context that
        // had KV and was deleted from source shows up in both maps, so the
        // diff must report both, independently (DECISION A).
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.environments
            .entry("default".into())
            .or_default()
            .kv
            .insert(
                "gone".into(),
                KvNamespace {
                    id: "kv-gone".into(),
                },
            );
        lock.record_deployed("default", "gone", None);
        // "still-here" is in the current build, "gone" is not.
        let resources = BTreeMap::from([("still-here".into(), Resources::default())]);

        let orphans = find_orphans(&lock, "default", &resources);
        assert_eq!(orphans.kv, names(&["gone"]));
        assert_eq!(orphans.workers, names(&["gone"]));
        assert!(orphans.queues.is_empty());
    }

    #[test]
    fn a_queue_two_contexts_share_is_never_orphaned_while_either_declares_it() {
        // The false-positive risk DECISION A's diff exists to avoid: a queue
        // consumed by two contexts must not be reported orphaned just because
        // a *third*, now-removed context used to consume it too.
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.record_queue("default", "jobs");
        let resources = BTreeMap::from([
            ("orders".into(), Resources::default().consumes(&["jobs"])),
            ("billing".into(), Resources::default().consumes(&["jobs"])),
        ]);

        let orphans = find_orphans(&lock, "default", &resources);
        assert!(
            orphans.queues.is_empty(),
            "jobs is still consumed by two live contexts: {orphans:?}"
        );
    }

    #[test]
    fn a_queue_no_context_declares_anymore_is_orphaned() {
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.record_queue("default", "stale-jobs");
        let resources = BTreeMap::from([("orders".into(), Resources::default())]);

        let orphans = find_orphans(&lock, "default", &resources);
        assert_eq!(orphans.queues, names(&["stale-jobs"]));
    }

    #[test]
    fn find_orphans_is_scoped_to_the_named_environment() {
        // A "staging" orphan must never leak into "default"'s report, and
        // vice versa — environments (slice 4) stay fully independent.
        let mut lock = DeployLock {
            version: 1,
            ..Default::default()
        };
        lock.environments
            .entry("staging".into())
            .or_default()
            .kv
            .insert(
                "gone".into(),
                KvNamespace {
                    id: "kv-staging-gone".into(),
                },
            );
        let resources = BTreeMap::new();

        assert!(
            find_orphans(&lock, "default", &resources).kv.is_empty(),
            "default"
        );
        assert_eq!(
            find_orphans(&lock, "staging", &resources).kv,
            names(&["gone"])
        );
    }

    #[test]
    fn an_absent_environment_has_no_orphans() {
        let lock = DeployLock::default();
        let orphans = find_orphans(&lock, "default", &BTreeMap::new());
        assert_eq!(orphans, Orphans::default());
    }

    #[test]
    fn has_prunable_ignores_worker_only_orphans() {
        // The bug the review caught: a project whose only orphan is an
        // unprunable Worker must not trigger confirm_prune's "Delete 0
        // resource(s)?" prompt at all.
        let worker_only = Orphans {
            workers: names(&["gone"]),
            ..Default::default()
        };
        assert!(!worker_only.has_prunable());

        let with_kv = Orphans {
            kv: names(&["gone"]),
            ..Default::default()
        };
        assert!(with_kv.has_prunable());

        let with_queue = Orphans {
            queues: names(&["gone"]),
            ..Default::default()
        };
        assert!(with_queue.has_prunable());

        assert!(!Orphans::default().has_prunable());
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
}
