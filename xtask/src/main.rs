//! `cargo xtask <command>` — Bynk repo automation (increment-allocation track).
//!
//! - `check-pending` (Slice 0) validates the files under `design/pending/`.
//! - `stamp` (Slice 1) assigns the version(s) + ADR number(s) for the pending
//!   files and materialises them; dry-run by default, `--apply` to write.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-pending") => check_pending(),
        Some("stamp") => {
            // Reject unknown flags rather than silently dry-running — on a
            // dry-run-by-default command, a typo'd `--aply` must not read as "no
            // --apply given" when the maintainer meant to write.
            if let Some(bad) = args[1..].iter().find(|a| *a != "--apply") {
                eprintln!("xtask stamp: unknown argument {bad:?}");
                usage();
                return ExitCode::from(2);
            }
            stamp(args.iter().any(|a| a == "--apply"))
        }
        Some(other) => {
            eprintln!("xtask: unknown command {other:?}");
            usage();
            ExitCode::from(2)
        }
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

/// The repo root — this crate's manifest dir is `<root>/xtask`.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn check_pending() -> ExitCode {
    match xtask::check_all() {
        Ok(n) => {
            println!("check-pending: {n} pending file(s) valid");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("{e}");
            }
            eprintln!("check-pending: {} error(s)", errors.len());
            ExitCode::FAILURE
        }
    }
}

fn stamp(apply: bool) -> ExitCode {
    let root = repo_root();
    let plan = match xtask::stamp::plan(&root) {
        Ok(p) => p,
        Err(errors) => {
            for e in &errors {
                eprintln!("{e}");
            }
            eprintln!("stamp: {} error(s) in design/pending/", errors.len());
            return ExitCode::FAILURE;
        }
    };

    if plan.is_empty() {
        println!("stamp: no pending increments — nothing to do");
        return ExitCode::SUCCESS;
    }

    println!("stamp: from v{}", plan.base_version);
    let mut adr_number = plan.first_adr_number;
    for inc in &plan.increments {
        println!("  {}  →  v{}", inc.file, inc.version);
        println!("      changelog: {}", inc.changelog);
        for adr in &inc.adrs {
            println!("      ADR {adr_number:04}-{}: {}", adr.slug, adr.title);
            adr_number += 1;
        }
    }

    if !apply {
        println!(
            "stamp: dry run — pass --apply to write (manifests would end on v{})",
            plan.final_version()
        );
        return ExitCode::SUCCESS;
    }

    // `--apply` edits tracked files in place. Refuse on a dirty worktree so the
    // debris of an earlier failed run (a half-written changelog, an orphaned ADR,
    // a partial `bump-version.sh`) is resolved first — otherwise a retry could
    // compound it. `apply` itself unwinds its own writes on failure, but a bump
    // script that dies mid-edit is outside its reach; this is the belt to that
    // rollback's suspenders.
    if let Err(e) = ensure_clean_worktree(&root) {
        eprintln!("stamp: {e}");
        return ExitCode::FAILURE;
    }

    match xtask::stamp::apply(&root, &plan, |v| run_bump_version(&root, v)) {
        Ok(()) => {
            println!("stamp: applied — manifests on v{}", plan.final_version());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("stamp: apply failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Run `scripts/bump-version.sh <version>` at the repo root (an absolute path so
/// child-cwd resolution is not a factor).
fn run_bump_version(root: &Path, v: xtask::stamp::Version) -> std::io::Result<()> {
    let script = root.join("scripts/bump-version.sh");
    let status = Command::new(&script)
        .arg(v.to_string())
        .current_dir(root)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{} exited with {status}",
            script.display()
        )))
    }
}

/// Refuse to `--apply` unless the worktree is clean. Runs `git status
/// --porcelain` at `root`; empty output means clean (no tracked modifications
/// and no untracked files — the orphaned ADR a partial run leaves is untracked,
/// so untracked files must count). A git that fails to run is treated as an
/// error rather than a pass: the guard exists precisely to be conservative.
fn ensure_clean_worktree(root: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git to check the worktree: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git status failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    if !out.stdout.is_empty() {
        return Err(format!(
            "refusing to --apply on a dirty worktree — commit or discard changes first:\n{}",
            String::from_utf8_lossy(&out.stdout).trim_end()
        ));
    }
    Ok(())
}

fn usage() {
    eprintln!("usage: cargo xtask <check-pending | stamp [--apply]>");
}
