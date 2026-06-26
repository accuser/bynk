//! Compiler diagnostics.
//!
//! Every error has a category (a dotted namespace string like
//! `bynk.parse.expected_token`), a primary span, a primary message, and
//! optionally some secondary labels and notes. Rendering goes through
//! [`ariadne`] for source-pointing colour output.

use ariadne::{Color, Config, Label, Report, ReportKind};

use crate::span::Span;

/// A compile error.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub category: &'static str,
    pub span: Span,
    pub message: String,
    pub labels: Vec<(Span, String)>,
    pub notes: Vec<String>,
    /// v0.26 (ADR 0054): machine-applicable fixes, authored at the diagnosis
    /// site — the only place the exact spans and replacement are known.
    /// Consumed by the LSP (`codeAction`) and, later, a CLI `--fix`.
    pub suggestions: Vec<Suggestion>,
}

/// A structured fix for the error it is attached to (v0.26, ADR 0054).
///
/// `edits` are span → replacement: an empty replacement deletes the span; an
/// empty span inserts at its position. Spans are offsets into the same source
/// text as the error's own span.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Human-facing action title, e.g. "remove `Clock` from the `given` clause".
    pub message: String,
    pub edits: Vec<(Span, String)>,
    pub applicability: Applicability,
}

/// Whether a [`Suggestion`] can be applied without review (mirrors rustc;
/// gates a future CLI `--fix` and the LSP's one-click apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// The fix is exactly right — safe to apply mechanically.
    MachineApplicable,
    /// The fix contains placeholder text a human must complete; never
    /// auto-applied.
    HasPlaceholders,
}

/// Severity classification for a [`CompileError`]. Mirrors LSP severity levels
/// so the LSP server can map diagnostics to the protocol without reinterpreting
/// error categories. Lives in the syntax leaf beside `CompileError` (it
/// classifies one): shared by the IDE diagnose path (`bynk-ide`) and the
/// `short`/`json` renderers, without either depending on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// Classify a [`CompileError`] by its category prefix.
    ///
    /// `bynk.parse.orphan_doc_block`, `bynk.given.unused_capability`,
    /// `bynk.list.deprecated_function`, and the `bynk.index.*` hygiene hints
    /// (`missing`/`unused`, ADR 0118 D4) are warnings; everything else is an
    /// error. Future categories can be added as the diagnostic surface grows.
    pub fn for_error(err: &CompileError) -> Severity {
        match err.category {
            "bynk.parse.orphan_doc_block"
            | "bynk.given.unused_capability"
            | "bynk.list.deprecated_function"
            | "bynk.index.missing"
            | "bynk.index.unused" => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

/// Split diagnostics into `(errors, warnings)` by severity (ADR 0117). The build
/// fails iff the `errors` half is non-empty; the `warnings` half surfaces but
/// does not gate compilation. Relative order within each half is preserved.
pub fn partition_by_severity(
    diagnostics: Vec<CompileError>,
) -> (Vec<CompileError>, Vec<CompileError>) {
    diagnostics
        .into_iter()
        .partition(|d| Severity::for_error(d) == Severity::Error)
}

impl CompileError {
    pub fn new(category: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            category,
            span,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    pub fn with_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.labels.push((span, label.into()));
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Attach a machine-applicable fix (v0.26). Mirrors [`Self::with_note`];
    /// the suggestion is authored where the diagnostic is raised.
    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        edits: Vec<(Span, String)>,
        applicability: Applicability,
    ) -> Self {
        self.suggestions.push(Suggestion {
            message: message.into(),
            edits,
            applicability,
        });
        self
    }

    /// Build an [`ariadne::Report`] for this error, anchored to the given
    /// filename. Colour is on (for the CLI and human-facing test output).
    pub fn report<'a>(
        &'a self,
        filename: &'a str,
    ) -> Report<'a, (&'a str, std::ops::Range<usize>)> {
        self.report_with_config(filename, Config::default())
    }

    /// Build a colourless [`ariadne::Report`], for transcripts committed to the
    /// repo — no ANSI escape codes, so the output is byte-stable across machines.
    pub fn report_plain<'a>(
        &'a self,
        filename: &'a str,
    ) -> Report<'a, (&'a str, std::ops::Range<usize>)> {
        self.report_with_config(filename, Config::default().with_color(false))
    }

    fn report_with_config<'a>(
        &'a self,
        filename: &'a str,
        config: Config,
    ) -> Report<'a, (&'a str, std::ops::Range<usize>)> {
        let primary_span = (filename, self.span.range());
        let mut builder = Report::build(ReportKind::Error, primary_span.clone())
            .with_config(config)
            .with_code(self.category)
            .with_message(&self.message)
            .with_label(
                Label::new(primary_span)
                    .with_message(&self.message)
                    .with_color(Color::Red),
            );

        for (span, label) in &self.labels {
            builder = builder.with_label(
                Label::new((filename, span.range()))
                    .with_message(label)
                    .with_color(Color::Yellow),
            );
        }

        for note in &self.notes {
            builder = builder.with_note(note);
        }

        builder.finish()
    }
}

#[cfg(test)]
mod warning_channel_tests {
    use super::*;
    use crate::span::Span;

    #[test]
    fn partition_splits_by_severity() {
        let warn = CompileError::new("bynk.given.unused_capability", Span::default(), "unused");
        let err = CompileError::new("bynk.types.argument_mismatch", Span::default(), "bad");
        let (errors, warnings) = partition_by_severity(vec![warn, err]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].category, "bynk.types.argument_mismatch");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].category, "bynk.given.unused_capability");
    }
}
