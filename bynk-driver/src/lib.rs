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
use bynk_fmt::{FormatOptions, IndentStyle, format_source};

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

/// How `--indent` spells the two [`IndentStyle`] variants. The words match the
/// `[fmt] indent` key in `bynk.toml`, which the language server already reads,
/// so a project states the same choice the same way in either place.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentKind {
    /// One tab per nesting level. The default — a reader sets their own tab
    /// width in the editor, which space indentation takes away from them.
    Tab,
    /// `--indent-width` spaces per nesting level.
    Spaces,
}

/// The `fmt` subcommand's arguments, flattened by both `bynkc::cli` and
/// `bynk::cli` so the two spell one contract rather than two copies of it (the
/// [`test_runner::TestArgs`] pattern, findings #40/#72). Field docs here are
/// the CLI help text for both commands' flags.
///
/// The formatting flags override [`FormatOptions`]'s spec defaults for this
/// run. Their defaults are those same spec defaults, so an invocation that
/// passes none formats exactly as before.
#[derive(clap::Args, Debug)]
pub struct FmtArgs {
    /// Files to format. Use `-` for stdin → stdout.
    pub inputs: Vec<PathBuf>,
    /// Check formatting without writing changes. Exits non-zero if any
    /// file is not already canonical.
    #[arg(long)]
    pub check: bool,
    /// Indent with tabs (the default) or spaces.
    #[arg(long, value_enum, default_value_t = IndentKind::Tab)]
    pub indent: IndentKind,
    /// Spaces per nesting level, with `--indent spaces`. Defaults to 2.
    /// Rejected with `--indent tab`, where it would have no effect.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=64))]
    pub indent_width: Option<u8>,
    /// Soft target line width in columns. A construct wider than this wraps
    /// across lines where the grammar allows; one with no break point in it
    /// (a long string literal) is left long. Defaults to 100.
    #[arg(long, value_name = "COLUMNS", default_value_t = 100,
          value_parser = clap::value_parser!(u32).range(1..))]
    pub max_line_width: u32,
    /// Emit a trailing comma in multi-line records, sums, list literals and
    /// `exports` clauses. The default; the flag exists so a script can state
    /// it, and to override an earlier `--no-trailing-comma`.
    #[arg(long, overrides_with = "no_trailing_comma")]
    pub trailing_comma: bool,
    /// Omit the trailing comma in multi-line records, sums, list literals and
    /// `exports` clauses. (Parameter and argument lists never carry one — the
    /// grammar rejects it — regardless of this flag.)
    #[arg(long, overrides_with = "trailing_comma")]
    pub no_trailing_comma: bool,
}

impl FmtArgs {
    /// The [`FormatOptions`] these arguments describe, or the reason they
    /// describe none. Every field falls back to the spec default the
    /// formatter's own `Default` uses.
    pub fn format_options(&self) -> Result<FormatOptions, String> {
        let indent = match (self.indent, self.indent_width) {
            (IndentKind::Tab, None) => IndentStyle::Tab,
            // A width alongside `--indent tab` is silently meaningless, which
            // is exactly the kind of ignored flag that costs an hour to
            // notice. Say so instead.
            (IndentKind::Tab, Some(_)) => {
                return Err(
                    "`--indent-width` applies only to `--indent spaces` (tabs have no width \
                     here — the reader's editor sets it)"
                        .to_string(),
                );
            }
            // 2 matches the `bynk.toml` `[fmt] indent_width` fallback the
            // language server uses, so CLI and editor agree.
            (IndentKind::Spaces, width) => IndentStyle::Spaces(width.unwrap_or(2)),
        };
        Ok(FormatOptions {
            indent,
            max_line_width: self.max_line_width,
            // Neither flag set leaves the spec default (a trailing comma);
            // clap's `overrides_with` pair makes the last one given win.
            trailing_comma: !self.no_trailing_comma,
        })
    }
}

/// The `fmt` command body shared by `bynkc fmt` and `bynk fmt`: each input is
/// formatted and rewritten only when it changes; `--check` reports
/// non-canonical files without writing; `-` reads stdin and writes the
/// formatted result to stdout. `prog` prefixes messages (`bynk fmt: …`).
pub fn run_fmt(prog: &str, args: &FmtArgs) -> ExitCode {
    let opts = match args.format_options() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{prog} fmt: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (inputs, check) = (&args.inputs, args.check);
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// A minimal parser around [`FmtArgs`], so these assert what the real CLIs
    /// parse rather than what a hand-built struct claims.
    #[derive(clap::Parser, Debug)]
    struct Harness {
        #[command(flatten)]
        args: FmtArgs,
    }

    fn parse(argv: &[&str]) -> FmtArgs {
        let mut full = vec!["fmt"];
        full.extend_from_slice(argv);
        Harness::parse_from(full).args
    }

    #[test]
    fn no_flags_is_the_canonical_style() {
        let opts = parse(&["a.bynk"]).format_options().expect("valid");
        let default = FormatOptions::default();
        assert_eq!(opts.indent, default.indent);
        assert_eq!(opts.max_line_width, default.max_line_width);
        assert_eq!(opts.trailing_comma, default.trailing_comma);
    }

    #[test]
    fn spaces_without_a_width_falls_back_to_two() {
        // The same fallback `bynk.toml`'s `[fmt] indent_width` uses, so the CLI
        // and the language server land on the same style from the same words.
        let opts = parse(&["--indent", "spaces", "a.bynk"])
            .format_options()
            .expect("valid");
        assert_eq!(opts.indent, IndentStyle::Spaces(2));
    }

    #[test]
    fn spaces_takes_the_given_width() {
        let opts = parse(&["--indent", "spaces", "--indent-width", "4", "a.bynk"])
            .format_options()
            .expect("valid");
        assert_eq!(opts.indent, IndentStyle::Spaces(4));
    }

    #[test]
    fn a_width_with_tabs_is_an_error_not_a_silent_no_op() {
        let err = parse(&["--indent", "tab", "--indent-width", "4", "a.bynk"])
            .format_options()
            .expect_err("a meaningless width must be reported");
        assert!(err.contains("--indent-width"), "{err}");
        assert!(err.contains("--indent spaces"), "{err}");
    }

    #[test]
    fn the_trailing_comma_pair_is_last_one_wins() {
        // `overrides_with` in both directions: a script may append either flag
        // to a shared argument list and have it win rather than conflict-error.
        assert!(
            parse(&["--no-trailing-comma", "--trailing-comma", "a.bynk"])
                .format_options()
                .expect("valid")
                .trailing_comma
        );
        assert!(
            !parse(&["--trailing-comma", "--no-trailing-comma", "a.bynk"])
                .format_options()
                .expect("valid")
                .trailing_comma
        );
    }

    #[test]
    fn max_line_width_is_taken_verbatim_and_zero_is_refused() {
        assert_eq!(
            parse(&["--max-line-width", "60", "a.bynk"])
                .format_options()
                .expect("valid")
                .max_line_width,
            60
        );
        // A zero-column budget is a nonsense input; clap rejects it at parse
        // time rather than the formatter wrapping every construct maximally.
        assert!(
            Harness::try_parse_from(["fmt", "--max-line-width", "0", "a.bynk"]).is_err(),
            "`--max-line-width 0` must not parse"
        );
    }
}
