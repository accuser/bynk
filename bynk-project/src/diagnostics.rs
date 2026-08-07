use std::path::PathBuf;

use bynk_syntax::error::CompileError;

/// v0.24 (ADR 0052): a compile error attributed — where possible — to the
/// project-relative source file it belongs to, tagged at the collection
/// point (the phase that produced it knows which file it was processing).
/// `None` is the project-level bucket: validations spanning files
/// (group/cycle/directory consistency) with no single owning file.
///
/// P4.0 (#1113): moved here from `bynk-emit`'s `project::diagnostics` — the
/// one item of that module with no `Mode`/`ErrorSink`-shaped driving logic
/// behind it, just a plain attributed-diagnostic value type. `Mode`,
/// `ErrorSink`, `ProjectAnalysis`, `ProjectFailure`, `ContextSequenceInfo`,
/// and `ContextBoundaryInfo` all stay in `bynk-emit` (each is a fact about
/// how the pipeline is driven or consumed, not about the project model).
pub struct AttributedError {
    pub source_path: Option<PathBuf>,
    pub error: CompileError,
}
