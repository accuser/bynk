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

use crate::compiler::Compiler;
use crate::doctor::{self, Capability, Context, DoctorOptions};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};

/// Parsed `bynk dev` flags (the project `PATH` is resolved into `project_root`
/// before we get here).
#[derive(Debug, Clone, Default)]
pub struct DevOptions {
    /// `--context NAME` — which context's worker to serve.
    pub context: Option<String>,
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
        eprintln!("bynk: environment not ready for `dev` — see below.\n");
        eprint!("{}", report::render(&report, Format::Human));
        return ExitCode::FAILURE;
    }
    // The compile floor is guaranteed resolved past the gate above.
    let Some(bynkc) = compiler.path.as_deref() else {
        eprintln!("bynk: bynkc could not be located");
        return ExitCode::FAILURE;
    };

    // 2. Compile — into the managed `.bynk/dev/` build dir (D1). `bynkc compile`
    //    is additive (never prunes), so clear `workers/` first; otherwise a
    //    renamed/deleted context would linger and spuriously trip the §2.4
    //    ambiguity check.
    let build_dir = project_root.join(".bynk").join("dev");
    if let Err(e) = prepare_build_dir(project_root, &build_dir) {
        eprintln!("bynk: could not prepare build directory: {e}");
        return ExitCode::FAILURE;
    }
    let src = project_root.join(src_rel);
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

/// Map a child exit code to a process exit byte. A `None` code means the child
/// was terminated by a signal (e.g. the Ctrl-C the terminal also delivered to
/// us) — treat that as a clean stop rather than a driver failure.
fn exit_byte(code: Option<i32>) -> u8 {
    code.unwrap_or(0).clamp(0, 255) as u8
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
}
