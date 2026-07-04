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

use std::path::Path;
use std::process::{Command, ExitCode};

use bynk_emit::project::{BuildTarget, CompileOptions, read_project_paths};

use crate::compiler::Compiler;
use crate::doctor::{self, Capability, Context, DoctorOptions, Report};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};
use crate::shell::exit_byte;

/// Parsed `bynk dev` flags (the project `PATH` is resolved into `project_root`
/// before we get here).
#[derive(Debug, Clone, Default)]
pub struct DevOptions {
    /// `--context NAME` — which context's worker to serve.
    pub context: Option<String>,
    /// `--inspect` (slice 3): start `wrangler dev` with the V8 inspector so a
    /// JavaScript debugger can attach; breakpoints in `.bynk` resolve through the
    /// emitted source maps composed into the worker bundle.
    pub inspect: bool,
    /// Inspector port for `--inspect` (default 9229).
    pub inspect_port: u16,
    /// Everything after `--`, forwarded to `wrangler dev` verbatim (D5).
    pub wrangler_args: Vec<String>,
}

/// Orchestrate a local dev session: pre-flight, compile, select the worker, and
/// hand off to `wrangler dev`. Returns wrangler's own exit code on a clean
/// hand-off, or a pre-flight/build failure code before serving.
pub fn run(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    project_root: &Path,
    src_rel: &Path,
    node_floor: u32,
    opts: &DevOptions,
) -> ExitCode {
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
    let src = project_root.join(src_rel);
    // Default: compile in-process. Escape hatch: if `BYNK_BYNKC` pointed the
    // driver at an external compiler (`Origin::Override`), shell *that* binary
    // instead — the only path on which a second, skewable compiler enters
    // (doctor reports its skew only here). With no override there is no separate
    // compiler to drift against.
    let used_override = matches!(compiler.origin, Some(crate::compiler::Origin::Override));
    if let (true, Some(bynkc)) = (used_override, compiler.path.as_deref()) {
        let status = Command::new(bynkc)
            .arg("compile")
            .arg(&src)
            .arg("--output")
            .arg(&build_dir)
            .arg("--target")
            .arg("workers")
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => return ExitCode::from(exit_byte(s.code())),
            Err(e) => {
                eprintln!("bynk: could not run bynkc ({}): {e}", bynkc.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        let options = dev_compile_options(&src);
        let output = match bynk_emit::project::compile_project(&options) {
            Ok(out) => out,
            Err(failure) => {
                // Render with full source context, exactly as the shelled `bynkc
                // compile` did — the front-end's flatten-then-delegate (ADR 0100),
                // shared with `bynk check` (see `crate::diagnostics`).
                crate::diagnostics::render_project_failure(&failure);
                return ExitCode::FAILURE;
            }
        };
        if let Err(e) = bynk_emit::write_output(&output, &build_dir) {
            eprintln!(
                "bynk: could not write build output under `{}`: {e}",
                build_dir.display()
            );
            return ExitCode::FAILURE;
        }
    }

    // 3. Select the worker — exactly one, or the one named by `--context` (D3).
    let workers_dir = build_dir.join("workers");
    let available = discover_workers(&workers_dir);
    let worker = match select_context(&available, opts.context.as_deref()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("bynk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let worker_dir = workers_dir.join(&worker);

    // 4. Serve — `wrangler dev` from inside the worker dir (its `index.ts`
    //    imports `../../runtime.js`, so cwd must be the worker dir, exactly the
    //    manual recipe's `cd`). Resolve wrangler with doctor's provenance
    //    ordering; an npx resolution downloads on first use, so it is a notice,
    //    never a silent green path.
    let probe = probe::detect(
        tb,
        "wrangler",
        DetectOpts {
            project_root: Some(project_root),
            allow_npx: true,
        },
    );
    let mut cmd = match wrangler_command(&probe.provenance) {
        Some(cmd) => cmd,
        None => {
            // The pre-flight gate should have caught this; defensive only.
            eprintln!("bynk: wrangler not found (run `bynk doctor --only deploy`)");
            return ExitCode::FAILURE;
        }
    };
    if matches!(probe.provenance, Provenance::Npx) {
        eprintln!("bynk: wrangler resolved via npx — it will download on first run.");
    }
    cmd.current_dir(&worker_dir);
    // Slice 3 (ADR 0104): `--inspect` starts wrangler with the V8 inspector so a
    // JavaScript debugger can attach. Injected before the `--` passthrough, so a
    // power user's explicit `-- --inspector-port N` still wins. A `.bynk`
    // breakpoint resolves through the emitted source map, which esbuild composes
    // into the worker bundle.
    for arg in inspector_args(opts) {
        cmd.arg(arg);
    }
    if opts.inspect {
        let port = opts.inspect_port;
        eprintln!("bynk dev --inspect: the worker runs with the V8 inspector enabled.");
        eprintln!("  Attach a JavaScript debugger to the inspector on port {port} (CDP discovery:");
        eprintln!("  http://127.0.0.1:{port}/json). Breakpoints set in `.bynk` sources resolve");
        eprintln!("  through the emitted source maps. A hand-rolled CDP client must send an");
        eprintln!("  `Origin` header — VS Code's JavaScript debugger does this for you.");
    }
    for arg in &opts.wrangler_args {
        cmd.arg(arg);
    }

    // Inherited stdio (the default) keeps the session interactive. The driver
    // and wrangler share the terminal's foreground process group, so a Ctrl-C
    // SIGINT reaches both — we must not bail before reaping the child; we wait
    // and propagate its exit code (proposal §2.5).
    match cmd.status() {
        Ok(s) => ExitCode::from(exit_byte(s.code())),
        Err(e) => {
            eprintln!("bynk: could not run wrangler: {e}");
            ExitCode::FAILURE
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
fn prepare_build_dir(project_root: &Path, build_dir: &Path) -> std::io::Result<()> {
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
fn discover_workers(workers_dir: &Path) -> Vec<String> {
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
    /// More than one context, and no `--context` to disambiguate.
    Ambiguous(Vec<String>),
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
            SelectError::Ambiguous(available) => write!(
                f,
                "this project has several contexts — pass --context to choose one of: {}",
                available.join(", ")
            ),
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

/// Pick the worker dir to serve. Pure (the FS scan is done by the caller) so the
/// select-or-default rule (D3) is unit-tested directly.
///
/// `available` are worker *directory* names (dots already dasherised, e.g.
/// `commerce-payment`). A requested `--context` matches either the raw name or
/// its dasherised form, so both `--context commerce.payment` and `--context
/// commerce-payment` resolve.
pub fn select_context(
    available: &[String],
    requested: Option<&str>,
) -> Result<String, SelectError> {
    match requested {
        Some(name) => {
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
        None => match available {
            [] => Err(SelectError::NoneBuilt),
            [one] => Ok(one.clone()),
            many => Err(SelectError::Ambiguous(many.to_vec())),
        },
    }
}

/// Build the `wrangler dev` invocation for a resolved provenance: an installed
/// binary is run directly; an npx-provisionable one goes through `npx --yes`.
/// `None` when wrangler is genuinely missing.
fn wrangler_command(provenance: &Provenance) -> Option<Command> {
    match provenance {
        Provenance::Path(p) | Provenance::ProjectLocal(p) => {
            let mut cmd = Command::new(p);
            cmd.arg("dev");
            Some(cmd)
        }
        Provenance::Npx => {
            let mut cmd = Command::new("npx");
            cmd.arg("--yes").arg("wrangler").arg("dev");
            Some(cmd)
        }
        Provenance::Missing => None,
    }
}

/// The `wrangler dev` flags `--inspect` injects (slice 3): the inspector port, so
/// a JavaScript debugger can attach. Empty without `--inspect`. Injected ahead of
/// the `--` passthrough, so an explicit `-- --inspector-port N` still wins.
fn inspector_args(opts: &DevOptions) -> Vec<String> {
    if opts.inspect {
        vec![
            "--inspector-port".to_string(),
            opts.inspect_port.to_string(),
        ]
    } else {
        Vec::new()
    }
}

/// The compile options `bynk dev` builds for an in-process Workers compile —
/// mirrors `bynkc`'s `project_options` (split when `<src>` is a project root,
/// else single) so the build is identical to the previously-shelled
/// `bynkc compile <src> --target workers`.
fn dev_compile_options(src: &Path) -> CompileOptions {
    if src.join("bynk.toml").exists() || src.join("src").is_dir() {
        CompileOptions::split(src.to_path_buf(), read_project_paths(src))
    } else {
        CompileOptions::single(src.to_path_buf())
    }
    .target(BuildTarget::Workers)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sole_context_is_served_without_a_flag() {
        assert_eq!(
            select_context(&names(&["links"]), None),
            Ok("links".to_string())
        );
    }

    #[test]
    fn ambiguous_without_context_lists_the_options() {
        assert_eq!(
            select_context(&names(&["api", "worker"]), None),
            Err(SelectError::Ambiguous(names(&["api", "worker"])))
        );
    }

    #[test]
    fn no_workers_is_its_own_error() {
        assert_eq!(select_context(&[], None), Err(SelectError::NoneBuilt));
    }

    #[test]
    fn context_flag_selects_by_raw_or_dasherised_name() {
        let avail = names(&["api", "commerce-payment"]);
        assert_eq!(
            select_context(&avail, Some("commerce-payment")),
            Ok("commerce-payment".to_string())
        );
        // Dotted context name resolves to its dasherised worker dir.
        assert_eq!(
            select_context(&avail, Some("commerce.payment")),
            Ok("commerce-payment".to_string())
        );
    }

    #[test]
    fn unknown_context_reports_what_is_available() {
        assert_eq!(
            select_context(&names(&["api"]), Some("nope")),
            Err(SelectError::NotFound {
                requested: "nope".to_string(),
                available: names(&["api"]),
            })
        );
    }

    #[test]
    fn exit_byte_maps_codes_and_signals() {
        assert_eq!(exit_byte(Some(0)), 0);
        assert_eq!(exit_byte(Some(1)), 1);
        // Signal termination (None) is a clean stop, not a driver failure.
        assert_eq!(exit_byte(None), 0);
    }

    #[test]
    fn inspect_injects_the_inspector_port() {
        let off = DevOptions::default();
        assert!(
            inspector_args(&off).is_empty(),
            "no inspector args without --inspect"
        );

        let on = DevOptions {
            inspect: true,
            inspect_port: 9229,
            ..Default::default()
        };
        assert_eq!(
            inspector_args(&on),
            vec!["--inspector-port".to_string(), "9229".to_string()]
        );
    }
}
