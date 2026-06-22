use super::*;

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
            SourceUnit::Test(_) | SourceUnit::Integration(_) => {
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
            SourceUnit::Test(t) => &t.uses,
            SourceUnit::Integration(i) => &i.uses,
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
            SourceUnit::Test(_) | SourceUnit::Integration(_) => &[],
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

    pub(crate) fn test(&self) -> Option<&TestDecl> {
        match &self.unit {
            SourceUnit::Test(t) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn integration(&self) -> Option<&IntegrationDecl> {
        match &self.unit {
            SourceUnit::Integration(i) => Some(i),
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
            SourceUnit::Test(t) => (
                t.target.clone(),
                t.uses.clone(),
                t.documentation.clone(),
                t.form,
                t.span,
            ),
            SourceUnit::Integration(i) => (
                i.name.clone(),
                i.uses.clone(),
                i.documentation.clone(),
                i.form,
                i.span,
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
pub(crate) fn parse_source(
    root: &Path,
    path: &Path,
    source: String,
) -> Result<ParsedFile, Vec<CompileError>> {
    let tokens = lexer::tokenize(&source).map_err(|e| vec![e])?;
    let unit = parser::parse_unit(&tokens, &source)?;
    let kind = match &unit {
        SourceUnit::Commons(_) => UnitKind::Commons,
        SourceUnit::Context(_) => UnitKind::Context,
        SourceUnit::Test(_) => UnitKind::Test,
        SourceUnit::Integration(_) => UnitKind::Integration,
        SourceUnit::Adapter(_) => UnitKind::Adapter,
    };
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    Ok(ParsedFile {
        // v0.72: store an *absolute* path — `path` is relative when the compiler
        // was invoked with a relative input (`bynkc test .`), and a relative map
        // `source` would resolve against the emitted `.ts`'s directory, not the
        // real file. `std::path::absolute` resolves against cwd without touching
        // the filesystem (so it works for not-yet-saved overlay buffers too).
        abs_path: std::path::absolute(path).ok(),
        source_path: rel,
        source,
        unit,
        kind,
        synthetic: false,
    })
}

pub(crate) fn discover_bynk_files(root: &Path) -> Result<Vec<PathBuf>, CompileError> {
    if !root.exists() {
        return Err(CompileError::new(
            "bynk.project.no_root",
            Span::default(),
            format!("project root does not exist: {}", root.display()),
        ));
    }
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
                stack.push(p);
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
