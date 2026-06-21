//! Bynk's shared diagnostic-rendering layer.
//!
//! The presentation layer over [`bynk_syntax::CompileError`]: ariadne human
//! output and the `short`/`json`-feeding line forms. Every renderer takes
//! `&[CompileError]` + `source` + `filename` — it is agnostic about *where* the
//! errors came from. Both CLI front-ends adopt it so they render identically
//! (ADR 0100).
//!
//! **Invariant (ADR 0100):** this crate depends on `bynk-syntax` **only** (plus
//! `ariadne`). It must never see `AttributedError`/`ProjectFailure` (which live
//! in `bynk-emit`): the `AttributedError → CompileError` flattening stays *above*
//! render, in the front-end, so there is no `render → emit` cycle. A function
//! here taking a `ProjectFailure` would not even compile — the dependency isn't
//! present, by design.
//!
//! Extracted from `bynkc` as slice 6 of the crate-decomposition track.

use std::path::Path;

use ariadne::Source;
use bynk_syntax::error::Severity;
use bynk_syntax::{CompileError, span};

/// Render a list of compile errors to a string (for tests) using the given
/// filename as the diagnostic source label.
pub fn render_errors(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report(filename)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render a list of compile errors to a string with colour disabled and the
/// given filename as the source label. Unlike [`render_errors`], the output
/// contains no ANSI escape codes, so it is byte-stable — suitable for the
/// committed diagnostic transcripts under `docs/diagnostics/`.
pub fn render_errors_plain(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report_plain(filename)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render to stderr with color, used by the CLI.
pub fn print_errors(errors: &[CompileError], source: &str, filename: &str) {
    let mut cache = (filename, Source::from(source));
    for err in errors {
        let _ = err.report(filename).eprint(&mut cache);
    }
}

/// Render project-level errors as plain `[category] message` lines — the
/// fallback for errors with no file attribution. Rich, source-context rendering
/// lives in the front-end's project-failure renderer (v0.24).
pub fn print_project_errors(root: &Path, errors: &[CompileError]) {
    let _ = root;
    for err in errors {
        eprintln!("[{}] {}", err.category, err.message);
        for note in &err.notes {
            eprintln!("  note: {note}");
        }
    }
}

/// v0.38 (ADR 0071): one terse line per diagnostic for tooling consumers
/// (`bynkc check --format short`):
/// `path:line:col: <severity>[<category>]: <message>`. Line/column are
/// 1-indexed, computed from the byte span against the source. The VS Code
/// `bynkc` problem-matcher keys off this exact shape — keep it stable.
pub fn print_errors_short(errors: &[CompileError], source: &str, filename: &str) {
    eprint!("{}", render_errors_short(errors, source, filename));
}

/// The string form of [`print_errors_short`] — one `…[category]: message` line
/// per error, each newline-terminated. The renderer behind the CLI's `--format
/// short`, exposed for testing.
pub fn render_errors_short(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = String::new();
    for err in errors {
        out.push_str(&short_line(filename, source, err));
        out.push('\n');
    }
    out
}

/// One terse `path:line:col: severity[category]: message` line for a single
/// error against its source. The front-end's project-failure short renderer
/// flattens an attributed error to `(label, text, error)` and calls this.
pub fn short_line(filename: &str, source: &str, err: &CompileError) -> String {
    let (line, col) = span::line_col(source, err.span.start);
    format!(
        "{filename}:{line}:{col}: {}[{}]: {}",
        severity_word(err),
        err.category,
        err.message
    )
}

/// `"error"` / `"warning"` for an error's [`Severity`].
pub fn severity_word(err: &CompileError) -> &'static str {
    match Severity::for_error(err) {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// Render a list of compile errors as plain `[category] message` lines (with
/// notes), for test assertion.
pub fn render_project_errors(errors: &[CompileError]) -> String {
    let mut out = String::new();
    for err in errors {
        out.push_str(&format!("[{}] {}\n", err.category, err.message));
        for note in &err.notes {
            out.push_str(&format!("  note: {note}\n"));
        }
    }
    out
}
