use super::*;
use bynk_syntax::ast::TestTier;

/// v0.118: a case's *effective* tier — its own `as <tier>`, else the suite
/// default, else `unit`.
pub(crate) fn case_effective_tier(case: &Case, suite: &SuiteDecl) -> TestTier {
    case.tier.or(suite.tier).unwrap_or(TestTier::Unit)
}

/// v0.118: whether a suite's *effective* tier is `system` — the suite default
/// is `system`, or any case opts up to `system`. Such a suite is emitted via
/// the wired cross-Worker (`Integration`) machinery; otherwise it stays
/// in-process (`Test`).
pub(crate) fn suite_effective_tier_is_system(suite: &SuiteDecl) -> bool {
    suite.tier == Some(TestTier::System)
        || suite.cases.iter().any(|c| c.tier == Some(TestTier::System))
}

/// Read a source file, honouring the overlay (keyed by canonicalised
/// absolute path; falls back to the literal path so a not-yet-created
/// overlay entry still matches).
pub(crate) fn read_source(
    path: &Path,
    overlay: &HashMap<PathBuf, String>,
) -> std::io::Result<String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(text) = overlay.get(&canonical).or_else(|| overlay.get(path)) {
        return Ok(text.clone());
    }
    fs::read_to_string(path)
}

/// A parsed `.bynk` file: its source, AST, and project-relative path.
pub(crate) struct ParsedFile {
    pub(crate) source_path: PathBuf,
    /// v0.72: the absolute path the compiler read this file from, used as the
    /// source-map `sources` entry so an editor's breakpoint (set on the real
    /// `.bynk` file) resolves to the same path the debugger loads. `None` for
    /// toolchain-injected synthetic units, which have no on-disk source.
    pub(crate) abs_path: Option<PathBuf>,
    #[allow(dead_code)]
    pub(crate) source: String,
    pub(crate) unit: SourceUnit,
    pub(crate) kind: UnitKind,
    /// v0.17: true for toolchain-injected units (the `bynk` surface) — exempt
    /// from the reserved-namespace and missing-binding checks.
    pub(crate) synthetic: bool,
}

impl ParsedFile {
    /// v0.72: the source-map `sources` entry for this file — the absolute path
    /// the compiler read it from (forward slashes), so an editor breakpoint set
    /// on the real `.bynk` resolves to the same path the debugger loads. A
    /// project-relative name would resolve against the emitted `.ts`'s directory,
    /// which is the wrong place. Synthetic units (no on-disk source) fall back to
    /// their relative path.
    pub(crate) fn map_source_name(&self) -> String {
        self.abs_path
            .as_deref()
            .unwrap_or(self.source_path.as_path())
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub(crate) fn items(&self) -> &Vec<CommonsItem> {
        match &self.unit {
            SourceUnit::Commons(c) => &c.items,
            SourceUnit::Context(c) => &c.items,
            SourceUnit::Adapter(a) => &a.items,
            SourceUnit::Suite(_) => {
                // Tests don't contribute CommonsItem items; the production
                // pipeline never asks them to. Return a singleton empty vec.
                static EMPTY: std::sync::OnceLock<Vec<CommonsItem>> = std::sync::OnceLock::new();
                EMPTY.get_or_init(Vec::new)
            }
        }
    }

    pub(crate) fn uses(&self) -> &Vec<UsesDecl> {
        match &self.unit {
            SourceUnit::Commons(c) => &c.uses,
            SourceUnit::Context(c) => &c.uses,
            SourceUnit::Adapter(a) => &a.uses,
            SourceUnit::Suite(t) => &t.uses,
        }
    }

    pub(crate) fn consumes(&self) -> &[ConsumesDecl] {
        match &self.unit {
            SourceUnit::Commons(_) => &[],
            SourceUnit::Context(c) => &c.consumes,
            // v0.18: adapter-to-adapter capability dependencies (spec §4.5).
            SourceUnit::Adapter(a) => &a.consumes,
            // An integration test's participant edges are resolved separately
            // (the harness root consumes every participant); it has no
            // `consumes` of its own.
            SourceUnit::Suite(_) => &[],
        }
    }

    /// `exports` clauses, for the unit kinds that have them (contexts and
    /// adapters). Empty for commons/tests.
    pub(crate) fn exports(&self) -> &[ExportsDecl] {
        match &self.unit {
            SourceUnit::Context(c) => &c.exports,
            SourceUnit::Adapter(a) => &a.exports,
            _ => &[],
        }
    }

    pub(crate) fn adapter(&self) -> Option<&AdapterDecl> {
        match &self.unit {
            SourceUnit::Adapter(a) => Some(a),
            _ => None,
        }
    }

    pub(crate) fn test(&self) -> Option<&SuiteDecl> {
        match &self.unit {
            SourceUnit::Suite(t) => Some(t),
            _ => None,
        }
    }

    /// v0.118: a suite whose *effective* tier is `system` is emitted through
    /// the wired cross-Worker machinery (the retired standalone `integration`
    /// path, now re-driven from tiers). Returns the underlying [`SuiteDecl`]
    /// when this file is such a suite.
    pub(crate) fn integration(&self) -> Option<&SuiteDecl> {
        match &self.unit {
            SourceUnit::Suite(t) if suite_effective_tier_is_system(t) => Some(t),
            _ => None,
        }
    }

    /// Build a synthetic Commons AST node carrying the given items, so the
    /// existing resolver/checker pipeline can be driven uniformly.
    pub(crate) fn as_synthetic_commons(&self, items: Vec<CommonsItem>) -> Commons {
        let (name, uses, documentation, form, span) = match &self.unit {
            SourceUnit::Commons(c) => (
                c.name.clone(),
                c.uses.clone(),
                c.documentation.clone(),
                c.form,
                c.span,
            ),
            SourceUnit::Context(c) => (
                c.name.clone(),
                c.uses.clone(),
                c.documentation.clone(),
                c.form,
                c.span,
            ),
            SourceUnit::Suite(t) => (
                t.target.clone(),
                t.uses.clone(),
                t.documentation.clone(),
                t.form,
                t.span,
            ),
            SourceUnit::Adapter(a) => (
                a.name.clone(),
                a.uses.clone(),
                a.documentation.clone(),
                a.form,
                a.span,
            ),
        };
        Commons {
            name,
            items,
            uses,
            documentation,
            form,
            span,
            trivia: Trivia::default(),
            trailing_comments: Vec::new(),
        }
    }
}

/// Parse already-read source text into a [`ParsedFile`]. The read happens
/// at the call site (v0.24): the pipeline owns the text for snapshots and
/// per-file error attribution, and the overlay supplies unsaved buffers.
pub(crate) fn parse_sources(
    root: &Path,
    path: &Path,
    source: String,
) -> Result<Vec<ParsedFile>, Vec<CompileError>> {
    let tokens = lexer::tokenize(&source).map_err(|e| vec![e])?;
    // v0.113: a file may declare more than one top-level unit — an *atomic*
    // file holding `commons`/`context` alongside a `suite` (DECISION S). Each
    // unit becomes its own `ParsedFile` sharing the file's source and path, so
    // the downstream grouping partitions *declarations* by kind: the source
    // units flow to the build, the suites to `bynkc test` only.
    let units = parser::parse_units(&tokens, &source)?;
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    // v0.72: store an *absolute* path — `path` is relative when the compiler
    // was invoked with a relative input (`bynkc test .`), and a relative map
    // `source` would resolve against the emitted `.ts`'s directory, not the
    // real file. `std::path::absolute` resolves against cwd without touching
    // the filesystem (so it works for not-yet-saved overlay buffers too).
    let abs_path = std::path::absolute(path).ok();
    Ok(units
        .into_iter()
        .map(|unit| {
            let kind = match &unit {
                SourceUnit::Commons(_) => UnitKind::Commons,
                SourceUnit::Context(_) => UnitKind::Context,
                // v0.118: a suite whose effective tier is `system` is emitted
                // through the wired cross-Worker machinery (classified as
                // `Integration`); unit/integration-tier suites stay in-process.
                SourceUnit::Suite(t) if suite_effective_tier_is_system(t) => UnitKind::Integration,
                SourceUnit::Suite(_) => UnitKind::Test,
                SourceUnit::Adapter(_) => UnitKind::Adapter,
            };
            ParsedFile {
                abs_path: abs_path.clone(),
                source_path: rel.clone(),
                source: source.clone(),
                unit,
                kind,
                synthetic: false,
            }
        })
        .collect())
}

pub(crate) fn discover_bynk_files(
    root: &Path,
    excludes: &[PathBuf],
) -> Result<Vec<PathBuf>, CompileError> {
    if !root.exists() {
        return Err(CompileError::new(
            "bynk.project.no_root",
            Span::default(),
            format!("project root does not exist: {}", root.display()),
        ));
    }
    // v0.113: skip excluded subtrees (author `exclude` + the tool's own caches)
    // and hidden directories, so an `include` root at the project root does not
    // sweep up generated, vendored, or dot-directory `.bynk`.
    let is_excluded = |dir: &Path| {
        excludes.iter().any(|ex| dir == ex || dir.starts_with(ex))
            || dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n != ".")
    };
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                return Err(CompileError::new(
                    "bynk.project.read_failed",
                    Span::default(),
                    format!("could not read directory `{}`: {e}", dir.display()),
                ));
            }
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if !is_excluded(&p) {
                    stack.push(p);
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("bynk") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

pub(crate) fn check_file_directory_conflicts(
    root: &Path,
    files: &[PathBuf],
) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    let mut bynk_files: HashSet<PathBuf> = HashSet::new();
    let mut dirs_with_bynk: HashSet<PathBuf> = HashSet::new();
    for p in files {
        let rel = p.strip_prefix(root).unwrap_or(p);
        bynk_files.insert(rel.to_path_buf());
        if let Some(parent) = rel.parent() {
            dirs_with_bynk.insert(parent.to_path_buf());
        }
    }
    for f in &bynk_files {
        let stem = f.with_extension("");
        if dirs_with_bynk.contains(&stem) {
            errors.push(
                CompileError::new(
                    "bynk.project.file_and_directory",
                    Span::default(),
                    format!(
                        "commons at `{}` is ambiguous: both `{}` and `{}/` exist with `.bynk` content",
                        f.with_extension("").display(),
                        f.display(),
                        stem.display()
                    ),
                )
                .with_note(
                    "a commons can be a single `.bynk` file OR a directory of `.bynk` files, not both",
                ),
            );
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
