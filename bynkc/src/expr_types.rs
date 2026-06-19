//! v0.30.2 (ADR 0063): the expression-type sink.
//!
//! The checker computes `expr_types: HashMap<Span, Ty>` per file as it types
//! each expression, but that map rides inside the `Ok(TypedCommons)` payload
//! `check_record` drops on error, and the LSP `Analyse` path discards it
//! entirely. This sink carries it out to the analysis so completion can ask
//! *"what is the type of the expression at this offset?"* (the receiver before
//! a `.`), mirroring [`HintSink`](crate::hints::HintSink).
//!
//! Capture is on the **Ok path** — a file's types are recorded only when it
//! checks clean (`check_record` returns `Ok`), so a mid-edit file with errors
//! yields nothing for that file (the slice-3 "clean-file ceiling", ADR 0063).
//! Unlike hints, **test/integration files are not muted** (completion runs in
//! them); only synthetic toolchain-injected files are.

use crate::checker::Ty;
use crate::span::Span;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Project-relative source path → that file's `(expr span, type)` entries,
/// ordered by span (innermost-last within a start, so a containment search can
/// prefer the tightest match).
pub type FileExprTypes = HashMap<PathBuf, Vec<(Span, Ty)>>;

/// Records per-file expression types. A fresh sink records nothing until
/// [`enter_file`](Self::enter_file) attributes it.
#[derive(Debug, Default)]
pub struct ExprTypeSink {
    files: FileExprTypes,
    file: Option<PathBuf>,
    /// Set for synthetic (toolchain-injected) files — their types never serve
    /// a user-visible completion.
    muted: bool,
}

impl ExprTypeSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a per-file recording context.
    pub fn enter_file(&mut self, file: &Path, muted: bool) {
        self.file = Some(file.to_path_buf());
        self.muted = muted;
    }

    /// Record a whole file's `expr_types` map (the Ok-path capture). Dropped
    /// when muted or before any `enter_file`.
    pub fn record_file(&mut self, expr_types: &HashMap<Span, Ty>) {
        if self.muted {
            return;
        }
        let Some(file) = &self.file else {
            return;
        };
        let entry = self.files.entry(file.clone()).or_default();
        entry.extend(expr_types.iter().map(|(span, ty)| (*span, ty.clone())));
    }

    /// Drain the recorded types, each file's entries ordered by span (start
    /// ascending, then **widest first** so a forward scan ends on the tightest
    /// containing span).
    pub fn take_files(&mut self) -> FileExprTypes {
        let mut files = std::mem::take(&mut self.files);
        for entries in files.values_mut() {
            entries.sort_by_key(|(span, _)| (span.start, std::cmp::Reverse(span.end)));
        }
        files
    }
}

/// The type of the **innermost** expression whose span contains `offset`, if
/// any — the receiver-typing query for `.`-member completion.
pub fn type_at_offset(entries: &[(Span, Ty)], offset: usize) -> Option<&Ty> {
    entries
        .iter()
        .filter(|(span, _)| span.start <= offset && offset <= span.end)
        .min_by_key(|(span, _)| span.end - span.start)
        .map(|(_, ty)| ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BaseType;

    fn span(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    #[test]
    fn type_at_offset_prefers_the_innermost_span() {
        let int = Ty::Base(BaseType::Int);
        let string = Ty::Base(BaseType::String);
        // An outer `String` expression 0..10 with an inner `Int` 2..4.
        let entries = vec![(span(0, 10), string.clone()), (span(2, 4), int.clone())];
        assert_eq!(type_at_offset(&entries, 3), Some(&int)); // inside the inner span
        assert_eq!(type_at_offset(&entries, 7), Some(&string)); // outer span only
        assert_eq!(type_at_offset(&entries, 20), None); // outside everything
    }
}
