//! `bynk dev` — build a project and serve it locally in one step.
//!
//! Collapses the manual recipe (compile → `cd` into the generated worker dir →
//! `wrangler dev`) into a single command (proposal v0.57). The orchestration is
//! **pre-flight → compile → select → serve**, and almost every piece is reused:
//! [`compiler::resolve`](crate::compiler) for `bynkc`, the doctor `Deploy`
//! capability for the Node + `wrangler` gate, and [`probe`] for locating
//! `wrangler` with the same provenance ordering doctor reports.
//!
//! The serve step runs `wrangler dev` in **local mode** (Miniflare), which
//! simulates KV / Durable Objects / queues keyed by *binding name* — so no
//! namespace provisioning is needed and the generated `wrangler.toml` is served
//! untouched (proposal §1, D4). Everything `wrangler`-specific is encapsulated
//! here so the serve step can later be swapped for a first-party `workerd`
//! server without touching the rest (proposal §4).
//!
//! Since #552 the step is **one `wrangler dev` per context**, not one per
//! project: the processes discover each other through wrangler's dev registry
//! and wire the emitted `[[services]]` bindings between themselves, so a
//! cross-context call resolves locally. That makes the driver a supervisor of N
//! children rather than a hand-off to one — hence the port allocation
//! ([`allocate`]), the joint teardown (`terminate`, private), and the plural
//! selection rule ([`select_contexts`]) that replaced ADR 0096 D3's ambiguity
//! error.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use bynk_emit::project::{BuildTarget, read_project_paths};

use crate::compiler::Compiler;
use crate::doctor::{self, Capability, Context, DoctorOptions, Report};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};
use crate::shell::exit_status_byte;

/// Parsed `bynk dev` flags (the project `PATH` is resolved into `project_root`
/// before we get here).
#[derive(Debug, Clone, Default)]
pub struct DevOptions {
    /// `--context NAME`, repeatable — which contexts' workers to serve. Empty
    /// serves **every** context in the project, wired (ADR 0096 D3 superseded).
    pub contexts: Vec<String>,
    /// `--base-port N` — the first port of the per-context allocation. `None`
    /// leaves a lone worker on wrangler's own default, so `-- --port N` keeps
    /// working exactly as it did when `dev` served one context.
    pub base_port: Option<u16>,
    /// `--inspect` (slice 3): start `wrangler dev` with the V8 inspector so a
    /// JavaScript debugger can attach; breakpoints in `.bynk` resolve through the
    /// emitted source maps composed into the worker bundle.
    pub inspect: bool,
    /// Base inspector port for `--inspect` (default 9229); allocated per context
    /// exactly as `base_port` is.
    pub inspect_port: u16,
    /// `--env NAME` (default `"default"`) — which `bynk.deploy.lock` section
    /// `--remote` reads the KV id from. `dev` never provisions and never
    /// writes the ledger (unchanged); this only selects which of `deploy`'s
    /// environments `--remote` connects the placeholder to. Purely a `bynk`
    /// concept — never forwarded to `wrangler dev` itself, since `dev` curates
    /// no Wrangler-side environment config (slice 4, #837 review: a project
    /// deployed only under a non-default `--env` previously read as
    /// "never provisioned" here, because this always looked at `"default"`).
    pub environment: String,
    /// Everything after `--`, forwarded to `wrangler dev` verbatim (D5).
    pub wrangler_args: Vec<String>,
}

/// Wrangler's own default dev port — the base of the per-context allocation
/// when `--base-port` is not given, so a multi-context project's first worker
/// lands where a single-context one always has.
const DEFAULT_BASE_PORT: u16 = 8787;

/// One worker to serve: its dasherised context dir, its HTTP port (`None` = let
/// wrangler choose, the lone-worker default), and its inspector port.
#[derive(Debug, PartialEq, Eq)]
pub struct Serving {
    pub worker: String,
    pub port: Option<u16>,
    pub inspector_port: Option<u16>,
}

/// Orchestrate a local dev session: pre-flight, compile, select the worker, and
/// hand off to `wrangler dev`. Returns wrangler's own exit code on a clean
/// hand-off, or a pre-flight/build failure code before serving.
pub fn run(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    project_root: &Path,
    node_floor: u32,
    opts: &DevOptions,
) -> ExitCode {
    // #837 review: once `--remote` reads a ledger section by `--env`, a
    // `-- --env`/`-- --environment` passthrough would silently diverge —
    // `bynk` materialises one environment's KV id while Wrangler actually
    // connects to a different one. Checked only when `--remote` is present,
    // since `--env` is otherwise inert (nothing reads the ledger without it).
    if opts.wrangler_args.iter().any(|arg| arg == "--remote")
        && let Some(conflict) = crate::deploy::conflicting_env_passthrough(&opts.wrangler_args)
    {
        eprintln!(
            "bynk: `--env {}` conflicts with `{conflict}` after `--` — pass one or the other, not both",
            opts.environment
        );
        return ExitCode::FAILURE;
    }

    // 1. Pre-flight — reuse doctor's Deploy gate (Node + wrangler) plus the
    //    always-on compile floor. Failing here, with doctor's remedy text, beats
    //    a confusing error out of a half-built tree (proposal §2.2).
    let ctx = Context {
        project_root: Some(project_root.to_path_buf()),
        in_repo: false,
        node_floor,
    };
    let preflight_opts = DoctorOptions {
        only: Some(Capability::Deploy),
        strict: false,
    };
    let report = doctor::diagnose(tb, compiler, &ctx, &preflight_opts);
    if report.exit_nonzero(&preflight_opts) {
        eprint!("{}", preflight_failure_message(&report));
        return ExitCode::FAILURE;
    }
    // 2. Compile — in-process (slice 7: the driver links the pipeline instead of
    //    shelling `bynkc`). Into the managed `.bynk/dev/` build dir (D1).
    //    Compilation is additive (never prunes), so clear `workers/` first;
    //    otherwise a renamed/deleted context would linger and spuriously trip the
    //    §2.4 ambiguity check.
    let build_dir = project_root.join(".bynk").join("dev");
    if let Err(e) = prepare_build_dir(project_root, &build_dir) {
        eprintln!("bynk: could not prepare build directory: {e}");
        return ExitCode::FAILURE;
    }
    // #524: compile the SAME project shape as `bynkc compile <project_root>`
    // — the shared rooting rule over the full `[paths]` layout. `dev`
    // previously re-rooted on the first `include` entry only, silently
    // dropping further includes and the whole `exclude` list.
    if !compile_once(compiler, project_root, &build_dir) {
        return ExitCode::FAILURE;
    }

    // 3. Select the workers — every context by default, or the `--context`
    //    subset (#552, superseding ADR 0096 D3's select-or-default). Serving
    //    them *together* is the whole point: a cross-context call only resolves
    //    when its callee is up too, so an ambiguity error here was the feature
    //    being withheld, not a project being wrong.
    let workers_dir = build_dir.join("workers");
    let available = discover_workers(&workers_dir);
    let workers = match select_contexts(&available, &opts.contexts) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let serving = allocate(&workers, opts.base_port, opts);

    // Where the driver injects a port it owns the allocation, so the same flag
    // arriving through `--` is a conflict — and wrangler rejects a repeated
    // `--port` with a usage dump rather than taking the last one. Catch it here
    // and name the driver flag that owns it.
    for (flag, owner, injected) in [
        (
            "--port",
            "--base-port",
            serving.iter().any(|s| s.port.is_some()),
        ),
        (
            "--inspector-port",
            "--inspect-port",
            serving.iter().any(|s| s.inspector_port.is_some()),
        ),
    ] {
        if injected && passthrough_has(&opts.wrangler_args, flag) {
            eprintln!(
                "bynk: `{flag}` is allocated per context — pass `{owner}` to `bynk dev` instead of `-- {flag}`."
            );
            return ExitCode::FAILURE;
        }
    }

    // Remote dev reads the real Cloudflare KV id, unlike Miniflare's local
    // mode. Resolve it from the deploy ledger immediately before Wrangler
    // runs; a never-deployed project gets an actionable error instead of
    // sending the generated placeholder to Cloudflare.
    if opts.wrangler_args.iter().any(|arg| arg == "--remote") {
        for s in &serving {
            if let Err(e) = crate::deploy::materialise_deploy_state(
                project_root,
                &s.worker,
                &workers_dir.join(&s.worker).join("wrangler.toml"),
                &opts.environment,
            ) {
                eprintln!("bynk: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // 4. Serve — one `wrangler dev` per context, each from inside its own worker
    //    dir (the emitted `index.ts` imports `../../runtime.js`, so cwd must be
    //    the worker dir, exactly the manual recipe's `cd`). The processes find
    //    each other through wrangler's **dev registry** and wire the generated
    //    `[[services]]` bindings between themselves — verified: a binding starts
    //    `[not connected]` and converges to `[connected]` once its callee is up,
    //    so start order does not matter and we need not stage the spawns.
    //    Resolve wrangler once with doctor's provenance ordering; an npx
    //    resolution downloads on first use, so it is a notice, never a silent
    //    green path.
    let probe = probe::detect(
        tb,
        "wrangler",
        DetectOpts {
            project_root: Some(project_root),
            allow_npx: true,
        },
    );
    if matches!(probe.provenance, Provenance::Npx) {
        eprintln!("bynk: wrangler resolved via npx — it will download on first run.");
    }
    if matches!(probe.provenance, Provenance::Missing) {
        // The pre-flight gate should have caught this; defensive only.
        eprintln!("bynk: wrangler not found (run `bynk doctor --only deploy`)");
        return ExitCode::FAILURE;
    }

    // Inherited stdio (the default) keeps every session interactive. The driver
    // and the wranglers share the terminal's foreground process group, so a
    // Ctrl-C SIGINT reaches them all — we must not bail before reaping; we reap
    // in the watch loop and propagate the first exit code (ADR 0096 §Exit).
    let mut children: Vec<(String, std::process::Child)> = Vec::new();
    for s in &serving {
        let Some(mut cmd) = wrangler_command(&probe.provenance, "dev") else {
            eprintln!("bynk: wrangler not found (run `bynk doctor --only deploy`)");
            terminate(&mut children);
            return ExitCode::FAILURE;
        };
        cmd.current_dir(workers_dir.join(&s.worker));
        for arg in serve_args(s) {
            cmd.arg(arg);
        }
        for arg in &opts.wrangler_args {
            cmd.arg(arg);
        }
        match cmd.spawn() {
            Ok(child) => children.push((s.worker.clone(), child)),
            Err(e) => {
                eprintln!("bynk: could not run wrangler for `{}`: {e}", s.worker);
                terminate(&mut children);
                return ExitCode::FAILURE;
            }
        }
    }
    eprint!("{}", serving_report(&serving));

    // 5. Watch — #524: `bynk dev` is the edit loop, so watch the project's
    // `.bynk` sources (the full `[paths]` layout plus `bynk.toml`) and rebuild
    // into the same build dir on change. Each `wrangler dev` watches its own
    // built worker files, so one rebuild hot-reloads every context that changed
    // without a restart; a failing rebuild renders diagnostics and keeps both
    // the watch and the last good build serving. std-only mtime polling
    // (500ms): no native watcher dependency, and an edit-loop latency well
    // under a keystroke-to-glance.
    eprintln!("bynk dev: watching for source changes (edit `.bynk` files to rebuild)");
    let mut fingerprint = watch_fingerprint(project_root);
    loop {
        // Any worker exiting ends the session: the survivors' bindings now
        // point at a context that is gone, so a half-served project would fail
        // in a way that looks like a code bug. Stop them and propagate the
        // first exit code.
        for i in 0..children.len() {
            let status = match children[i].1.try_wait() {
                Ok(status) => status,
                Err(e) => {
                    eprintln!("bynk: could not poll wrangler: {e}");
                    terminate(&mut children);
                    return ExitCode::FAILURE;
                }
            };
            if let Some(status) = status {
                let (name, _) = children.remove(i);
                if !children.is_empty() {
                    eprintln!("bynk dev: `{name}` exited — stopping the other contexts.");
                }
                terminate(&mut children);
                return ExitCode::from(exit_status_byte(&status));
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let now = watch_fingerprint(project_root);
        if now != fingerprint {
            fingerprint = now;
            eprintln!("bynk dev: change detected — rebuilding…");
            if compile_once(compiler, project_root, &build_dir) {
                eprintln!("bynk dev: rebuilt");
            }
            // On failure the diagnostics are already rendered; keep serving
            // the last good build and keep watching.
        }
    }
}

/// Stop every remaining `wrangler dev` and reap it, so a session that ends on
/// one worker's exit does not strand the others — each holds a port and a
/// `workerd` child, and a stranded one makes the *next* `bynk dev` fail on a
/// port clash. Signal them all first, then reap, so the shutdowns overlap.
fn terminate(children: &mut Vec<(String, std::process::Child)>) {
    for (_, child) in children.iter_mut() {
        request_stop(child);
    }
    for (_, child) in children.iter_mut() {
        reap(child);
    }
    children.clear();
}

/// Ask one `wrangler dev` to stop **and take its own process tree with it**.
///
/// SIGTERM, not [`std::process::Child::kill`]'s SIGKILL: wrangler traps SIGTERM
/// and tears down the `node` and `workerd` processes it spawned, whereas SIGKILL
/// is untrappable — verified, a SIGKILLed wrangler strands an orphaned `workerd
/// serve` still holding the port. std exposes no SIGTERM, so we go through POSIX
/// `kill(1)`; off unix, SIGKILL is the only thing std offers.
fn request_stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let sent = Command::new("kill")
            .arg("-TERM")
            .arg(child.id().to_string())
            .status()
            .is_ok_and(|s| s.success());
        if sent {
            return;
        }
        // `kill` missing or the process already gone — fall through.
    }
    let _ = child.kill();
}

/// Reap a signalled child, giving it a moment to run wrangler's own teardown
/// before escalating to SIGKILL. Without the escalation a wrangler wedged in
/// shutdown would hang `bynk dev` forever; without the grace period we would be
/// back to stranding `workerd`.
fn reap(child: &mut std::process::Child) {
    const GRACE: Duration = Duration::from_secs(10);
    const TICK: Duration = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < GRACE {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(_) => break,
        }
        std::thread::sleep(TICK);
        waited += TICK;
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// One compile of the project into `build_dir`, on the same rooting rule as
/// `bynkc compile <project_root>` (#524, via [`bynk_driver::project_options`]).
/// Default: in-process. Escape hatch: a `BYNK_BYNKC` override shells *that*
/// binary instead — the only path on which a second, skewable compiler enters
/// (doctor reports its skew only here). Returns `false` on failure with the
/// diagnostics already rendered.
pub fn compile_once(compiler: &Compiler, project_root: &Path, build_dir: &Path) -> bool {
    let used_override = matches!(compiler.origin, Some(crate::compiler::Origin::Override));
    if let (true, Some(bynkc)) = (used_override, compiler.path.as_deref()) {
        let status = Command::new(bynkc)
            .arg("compile")
            .arg(project_root)
            .arg("--output")
            .arg(build_dir)
            .arg("--target")
            .arg("workers")
            .status();
        return match status {
            Ok(s) if s.success() => true,
            Ok(_) => false,
            Err(e) => {
                eprintln!("bynk: could not run bynkc ({}): {e}", bynkc.display());
                false
            }
        };
    }
    let options = bynk_driver::project_options(project_root).target(BuildTarget::Workers);
    let output = match bynk_emit::project::compile_project(&options) {
        Ok(out) => out,
        Err(failure) => {
            // Render with full source context, exactly as the shelled `bynkc
            // compile` did — the front-end's flatten-then-delegate (ADR 0100),
            // shared with `bynk check` (see `crate::diagnostics`).
            crate::diagnostics::render_project_failure(&failure);
            return false;
        }
    };
    if let Err(e) = bynk_emit::write_output(&output, build_dir) {
        eprintln!(
            "bynk: could not write build output under `{}`: {e}",
            build_dir.display()
        );
        return false;
    }
    true
}

/// #524: a change fingerprint over the project's watched inputs — every
/// `.bynk` file under the `[paths] include` roots (author `exclude` subtrees
/// and tool/VCS directories skipped) plus `bynk.toml` itself. Hashes each
/// file's path, mtime, and length, so an edit, add, delete, or rename all
/// change the fingerprint. I/O errors skip the entry rather than aborting the
/// watch.
fn watch_fingerprint(project_root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let paths = read_project_paths(project_root);
    let excludes: Vec<PathBuf> = paths.exclude.iter().map(|e| project_root.join(e)).collect();
    let mut entries: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    let record = |path: &Path, entries: &mut Vec<(PathBuf, std::time::SystemTime, u64)>| {
        if let Ok(meta) = std::fs::metadata(path)
            && let Ok(mtime) = meta.modified()
        {
            entries.push((path.to_path_buf(), mtime, meta.len()));
        }
    };
    record(&project_root.join("bynk.toml"), &mut entries);
    for root in &paths.include {
        collect_bynk_files(&project_root.join(root), &excludes, &mut |p| {
            record(p, &mut entries)
        });
    }
    entries.sort();
    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime, len) in &entries {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
        len.hash(&mut hasher);
    }
    hasher.finish()
}

/// Walk `dir` recursively, calling `visit` for each `.bynk` file. Skips the
/// author `exclude` subtrees and the tool/VCS directories a source walk never
/// wants (`.bynk` build dir, `.git`, `node_modules`, `target`).
fn collect_bynk_files(dir: &Path, excludes: &[PathBuf], visit: &mut dyn FnMut(&Path)) {
    const SKIP_DIRS: [&str; 4] = [".bynk", ".git", "node_modules", "target"];
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if SKIP_DIRS.iter().any(|s| name == *s) {
                continue;
            }
            if excludes.iter().any(|e| path.starts_with(e)) {
                continue;
            }
            collect_bynk_files(&path, excludes, visit);
        } else if path.extension().is_some_and(|e| e == "bynk") {
            visit(&path);
        }
    }
}

/// The text `bynk dev` prints when the deploy pre-flight fails: a lead line plus
/// doctor's own human report, so the remedy lines are identical to `bynk
/// doctor`. Pure (no I/O) so this deterministic surface is pinned by a golden
/// (§5), unlike the non-deterministic `wrangler dev` stream.
pub fn preflight_failure_message(report: &Report) -> String {
    format!(
        "bynk: environment not ready for `dev` — see below.\n\n{}",
        report::render(report, Format::Human)
    )
}

/// Ensure `.bynk/` is gitignored on first build (cargo's `target/.gitignore`
/// precedent — a `dev` run never dirties `git status`), then clear the
/// `workers/` tree so selection only ever sees this build's contexts (D1).
pub fn prepare_build_dir(project_root: &Path, build_dir: &Path) -> std::io::Result<()> {
    let bynk_dir = project_root.join(".bynk");
    std::fs::create_dir_all(&bynk_dir)?;
    let gitignore = bynk_dir.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, "*\n")?;
    }
    let workers = build_dir.join("workers");
    match std::fs::remove_dir_all(&workers) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// The worker directories under `<build>/workers/` that carry a `wrangler.toml`
/// (the unit `wrangler dev` can serve), sorted for deterministic messages.
pub fn discover_workers(workers_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(workers_dir) else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join("wrangler.toml").is_file()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

/// Why context selection failed — rendered to the user with the next step.
#[derive(Debug, PartialEq, Eq)]
pub enum SelectError {
    /// No worker was produced by the compile (e.g. an empty project).
    NoneBuilt,
    /// `--context NAME` named a context that doesn't exist.
    NotFound {
        requested: String,
        available: Vec<String>,
    },
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectError::NoneBuilt => {
                write!(
                    f,
                    "no workers were built — does the project define any contexts?"
                )
            }
            SelectError::NotFound {
                requested,
                available,
            } => write!(
                f,
                "no context `{requested}` — available: {}",
                available.join(", ")
            ),
        }
    }
}

/// Match one requested context against the built worker dirs, accepting either
/// the dotted name or its dasherised form (`commerce.payment` /
/// `commerce-payment`).
fn resolve_one(available: &[String], name: &str) -> Result<String, SelectError> {
    let dashed = name.replace('.', "-");
    available
        .iter()
        .find(|d| d.as_str() == name || d.as_str() == dashed)
        .cloned()
        .ok_or_else(|| SelectError::NotFound {
            requested: name.to_string(),
            available: available.to_vec(),
        })
}

/// Pick the workers `dev` will serve **together** (#552). No `--context` serves
/// every context in the project — the whole point of the increment, since a
/// cross-context call only resolves when its callee is up too. `--context` is
/// repeatable and narrows to a subset, in `available`'s deterministic order
/// rather than the order they were typed, and duplicates collapse.
///
/// There is no `Ambiguous` case: several contexts is the expected shape, not a
/// failure. Pure (the FS scan is the caller's) so the rule is unit-tested.
pub fn select_contexts(
    available: &[String],
    requested: &[String],
) -> Result<Vec<String>, SelectError> {
    if available.is_empty() {
        return Err(SelectError::NoneBuilt);
    }
    if requested.is_empty() {
        return Ok(available.to_vec());
    }
    let mut chosen = Vec::new();
    for name in requested {
        let worker = resolve_one(available, name)?;
        if !chosen.contains(&worker) {
            chosen.push(worker);
        }
    }
    chosen.sort();
    Ok(chosen)
}

/// Allocate a port per worker (#552): `wrangler dev` binds one port per process,
/// so serving N contexts means N distinct ports, assigned `base + i` over the
/// deterministic worker order.
///
/// The one exception preserves the pre-#552 contract: a **lone** worker with no
/// explicit `--base-port` gets `None` — no injected `--port` at all — so it
/// lands on wrangler's own default and `-- --port N` still works. Injecting
/// unconditionally would break that, because a repeated `--port` is a hard
/// wrangler error, not last-wins.
pub fn allocate(workers: &[String], base_port: Option<u16>, opts: &DevOptions) -> Vec<Serving> {
    let lone = workers.len() == 1 && base_port.is_none();
    let base = base_port.unwrap_or(DEFAULT_BASE_PORT);
    workers
        .iter()
        .enumerate()
        .map(|(i, worker)| Serving {
            worker: worker.clone(),
            port: (!lone).then(|| base.saturating_add(i as u16)),
            inspector_port: opts
                .inspect
                .then(|| opts.inspect_port.saturating_add(i as u16)),
        })
        .collect()
}

/// Whether the `--` passthrough carries `flag`, which the driver also injects.
/// Wrangler rejects a repeated `--port`/`--inspector-port` outright ("expects a
/// single value, but received multiple"), so we catch the clash ourselves and
/// say which driver flag owns it instead of letting wrangler's usage dump land.
fn passthrough_has(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
}

/// Build the `wrangler dev` invocation for a resolved provenance: an installed
/// binary is run directly; an npx-provisionable one goes through `npx --yes`.
/// `None` when wrangler is genuinely missing.
pub fn wrangler_command(provenance: &Provenance, subcommand: &str) -> Option<Command> {
    match provenance {
        Provenance::Path(p) | Provenance::ProjectLocal(p) => {
            let mut cmd = Command::new(p);
            cmd.arg(subcommand);
            Some(cmd)
        }
        Provenance::Npx => {
            let mut cmd = Command::new("npx");
            // #524: pinned provisioning, per the repo's npx convention — an
            // unpinned `wrangler` here meant the dev server could drift from
            // the wrangler the tests and deploys run.
            cmd.arg("--yes").arg("wrangler@4").arg(subcommand);
            Some(cmd)
        }
        Provenance::Missing => None,
    }
}

/// The `wrangler dev` flags the driver injects for one worker: the ports it
/// allocated (#552) — `--port` when serving several contexts, `--inspector-port`
/// under `--inspect` (slice 3, ADR 0104), so a JavaScript debugger can attach and
/// `.bynk` breakpoints resolve through the emitted source maps.
///
/// Empty for a lone worker without `--base-port` or `--inspect` — byte-for-byte
/// the pre-#552 invocation, so that path keeps its `-- --port N` passthrough.
fn serve_args(s: &Serving) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(port) = s.port {
        args.push("--port".to_string());
        args.push(port.to_string());
    }
    if let Some(port) = s.inspector_port {
        args.push("--inspector-port".to_string());
        args.push(port.to_string());
    }
    args
}

/// The start-up report: which context answers on which URL, plus the inspector
/// notice under `--inspect`. Pure and deterministic, so it is golden-pinned in
/// the style of ADR 0096 §Exit — unlike the `wrangler dev` streams it precedes.
///
/// A lone worker on wrangler's own default port prints no table: there is no
/// allocation to disclose and wrangler announces its own `Ready on` line, so
/// that session reads exactly as it did before #552.
pub fn serving_report(serving: &[Serving]) -> String {
    let mut out = String::new();
    if serving.iter().any(|s| s.port.is_some()) {
        // Only claim the wiring when there is something to wire: a subset of
        // one has no sibling to bind to, and saying otherwise would explain a
        // cross-context call's failure as a bug rather than as the missing
        // context it is.
        out.push_str(&match serving.len() {
            1 => "bynk dev: serving 1 context.\n".to_string(),
            n => format!(
                "bynk dev: serving {n} contexts — service bindings between them are wired.\n"
            ),
        });
        let width = serving.iter().map(|s| s.worker.len()).max().unwrap_or(0);
        for s in serving {
            let Some(port) = s.port else { continue };
            out.push_str(&format!(
                "  {:width$}  http://localhost:{port}\n",
                s.worker,
                width = width
            ));
        }
    }
    let inspected = serving
        .iter()
        .filter(|s| s.inspector_port.is_some())
        .count();
    if inspected > 0 {
        out.push_str(&match inspected {
            1 => "bynk dev --inspect: the worker runs with the V8 inspector enabled.\n".to_string(),
            _ => "bynk dev --inspect: each worker runs with the V8 inspector enabled, on its own port.\n"
                .to_string(),
        });
        for s in serving {
            let Some(port) = s.inspector_port else {
                continue;
            };
            out.push_str(&format!(
                "  {} — inspector on port {port} (CDP discovery: http://127.0.0.1:{port}/json)\n",
                s.worker
            ));
        }
        out.push_str(
            "  Breakpoints set in `.bynk` sources resolve through the emitted source maps.\n\
             \x20 A hand-rolled CDP client must send an `Origin` header — VS Code's\n\
             \x20 JavaScript debugger does this for you.\n",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_sole_context_is_selected_without_a_flag() {
        assert_eq!(
            select_contexts(&names(&["links"]), &[]),
            Ok(names(&["links"]))
        );
    }

    #[test]
    fn no_workers_is_its_own_error() {
        assert_eq!(select_contexts(&[], &[]), Err(SelectError::NoneBuilt));
    }

    #[test]
    fn exit_status_byte_maps_codes_and_signals() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            use std::process::ExitStatus;
            // Wait statuses: exit codes sit in the high byte; the low byte is
            // the terminating signal.
            assert_eq!(exit_status_byte(&ExitStatus::from_raw(0)), 0);
            assert_eq!(exit_status_byte(&ExitStatus::from_raw(1 << 8)), 1);
            // A shared Ctrl-C (SIGINT = 2) is a clean stop…
            assert_eq!(exit_status_byte(&ExitStatus::from_raw(2)), 0);
            // …but a SIGSEGV (11) or SIGKILL (9, the OOM killer) is a real
            // failure — previously these read as passing in CI.
            assert_eq!(exit_status_byte(&ExitStatus::from_raw(11)), 128 + 11);
            assert_eq!(exit_status_byte(&ExitStatus::from_raw(9)), 128 + 9);
        }
    }

    #[test]
    fn inspect_injects_the_inspector_port() {
        let off = DevOptions::default();
        let lone = allocate(&names(&["links"]), None, &off);
        assert!(
            serve_args(&lone[0]).is_empty(),
            "a lone worker without --inspect keeps the pre-#552 invocation"
        );

        let on = DevOptions {
            inspect: true,
            inspect_port: 9229,
            ..Default::default()
        };
        let lone = allocate(&names(&["links"]), None, &on);
        assert_eq!(
            serve_args(&lone[0]),
            vec!["--inspector-port".to_string(), "9229".to_string()]
        );
    }

    // ---- #552: multi-context selection ------------------------------------

    #[test]
    fn no_context_flag_serves_every_context() {
        // The defining change: several contexts is the expected shape, so the
        // whole project is served rather than refused as ambiguous.
        assert_eq!(
            select_contexts(&names(&["api", "worker"]), &[]),
            Ok(names(&["api", "worker"]))
        );
    }

    #[test]
    fn context_flags_narrow_to_a_subset() {
        let avail = names(&["api", "commerce-payment", "worker"]);
        assert_eq!(
            select_contexts(&avail, &names(&["worker", "api"])),
            Ok(names(&["api", "worker"])),
            "the subset is served in the deterministic order, not the typed one"
        );
        // Dotted and dasherised forms both resolve, and repeats collapse.
        assert_eq!(
            select_contexts(&avail, &names(&["commerce.payment", "commerce-payment"])),
            Ok(names(&["commerce-payment"]))
        );
    }

    #[test]
    fn selecting_many_reports_an_unknown_context() {
        assert_eq!(
            select_contexts(&names(&["api"]), &names(&["api", "nope"])),
            Err(SelectError::NotFound {
                requested: "nope".to_string(),
                available: names(&["api"]),
            })
        );
    }

    #[test]
    fn selecting_many_from_an_empty_build_is_still_none_built() {
        assert_eq!(select_contexts(&[], &[]), Err(SelectError::NoneBuilt));
    }

    // ---- #552: port allocation --------------------------------------------

    #[test]
    fn ports_are_allocated_per_context_from_the_base() {
        let opts = DevOptions {
            inspect: true,
            inspect_port: 9229,
            ..Default::default()
        };
        let serving = allocate(&names(&["api", "worker"]), None, &opts);
        assert_eq!(
            serving.iter().map(|s| s.port).collect::<Vec<_>>(),
            vec![Some(8787), Some(8788)],
            "each wrangler dev binds its own port"
        );
        assert_eq!(
            serving.iter().map(|s| s.inspector_port).collect::<Vec<_>>(),
            vec![Some(9229), Some(9230)],
            "inspector ports must not collide either"
        );
        assert_eq!(
            serve_args(&serving[1]),
            names(&["--port", "8788", "--inspector-port", "9230"])
        );
    }

    #[test]
    fn base_port_moves_the_whole_allocation() {
        let serving = allocate(&names(&["a", "b"]), Some(9000), &DevOptions::default());
        assert_eq!(
            serving.iter().map(|s| s.port).collect::<Vec<_>>(),
            vec![Some(9000), Some(9001)]
        );
    }

    #[test]
    fn a_lone_worker_keeps_wranglers_own_port() {
        // The back-compat contract: no injected --port, so `-- --port N` still
        // reaches wrangler (a repeated --port is a hard error, not last-wins).
        let serving = allocate(&names(&["links"]), None, &DevOptions::default());
        assert_eq!(serving[0].port, None);
        assert!(serve_args(&serving[0]).is_empty());
        // …but an explicit --base-port is honoured even for one context.
        let pinned = allocate(&names(&["links"]), Some(8900), &DevOptions::default());
        assert_eq!(pinned[0].port, Some(8900));
    }

    #[test]
    fn passthrough_port_is_detected_in_both_spellings() {
        assert!(passthrough_has(&names(&["--port", "8788"]), "--port"));
        assert!(passthrough_has(&names(&["--port=8788"]), "--port"));
        assert!(!passthrough_has(&names(&["--remote"]), "--port"));
        // `--inspector-port` must not be mistaken for `--port`.
        assert!(!passthrough_has(
            &names(&["--inspector-port", "9229"]),
            "--port"
        ));
    }

    #[test]
    fn the_serving_report_lists_context_urls() {
        let serving = allocate(
            &names(&["commerce-orders", "commerce-payment"]),
            None,
            &DevOptions::default(),
        );
        let report = serving_report(&serving);
        assert!(report.contains("serving 2 contexts"), "{report}");
        // Names are padded to the widest, so the URLs line up in a column.
        assert!(
            report.contains("commerce-orders   http://localhost:8787"),
            "{report}"
        );
        assert!(
            report.contains("commerce-payment  http://localhost:8788"),
            "{report}"
        );
        // A lone default-port worker discloses no allocation — wrangler's own
        // `Ready on` line is the announcement, exactly as before #552.
        assert_eq!(
            serving_report(&allocate(&names(&["links"]), None, &DevOptions::default())),
            ""
        );
    }

    #[test]
    fn a_subset_of_one_claims_no_wiring() {
        // "bindings between them are wired" would be a lie for a single
        // context, and a misleading one: it invites reading a cross-context
        // call's failure as a bug rather than as the context left unserved.
        let report = serving_report(&allocate(
            &names(&["commerce-payment"]),
            Some(8890),
            &DevOptions::default(),
        ));
        assert!(report.contains("serving 1 context."), "{report}");
        assert!(!report.contains("bindings"), "{report}");
    }
}
