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
        // A `BrokenPipe` here is the child having exited before reading its
        // input — legitimate for any invocation rejected on its arguments
        // (`fmt --indent tab --indent-width 4 -`), which never reaches the
        // read. Whether we lose that race depends on machine load, so
        // unwrapping made such a case a flake rather than a failure. Swallow
        // only that error; the exit code and stderr are what the test asserts.
        let res = child.stdin.take().unwrap().write_all(text.as_bytes());
        match res {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => panic!("feeding stdin to {}: {e}", bin.display()),
        }
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
// bynk fmt — style overrides (#968)
// ---------------------------------------------------------------------------

/// A record wide enough that the canonical 100-column budget keeps it on one
/// line, so a narrowed `--max-line-width` visibly changes the output, and
/// nested one level so the indent flags have something to indent.
const WIDE_RECORD: &str =
    "commons calc {\ntype R = { id: String, guestName: String, roomNumber: Int, nights: Int }\n}\n";

#[test]
fn fmt_indent_spaces_replaces_tabs() {
    let dir = scratch("fmt-indent-spaces");
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &["fmt", "--indent", "spaces", "--indent-width", "4", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    assert!(
        !out.contains('\t'),
        "no tab may survive `--indent spaces`:\n{out}"
    );
    assert!(
        out.contains("\n    type R"),
        "expected a 4-space indent, got:\n{out}"
    );
}

#[test]
fn fmt_indent_spaces_defaults_to_two() {
    // The fallback matches `bynk.toml`'s `[fmt] indent_width`, so the CLI and
    // the editor's format-on-save agree when only `indent = "spaces"` is set.
    let dir = scratch("fmt-indent-spaces-default");
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &["fmt", "--indent", "spaces", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    assert!(
        out.contains("\n  type R"),
        "expected a 2-space indent, got:\n{out}"
    );
}

#[test]
fn fmt_indent_width_without_spaces_is_refused() {
    // Silently ignoring an explicit flag is the failure mode this replaces.
    let dir = scratch("fmt-indent-width-tab");
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &["fmt", "--indent", "tab", "--indent-width", "4", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(
        code, 1,
        "a meaningless `--indent-width` must not pass silently"
    );
    assert!(out.is_empty(), "nothing should be formatted, got:\n{out}");
    assert!(
        err.contains("--indent-width") && err.contains("--indent spaces"),
        "expected an actionable message, got:\n{err}"
    );
}

#[test]
fn fmt_max_line_width_narrows_the_budget() {
    let dir = scratch("fmt-max-line-width");
    let wide = run_in(&bynk(), &dir, &["fmt", "-"], Some(WIDE_RECORD));
    let narrow = run_in(
        &bynk(),
        &dir,
        &["fmt", "--max-line-width", "50", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(wide.0, 0, "default fmt should succeed; stderr:\n{}", wide.2);
    assert_eq!(
        narrow.0, 0,
        "narrow fmt should succeed; stderr:\n{}",
        narrow.2
    );
    assert!(
        wide.1.contains("type R = { id: String,"),
        "the record fits the 100-column default on one line, got:\n{}",
        wide.1
    );
    assert!(
        narrow.1.contains("\t\tid: String,\n"),
        "a 50-column budget must break the record open, got:\n{}",
        narrow.1
    );
}

#[test]
fn fmt_no_trailing_comma_drops_the_last_comma() {
    let dir = scratch("fmt-no-trailing-comma");
    let args = ["fmt", "--max-line-width", "50", "-"];
    let with = run_in(&bynk(), &dir, &args, Some(WIDE_RECORD));
    let without = run_in(
        &bynk(),
        &dir,
        &["fmt", "--max-line-width", "50", "--no-trailing-comma", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(with.0, 0, "stderr:\n{}", with.2);
    assert_eq!(without.0, 0, "stderr:\n{}", without.2);
    assert!(
        with.1.contains("nights: Int,\n"),
        "the default emits a trailing comma, got:\n{}",
        with.1
    );
    assert!(
        without.1.contains("nights: Int\n"),
        "`--no-trailing-comma` must drop it, got:\n{}",
        without.1
    );
}

#[test]
fn fmt_trailing_comma_overrides_an_earlier_no_trailing_comma() {
    // The pair is `overrides_with`, so a script may append a flag to a shared
    // argument list and have it win rather than conflict-error.
    let dir = scratch("fmt-trailing-comma-last-wins");
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &[
            "fmt",
            "--max-line-width",
            "50",
            "--no-trailing-comma",
            "--trailing-comma",
            "-",
        ],
        Some(WIDE_RECORD),
    );
    assert_eq!(code, 0, "stderr:\n{err}");
    assert!(
        out.contains("nights: Int,\n"),
        "the later `--trailing-comma` must win, got:\n{out}"
    );
}

#[test]
fn fmt_check_is_judged_against_the_overridden_style() {
    // `--check` must gate on the style the run asks for, not the canonical one
    // — otherwise a project on a non-default style has no usable CI gate.
    let dir = scratch("fmt-check-overridden");
    let file = dir.join("calc.bynk");
    let spaced = run_in(
        &bynk(),
        &dir,
        &["fmt", "--indent", "spaces", "--indent-width", "4", "-"],
        Some(WIDE_RECORD),
    )
    .1;
    write(&file, &spaced);

    let under_override = run_bynk_in(
        &dir,
        &[
            "fmt",
            "--check",
            "--indent",
            "spaces",
            "--indent-width",
            "4",
            "calc.bynk",
        ],
    );
    assert_eq!(
        under_override.0, 0,
        "canonical under the override must pass --check; stderr:\n{}",
        under_override.2
    );

    let under_default = run_bynk_in(&dir, &["fmt", "--check", "calc.bynk"]);
    assert_eq!(
        under_default.0, 1,
        "the same file is not canonical under the defaults; stderr:\n{}",
        under_default.2
    );
}

#[test]
fn fmt_style_overrides_match_bynkc_when_present() {
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (run the workspace test suite for parity)");
        return;
    };
    let dir = scratch("fmt-override-parity");
    for args in [
        vec!["fmt", "--indent", "spaces", "--indent-width", "4", "-"],
        vec!["fmt", "--max-line-width", "50", "-"],
        vec!["fmt", "--max-line-width", "50", "--no-trailing-comma", "-"],
        vec!["fmt", "--indent", "tab", "--indent-width", "4", "-"],
    ] {
        let driven = run_in(&bynk(), &dir, &args, Some(WIDE_RECORD));
        let direct = run_in(&bynkc, &dir, &args, Some(WIDE_RECORD));
        assert_eq!(
            driven.0,
            direct.0,
            "`bynk {0}` exit must match `bynkc {0}`",
            args.join(" ")
        );
        assert_eq!(
            driven.1,
            direct.1,
            "`bynk {0}` output must match `bynkc {0}`",
            args.join(" ")
        );
    }
}

#[test]
fn fmt_style_overrides_reach_a_pinned_bynkc() {
    // Under `BYNK_BYNKC` the driver shells the pinned compiler; the style flags
    // must be respelled onto that argv, or an override silently formats to the
    // canonical style instead of the one asked for.
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (the delegate path needs it)");
        return;
    };
    let dir = scratch("fmt-override-delegated");
    let mut cmd = Command::new(bynk());
    cmd.args(["fmt", "--indent", "spaces", "--indent-width", "3", "-"])
        .env("BYNK_BYNKC", &bynkc)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(WIDE_RECORD.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "delegated fmt should succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("\n   type R"),
        "the 3-space override must reach the pinned bynkc, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// bynk fmt — the bynk.toml [fmt] layer (#972)
// ---------------------------------------------------------------------------

/// A project whose manifest states a style, plus one source file under `src/`
/// so the manifest is found by walking *up* rather than sitting beside it.
fn project_with_fmt(name: &str, fmt_section: &str) -> PathBuf {
    let dir = scratch(name);
    write(
        &dir.join("bynk.toml"),
        &format!("[project]\nname = \"calc\"\nversion = \"0.1.0\"\n\n{fmt_section}"),
    );
    write(&dir.join("src/calc.bynk"), WIDE_RECORD);
    dir
}

#[test]
fn fmt_takes_its_style_from_the_projects_manifest() {
    let dir = project_with_fmt(
        "fmt-manifest",
        "[fmt]\nindent = \"spaces\"\nindent_width = 4\nmax_line_width = 50\n",
    );
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "src/calc.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    let got = std::fs::read_to_string(dir.join("src/calc.bynk")).expect("read back");
    assert!(!got.contains('\t'), "manifest asked for spaces:\n{got}");
    assert!(
        got.contains("\n        id: String,"),
        "expected 4-space indent at depth 2 and a 50-column break:\n{got}"
    );
}

#[test]
fn fmt_finds_the_manifest_by_walking_up_from_the_file() {
    // The manifest is two directories above the source file, and the run is
    // launched from the project root — the lookup starts at the *file*.
    let dir = scratch("fmt-manifest-nested");
    write(
        &dir.join("bynk.toml"),
        "[project]\nname = \"calc\"\n\n[fmt]\nindent = \"spaces\"\n",
    );
    write(&dir.join("src/deep/calc.bynk"), WIDE_RECORD);
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "src/deep/calc.bynk"]);
    assert_eq!(code, 0, "fmt should succeed; stderr:\n{err}");
    let got = std::fs::read_to_string(dir.join("src/deep/calc.bynk")).expect("read back");
    assert!(
        !got.contains('\t'),
        "manifest two levels up was not found:\n{got}"
    );
}

#[test]
fn a_flag_beats_the_manifest_and_silence_does_not() {
    let dir = project_with_fmt(
        "fmt-manifest-override",
        "[fmt]\nindent = \"spaces\"\nmax_line_width = 50\n",
    );
    // `--indent tab` overrides the manifest's spaces; `max_line_width` is not
    // named on the command line, so the manifest's 50 still applies.
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &["fmt", "--indent", "tab", "src/calc.bynk", "--check"],
        None,
    );
    assert_eq!(
        code, 1,
        "the file is not canonical under that mix; err:\n{err}"
    );
    assert!(out.is_empty());

    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "--indent", "tab", "src/calc.bynk"]);
    assert_eq!(code, 0, "stderr:\n{err}");
    let got = std::fs::read_to_string(dir.join("src/calc.bynk")).expect("read back");
    assert!(got.contains('\t'), "`--indent tab` must win:\n{got}");
    assert!(
        got.contains("\n\t\tid: String,"),
        "the manifest's 50-column budget must still apply:\n{got}"
    );
}

#[test]
fn no_config_ignores_the_manifest_entirely() {
    let dir = project_with_fmt(
        "fmt-no-config",
        "[fmt]\nindent = \"spaces\"\nmax_line_width = 50\n",
    );
    let (code, out, err) = run_in(
        &bynk(),
        &dir,
        &["fmt", "--no-config", "-"],
        Some(WIDE_RECORD),
    );
    assert_eq!(code, 0, "stderr:\n{err}");
    assert!(
        out.contains('\t'),
        "`--no-config` must format canonically:\n{out}"
    );
    assert!(
        out.contains("type R = { id: String,"),
        "the manifest's 50-column budget must not apply:\n{out}"
    );
}

#[test]
fn fmt_check_gates_on_the_manifests_style() {
    // The point of the layer: a project on a non-default style gets a CI gate
    // that can actually pass, which `bynk fmt --check` alone could not give it
    // before the manifest was read.
    let dir = project_with_fmt("fmt-manifest-check", "[fmt]\nindent = \"spaces\"\n");
    assert_eq!(
        run_bynk_in(&dir, &["fmt", "src/calc.bynk"]).0,
        0,
        "format it to the project's style first"
    );
    let (code, _out, err) = run_bynk_in(&dir, &["fmt", "--check", "src/calc.bynk"]);
    assert_eq!(
        code, 0,
        "a file in the project's own style passes; err:\n{err}"
    );
    // …and the canonical rendering does *not*, since the project chose spaces.
    let (code, _out, _err) = run_bynk_in(&dir, &["fmt", "--check", "--no-config", "src/calc.bynk"]);
    assert_eq!(code, 1, "canonical style is not this project's style");
}

#[test]
fn one_run_resolves_each_input_against_its_own_project() {
    // Two projects, one command. A single set of options for the whole run
    // would have to pick one of them and be wrong about the other.
    let a = project_with_fmt("fmt-two-projects-a", "[fmt]\nindent = \"spaces\"\n");
    let b = project_with_fmt("fmt-two-projects-b", "[fmt]\nmax_line_width = 40\n");
    let b_src = b.join("src/calc.bynk");
    let (code, _out, err) = run_in(
        &bynk(),
        &a,
        &["fmt", "src/calc.bynk", b_src.to_str().expect("utf-8")],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{err}");
    let got_a = std::fs::read_to_string(a.join("src/calc.bynk")).expect("read a");
    let got_b = std::fs::read_to_string(&b_src).expect("read b");
    assert!(!got_a.contains('\t'), "a/ chose spaces:\n{got_a}");
    assert!(got_b.contains('\t'), "b/ left indentation alone:\n{got_b}");
    assert!(
        got_b.contains("\n\t\tid: String,"),
        "b/'s 40-column budget must break the record:\n{got_b}"
    );
}

#[test]
fn a_broken_manifest_is_reported_not_ignored() {
    for (name, section, needle) in [
        (
            "fmt-bad-indent",
            "[fmt]\nindent = \"tabs\"\n",
            "\"tab\" or \"spaces\"",
        ),
        (
            "fmt-bad-key",
            "[fmt]\nmax_line_length = 120\n",
            "unknown field",
        ),
        (
            "fmt-bad-toml",
            "[fmt\nindent = \"tab\"\n",
            "TOML parse error",
        ),
        ("fmt-bad-width", "[fmt]\nmax_line_width = 0\n", "at least 1"),
    ] {
        let dir = scratch(name);
        write(&dir.join("bynk.toml"), section);
        write(&dir.join("src/calc.bynk"), WIDE_RECORD);
        let (code, _out, err) = run_bynk_in(&dir, &["fmt", "src/calc.bynk"]);
        assert_eq!(
            code, 1,
            "{name}: a broken manifest must fail the run; err:\n{err}"
        );
        assert!(
            err.contains(needle),
            "{name}: expected {needle:?} in the message, got:\n{err}"
        );
        assert!(
            err.contains("bynk.toml"),
            "{name}: the message must name the manifest, got:\n{err}"
        );
        // The file is untouched — a config error stops before any write.
        let got = std::fs::read_to_string(dir.join("src/calc.bynk")).expect("read back");
        assert_eq!(got, WIDE_RECORD, "{name}: nothing should have been written");
    }
}

#[test]
fn the_manifest_layer_matches_bynkc_when_present() {
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (run the workspace test suite for parity)");
        return;
    };
    let dir = project_with_fmt(
        "fmt-manifest-parity",
        "[fmt]\nindent = \"spaces\"\nindent_width = 4\nmax_line_width = 50\n",
    );
    for args in [
        vec!["fmt", "-"],
        vec!["fmt", "--no-config", "-"],
        vec!["fmt", "--indent", "tab", "-"],
        vec!["fmt", "--max-line-width", "100", "-"],
    ] {
        let driven = run_in(&bynk(), &dir, &args, Some(WIDE_RECORD));
        let direct = run_in(&bynkc, &dir, &args, Some(WIDE_RECORD));
        assert_eq!(
            driven.0,
            direct.0,
            "`bynk {0}` exit must match `bynkc {0}`",
            args.join(" ")
        );
        assert_eq!(
            driven.1,
            direct.1,
            "`bynk {0}` output must match `bynkc {0}`",
            args.join(" ")
        );
    }
}

#[test]
fn the_manifest_layer_survives_delegation_to_a_pinned_bynkc() {
    // `bynk` forwards only the flags the run passed; the manifest lookup is the
    // child's, done from the same working directory against the same paths. A
    // resolved-and-forwarded default would override the project's own style.
    let Some(bynkc) = bynkc_sibling() else {
        eprintln!("skipping: sibling bynkc not built (the delegate path needs it)");
        return;
    };
    let dir = project_with_fmt("fmt-manifest-delegated", "[fmt]\nmax_line_width = 50\n");
    let mut cmd = Command::new(bynk());
    cmd.args(["fmt", "-"])
        .env("BYNK_BYNKC", &bynkc)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(WIDE_RECORD.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "delegated fmt should succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("\n\t\tid: String,"),
        "the project's 50-column budget must reach the pinned bynkc:\n{stdout}"
    );
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
