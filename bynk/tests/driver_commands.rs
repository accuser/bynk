//! `bynk check` / `bynk fmt` / `bynk test` — the everyday commands the driver
//! grew in v0.138 (#487).
//!
//! Two things are pinned:
//!  1. **The in-process commands work through the driver** — `check` and `fmt`
//!     run with no `bynkc` binary in play, over the real CLI wiring (clap →
//!     dispatch → module), with deterministic (relative-path) output.
//!  2. **Drop-in equivalence with `bynkc`** — when the sibling `bynkc` binary is
//!     present next to `bynk` (a full workspace build), `bynk check`/`fmt` are
//!     byte-for-byte identical to `bynkc check`/`fmt`, and `bynk test` delegates
//!     to it. These parity checks **skip** (rather than fail) when `bynkc` was
//!     not built — e.g. under a bare `cargo test -p bynk` — so the file is green
//!     either way and exhaustive under a workspace test run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// The `bynk` binary under test.
fn bynk() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bynk"))
}

/// The sibling `bynkc` the driver would resolve — present only when the whole
/// workspace was built. Parity tests gate on this.
fn bynkc_sibling() -> Option<PathBuf> {
    let p = bynk().parent().unwrap().join("bynkc");
    p.exists().then_some(p)
}

/// A clean, empty scratch directory unique to `name`, under the crate's test
/// tmpdir (cleaned between runs).
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Run `bynk <args>` in `cwd`, capturing (code, stdout, stderr).
fn run_bynk_in(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    run_in(&bynk(), cwd, args, None)
}

/// Run a binary in `cwd`, optionally feeding `stdin`, capturing (code, out, err).
fn run_in(bin: &Path, cwd: &Path, args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().expect("spawn");
    if let Some(text) = stdin {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().expect("wait");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const CLEAN_COMMONS: &str = "commons calc {\n  fn dbl(n: Int) -> Int { n + n }\n}\n";
const BAD_SOURCE: &str = "commons demo\n\nlet x: Int = \"nope\"\n";

// ---------------------------------------------------------------------------
// bynk check
// ---------------------------------------------------------------------------

#[test]
fn check_clean_project_exits_zero() {
    let proj = scratch("check-clean");
    write(
        &proj.join("bynk.toml"),
        "[project]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    );
    write(&proj.join("src/calc.bynk"), CLEAN_COMMONS);
    let (code, _out, err) = run_bynk_in(&proj, &["check", "."]);
    assert_eq!(code, 0, "clean project should check clean; stderr:\n{err}");
}

#[test]
fn check_single_file_error_short_is_deterministic() {
    // A single-file error, `--format short`, rendered against a *relative* path
    // so the tooling line is stable across machines.
    let dir = scratch("check-short");
    write(&dir.join("bad.bynk"), BAD_SOURCE);
    let (code, out, err) = run_bynk_in(&dir, &["check", "bad.bynk", "--format", "short"]);
    assert_eq!(code, 1, "an error source must exit non-zero");
    assert!(out.is_empty(), "diagnostics go to stderr, not stdout");
    let first = err.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("bad.bynk:") && first.contains("error["),
        "expected a `bad.bynk:line:col: error[...]` short line, got:\n{err}"
    );
}

#[test]
fn check_matches_bynkc_when_present() {
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (run the workspace test suite for parity)");
        return;
    };
    let dir = scratch("check-parity");
    write(&dir.join("bad.bynk"), BAD_SOURCE);
    write(&dir.join("ok.bynk"), CLEAN_COMMONS);
    for args in [
        vec!["check", "bad.bynk"],
        vec!["check", "bad.bynk", "--format", "short"],
        vec!["check", "ok.bynk"],
    ] {
        let driven = run_bynk_in(&dir, &args);
        let direct = run_in(&bynkc, &dir, &args, None);
        assert_eq!(
            driven,
            direct,
            "`bynk {0}` must match `bynkc {0}` exactly",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// bynk fmt
// ---------------------------------------------------------------------------

/// A valid commons whose whitespace is not canonical (a doubled blank line the
/// formatter collapses), so `fmt` has something to change.
const MESSY: &str = "commons calc {\n\n\n  fn dbl(n: Int) -> Int { n + n }\n}\n";

fn canonical(source: &str) -> String {
    bynk_fmt::format_source(source, &bynk_fmt::FormatOptions::default()).expect("MESSY parses")
}

#[test]
fn fmt_stdin_writes_canonical_to_stdout() {
    let dir = scratch("fmt-stdin");
    let want = canonical(MESSY);
    assert_ne!(
        MESSY, want,
        "MESSY must be non-canonical for this test to bite"
    );
    let (code, out, err) = run_in(&bynk(), &dir, &["fmt", "-"], Some(MESSY));
    assert_eq!(code, 0, "stdin fmt should succeed; stderr:\n{err}");
    assert_eq!(out, want, "`bynk fmt -` must emit the canonical form");
}

#[test]
fn fmt_stdin_check_flags_noncanonical_and_stays_silent() {
    // `--check` on stdin must behave like `--check` on a file: report a diff on
    // stderr and exit non-zero, *without* echoing the formatted text to stdout.
    // A `generator | bynk fmt --check -` CI gate is otherwise dead — it passes
    // green on non-canonical input and pollutes the log with the reformat.
    let dir = scratch("fmt-stdin-check-bad");
    let (code, out, err) = run_in(&bynk(), &dir, &["fmt", "--check", "-"], Some(MESSY));
    assert_eq!(code, 1, "--check on non-canonical stdin must exit non-zero");
    assert!(
        out.is_empty(),
        "--check must not echo the reformatted text to stdout, got:\n{out}"
    );
    assert!(
        err.contains("not canonically formatted"),
        "expected a non-canonical notice, got:\n{err}"
    );
}

#[test]
fn fmt_stdin_check_passes_on_canonical() {
    // Already-canonical stdin exits zero and prints nothing.
    let dir = scratch("fmt-stdin-check-ok");
    let want = canonical(MESSY);
    let (code, out, err) = run_in(&bynk(), &dir, &["fmt", "--check", "-"], Some(&want));
    assert_eq!(
        code, 0,
        "--check on canonical stdin must exit zero; err:\n{err}"
    );
    assert!(
        out.is_empty(),
        "--check prints nothing on stdout, got:\n{out}"
    );
}

#[test]
fn fmt_check_flags_noncanonical_without_writing() {
    let dir = scratch("fmt-check");
    let file = dir.join("calc.bynk");
    write(&file, MESSY);
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "calc.bynk", "--check"]);
    assert_eq!(
        code, 1,
        "--check must exit non-zero on a non-canonical file"
    );
    assert!(
        err.contains("not canonically formatted"),
        "expected a non-canonical notice, got:\n{err}"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        MESSY,
        "--check must not rewrite the file"
    );
}

#[test]
fn fmt_rewrites_in_place() {
    let dir = scratch("fmt-write");
    let file = dir.join("calc.bynk");
    write(&file, MESSY);
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "calc.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        canonical(MESSY),
        "fmt must rewrite the file to its canonical form"
    );
}

#[test]
fn fmt_write_leaves_no_temp_litter() {
    // The in-place rewrite goes through a sibling temp file + atomic rename.
    // After a successful format the directory holds exactly the source file —
    // no `.calc.bynk.bynk-fmt.*.tmp` left behind.
    let dir = scratch("fmt-atomic");
    let file = dir.join("calc.bynk");
    write(&file, MESSY);
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "calc.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        canonical(MESSY),
        "fmt must rewrite the file to its canonical form"
    );
    let entries: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["calc.bynk".to_string()],
        "atomic rename must leave no temp file behind, found: {entries:?}"
    );
}

#[cfg(unix)]
#[test]
fn fmt_write_through_symlink_replaces_the_link() {
    // The atomic rename swaps in a fresh inode, so formatting a symlinked source
    // replaces the *link* with the formatted file rather than writing through to
    // its target (the old `std::fs::write` wrote through). This documents the
    // deliberate behaviour change: the formatted content must land, and the
    // target must be left untouched.
    let dir = scratch("fmt-symlink");
    let target = dir.join("target.bynk");
    let link = dir.join("link.bynk");
    // Non-canonical, so `fmt` actually rewrites (a canonical source is left
    // byte-identical and never hits the write path).
    write(&target, MESSY);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "link.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    assert_eq!(
        std::fs::read_to_string(&link).unwrap(),
        canonical(MESSY),
        "the link path must hold the formatted content"
    );
    assert!(
        !std::fs::symlink_metadata(&link).unwrap().is_symlink(),
        "the symlink must have been replaced by a regular file"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        MESSY,
        "the symlink target must be left untouched"
    );
}

#[cfg(unix)]
#[test]
fn fmt_write_preserves_file_permissions() {
    // The atomic rewrite replaces the destination inode, so it must carry the
    // original file's mode across — a plain temp-file + rename would otherwise
    // reset the formatted file to the process umask default.
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("fmt-perms");
    let file = dir.join("calc.bynk");
    write(&file, MESSY);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "calc.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "atomic rewrite must preserve the file mode");
}

// ---------------------------------------------------------------------------
// bynk test (delegation)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_delegates_and_matches_bynkc() {
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (delegation parity needs it)");
        return;
    };
    // `--no-run --format json` is a pure compile — no `tsc`/`node` needed — so
    // this exercises delegation and the discovery document on any machine.
    let proj = scratch("test-discovery");
    write(
        &proj.join("bynk.toml"),
        "[project]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    );
    write(
        &proj.join("src/calc.bynk"),
        "commons calc {\n  fn dbl(n: Int) -> Int { n + n }\n}\n\nsuite calc {\n  case \"doubles\" {\n    expect dbl(3) == 6\n  }\n}\n",
    );
    let args = ["test", ".", "--no-run", "--format", "json"];
    let driven = run_bynk_in(&proj, &args);
    let direct = run_in(&bynkc, &proj, &args, None);
    assert_eq!(driven.0, direct.0, "delegated exit code must match bynkc's");
    assert_eq!(
        driven.1, direct.1,
        "delegated discovery document must match bynkc's byte-for-byte"
    );
}
