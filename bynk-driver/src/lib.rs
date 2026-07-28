//! bynk-driver — the shared front-end of the `bynkc` and `bynk` CLIs (#521).
//!
//! Both binaries expose `fmt` and `check` with identical semantics; before
//! this crate each re-implemented the command bodies (and the project-failure
//! flattening layer, and the project-rooting rule) as by-hand copies pinned
//! only by comments and a skip-able parity test. The single implementation
//! lives here, parameterised by the program name that prefixes messages.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bynk_emit::project::{
    self, CompileOptions, ProjectPathsError, read_project_paths, try_read_project_paths,
};
use bynk_fmt::{FormatOptions, format_source};

pub mod coverage;
pub mod probe;
pub mod test_json;
pub mod test_runner;

/// Root a directory project the way every project command should (#46): a
/// `bynk.toml` or a `src/` subdir selects **project** mode, whose flat
/// `[paths] include`/`exclude` layout (v0.113, DECISION S) defaults to the
/// conventional roots that exist (`src`, `tests`) or the project root itself;
/// otherwise the legacy **single-tree** where `<dir>` is itself the root.
/// `check`, `compile`, `test`, and `dev` all route through this so the
/// conventional layout works the same from any of them.
pub fn project_options(input: &Path) -> CompileOptions {
    if input.join("bynk.toml").exists() || input.join("src").is_dir() {
        CompileOptions::split(input.to_path_buf(), read_project_paths(input))
    } else {
        CompileOptions::single(input.to_path_buf())
    }
}

/// [`project_options`], but a malformed `bynk.toml` is an error rather than a
/// silent fall-back to the conventional layout — the one input a user
/// hand-edits that the compiler otherwise reads without checking, after which
/// a cascade of `bynk.uses.unknown_target` errors points at units that
/// plainly exist on disk.
pub fn try_project_options(input: &Path) -> Result<CompileOptions, ProjectPathsError> {
    if input.join("bynk.toml").exists() || input.join("src").is_dir() {
        Ok(CompileOptions::split(
            input.to_path_buf(),
            try_read_project_paths(input)?,
        ))
    } else {
        Ok(CompileOptions::single(input.to_path_buf()))
    }
}

/// Render a project build failure with per-file ariadne context, exactly as
/// single-file mode had rich rendering. Unattributed (project-level) errors
/// keep the plain form.
///
/// This is the **flattening layer** (ADR 0100): it attributes each
/// `AttributedError` to its file snapshot and delegates the actual rendering to
/// [`bynk_render::print_errors`]. The `ProjectFailure → CompileError` flattening
/// stays here, above `bynk-render`, so there is no `render → emit` edge.
pub fn print_project_failure(failure: &project::ProjectFailure) {
    for ae in &failure.errors {
        match attributed_snapshot(ae, &failure.snapshots) {
            Some((label, text)) => {
                bynk_render::print_errors(std::slice::from_ref(&ae.error), text, &label);
            }
            None => {
                eprintln!("[{}] {}", ae.error.category, ae.error.message);
                for note in &ae.error.notes {
                    eprintln!("  note: {note}");
                }
                // Finding #47: a label's text still surfaces even with no
                // file to underline it against.
                for (_, label) in &ae.error.labels {
                    eprintln!("  label: {label}");
                }
            }
        }
    }
}

/// v0.89 (ADR 0117): print a successful build's non-failing warnings, with
/// real per-file ariadne context now that a successful build's `snapshots`
/// (mirroring `ProjectFailure::snapshots`) make that possible. A warning whose
/// source isn't attributable (or doesn't fit the snapshot) falls back to the
/// plain `warning[<category>]: <message>` form.
pub fn print_project_warnings(
    warnings: &[project::AttributedError],
    snapshots: &[(PathBuf, String)],
) {
    for w in warnings {
        match attributed_snapshot(w, snapshots) {
            Some((label, text)) => {
                bynk_render::print_errors(std::slice::from_ref(&w.error), text, &label)
            }
            None => {
                let where_ = w
                    .source_path
                    .as_deref()
                    .map(|p| format!("{}: ", p.to_string_lossy().replace('\\', "/")))
                    .unwrap_or_default();
                eprintln!("{where_}warning[{}]: {}", w.error.category, w.error.message);
                for note in &w.error.notes {
                    eprintln!("  note: {note}");
                }
                for (_, label) in &w.error.labels {
                    eprintln!("  label: {label}");
                }
            }
        }
    }
}

/// [`print_project_warnings`]'s `--format short` analogue: one
/// `path:line:col: warning[category]: message` line per warning, falling
/// back to `warning[category]: message` when unattributed. Strictly one
/// line per warning throughout (like [`bynk_render::render_errors_short`],
/// this mirrors the VS Code problem-matcher's contract), so — unlike
/// [`print_project_warnings`] — finding #47 doesn't reach this one.
pub fn print_project_warnings_short(
    warnings: &[project::AttributedError],
    snapshots: &[(PathBuf, String)],
) {
    for w in warnings {
        match attributed_snapshot(w, snapshots) {
            Some((label, text)) => eprintln!("{}", bynk_render::short_line(&label, text, &w.error)),
            // Every entry in `warnings` is warning-severity by construction
            // (ADR 0117's own split), so `severity_word` here is always
            // "warning" — read off the shared helper (finding #48) rather
            // than hardcoding the string a second time.
            None => eprintln!(
                "{}[{}]: {}",
                bynk_render::severity_word(&w.error),
                w.error.category,
                w.error.message
            ),
        }
    }
}

/// The `(label, source text)` an `AttributedError`'s `source_path` resolves
/// to in `snapshots`, if any — the one attribution lookup every renderer in
/// this file shares (finding #48; previously `print_project_failure` and
/// [`project_failure_short_lines`] each hand-rolled their own copy).
fn attributed_snapshot<'a>(
    ae: &project::AttributedError,
    snapshots: &'a [(PathBuf, String)],
) -> Option<(String, &'a str)> {
    let path = ae.source_path.as_deref()?;
    let text = snapshots
        .iter()
        .find(|(p, _)| p.as_path() == path)
        .map(|(_, t)| t.as_str())?;
    Some((path.to_string_lossy().replace('\\', "/"), text))
}

/// The project-failure analogue of [`bynk_render::print_errors_short`]: each
/// attributed error is positioned against its file's snapshot; an unattributed
/// (project-level) error falls back to `<severity>[<category>]: <message>`.
pub fn print_project_failure_short(failure: &project::ProjectFailure) {
    for line in project_failure_short_lines(failure) {
        eprintln!("{line}");
    }
}

/// The string form of [`print_project_failure_short`]: one `path:line:col:
/// severity[category]: message` line per attributed error (an unattributed
/// project-level error falls back to `severity[category]: message`). Backs both
/// the printer above and the `bynkc test --format json` compile-error document,
/// whose `diagnostics` the VS Code `bynkc` problem-matcher re-parses — each
/// `Vec` entry is exactly one line by that contract, so unlike the other
/// renderers in this file this one deliberately does *not* grow note/label
/// continuation lines (finding #47): doing so would break a machine consumer
/// that re-parses every entry as a single diagnostic line.
///
/// The flattening layer (ADR 0100): it delegates the per-error formatting to
/// [`bynk_render::short_line`] / [`bynk_render::severity_word`], and the
/// attribution lookup to the crate-private `attributed_snapshot` (finding #48).
pub fn project_failure_short_lines(failure: &project::ProjectFailure) -> Vec<String> {
    failure
        .errors
        .iter()
        .map(|ae| match attributed_snapshot(ae, &failure.snapshots) {
            Some((label, text)) => bynk_render::short_line(&label, text, &ae.error),
            None => format!(
                "{}[{}]: {}",
                bynk_render::severity_word(&ae.error),
                ae.error.category,
                ae.error.message
            ),
        })
        .collect()
}

/// Render every diagnostic from a [`project::ProjectCheck`] (finding #64) with
/// the same per-file ariadne context [`print_project_failure`] gives its own,
/// errors-only list. Unlike that renderer, a `ProjectCheck`'s list can
/// legitimately mix both severities — the unattributed fallback line names its
/// actual severity ([`bynk_render::severity_word`]) rather than
/// `print_project_failure`'s bare `[category]: message` (silently correct only
/// because that list is errors-only by construction).
pub fn print_project_check(check: &project::ProjectCheck) {
    for ae in &check.errors {
        match attributed_snapshot(ae, &check.snapshots) {
            Some((label, text)) => {
                bynk_render::print_errors(std::slice::from_ref(&ae.error), text, &label);
            }
            None => {
                eprintln!(
                    "{}[{}]: {}",
                    bynk_render::severity_word(&ae.error),
                    ae.error.category,
                    ae.error.message
                );
                for note in &ae.error.notes {
                    eprintln!("  note: {note}");
                }
                for (_, label) in &ae.error.labels {
                    eprintln!("  label: {label}");
                }
            }
        }
    }
}

/// [`print_project_check`]'s `--format short` analogue, mirroring
/// [`project_failure_short_lines`].
pub fn project_check_short_lines(check: &project::ProjectCheck) -> Vec<String> {
    check
        .errors
        .iter()
        .map(|ae| match attributed_snapshot(ae, &check.snapshots) {
            Some((label, text)) => bynk_render::short_line(&label, text, &ae.error),
            None => format!(
                "{}[{}]: {}",
                bynk_render::severity_word(&ae.error),
                ae.error.category,
                ae.error.message
            ),
        })
        .collect()
}

/// [`print_project_check`] via [`project_check_short_lines`].
pub fn print_project_check_short(check: &project::ProjectCheck) {
    for line in project_check_short_lines(check) {
        eprintln!("{line}");
    }
}

/// The `fmt` command body shared by `bynkc fmt` and `bynk fmt`: each input is
/// formatted and rewritten only when it changes; `--check` reports
/// non-canonical files without writing; `-` reads stdin and writes the
/// formatted result to stdout. `prog` prefixes messages (`bynk fmt: …`).
pub fn run_fmt(prog: &str, inputs: &[PathBuf], check: bool) -> ExitCode {
    let opts = FormatOptions::default();
    if inputs.is_empty() {
        eprintln!("{prog} fmt: no input files (pass file paths or `-` for stdin)");
        return ExitCode::FAILURE;
    }
    let mut had_diff = false;
    let mut had_error = false;
    for input in inputs {
        if input.as_os_str() == "-" {
            use std::io::Read;
            let mut source = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut source) {
                eprintln!("{prog} fmt: read from stdin: {e}");
                return ExitCode::FAILURE;
            }
            match format_source(&source, &opts) {
                Ok(formatted) => {
                    if check {
                        // `--check` on stdin must not print the formatted text
                        // (it would pollute a CI log) and must report a diff the
                        // same way the file path does — a `generator | bynk fmt
                        // --check -` gate is otherwise dead, passing green on
                        // non-canonical input.
                        if formatted != source {
                            eprintln!("{prog} fmt: <stdin> is not canonically formatted");
                            had_diff = true;
                        }
                    } else {
                        print!("{formatted}");
                    }
                }
                Err(e) => {
                    bynk_render::print_errors(&e.errors, &source, "<stdin>");
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{prog} fmt: read `{}`: {e}", input.display());
                had_error = true;
                continue;
            }
        };
        let filename = input.display().to_string();
        match format_source(&source, &opts) {
            Ok(formatted) => {
                if check {
                    if formatted != source {
                        eprintln!(
                            "{prog} fmt: {} is not canonically formatted",
                            input.display()
                        );
                        had_diff = true;
                    }
                } else if formatted != source
                    && let Err(e) = atomic_write(input, &formatted)
                {
                    eprintln!("{prog} fmt: write `{}`: {e}", input.display());
                    had_error = true;
                }
            }
            Err(e) => {
                bynk_render::print_errors(&e.errors, &source, &filename);
                had_error = true;
            }
        }
    }
    if had_error || (check && had_diff) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Write `contents` to `path` atomically: the bytes land in a sibling temp
/// file that is then `rename`d over `path`. A plain `std::fs::write` truncates
/// the destination *before* writing, so an ENOSPC, a signal, or a crash
/// mid-write leaves the file truncated or empty — and for `fmt`, whose only
/// copy of the original is the in-memory `source`, that original is then gone.
/// The rename is atomic on POSIX and Windows, so a reader sees either the whole
/// old file or the whole new one, never a half-written mix.
///
/// The temp file is a sibling (same directory) so the rename stays within one
/// filesystem — a cross-device rename would fail with `EXDEV`. Its name carries
/// the PID and a per-process counter so concurrent `fmt` runs, or two files in
/// one run, never collide, and it is opened with `create_new` (`O_EXCL`): a
/// pre-existing path — a stale temp from an earlier crashed run, or a symlink a
/// local actor pre-planted to redirect the formatted bytes — is refused rather
/// than opened, and we bump the counter and retry. On any failure the temp file
/// is removed so a botched write leaves no litter beside the untouched original.
///
/// The `rename` swaps in a fresh inode, so if `path` was a symlink or a
/// hardlink the formatted file replaces the link rather than being written
/// through it (the old `std::fs::write` wrote through). Uncommon for source
/// files, and the atomicity is worth it.
fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Open a fresh sibling temp file exclusively, bumping the counter past any
    // name that is already taken (stale temp or planted symlink).
    let (mut file, tmp) = loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!(".{file_name}.bynk-fmt.{}.{n}.tmp", std::process::id());
        let tmp = match dir {
            Some(d) => d.join(tmp_name),
            None => PathBuf::from(tmp_name),
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (f, tmp),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };

    // Scope the write so the handle is flushed and closed before the rename.
    // The rename replaces the destination inode, so carry the original file's
    // permissions onto the temp file first — otherwise a formatted file would
    // silently pick up the process umask's default mode (e.g. an executable or
    // group-restricted source would lose its bits).
    let write_result = (|| {
        // Best-effort: a filesystem that cannot honour the mode must not fail
        // the whole write.
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = file.set_permissions(meta.permissions());
        }
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// The `check` command body shared by `bynkc check` and `bynk check`: a
/// directory routes through [`project::check_project`] (finding #64 —
/// non-bailing, so a structural error anywhere does not hide diagnostics
/// elsewhere the way `compile_project`'s bail-fast `Mode::Build` would), a
/// single file through [`bynk_emit::compile_with_warnings`]. `short` selects
/// the one-line `--format short` rendering. `prog` prefixes messages
/// (`bynk: …`).
pub fn run_check(prog: &str, input: &Path, short: bool) -> ExitCode {
    if input.is_dir() {
        let options = match try_project_options(input) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("{prog}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let check = project::check_project(&options);
        let has_errors = check.has_errors();
        if short {
            print_project_check_short(&check);
        } else {
            print_project_check(&check);
        }
        if has_errors {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    } else {
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{prog}: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match bynk_emit::compile_with_warnings(&source, &filename) {
            Ok(compiled) => {
                if !compiled.warnings.is_empty() {
                    if short {
                        bynk_render::print_errors_short(&compiled.warnings, &source, &filename);
                    } else {
                        bynk_render::print_errors(&compiled.warnings, &source, &filename);
                    }
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                if short {
                    bynk_render::print_errors_short(&errors, &source, &filename);
                } else {
                    bynk_render::print_errors(&errors, &source, &filename);
                }
                ExitCode::FAILURE
            }
        }
    }
}
