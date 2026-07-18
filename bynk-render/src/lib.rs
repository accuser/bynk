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
        err.report_for(filename, source)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render a list of compile errors to a string with colour disabled and the
/// given filename as the source label. Unlike [`render_errors`], the output
/// contains no ANSI escape codes, so it is byte-stable — suitable for the
/// committed diagnostic transcripts under `site/src/diagnostics/`.
pub fn render_errors_plain(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report_plain_for(filename, source)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render to stderr with color, used by the CLI.
pub fn print_errors(errors: &[CompileError], source: &str, filename: &str) {
    let mut cache = (filename, Source::from(source));
    for err in errors {
        let _ = err.report_for(filename, source).eprint(&mut cache);
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

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_syntax::span::Span;

    /// Spans are byte offsets; ariadne 0.6 defaults to character indexing.
    /// On a line with non-ASCII text before the span, the char-indexed
    /// underline lands past the target. Pin the byte-indexed placement by
    /// checking the caret column against the target's display column.
    #[test]
    fn underline_is_byte_indexed_on_non_ascii_lines() {
        // `é` is 2 bytes / 1 display column; `bad` starts at byte 11,
        // display column 10.
        let source = "-- caféxyz bad\n";
        let start = source.find("bad").unwrap();
        let err = CompileError::new(
            "bynk.test.example",
            Span::new(start, start + 3),
            "bad thing",
        );
        let rendered = render_errors_plain(&[err], source, "probe.bynk");
        let source_line = rendered
            .lines()
            .find(|l| l.contains("caféxyz"))
            .expect("snippet line present");
        let marker_line = rendered
            .lines()
            .find(|l| l.contains('┬'))
            .expect("marker line present");
        let col_of = |line: &str, target: char| line.chars().take_while(|&c| c != target).count();
        // The `┬` sits within the underline under `bad` — same display
        // column as `b`, or one to its right for spans wider than 1.
        let b_col = col_of(source_line, 'b');
        let caret_col = col_of(marker_line, '┬');
        assert!(
            (b_col..b_col + 3).contains(&caret_col),
            "caret at display column {caret_col}, expected within `bad` at {b_col}..{}:\n{rendered}",
            b_col + 3
        );
    }

    /// A label whose span lies past the end of the rendered source belongs to
    /// another file; it must be demoted to a note, not underline unrelated
    /// text (or panic).
    #[test]
    fn out_of_bounds_label_demotes_to_note() {
        let source = "commons demo\n";
        let err = CompileError::new("bynk.test.example", Span::new(0, 7), "problem here")
            .with_label(
                Span::new(5_000, 5_010),
                "parameter declared here (in another file)",
            );
        let rendered = render_errors_plain(&[err], source, "probe.bynk");
        assert!(
            rendered.contains("parameter declared here"),
            "label text survives as a note:\n{rendered}"
        );
    }

    /// A cross-file label whose byte span is *in-bounds* but lands mid-codepoint
    /// (the file it really belongs to has non-ASCII text) must be demoted, not
    /// fed to ariadne — a byte offset splitting a codepoint panics its byte→char
    /// mapping (#716). The rendered source here is all multi-byte, so an odd
    /// offset is never a char boundary.
    #[test]
    fn mid_codepoint_label_demotes_to_note() {
        let source = "café ☕\n"; // `é` and `☕` are multi-byte
        let err = CompileError::new("bynk.test.example", Span::new(0, 3), "problem here")
            .with_label(
                Span::new(4, 5),
                "declared here (mid-codepoint, another file)",
            );
        // Must not panic, and the label survives as a note rather than a caret.
        let rendered = render_errors_plain(&[err], source, "probe.bynk");
        assert!(
            rendered.contains("declared here (mid-codepoint, another file)"),
            "a mid-codepoint label must survive as a note:\n{rendered}"
        );
    }
}
