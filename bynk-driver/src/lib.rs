//! bynk-driver — the shared front-end of the `bynkc` and `bynk` CLIs (#521).
//!
//! Both binaries expose `fmt` and `check` with identical semantics; before
//! this crate each re-implemented the command bodies (and the project-failure
//! flattening layer, and the project-rooting rule) as by-hand copies pinned
//! only by comments and a skip-able parity test. The single implementation
//! lives here, parameterised by the program name that prefixes messages.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bynk_emit::project::{self, CompileOptions, read_project_paths};
use bynk_fmt::{FormatOptions, format_source};

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

/// Render a project build failure with per-file ariadne context, exactly as
/// single-file mode had rich rendering. Unattributed (project-level) errors
/// keep the plain form.
///
/// This is the **flattening layer** (ADR 0100): it attributes each
/// `AttributedError` to its file snapshot and delegates the actual rendering to
/// [`bynk_render::print_errors`]. The `ProjectFailure → CompileError` flattening
/// stays here, above `bynk-render`, so there is no `render → emit` edge.
pub fn print_project_failure(failure: &project::ProjectFailure) {
    let texts: std::collections::HashMap<&Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();
    for ae in &failure.errors {
        match ae
            .source_path
            .as_deref()
            .and_then(|p| texts.get(p).map(|t| (p, *t)))
        {
            Some((path, text)) => {
                let label = path.to_string_lossy().replace('\\', "/");
                bynk_render::print_errors(std::slice::from_ref(&ae.error), text, &label);
            }
            None => {
                eprintln!("[{}] {}", ae.error.category, ae.error.message);
                for note in &ae.error.notes {
                    eprintln!("  note: {note}");
                }
            }
        }
    }
}

/// v0.89 (ADR 0117): print a successful build's non-failing warnings. A
/// successful build keeps no per-file snapshots, so warnings render in the
/// plain `warning[<category>]: <message>` form (with the owning file, when
/// known) rather than ariadne source context.
pub fn print_project_warnings(warnings: &[project::AttributedError]) {
    for w in warnings {
        let where_ = w
            .source_path
            .as_deref()
            .map(|p| format!("{}: ", p.to_string_lossy().replace('\\', "/")))
            .unwrap_or_default();
        eprintln!("{where_}warning[{}]: {}", w.error.category, w.error.message);
        for note in &w.error.notes {
            eprintln!("  note: {note}");
        }
    }
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
/// whose `diagnostics` the VS Code `bynkc` problem-matcher re-parses.
///
/// The flattening layer (ADR 0100): it delegates the per-error formatting to
/// [`bynk_render::short_line`] / [`bynk_render::severity_word`].
pub fn project_failure_short_lines(failure: &project::ProjectFailure) -> Vec<String> {
    let texts: std::collections::HashMap<&Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();
    failure
        .errors
        .iter()
        .map(|ae| {
            match ae
                .source_path
                .as_deref()
                .and_then(|p| texts.get(p).map(|t| (p, *t)))
            {
                Some((path, text)) => {
                    let label = path.to_string_lossy().replace('\\', "/");
                    bynk_render::short_line(&label, text, &ae.error)
                }
                None => format!(
                    "{}[{}]: {}",
                    bynk_render::severity_word(&ae.error),
                    ae.error.category,
                    ae.error.message
                ),
            }
        })
        .collect()
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
/// one run, never collide. On any failure the temp file is removed so a botched
/// write leaves no litter beside the untouched original.
fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".{file_name}.bynk-fmt.{}.{n}.tmp", std::process::id());
    let tmp = match dir {
        Some(d) => d.join(tmp_name),
        None => PathBuf::from(tmp_name),
    };

    // Scope the write so the handle is flushed and closed before the rename.
    // The rename replaces the destination inode, so carry the original file's
    // permissions onto the temp file first — otherwise a formatted file would
    // silently pick up the process umask's default mode (e.g. an executable or
    // group-restricted source would lose its bits).
    let write_result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = f.set_permissions(meta.permissions());
        }
        f.write_all(contents.as_bytes())?;
        f.sync_all()
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
/// directory routes through [`project::compile_project`], a single file
/// through [`bynk_emit::compile_with_warnings`]. `short` selects the one-line
/// `--format short` rendering. `prog` prefixes messages (`bynk: …`).
pub fn run_check(prog: &str, input: &Path, short: bool) -> ExitCode {
    if input.is_dir() {
        match project::compile_project(&project_options(input)) {
            Ok(out) => {
                print_project_warnings(&out.warnings);
                ExitCode::SUCCESS
            }
            Err(failure) => {
                if short {
                    print_project_failure_short(&failure);
                } else {
                    print_project_failure(&failure);
                }
                ExitCode::FAILURE
            }
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
