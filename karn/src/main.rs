//! `karn` — the Karn driver binary. See the crate docs in `lib.rs`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use karn::cli::{Cli, Command};
use karn::compiler;
use karn::doctor::{self, Context, DoctorOptions};
use karn::probe::{SystemToolbox, Version};
use karn::report::{self, Format};

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
            karn::cli::doctor_options(only, strict),
            format.into(),
        ),
    }
}

fn run_doctor(input: PathBuf, opts: DoctorOptions, format: Format) -> ExitCode {
    let tb = SystemToolbox;

    // Locate the compiler the driver shells: $KARN_KARNC override, else PATH,
    // else a sibling of this `karn` binary.
    let override_path = std::env::var_os("KARN_KARNC").map(PathBuf::from);
    let karn_bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let driver = Version::parse(karn::DRIVER_VERSION).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });
    let compiler = compiler::resolve(
        &tb,
        override_path.as_deref(),
        karn_bin_dir.as_deref(),
        driver,
    );

    let project_root = find_project_root(&input);
    let ctx = Context {
        in_repo: in_karn_repo(&input),
        project_root,
        node_floor: karnc::NODE_MAJOR_FLOOR,
    };

    let report = doctor::diagnose(&tb, &compiler, &ctx, &opts);
    print!("{}", report::render(&report, format));

    if report.exit_nonzero(&opts) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Walk up from `start` for the nearest `karn.toml` (the project root).
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().ok()?;
    loop {
        if dir.join("karn.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Are we inside the Karn source repo? Gates the contributor `build`
/// capability. Identified by the workspace markers `karnc/Cargo.toml` and
/// `design/decisions` in some ancestor.
fn in_karn_repo(start: &Path) -> bool {
    let Ok(mut dir) = start.canonicalize() else {
        return false;
    };
    loop {
        if dir.join("karnc").join("Cargo.toml").is_file()
            && dir.join("design").join("decisions").is_dir()
        {
            return true;
        }
        if !dir.pop() {
            return false;
        }
    }
}
