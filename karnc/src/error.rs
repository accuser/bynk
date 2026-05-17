//! Compiler diagnostics.
//!
//! Every error has a category (a dotted namespace string like
//! `karn.parse.expected_token`), a primary span, a primary message, and
//! optionally some secondary labels and notes. Rendering goes through
//! [`ariadne`] for source-pointing colour output.

use ariadne::{Color, Label, Report, ReportKind};

use crate::span::Span;

/// A compile error.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub category: &'static str,
    pub span: Span,
    pub message: String,
    pub labels: Vec<(Span, String)>,
    pub notes: Vec<String>,
}

impl CompileError {
    pub fn new(category: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            category,
            span,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
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

    /// Build an [`ariadne::Report`] for this error, anchored to the given filename.
    pub fn report<'a>(
        &'a self,
        filename: &'a str,
    ) -> Report<'a, (&'a str, std::ops::Range<usize>)> {
        let primary_span = (filename, self.span.range());
        let mut builder = Report::build(ReportKind::Error, primary_span.clone())
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
