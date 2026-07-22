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
    /// v0.89 (ADR 0117): non-failing warnings, classified on push by
    /// `Severity::for_error`. Kept apart so `is_empty`/`len` — the build-failure
    /// gates — stay errors-only, while every warning source (commons-fn checks,
    /// service/agent handler validation, parser) is captured uniformly.
    warnings: Vec<AttributedError>,
}

impl ErrorSink {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub(crate) fn push_for(&mut self, file: Option<&Path>, error: CompileError) {
        let attributed = AttributedError {
            source_path: file.map(Path::to_path_buf),
            error,
        };
        match bynk_syntax::Severity::for_error(&attributed.error) {
            bynk_syntax::Severity::Warning => self.warnings.push(attributed),
            bynk_syntax::Severity::Error => self.entries.push(attributed),
        }
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
    /// True when no **error-severity** diagnostic has been collected — the
    /// build-failure gate. Warnings do not count (ADR 0117).
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Consume the sink, yielding the non-failing **warnings** (ADR 0117).
    pub(crate) fn into_warnings(self) -> Vec<AttributedError> {
        self.warnings
    }
    /// Consume the sink, yielding errors then warnings — the full diagnostic
    /// list the LSP and a failed build render together.
    pub(crate) fn into_all(self) -> Vec<AttributedError> {
        let mut all = self.entries;
        all.extend(self.warnings);
        all
    }
    /// The count of **error-severity** diagnostics.
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
    ///
    /// Slice 0: this is genuinely project-relative now — it keys on
    /// `ParsedFile::identity_path`, so `src/todos.bynk` and `tests/todos.bynk`
    /// are distinct entries. Before slice 0 the path was relative to each
    /// `include` root, so those two collided on `todos.bynk` and a consumer
    /// mapping by this key silently dropped one; this comment described the
    /// intent rather than the behaviour, which is why it went unnoticed.
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
    /// v0.31 (ADR 0064): per-file local bindings with their scope ranges —
    /// `let`/`let <-`, fn/handler/lambda params — for the scope-at-offset
    /// query backing locals completion + navigation. Synthetic files muted.
    pub locals: FileLocals,
    /// v0.99: per-file capability-requirement ledger — every capability-consuming
    /// site (direct call, store op), covered or not, with its provenance. Drives
    /// the ghost `given` inlay hint and capability hover. Empty for files the
    /// pipeline never type-checked, and for synthetic/test files (muted).
    pub requirements: FileRequirements,
    /// Slice 6b (ADR 0095): qualified unit name → the project source file(s)
    /// that comprise it, in discovery order — the unit→file map backing document
    /// links and consumed-context navigation. Excludes synthetic (toolchain-
    /// injected) units; empty when the pipeline bails before the checker.
    pub unit_sources: HashMap<String, Vec<PathBuf>>,
    /// #846: qualified context/adapter unit name → the cross-context and agent
    /// tables needed to classify a handler call as a lifeline (consumed
    /// capability, consumed-context call, or agent) for the sequence-diagram
    /// query. Reconstructed from the same retained per-project tables
    /// (`unit_uses`/`unit_consumes`/`unit_tables`) the per-file checking pass
    /// builds its own transient `ResolvedCommons.cross_context` from — that
    /// transient value is never itself retained, so this is a deliberately
    /// lean re-derivation (not the full `ResolvedCommons`: no `types`/`fns`/
    /// `methods`, which the classifier never needs). Only contexts/adapters
    /// have an entry; empty when the pipeline bails before the checker.
    pub sequence_info: HashMap<String, ContextSequenceInfo>,
}

/// #846: the per-unit slice of resolution the sequence-diagram classifier
/// needs — see [`ProjectAnalysis::sequence_info`].
#[derive(Debug, Clone, Default)]
pub struct ContextSequenceInfo {
    pub cross_context: resolver::CrossContextInfo,
    pub agents: HashMap<String, AgentDecl>,
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
