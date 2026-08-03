//! `cargo xtask ci` at the process boundary, plus the `pre-push` hook that
//! execs it.
//!
//! The step *selection* is unit-tested next to `ci_steps` in `main.rs` (a
//! binary crate, so it cannot be reached from here). What is left over is the
//! part only observable from outside: argument rejection, and the hook's
//! early-exit path.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("run xtask")
}

/// A typo'd flag must not silently read as "no flag given". `--fastt` running
/// the full suite instead of the fast tier would be a slow surprise; `--aply`
/// on `stamp` (the same shape) would be a destructive one, which is why the
/// existing commands reject unknown arguments and why `ci` does too.
#[test]
fn a_misspelled_flag_is_rejected_rather_than_ignored() {
    let out = xtask(&["ci", "--fastt"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, got {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown argument"), "stderr was: {stderr}");
    assert!(stderr.contains("usage:"), "stderr was: {stderr}");
}

#[test]
fn an_unknown_subcommand_is_rejected() {
    let out = xtask(&["cii"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command"));
}

/// `usage:` must name `ci`, so a contributor who runs `cargo xtask` bare finds
/// the command rather than only learning of it from the README.
#[test]
fn usage_advertises_the_ci_command() {
    let out = xtask(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ci [--fast]"));
}

/// Deleting a remote branch (`git push origin :topic`) sends an all-zero local
/// SHA and no worktree change worth linting. The hook must take its early exit
/// instead of spending a workspace clippy build on it.
///
/// Asserted through stdout rather than by watching for a cargo process: the
/// hook announces itself immediately before `exec`, so the absence of that line
/// is exactly "it never got as far as running the gates".
///
/// Unix-only: this drives the hook through `sh`. The hook is opt-in via
/// `core.hooksPath` and its content is platform-independent, so covering the
/// logic on one platform is enough.
#[cfg(unix)]
#[test]
fn the_hook_skips_a_branch_deletion_without_running_the_gates() {
    let zero = "0".repeat(40);
    let refline = format!("refs/heads/x {zero} refs/heads/x {zero}\n");

    let mut child = Command::new("sh")
        .arg(".githooks/pre-push")
        .current_dir(repo_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the hook");

    use std::io::Write;
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(refline.as_bytes())
        .expect("write the ref line");

    let out = child.wait_with_output().expect("wait for the hook");
    assert!(
        out.status.success(),
        "hook must exit 0 on a branch deletion, got {:?}",
        out.status.code()
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("running the fast CI gates"),
        "hook ran the gates on a branch deletion; stdout was: {stdout}"
    );
}

/// The converse of the above: a real push (a non-zero SHA) must NOT take the
/// early exit. Without this, a hook that exited 0 unconditionally would still
/// pass the deletion test while checking nothing on any push.
///
/// Stops at the announcement rather than letting the gates run — this test must
/// not spend a workspace clippy build. `sh -e` makes the failed write when the
/// hook's stdout closes end the process, which is the signal we assert on.
#[cfg(unix)]
#[test]
fn the_hook_proceeds_to_the_gates_on_a_real_push() {
    let sha = "1".repeat(40);
    let zero = "0".repeat(40);
    let refline = format!("refs/heads/x {sha} refs/heads/x {zero}\n");

    // `head -1` closes the pipe after the first line, so the hook dies on the
    // next write instead of running cargo to completion.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "printf '%s' '{refline}' | sh .githooks/pre-push 2>/dev/null | head -1"
        ))
        .current_dir(repo_root())
        .output()
        .expect("run the hook");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("running the fast CI gates"),
        "hook took the early exit on a real push; stdout was: {stdout:?}"
    );
}
