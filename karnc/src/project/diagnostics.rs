use super::*;

/// Internal: do the work, given a source root (for commons/contexts) and a
/// test root (for test units). When both roots are the same path the
/// behaviour is identical to the v0.4+ single-tree layout. When they differ
/// — v0.9.1's split-paths mode — sources and tests are discovered separately
/// and the new `inconsistent_test_path` check fires.
/// v0.24 (ADR 0052): how the project pipeline is driven. `Build` preserves
/// the CLI contract exactly (bail at the structural and pre-emit gates);
/// `Analyse` never bails after discovery, skips all emission, and lets
/// independent unit groups resolve/check past another group's errors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Build,
    Analyse,
}

/// v0.24 (ADR 0052): a compile error attributed — where possible — to the
/// project-relative source file it belongs to, tagged at the collection
/// point (the phase that produced it knows which file it was processing).
/// `None` is the project-level bucket: validations spanning files
/// (group/cycle/directory consistency) with no single owning file.
pub struct AttributedError {
    pub source_path: Option<PathBuf>,
    pub error: CompileError,
}

/// Collection-point error sink (ADR 0052). Helpers keep their plain
/// `&mut Vec<CompileError>` signatures; call sites attribute via
/// `extend_for` with the file in scope at that point.
pub(crate) struct ErrorSink {
    entries: Vec<AttributedError>,
}

impl ErrorSink {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub(crate) fn push_for(&mut self, file: Option<&Path>, error: CompileError) {
        self.entries.push(AttributedError {
            source_path: file.map(Path::to_path_buf),
            error,
        });
    }
    pub(crate) fn extend_for(
        &mut self,
        file: Option<&Path>,
        errs: impl IntoIterator<Item = CompileError>,
    ) {
        for e in errs {
            self.push_for(file, e);
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Consume the sink, yielding the collection-ordered attributed errors —
    /// the shape both `ProjectFailure` and `ProjectAnalysis` carry.
    pub(crate) fn into_entries(self) -> Vec<AttributedError> {
        self.entries
    }
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// v0.24: the analyse-mode result — every discovered file's analysed text
/// snapshot (positions must convert against the text that was analysed,
/// not a newer buffer) plus the attributed diagnostics.
pub struct ProjectAnalysis {
    /// `(project-relative source path, analysed text)` for every file read,
    /// including clean files (the LSP needs them to clear diagnostics).
    pub snapshots: Vec<(PathBuf, String)>,
    pub errors: Vec<AttributedError>,
    /// v0.25 (ADR 0053): the project-wide binding index. Empty when the
    /// pipeline bails before resolution (discovery/parse failures).
    pub index: ProjectIndex,
    /// v0.27 (ADR 0056): per-file inferred-type inlay hints — `(binding-name
    /// span, label)`, span-ordered, harvested from the checker's binding
    /// sites. Empty for files the pipeline never type-checked.
    pub hints: FileHints,
    /// v0.30.2 (ADR 0063): per-file expression types — `(expr span, Ty)`,
    /// captured on the Ok path (a file that checks clean), for `.`-member
    /// completion's receiver typing. Empty for files with errors (the
    /// clean-file ceiling) and for synthetic files.
    pub expr_types: FileExprTypes,
}

/// v0.24: a failed build with its attribution and snapshots intact — what
/// the CLI renders rich (ariadne source context per file); the plain
/// `compile_project*` wrappers flatten it to the pre-v0.24 error list.
pub struct ProjectFailure {
    pub errors: Vec<AttributedError>,
    pub snapshots: Vec<(PathBuf, String)>,
}

impl ProjectFailure {
    /// The pre-v0.24 contract: collection-ordered, attribution dropped.
    pub fn flatten(self) -> Vec<CompileError> {
        self.errors.into_iter().map(|a| a.error).collect()
    }
}
