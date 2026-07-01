//! `bynk` — the Bynk driver binary. See the crate docs in `lib.rs`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bynk::cli::{Cli, Command};
use bynk::compiler::{self, Compiler};
use bynk::dev::{self, DevOptions};
use bynk::doctor::{self, Context, DoctorOptions};
use bynk::new::{self, NewOptions};
use bynk::probe::{SystemToolbox, Toolbox, Version};
use bynk::report::{self, Format};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor {
            input,
            only,
            strict,
            format,
        } => run_doctor(
            input,
            bynk::cli::doctor_options(only, strict),
            format.into(),
        ),
        Command::Dev {
            path,
            context,
            inspect,
            inspect_port,
            wrangler_args,
        } => run_dev(
            path,
            DevOptions {
                context,
                inspect,
                inspect_port,
                wrangler_args,
            },
        ),
        Command::New { path, name } => new::run(&NewOptions { path, name }),
    }
}

/// Locate the compiler the driver shells: `$BYNK_BYNKC` override, else `PATH`,
/// else a sibling of this `bynk` binary. Shared by every subcommand.
fn resolve_compiler(tb: &dyn Toolbox) -> Compiler {
    let override_path = std::env::var_os("BYNK_BYNKC").map(PathBuf::from);
    let bynk_bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let driver = Version::parse(bynk::DRIVER_VERSION).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });
    compiler::resolve(
        tb,
        override_path.as_deref(),
        bynk_bin_dir.as_deref(),
        driver,
    )
}

fn run_doctor(input: PathBuf, opts: DoctorOptions, format: Format) -> ExitCode {
    let tb = SystemToolbox;
    let compiler = resolve_compiler(&tb);

    let project_root = find_project_root(&input);
    let ctx = Context {
        in_repo: in_bynk_repo(&input),
        project_root,
        node_floor: bynk_emit::NODE_MAJOR_FLOOR,
    };

    let report = doctor::diagnose(&tb, &compiler, &ctx, &opts);
    print!("{}", report::render(&report, format));

    if report.exit_nonzero(&opts) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_dev(path: PathBuf, opts: DevOptions) -> ExitCode {
    let tb = SystemToolbox;
    let compiler = resolve_compiler(&tb);

    let Some(project_root) = find_project_root(&path) else {
        eprintln!(
            "bynk: not inside a Bynk project (no bynk.toml found from `{}`)",
            path.display()
        );
        return ExitCode::FAILURE;
    };
    // v0.113: the first `include` tree is the primary source root to watch
    // (defaults to `src`, else the project root).
    let src_rel = bynk_emit::project::read_project_paths(&project_root)
        .include
        .into_iter()
        .next()
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    dev::run(
        &tb,
        &compiler,
        &project_root,
        &src_rel,
        bynk_emit::NODE_MAJOR_FLOOR,
        &opts,
    )
}

/// Walk up from `start` for the nearest `bynk.toml` (the project root).
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().ok()?;
    loop {
        if dir.join("bynk.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Are we inside the Bynk source repo? Gates the contributor `build`
/// capability. Identified by the workspace markers `bynkc/Cargo.toml` and
/// `design/decisions` in some ancestor.
fn in_bynk_repo(start: &Path) -> bool {
    let Ok(mut dir) = start.canonicalize() else {
        return false;
    };
    loop {
        if dir.join("bynkc").join("Cargo.toml").is_file()
            && dir.join("design").join("decisions").is_dir()
        {
            return true;
        }
        if !dir.pop() {
            return false;
        }
    }
}
