//! v0.27 (ADR 0056): the inlay-hint sink.
//!
//! A curated set of inferred-type hints harvested from the checker as it
//! computes each binding's type — `let` / `let <-` bindings and lambda
//! parameters whose annotation is absent. The sink mirrors [`RefSink`]
//! (`index.rs`): it is a `&mut` parameter threaded through the checker
//! entry points, NOT part of the `Ok(TypedCommons)` payload `check_record`
//! drops on error — so hints persist through a transient type error at
//! every site the checker still reaches.
//!
//! Labels are pre-rendered (`": " + Ty::display()`) so no `Ty` leaks
//! through the public surface; spans are bare byte offsets into the file
//! the sink was attributed to via [`HintSink::enter_file`].
//!
//! [`RefSink`]: crate::index::RefSink

use crate::span::Span;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Project-relative source path → that file's hints, span-ordered.
/// Each entry is `(binding-name span, label)`, e.g. `(…, ": List[Int]")`.
pub type FileHints = HashMap<PathBuf, Vec<(Span, String)>>;

/// Records inferred-type hints per file. A fresh sink records nothing
/// until [`enter_file`](Self::enter_file) attributes it.
#[derive(Debug, Default)]
pub struct HintSink {
    files: FileHints,
    file: Option<PathBuf>,
    /// Set for synthetic (toolchain-injected) and test/integration files:
    /// hints are discarded — neither surfaces in an editor.
    muted: bool,
}

impl HintSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a per-file recording context.
    pub fn enter_file(&mut self, file: &Path, muted: bool) {
        self.file = Some(file.to_path_buf());
        self.muted = muted;
    }

    /// Record a hint at `span` in the current file. Dropped when muted or
    /// before any `enter_file` (the fresh-sink contract).
    pub fn record(&mut self, span: Span, label: String) {
        if self.muted {
            return;
        }
        let Some(file) = &self.file else {
            return;
        };
        self.files
            .entry(file.clone())
            .or_default()
            .push((span, label));
    }

    /// Drain the recorded hints, each file's entries ordered by span start.
    pub fn take_files(&mut self) -> FileHints {
        let mut files = std::mem::take(&mut self.files);
        for hints in files.values_mut() {
            hints.sort_by_key(|(span, _)| (span.start, span.end));
        }
        files
    }
}
