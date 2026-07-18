//! Multi-file project compilation (v0.3 §3.2 and §3.3, v0.4 §3.5).
//!
//! A "project" is a directory tree of `.bynk` source files. The dotted name
//! of a commons or context (e.g., `bynk.time`, `commerce.orders`) maps to a
//! path under the project root — either a single file (`bynk/time.bynk`) or
//! a directory of files all sharing the same header (`bynk/time/*.bynk`).
//!
//! v0.4: each file is one of two kinds — commons or context. Both kinds share
//! the same multi-file directory machinery; they differ in body content
//! (contexts have `consumes`/`exports`, types are nominally per-context), in
//! visibility (contexts export only the types listed), and in TypeScript
//! emission (contexts re-brand types from used commons).
//!
//! Compilation proceeds in two passes:
//!   1. **Discover and parse** every `.bynk` file. Group by qualified name
//!      and kind. Build a global symbol table where each unit contributes
//!      its declarations.
//!   2. **Resolve, type-check, and emit** each unit with full visibility of
//!      the units it transitively `uses` or `consumes`. Two passes keep
//!      `uses` cycles trivial — there is no order-of-evaluation, only
//!      declarative mixin.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::emitter;
use bynk_check::checker;
use bynk_check::checker::{CapabilityInfo, CapabilityOpInfo, Ty};
use bynk_check::expr_types::{ExprTypeSink, FileExprTypes};
use bynk_check::firstparty::{self, Platform};
use bynk_check::hints::{FileHints, HintSink};
use bynk_check::index::{IndexBuilder, ProjectIndex, RefSink, SiteRef, SymbolKind};
use bynk_check::locals::{FileLocals, LocalsSink};
use bynk_check::requirements::{FileRequirements, RequirementSink};
use bynk_check::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use bynk_syntax::ast::*;
use bynk_syntax::error::CompileError;
use bynk_syntax::lexer;
use bynk_syntax::parser;
use bynk_syntax::span::Span;

mod consistency;
mod diagnostics;
mod discovery;
mod graph;
mod paths;
pub(crate) mod symbols;
mod tests_emit;
mod validate;

use consistency::*;
use diagnostics::*;
use discovery::*;
use graph::*;
use paths::*;
use symbols::*;
use tests_emit::*;
use validate::*;

// External facade: items referenced as `crate::project::X` from outside this
// module (emitter, main, lib) must stay reachable at that path.
pub use diagnostics::{AttributedError, ProjectAnalysis, ProjectFailure};
pub use paths::{
    ProjectPaths, read_project_paths, worker_dir_name, worker_handlers_output_path,
    worker_handlers_source_path,
};
pub use symbols::{FileDeclIndex, UnitTable};
pub use validate::check_function_type_boundary_items;
pub(crate) use validate::type_refs_match;
pub(crate) use validate::{collect_type_decls, type_ref_is_held};

/// One generated TypeScript file.
pub struct CompiledFile {
    /// The originating Bynk source file, relative to the project root.
    pub source_path: PathBuf,
    /// Where the TS output should be written, relative to the output root.
    /// Mirrors the source tree, with `.bynk` rewritten to `.ts`.
    pub output_path: PathBuf,
    /// The emitted TypeScript content.
    pub typescript: String,
    /// Slice 1 (ADR 0103): the serialised source-map v3 document for this file,
    /// when one was produced (the emitted `.bynk`-sourced units). `None` for
    /// generated glue and config (runtime, compose, worker entry, `wrangler.toml`,
    /// `package.json`, adapter bindings) — those have no `.bynk` to map back to.
    /// `write_output` writes it as a sibling `.ts.map` and appends the
    /// `//# sourceMappingURL` trailer; the in-memory `typescript` stays
    /// trailer-free, so golden comparisons are unaffected.
    pub source_map: Option<String>,
    /// Slice 3 (semantic-debugging track, ADR 0105): the debug-metadata sidecar —
    /// a JSON `{ fn → Bynk-operation-label }` map so the debugger names stack
    /// frames `GET "/"` rather than `http_GET`. `Some` only for `.bynk` units that
    /// declare handlers; `write_output` writes it as a sibling `.bynkdbg.json`.
    pub debug_metadata: Option<String>,
}

/// Result of compiling a project.
pub struct ProjectOutput {
    pub files: Vec<CompiledFile>,
    /// v0.89 (ADR 0117): non-failing warnings emitted on a successful build —
    /// surfaced (the CLI prints them, the LSP shows them) but not gating.
    pub warnings: Vec<AttributedError>,
    /// v0.67: the test manifest — every discovered suite and case, retained at
    /// emit time so `bynkc test --no-run --format json` can render a discovery
    /// document without running the suite. Built from the same names + spans the
    /// runner would emit at `suite-begin`/`case`, so a discovery document
    /// reconciles cleanly against a later run's document (same suite name/kind,
    /// same case names). Ordered to match the runner (`emit_test_main`).
    pub discovered: Vec<DiscoveredSuite>,
}

/// v0.67: a discovered test suite — one `test <target>` group (unit) or
/// `test integration "<suite>"` (integration). `name` + `kind` mirror exactly
/// what the NDJSON runner emits at `suite-begin` (kind `"unit"` carries the
/// joined target name; `"integration"` carries the bare suite name), so the
/// editor reconciles discovery and run documents to the same tree items.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredSuite {
    pub name: String,
    pub kind: &'static str,
    pub cases: Vec<DiscoveredCase>,
}

/// One discovered `test "<name>"` case. `location` points at the case-name
/// literal (a run *failure* instead points at the failing `assert`), giving the
/// editor click-through to the declaration before any run.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredCase {
    pub name: String,
    pub location: Option<TestLocation>,
}

/// A project-root-relative `path:line:col` source location, structured. Line and
/// col are 1-indexed (the [`bynk_syntax::span::line_col`] convention).
#[derive(Debug, Clone, PartialEq)]
pub struct TestLocation {
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// v0.17: a resolved adapter binding — the user-authored `.binding.ts` module
/// that supplies an adapter's external provider symbols. Copied verbatim into
/// the output beside the adapter's emitted interface module so that `tsc`
/// checks the `implements` contract and compose can import the symbols.
struct AdapterBinding {
    /// Output path, relative to the output root (e.g. `tokens.binding.ts`).
    output_path: PathBuf,
    /// Verbatim TypeScript content read from the source tree.
    content: String,
}

/// The build target. Determines how cross-context calls and per-context
/// modules are emitted (v0.8). Bundle mode is the default — all contexts
/// emit into one TypeScript bundle and cross-context calls are direct
/// function invocations. Workers mode produces per-context Cloudflare
/// Worker bundles that communicate via Service Bindings.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BuildTarget {
    /// Existing behaviour: one TS bundle, direct function calls between
    /// contexts.
    #[default]
    Bundle,
    /// One Worker per context. Cross-context calls become Service Binding
    /// invocations using a JSON wire format with refinement validation on
    /// the receiving side.
    Workers,
}

/// Distinguishes a commons from a context (and from a test) in the project
/// graph. Tests are a third kind in v0.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Commons,
    Context,
    Test,
    /// v0.16: a `test integration` multi-Worker integration test.
    Integration,
    /// v0.17: an `adapter` — the host boundary (capability contract + binding).
    Adapter,
}

impl UnitKind {
    pub fn display(self) -> &'static str {
        match self {
            UnitKind::Commons => "commons",
            UnitKind::Context => "context",
            UnitKind::Test => "test",
            UnitKind::Integration => "integration test",
            UnitKind::Adapter => "adapter",
        }
    }
}

/// Where a project's `.bynk` files live.
pub enum Roots {
    /// A single tree walked as one root (in-memory builds and legacy
    /// single-file/single-tree inputs).
    Single(PathBuf),
    /// v0.113 (DECISION S): a project rooted at `project_root`, with a flat
    /// `include`/`exclude` layout (`ProjectPaths`). Test-ness is structural (a
    /// `suite` declaration), so there is no source/test role split — the tree is
    /// walked for `.bynk` files and each declaration is partitioned by kind.
    Split {
        project_root: PathBuf,
        paths: ProjectPaths,
    },
}

impl Roots {
    /// Resolve to `(primary_root, secondary_root)` — the up-to-two `include`
    /// trees walked for `.bynk` files. Most projects have a single root; a
    /// conventional `src/`+`tests/` layout yields two. A file's identity path is
    /// relative to the root that contains it.
    fn resolve(&self) -> (PathBuf, PathBuf) {
        match self {
            Roots::Single(root) => (root.clone(), root.clone()),
            Roots::Split {
                project_root,
                paths,
            } => {
                let primary = project_root.join(paths.include.first().cloned().unwrap_or_default());
                let secondary = paths
                    .include
                    .get(1)
                    .map(|p| project_root.join(p))
                    .unwrap_or_else(|| primary.clone());
                (primary, secondary)
            }
        }
    }

    /// Slice 0: the project-root-relative prefix of the **primary** `include`
    /// root — the counterpart to [`Self::tests_prefix`]. Joined onto that
    /// tree's (root-relative) `source_path` to build each file's
    /// `identity_path`, so a file is named uniquely across `include` roots.
    ///
    /// **Which projects this moves.** Empty only for [`Roots::Single`] — the
    /// legacy single-tree mode, reached when there is no `bynk.toml` *and* no
    /// `src/` (`bynk-driver`'s `project_options`) — and for in-memory builds.
    /// Every [`Roots::Split`] project re-bases: a conventional `src/`-only
    /// project has **one** `include` root and still moves `math.bynk` to
    /// `src/math.bynk`, because one root is not the same thing as
    /// `Roots::Single`. No fixture observes this (`expected_error.txt` asserts
    /// categories, never paths), which is why the suite does not move — not
    /// because the paths do not.
    fn src_prefix(&self) -> PathBuf {
        self.prefix_at(0)
    }

    /// The project-root-relative prefix of the **secondary** `include` root,
    /// prepended to that tree's files' (root-relative) `source_path` so an
    /// `expect`'s emitted `path:line:col` resolves from the project root (for
    /// `--format json` click-through). Empty when there is a single root.
    ///
    /// Slice 0 also joins it onto that tree's `identity_path` (see
    /// [`Self::src_prefix`]) — the same prefix, now serving both the emitted
    /// test path and the file's identity.
    fn tests_prefix(&self) -> PathBuf {
        self.prefix_at(1)
    }

    /// Slice 0: the `include` entry at `i` as a *join-safe* prefix.
    ///
    /// `.` normalises to empty. [`ProjectPaths::conventional`] pushes `"."` for
    /// the documented flat layout (`.bynk` at the root, no `src/`), and
    /// `bynk-driver` selects [`Roots::Split`] whenever a `bynk.toml` exists — so
    /// a flat project *with* a manifest reaches here with `include = ["."]`.
    /// `Path::new(".").join("x.bynk")` is `./x.bynk`, which is **not** equal to
    /// `x.bynk` (`Components` keeps the leading `CurDir`), so it would leak a
    /// `./` into every snapshot key and every reported path. An empty prefix is
    /// a join identity; `.` only looks like one.
    fn prefix_at(&self, i: usize) -> PathBuf {
        match self {
            Roots::Single(_) => PathBuf::new(),
            Roots::Split { paths, .. } => match paths.include.get(i) {
                Some(p) if p != Path::new(".") => p.clone(),
                _ => PathBuf::new(),
            },
        }
    }

    /// Absolute subtrees to skip during discovery: the author's `exclude` list
    /// plus the tool's own build-output and dependency caches (`out`,
    /// `node_modules`), so a project whose `include` is the root does not sweep
    /// up generated or vendored files.
    fn excludes(&self) -> Vec<PathBuf> {
        match self {
            Roots::Single(_) => Vec::new(),
            Roots::Split {
                project_root,
                paths,
            } => {
                let mut ex: Vec<PathBuf> =
                    paths.exclude.iter().map(|p| project_root.join(p)).collect();
                for cache in ["out", "node_modules"] {
                    ex.push(project_root.join(cache));
                }
                ex
            }
        }
    }
}

/// The extension emitted import specifiers use (`import … from "./x.<ext>"`).
///
/// `Js` is the default and the only shape for normal builds: NodeNext resolution
/// and `tsc` require `.js` specifiers even though the sources are `.ts`. `Ts` is
/// the **debug build** (slice 2, ADR 0104): `bynkc test --inspect` runs the
/// emitted `.ts` directly under Node's line-preserving strip-only type-stripping,
/// where slice 1's source maps apply unchanged — but Node will not resolve a `.js`
/// specifier to the `.ts` on disk, so the debug build emits `.ts` specifiers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ImportExt {
    #[default]
    Js,
    Ts,
}

impl ImportExt {
    /// The bare extension string (`"js"` / `"ts"`), for `Path::with_extension`
    /// and specifier formatting.
    pub fn as_str(self) -> &'static str {
        match self {
            ImportExt::Js => "js",
            ImportExt::Ts => "ts",
        }
    }
}

/// Options for [`compile_project`]. Construct with [`CompileOptions::single`] or
/// [`CompileOptions::split`], then chain `.target(…)` / `.platform(…)` /
/// `.import_ext(…)` to override the bundle/default-platform/`.js` defaults.
pub struct CompileOptions {
    pub target: BuildTarget,
    pub platform: Platform,
    pub roots: Roots,
    /// The import-specifier extension (slice 2). `Js` (default) for normal builds;
    /// `Ts` for the `bynkc test --inspect` debug build.
    pub import_ext: ImportExt,
    /// v0.115 (testing track slice 3, DECISION J): the build profile for function
    /// contracts. `true` (dev/test) emits the call-site guard around a contracted
    /// `fn`; `false` (release/deploy) strips it entirely for zero runtime cost.
    /// `bynkc test` and `--inspect` set it on; `bynkc compile` leaves it off.
    pub contracts: bool,
}

impl CompileOptions {
    /// Single-root project (`src == tests`), bundle target, default platform.
    pub fn single(root: impl Into<PathBuf>) -> Self {
        Self {
            target: BuildTarget::Bundle,
            platform: Platform::default(),
            roots: Roots::Single(root.into()),
            import_ext: ImportExt::default(),
            contracts: false,
        }
    }

    /// v0.9.1 split layout (source and test units in separate subdirectories
    /// under `project_root`), bundle target, default platform. Use this from
    /// `bynkc test` so its rooting matches `bynkc compile`'s.
    pub fn split(project_root: impl Into<PathBuf>, paths: ProjectPaths) -> Self {
        Self {
            target: BuildTarget::Bundle,
            platform: Platform::default(),
            roots: Roots::Split {
                project_root: project_root.into(),
                paths,
            },
            import_ext: ImportExt::default(),
            contracts: false,
        }
    }

    /// Select the build target. `Bundle` (default) is the v0.6+ single-bundle
    /// layout; `Workers` (v0.8) emits per-context Cloudflare Workers.
    pub fn target(mut self, target: BuildTarget) -> Self {
        self.target = target;
        self
    }

    /// Slice 2: select the import-specifier extension. `Ts` is the debug build
    /// for `bynkc test --inspect` (run the `.ts` directly under Node strip-only).
    pub fn import_ext(mut self, ext: ImportExt) -> Self {
        self.import_ext = ext;
        self
    }

    /// v0.115: enable the function-contract call-site guard (dev/test profile).
    /// `bynkc test` and `--inspect` call this; the deploy build leaves it off so
    /// contract checks never reach production (DECISION J).
    pub fn contracts(mut self, on: bool) -> Self {
        self.contracts = on;
        self
    }

    /// v0.17: select the deploy [`Platform`] (selects the `bynk` surface
    /// binding). The MVP ships `cloudflare` only.
    pub fn platform(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }
}

/// Compile a Bynk project, keeping error attribution + snapshots on failure
/// (so the CLI can render project errors with source context, ADR 0052). Use
/// `.map_err(ProjectFailure::flatten)` for the flattened `Vec<CompileError>`
/// shape.
pub fn compile_project(options: &CompileOptions) -> Result<ProjectOutput, ProjectFailure> {
    let (src_root, tests_root) = options.roots.resolve();
    let src_prefix = options.roots.src_prefix();
    let tests_prefix = options.roots.tests_prefix();
    let excludes = options.roots.excludes();
    let run = run_checks(
        &src_root,
        &tests_root,
        &src_prefix,
        &tests_prefix,
        options.target,
        options.platform,
        options.import_ext,
        Mode::Build,
        &HashMap::new(),
        &excludes,
        None,
        options.contracts,
    );
    finish_build(run, options.import_ext)
}

/// Compile a single **in-memory** Bynk source through the full project pipeline —
/// no filesystem access (in-browser track, slice 3). The source is the in-process
/// `Bundle` subset that `consumes bynk`; first-party injection and the per-platform
/// binding emission run exactly as for an on-disk build, so the returned
/// [`ProjectOutput`] is the complete module graph (the user unit + `runtime.ts` +
/// the `bynk-<platform>.ts` binding + `compose.ts`). The wasm entry point pairs
/// this with `bynk-strip` to produce JavaScript for the playground.
///
/// The module's logical path is **derived from its declared unit name** (a context
/// `app.demo` ⇒ `app/demo.bynk`), so the name↔path alignment check passes without
/// real files; a source that does not parse falls back to `main.bynk` and the parse
/// error is reported normally.
pub fn compile_in_memory(
    source: &str,
    target: BuildTarget,
    platform: Platform,
) -> Result<ProjectOutput, ProjectFailure> {
    // A single-tree (`src_root == tests_root`) virtual project rooted at `.`: the
    // one source file is supplied directly and its text layered in via the
    // overlay, so discovery and every other disk read are bypassed.
    let root = PathBuf::from(".");
    let path = in_memory_logical_path(source);
    let mut overlay = HashMap::new();
    overlay.insert(path.clone(), source.to_string());
    let run = run_checks(
        &root,
        &root,
        // Slice 0: an in-memory build is single-root — an empty prefix keeps
        // `identity_path == source_path`. (`&root` here is `.`, which would
        // otherwise leak in as `./<file>`.)
        Path::new(""),
        &root,
        target,
        platform,
        ImportExt::Js,
        Mode::Build,
        &overlay,
        &[],
        Some((vec![path], Vec::new())),
        false,
    );
    finish_build(run, ImportExt::Js)
}

/// Analyse a single **in-memory** Bynk source and return all diagnostics —
/// non-bailing, no emission (in-browser track, slice 5d). The editor calls this
/// on every (debounced) keystroke for live diagnostics: unlike [`compile_in_memory`]
/// (build mode, which bails at the first failing phase), this runs in `Analyse`
/// mode, so parse / resolve / check diagnostics are recovered and reported together
/// — and it works for a `context` (the playground's typical program), not only a
/// commons. Same fs-free seam as `compile_in_memory`.
pub fn analyse_in_memory(
    source: &str,
    target: BuildTarget,
    platform: Platform,
) -> Vec<AttributedError> {
    let root = PathBuf::from(".");
    let path = in_memory_logical_path(source);
    let mut overlay = HashMap::new();
    overlay.insert(path.clone(), source.to_string());
    let run = run_checks(
        &root,
        &root,
        // Slice 0: an in-memory build is single-root — an empty prefix keeps
        // `identity_path == source_path`. (`&root` here is `.`, which would
        // otherwise leak in as `./<file>`.)
        Path::new(""),
        &root,
        target,
        platform,
        ImportExt::Js,
        Mode::Analyse,
        &overlay,
        &[],
        Some((vec![path], Vec::new())),
        false,
    );
    match run {
        RunChecks::Bailed { errors, .. } | RunChecks::Checked { errors, .. } => errors.into_all(),
    }
}

/// Derive the conventional single-file path for an in-memory source from its
/// declared unit name (`app.demo` ⇒ `app/demo.bynk`), so `check_path_name_alignment`
/// is satisfied without a real file tree. Falls back to `main.bynk` when the source
/// does not parse — `run_checks` then re-parses and reports the error against it.
fn in_memory_logical_path(source: &str) -> PathBuf {
    let parts: Option<Vec<String>> = lexer::tokenize(source)
        .ok()
        .and_then(|tokens| parser::parse_unit(&tokens, source).ok())
        .map(|unit| {
            let name = match &unit {
                SourceUnit::Commons(c) => &c.name,
                SourceUnit::Context(c) => &c.name,
                SourceUnit::Adapter(a) => &a.name,
                SourceUnit::Suite(t) => &t.target,
            };
            name.parts.iter().map(|i| i.name.clone()).collect()
        });
    match parts {
        Some(p) if !p.is_empty() => {
            let mut path = PathBuf::from(p.join("/"));
            path.set_extension("bynk");
            path
        }
        _ => PathBuf::from("main.bynk"),
    }
}

/// Assemble a finished [`ProjectOutput`] (or a [`ProjectFailure`]) from a
/// [`RunChecks`] result — the shared tail of `compile_project` and
/// `compile_in_memory`.
fn finish_build(run: RunChecks, import_ext: ImportExt) -> Result<ProjectOutput, ProjectFailure> {
    match run {
        RunChecks::Bailed {
            errors, snapshots, ..
        } => Err(ProjectFailure {
            // ADR 0117: a failed build still renders any warnings it produced
            // (the sink yields errors then warnings).
            errors: errors.into_all(),
            snapshots,
        }),
        RunChecks::Checked {
            errors, snapshots, ..
        } if !errors.is_empty() => Err(ProjectFailure {
            errors: errors.into_all(),
            snapshots,
        }),
        RunChecks::Checked {
            errors,
            compiled,
            runnable_tests,
            integration_outputs,
            integration_runnables,
            groups,
            kinds,
            unit_consumes,
            unit_consumes_aliases,
            unit_tables,
            unit_uses,
            unit_flattened,
            adapter_bindings,
            npm_deps,
            target,
            ..
        } => {
            let mut out = build_output(
                compiled,
                runnable_tests,
                integration_outputs,
                integration_runnables,
                groups,
                kinds,
                unit_consumes,
                unit_consumes_aliases,
                unit_tables,
                unit_uses,
                unit_flattened,
                adapter_bindings,
                npm_deps,
                target,
                import_ext,
            );
            // ADR 0117: surface non-failing warnings on the successful build
            // (errors is empty here — the guard arm above caught any).
            out.warnings = errors.into_warnings();
            Ok(out)
        }
    }
}

/// Slice A: the `.bynk` files these roots contain — the **same walk**
/// `compile_project` performs, honouring `exclude` and the tool's own `out`/
/// `node_modules` caches.
///
/// Exposed so the IDE surface consumes the compiler's discovery instead of
/// re-deriving a lesser one. The LSP's completion previously hand-rolled a walk
/// of a single directory with no excludes, which is the same class of defect as
/// the analysis root itself being wrong.
pub fn discover_project_files(roots: &Roots) -> Vec<PathBuf> {
    let (src_root, tests_root) = roots.resolve();
    let excludes = roots.excludes();
    let mut out = discover_bynk_files(&src_root, &excludes).unwrap_or_default();
    // The secondary tree is optional, and equals the primary when `include` has
    // one entry — in which case `run_checks` walks once and so must this.
    if src_root != tests_root && tests_root.exists() {
        out.extend(discover_bynk_files(&tests_root, &excludes).unwrap_or_default());
    }
    out.sort();
    out.dedup();
    out
}

/// v0.24: analyse a project without building — non-bailing, overlay-aware,
/// file-attributed (ADR 0052). `overlay` maps canonicalised absolute paths
/// to buffer text layered over disk reads (unsaved editor buffers).
///
/// Slice A: the single-tree convenience over [`analyse_project_with`]
/// (`Roots::Single`), preserving the pre-slice-A behaviour for callers that
/// hand in one fixture root and want one tree walked.
pub fn analyse_project(root: &Path, overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis {
    analyse_project_with(&Roots::Single(root.to_path_buf()), overlay)
}

/// Slice A: analyse a project whose roots are resolved from its manifest — the
/// same [`Roots`] `compile_project` consumes, resolved the same way, so the LSP
/// discovers exactly the files `bynkc` compiles.
///
/// Identity is project-relative (ADR 0198): a file's `source_path` here is
/// unique across `include` roots.
pub fn analyse_project_with(roots: &Roots, overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis {
    // Resolved exactly as `compile_project` does — one project model, not two.
    let (src_root, tests_root) = roots.resolve();
    let src_prefix = roots.src_prefix();
    let tests_prefix = roots.tests_prefix();
    let excludes = roots.excludes();
    match run_checks(
        &src_root,
        &tests_root,
        &src_prefix,
        &tests_prefix,
        BuildTarget::Bundle,
        Platform::default(),
        ImportExt::Js,
        Mode::Analyse,
        overlay,
        &excludes,
        None,
        false,
    ) {
        RunChecks::Bailed {
            errors,
            snapshots,
            mut hints,
            mut locals,
            mut exprs,
            mut requirements,
        } => ProjectAnalysis {
            snapshots,
            // ADR 0117: the LSP renders warnings alongside errors (severity is
            // applied downstream), so analyse surfaces the full diagnostic list.
            errors: errors.into_all(),
            index: ProjectIndex::default(),
            hints: hints.take_files(),
            locals: locals.take_files(),
            expr_types: exprs.take_files(),
            requirements: requirements.take_files(),
            // No parsed tree on the bail path — the map stays empty (ADR 0095).
            unit_sources: HashMap::new(),
        },
        RunChecks::Checked {
            errors,
            snapshots,
            mut refs,
            mut hints,
            mut locals,
            mut exprs,
            mut requirements,
            parsed,
            unit_uses,
            unit_consumes,
            ..
        } => {
            let index = assemble_index(
                &parsed,
                &unit_uses,
                &unit_consumes,
                std::mem::take(&mut refs),
            );
            // ADR 0095: qualified unit name → its project source file(s), in
            // discovery order. Synthetic (toolchain-injected `bynk` surface)
            // units have no openable file and are excluded.
            let mut unit_sources: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for pf in &parsed {
                if pf.synthetic {
                    continue;
                }
                unit_sources
                    .entry(pf.unit.name().joined())
                    .or_default()
                    .push(pf.identity_path.clone());
            }
            ProjectAnalysis {
                snapshots,
                errors: errors.into_all(),
                index,
                hints: hints.take_files(),
                locals: locals.take_files(),
                expr_types: exprs.take_files(),
                requirements: requirements.take_files(),
                unit_sources,
            }
        }
    }
}

/// v0.155: resolve service-level `by`/`given` defaults by injecting each into the
/// handlers that omit their own clause. A `by`/`given` on the service header is the
/// ambient contract every handler inherits unless it declares its own — the
/// "public / bearer-authed" fact is usually a service fact, not a per-handler one,
/// and HTTP has no safe default actor, so this removes the per-route repetition.
///
/// Runs once, in place, on the parsed AST before grouping/checking. Downstream
/// phases then see fully-formed handler `by_clause`/`given`, so the checker,
/// project validation, and the emitter need no default-awareness. The injected
/// clause carries the service-header span, so a diagnostic about a malformed
/// default points at the header (a bad default repeats per inheriting handler —
/// an error-path wrinkle noted in the ADR; the happy path is clean).
fn normalize_service_defaults(parsed: &mut [ParsedFile]) {
    for pf in parsed.iter_mut() {
        let items = match &mut pf.unit {
            SourceUnit::Commons(c) => &mut c.items,
            SourceUnit::Context(c) => &mut c.items,
            SourceUnit::Adapter(a) => &mut a.items,
            SourceUnit::Suite(_) => continue,
        };
        for item in items.iter_mut() {
            if let CommonsItem::Service(svc) = item {
                inject_service_defaults(svc);
            }
        }
    }
}

/// Inject a single service's `by`/`given` defaults into its handlers. A handler
/// that names its own `by` (or `given`) overrides the default outright — the
/// default fills only an *absent* clause, never merges. A service with no default
/// is left untouched (byte-for-byte the pre-v0.155 behaviour).
fn inject_service_defaults(svc: &mut ServiceDecl) {
    let default_by = svc.default_by.clone();
    let default_given = svc.default_given.clone();
    if default_by.is_none() && default_given.is_empty() {
        return;
    }
    for handler in svc.handlers.iter_mut() {
        if handler.by_clause.is_none()
            && let Some(def) = &default_by
        {
            handler.by_clause = Some(def.clone());
        }
        if handler.given.is_empty() && !default_given.is_empty() {
            handler.given = default_given.clone();
        }
    }
}

/// Record a file's (possibly partial) expression types into the Analyse-mode
/// sink. Called at every per-file exit in the check loop so `.`-member completion
/// and signature help get the receiver's type even when a later check phase
/// errors for the file (ADR 0094). A no-op-shaped wrapper, factored out so the
/// four exits share one call.
fn record_analyse_types(
    exprs: &mut ExprTypeSink,
    source_path: &Path,
    synthetic: bool,
    types: &HashMap<Span, Ty>,
) {
    exprs.enter_file(source_path, synthetic);
    exprs.record_file(types);
}

/// Phase 1: discover the `.bynk` files under the source (and, in split mode,
/// the tests) root, and run the file-vs-directory conflict checks. Pushes any
/// discovery errors into `errors` and signals a pipeline bail via `Err(())`
/// (the caller terminates with `finish`); otherwise returns the discovered
/// `(src_files, tests_files)`.
fn phase_discovery(
    src_root: &Path,
    tests_root: &Path,
    split_mode: bool,
    excludes: &[PathBuf],
    errors: &mut ErrorSink,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), ()> {
    let src_files = match discover_bynk_files(src_root, excludes) {
        Ok(f) => f,
        Err(e) => {
            errors.push_for(None, e);
            return Err(());
        }
    };
    let tests_files = if split_mode {
        // The secondary `include` root is optional — a project may have no such
        // tree. A missing directory is not an error.
        if tests_root.exists() {
            match discover_bynk_files(tests_root, excludes) {
                Ok(f) => f,
                Err(e) => {
                    errors.push_for(None, e);
                    return Err(());
                }
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if src_files.is_empty() && tests_files.is_empty() {
        errors.push_for(
            None,
            CompileError::new(
                "bynk.project.no_sources",
                Span::default(),
                format!("no `.bynk` source files found under {}", src_root.display()),
            ),
        );
        return Err(());
    }
    if let Err(e) = check_file_directory_conflicts(src_root, &src_files) {
        errors.extend_for(None, e);
    }
    if split_mode && let Err(e) = check_file_directory_conflicts(tests_root, &tests_files) {
        errors.extend_for(None, e);
    }
    Ok((src_files, tests_files))
}

/// Phase 2: parse every discovered file into a `ParsedFile`, recording each
/// file's source text into `snapshots` and any parse errors into `errors`.
/// Then inject the first-party synthetic units (the `bynk`/`bynk.cloudflare`
/// adapters and the `bynk.{list,map,string}` commons) that the project
/// consumes/uses. Returns the parsed units plus whether the `bynk` and
/// `bynk.cloudflare` adapters were injected; signals a pipeline bail via
/// `Err(())` when parsing produced errors and yielded no units at all.
#[allow(clippy::too_many_arguments)]
fn phase_parse(
    src_root: &Path,
    tests_root: &Path,
    // Slice 0: each tree's project-root-relative `include` prefix, empty for a
    // single-root project. Builds `identity_path`; `source_path` stays
    // tree-relative for unit validation. See `ParsedFile`.
    src_prefix: &Path,
    tests_prefix: &Path,
    split_mode: bool,
    src_files: &[PathBuf],
    tests_files: &[PathBuf],
    overlay: &HashMap<PathBuf, String>,
    errors: &mut ErrorSink,
    snapshots: &mut Vec<(PathBuf, String)>,
) -> Result<(Vec<ParsedFile>, bool, bool), ()> {
    let mut parsed: Vec<ParsedFile> = Vec::new();
    let parse_tree = |root: &Path,
                      prefix: &Path,
                      files: &[PathBuf],
                      parsed: &mut Vec<ParsedFile>,
                      errors: &mut ErrorSink,
                      snapshots: &mut Vec<(PathBuf, String)>| {
        for path in files {
            // Tree-relative: what unit validation reads.
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            // Slice 0 — project-relative: what *names* the file. Equal to `rel`
            // for a single root (empty prefix).
            let id = prefix.join(&rel);
            let source = match read_source(path, overlay) {
                Ok(s) => s,
                Err(e) => {
                    errors.push_for(
                        Some(&id),
                        CompileError::new(
                            "bynk.project.read_failed",
                            Span::default(),
                            format!("could not read `{}`: {e}", path.display()),
                        ),
                    );
                    continue;
                }
            };
            snapshots.push((id.clone(), source.clone()));
            match parse_sources(root, prefix, path, source) {
                Ok((pfs, warnings)) => {
                    parsed.extend(pfs);
                    // ADR 0117: the sink classifies these as warnings — they
                    // surface with the build but never gate it.
                    errors.extend_for(Some(&id), warnings);
                }
                Err(errs) => errors.extend_for(Some(&id), errs),
            }
        }
    };
    parse_tree(
        src_root,
        src_prefix,
        src_files,
        &mut parsed,
        errors,
        snapshots,
    );
    if split_mode {
        parse_tree(
            tests_root,
            tests_prefix,
            tests_files,
            &mut parsed,
            errors,
            snapshots,
        );
    }
    if !errors.is_empty() && parsed.is_empty() {
        return Err(());
    }

    // v0.17: if any user unit consumes the first-party `bynk` surface, inject it
    // as a synthetic adapter so it flows through the normal pipeline (tables,
    // exports, emission, compose). Its binding is supplied by the toolchain for
    // the selected platform (§4.2). Injected only when consumed, so adapter-free
    // projects are unchanged.
    let consumes_bynk = parsed.iter().any(|pf| {
        pf.consumes()
            .iter()
            .any(|c| c.target.joined() == firstparty::BYNK_UNIT)
    });
    if consumes_bynk {
        match lexer::tokenize(firstparty::BYNK_ADAPTER_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::BYNK_ADAPTER_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                identity_path: PathBuf::from("bynk.bynk"),
                source_path: PathBuf::from("bynk.bynk"),
                abs_path: None,
                source: firstparty::BYNK_ADAPTER_SRC.to_string(),
                unit,
                kind: UnitKind::Adapter,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.19: likewise the first-party `bynk.cloudflare` platform adapter —
    // injected only when consumed, binding supplied by the toolchain. The
    // unit name sits inside the reserved `bynk.*` prefix (decision 0026).
    let consumes_cloudflare = parsed.iter().any(|pf| {
        pf.consumes()
            .iter()
            .any(|c| c.target.joined() == firstparty::CLOUDFLARE_UNIT)
    });
    if consumes_cloudflare {
        match lexer::tokenize(firstparty::CLOUDFLARE_ADAPTER_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::CLOUDFLARE_ADAPTER_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                identity_path: PathBuf::from("bynk/cloudflare.bynk"),
                source_path: PathBuf::from("bynk/cloudflare.bynk"),
                abs_path: None,
                source: firstparty::CLOUDFLARE_ADAPTER_SRC.to_string(),
                unit,
                kind: UnitKind::Adapter,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.20b: the first-party collection commons. Unlike the adapters above
    // these are *library* units — plain Bynk commons of generic functions —
    // imported via `uses` rather than `consumes`, and injected the same way
    // so they flow through the ordinary commons pipeline (tables, uses
    // resolution, emission). `bynk.map` itself `uses bynk.list`, so using
    // the former injects both.
    let uses_unit = |parsed: &[ParsedFile], unit: &str| {
        parsed
            .iter()
            .any(|pf| pf.uses().iter().any(|u| u.target.joined() == unit))
    };
    let uses_map = uses_unit(&parsed, firstparty::MAP_UNIT);
    if uses_map {
        match lexer::tokenize(firstparty::BYNK_MAP_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::BYNK_MAP_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                identity_path: PathBuf::from("bynk/map.bynk"),
                source_path: PathBuf::from("bynk/map.bynk"),
                abs_path: None,
                source: firstparty::BYNK_MAP_SRC.to_string(),
                unit,
                kind: UnitKind::Commons,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    if uses_map || uses_unit(&parsed, firstparty::LIST_UNIT) {
        match lexer::tokenize(firstparty::BYNK_LIST_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::BYNK_LIST_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                identity_path: PathBuf::from("bynk/list.bynk"),
                source_path: PathBuf::from("bynk/list.bynk"),
                abs_path: None,
                source: firstparty::BYNK_LIST_SRC.to_string(),
                unit,
                kind: UnitKind::Commons,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.22a: the first-party string commons — derived helpers over the
    // built-in string kernel (ADR 0046).
    if uses_unit(&parsed, firstparty::STRING_UNIT) {
        match lexer::tokenize(firstparty::BYNK_STRING_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::BYNK_STRING_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                identity_path: PathBuf::from("bynk/string.bynk"),
                source_path: PathBuf::from("bynk/string.bynk"),
                abs_path: None,
                source: firstparty::BYNK_STRING_SRC.to_string(),
                unit,
                kind: UnitKind::Commons,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }

    Ok((parsed, consumes_bynk, consumes_cloudflare))
}

/// Phase 3: group the parsed units by qualified name (production units, unit
/// tests, and integration suites tracked separately), run the per-directory
/// and path/name consistency checks, enforce the reserved `bynk` namespace and
/// the adapter `binding` rules, resolve each adapter's binding module, and fold
/// the adapters' pinned npm dependencies. Pushes diagnostics into `errors` and
/// returns the production `groups`/`kinds`, the `test`/`integration` groups, the
/// resolved `adapter_bindings`, and the collected `npm_deps`.
#[allow(clippy::type_complexity)]
fn phase_group(
    parsed: &[ParsedFile],
    src_root: &Path,
    platform: Platform,
    consumes_bynk: bool,
    consumes_cloudflare: bool,
    errors: &mut ErrorSink,
) -> (
    HashMap<String, Vec<usize>>,
    HashMap<String, UnitKind>,
    HashMap<String, Vec<usize>>,
    HashMap<String, Vec<usize>>,
    HashMap<String, AdapterBinding>,
    std::collections::BTreeMap<String, String>,
) {
    // Tests (v0.7) are tracked separately from production units. Their
    // `target` joined-name can intentionally coincide with a commons or
    // context name; they don't enter the production groups/kinds maps.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut kinds: HashMap<String, UnitKind> = HashMap::new();
    let mut test_groups: HashMap<String, Vec<usize>> = HashMap::new();
    // v0.16: integration tests are tracked by suite name, separately again from
    // unit tests — their `name()` is the synthetic `integration <suite>`.
    let mut integration_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let name = pf.unit.name().joined();
        if pf.kind == UnitKind::Integration {
            integration_groups.entry(name).or_default().push(i);
        } else if pf.kind == UnitKind::Test {
            test_groups.entry(name).or_default().push(i);
        } else {
            groups.entry(name.clone()).or_default().push(i);
            kinds.entry(name).or_insert(pf.kind);
        }
    }
    // #696: the consistency checks pair each error with the project-relative
    // path of the file its primary span belongs to, so the CLI renders them
    // with ariadne source context rather than the plain fallback.
    if let Err(e) = check_directory_name_consistency(parsed) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }
    if let Err(e) = check_directory_kind_consistency(parsed) {
        errors.extend_for(None, e);
    }
    // A group must agree on kind across all its files (different name but
    // same kind is fine; same name but different kind is an error).
    if let Err(e) = check_group_kind_consistency(parsed, &groups) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }
    // Each *source* unit's file path must match its declared qualified name.
    // v0.113 (DECISION S): a `suite` has no path-identity requirement — it names
    // its target and is legal in any file — so test-ness carries no path check.
    if let Err(e) = check_path_name_alignment(parsed) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }

    // v0.20a: function types are confined to non-boundary positions.
    for (path, err) in check_function_type_boundaries(parsed) {
        errors.push_for(Some(&path), err);
    }

    // v0.17: the `bynk` root namespace is reserved for the toolchain. No user
    // unit of any kind may be named `bynk` or `bynk.*` (§3.4).
    for pf in parsed {
        if pf.synthetic {
            continue;
        }
        let qn = pf.unit.name();
        if qn.parts.first().is_some_and(|p| p.name == "bynk") {
            errors.push_for(Some(&pf.identity_path),
                CompileError::new(
                    "bynk.namespace.reserved",
                    qn.span,
                    format!(
                        "`{}` uses the reserved `bynk` namespace — the `bynk` root is reserved for the toolchain's conformance surface",
                        qn.joined()
                    ),
                )
                .with_note("rename the unit so its first segment is not `bynk`"),
            );
        }
    }

    // v0.17: an adapter that declares any external provider must name a
    // `binding` module to supply the implementation symbols (§3.5). First-party
    // (synthetic) adapters omit the clause — the toolchain supplies the binding.
    for pf in parsed {
        if pf.synthetic {
            continue;
        }
        if let Some(a) = pf.adapter() {
            let has_external = a
                .items
                .iter()
                .any(|it| matches!(it, CommonsItem::Provider(p) if p.external));
            if has_external && a.binding.is_none() {
                errors.push_for(Some(&pf.identity_path),
                    CompileError::new(
                        "bynk.adapter.no_binding",
                        a.span,
                        format!(
                            "adapter `{}` declares an external provider but has no `binding` clause to supply its implementation",
                            a.name.joined()
                        ),
                    )
                    .with_note(
                        "add a `binding \"<module>\"` clause naming the TypeScript module that exports the provider symbols",
                    ),
                );
            }
        }
    }

    // v0.17: resolve each adapter's binding module (relative to the adapter's
    // source file) and read it, so compose can import the external provider
    // symbols and the binding is copied into the output for the `tsc` gate.
    let mut adapter_bindings: HashMap<String, AdapterBinding> = HashMap::new();
    // v0.17: the toolchain supplies the `bynk` surface's binding, platform-keyed.
    if consumes_bynk {
        adapter_bindings.insert(
            firstparty::BYNK_UNIT.to_string(),
            AdapterBinding {
                output_path: PathBuf::from(platform.bynk_binding_filename()),
                content: platform.bynk_binding_source().to_string(),
            },
        );
    }
    // v0.19: the platform adapter's binding is single — it runs only on its
    // own platform (the lock check rejects other `--platform` selections).
    if consumes_cloudflare {
        adapter_bindings.insert(
            firstparty::CLOUDFLARE_UNIT.to_string(),
            AdapterBinding {
                output_path: PathBuf::from(firstparty::CLOUDFLARE_BINDING_FILENAME),
                content: firstparty::cloudflare_binding_source().to_string(),
            },
        );
    }
    for pf in parsed {
        let Some(a) = pf.adapter() else { continue };
        let Some(b) = &a.binding else { continue };
        let adapter_dir = pf.source_path.parent().unwrap_or(Path::new(""));
        let out_rel = normalize_rel(&adapter_dir.join(&b.module));
        let src_abs = src_root.join(&out_rel);
        match fs::read_to_string(&src_abs) {
            Ok(content) => {
                adapter_bindings.insert(
                    a.name.joined(),
                    AdapterBinding {
                        output_path: out_rel,
                        content,
                    },
                );
            }
            Err(e) => {
                errors.push_for(Some(&pf.identity_path),
                    CompileError::new(
                        "bynk.adapter.no_binding",
                        b.module_span,
                        format!(
                            "adapter `{}` names binding module `{}`, which could not be read ({e})",
                            a.name.joined(),
                            b.module
                        ),
                    )
                    .with_note(
                        "the binding path is resolved relative to the adapter's source file; author the `.binding.ts` there",
                    ),
                );
            }
        }
    }

    // v0.17: collect adapter npm dependencies for `package.json`, rejecting
    // unpinned ranges ([DECISION L] stub — fold + pin-check only, no allow-list).
    let mut npm_deps: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for pf in parsed {
        let Some(a) = pf.adapter() else { continue };
        let Some(b) = &a.binding else { continue };
        for dep in &b.requires {
            if is_unpinned_range(&dep.range) {
                errors.push_for(Some(&pf.identity_path),
                    CompileError::new(
                        "bynk.requires.unpinned_dependency",
                        dep.span,
                        format!(
                            "dependency `{}` has an unpinned version range `{}` — pin a concrete range (e.g. `^1.2.0`)",
                            dep.package, dep.range
                        ),
                    )
                    .with_note(
                        "unpinned ranges (`*`, `latest`, …) make builds irreproducible and are rejected",
                    ),
                );
                continue;
            }
            npm_deps.insert(dep.package.clone(), dep.range.clone());
        }
    }

    (
        groups,
        kinds,
        test_groups,
        integration_groups,
        adapter_bindings,
        npm_deps,
    )
}

/// Phase 4: build each production unit's combined symbol table from its files,
/// pushing any table-construction errors into `errors`.
fn phase_symbol_tables(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    errors: &mut ErrorSink,
) -> HashMap<String, UnitTable> {
    let mut unit_tables: HashMap<String, UnitTable> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).expect("every group has a kind");
        // #696: build_unit_table pairs each diagnostic with its declaring file.
        let mut table_errors: Vec<(PathBuf, CompileError)> = Vec::new();
        let table = build_unit_table(name, kind, indices, parsed, &mut table_errors);
        for (path, err) in table_errors {
            errors.push_for(Some(&path), err);
        }
        unit_tables.insert(name.clone(), table);
    }
    unit_tables
}

/// Phase 5: resolve each unit's `uses` clauses, checking the target exists, is
/// a commons, and is not self-referential. Returns unit → deduplicated list of
/// used commons; diagnostics go into `errors`.
fn phase_resolve_uses(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
) -> HashMap<String, Vec<String>> {
    let mut unit_uses: HashMap<String, Vec<String>> = HashMap::new();
    for (name, indices) in groups {
        let mut uses_targets: Vec<String> = Vec::new();
        for &i in indices {
            for u in parsed[i].uses() {
                let target = u.target.joined();
                if !unit_tables.contains_key(&target) {
                    errors.push_for(
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.uses.unknown_commons",
                            u.span,
                            format!("unknown commons `{target}`"),
                        )
                        .with_note(
                            "the target of a `uses` clause must be a commons in the project",
                        ),
                    );
                    continue;
                }
                let target_kind = *kinds.get(&target).unwrap();
                if target_kind != UnitKind::Commons {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.uses.target_is_context",
                            u.span,
                            format!(
                                "`uses {target}` targets a context — `uses` may only target a commons"
                            ),
                        )
                        .with_note(
                            "to declare a dependency on a context, use `consumes` instead",
                        ),
                    );
                    continue;
                }
                if target == *name {
                    errors.push_for(
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.uses.self_reference",
                            u.span,
                            format!("`{name}` cannot `uses` itself"),
                        ),
                    );
                    continue;
                }
                if !uses_targets.contains(&target) {
                    uses_targets.push(target);
                }
            }
        }
        unit_uses.insert(name.clone(), uses_targets);
    }
    unit_uses
}

/// Phase 5b: resolve each unit's `consumes` clauses (target exists, is a context
/// or adapter, not self-referential, obeys the adapter selection rules), and for
/// the braced `consumes U { Cap, … }` form validate and record the flattened
/// capabilities. Returns unit → consumed targets and unit → flattened-cap → owning
/// unit; diagnostics go into `errors` and clause-position references into `refs`.
#[allow(clippy::type_complexity)]
fn phase_resolve_consumes(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) -> (
    HashMap<String, Vec<String>>,
    HashMap<String, HashMap<String, String>>,
) {
    let mut unit_consumes: HashMap<String, Vec<String>> = HashMap::new();
    // v0.17: `consumes U { Cap, … }` flattens selected caps into the consumer's
    // local namespace. unit → bare-cap → consumed unit providing it.
    let mut unit_flattened: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        let mut consumes_targets: Vec<String> = Vec::new();
        let mut flattened: HashMap<String, String> = HashMap::new();
        let local_caps: HashSet<String> = unit_tables
            .get(name)
            .map(|t| t.capabilities.keys().cloned().collect())
            .unwrap_or_default();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path, name, parsed[i].synthetic);
            for c in parsed[i].consumes() {
                let target = c.target.joined();
                if kind != UnitKind::Context && kind != UnitKind::Adapter {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.consumes.in_commons",
                            c.span,
                            format!(
                                "`consumes` is only valid inside a context or adapter, not a commons `{name}`",
                            ),
                        )
                        .with_note(
                            "commons declare vocabulary; only contexts and adapters can declare behavioural dependencies",
                        ),
                    );
                    continue;
                }
                // v0.18: an adapter's `consumes` is the braced capability-selection
                // form only — an adapter has no services to RPC-call, so the
                // whole-unit and `as Alias` forms are meaningless inside one.
                if kind == UnitKind::Adapter && c.selected.is_none() {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.adapter.consumes_requires_selection",
                            c.span,
                            format!(
                                "an adapter's `consumes` must select capabilities — write `consumes {target} {{ Cap, … }}`",
                            ),
                        )
                        .with_note(
                            "adapters depend on capabilities, never on services; the whole-unit and aliased forms are context-only",
                        ),
                    );
                    continue;
                }
                if !unit_tables.contains_key(&target) {
                    errors.push_for(
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.consumes.unknown_context",
                            c.span,
                            format!("unknown context `{target}`"),
                        )
                        .with_note(
                            "the target of a `consumes` clause must be a context in the project",
                        ),
                    );
                    continue;
                }
                let target_kind = *kinds.get(&target).unwrap();
                // v0.17: `consumes` may target a context or an adapter (the host
                // boundary). It may not target a commons (use `uses` for that).
                if target_kind != UnitKind::Context && target_kind != UnitKind::Adapter {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.consumes.target_is_commons",
                            c.span,
                            format!(
                                "`consumes {target}` targets a commons — `consumes` may only target a context or adapter"
                            ),
                        )
                        .with_note(
                            "to mix in declarations from a commons, use `uses` instead",
                        ),
                    );
                    continue;
                }
                // v0.18: adapter dependencies are adapter-to-adapter (spec §4.5) —
                // an adapter consuming a *context* would pull service logic into
                // the host boundary.
                if kind == UnitKind::Adapter && target_kind == UnitKind::Context {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.adapter.consumes_context",
                            c.span,
                            format!(
                                "adapter `{name}` cannot `consumes` the context `{target}` — adapter dependencies are adapter-to-adapter"
                            ),
                        )
                        .with_note(
                            "an adapter may only depend on capabilities exported by other adapters (e.g. the `bynk` surface)",
                        ),
                    );
                    continue;
                }
                if target == *name {
                    let kind_word = if kind == UnitKind::Adapter {
                        "adapter"
                    } else {
                        "context"
                    };
                    errors.push_for(
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.consumes.self_reference",
                            c.span,
                            format!("{kind_word} `{name}` cannot `consumes` itself"),
                        ),
                    );
                    continue;
                }
                // v0.17: `consumes U { Cap, … }` — validate each selected name is
                // a capability `U` exports, detect clashes, and record the
                // flattening so bare `given Cap` resolves through the local path.
                if let Some(names) = &c.selected {
                    let exported = unit_tables
                        .get(&target)
                        .map(|t| &t.exported_capabilities)
                        .cloned()
                        .unwrap_or_default();
                    for cap in names {
                        if !exported.contains(&cap.name) {
                            errors.push_for(
                                Some(&parsed[i].identity_path),
                                CompileError::new(
                                    "bynk.given.cross_context_unknown_capability",
                                    cap.span,
                                    format!(
                                        "`{target}` does not export a capability named `{}`",
                                        cap.name
                                    ),
                                ),
                            );
                            continue;
                        }
                        if local_caps.contains(&cap.name) {
                            errors.push_for(Some(&parsed[i].identity_path), CompileError::new(
                                "bynk.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "flattened capability `{}` clashes with a capability declared locally — use qualified `given {target}.{}` instead",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
                        if let Some(prev) = flattened.get(&cap.name) {
                            errors.push_for(Some(&parsed[i].identity_path), CompileError::new(
                                "bynk.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "capability `{}` is flattened from both `{prev}` and `{target}` — qualify one with `given U.{}`",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
                        // v0.25: the selection list names the capability in
                        // the consumed unit (clause-position reference).
                        refs.record_in_unit(cap.span, SymbolKind::Capability, &cap.name, &target);
                        flattened.insert(cap.name.clone(), target.clone());
                    }
                }
                if !consumes_targets.contains(&target) {
                    consumes_targets.push(target);
                }
            }
        }
        unit_consumes.insert(name.clone(), consumes_targets);
        unit_flattened.insert(name.clone(), flattened);
    }
    (unit_consumes, unit_flattened)
}

/// Phases 5b'/5b'': collect each context's `consumes` aliases (alias →
/// consumed-context name), reporting alias-vs-alias conflicts (5b'), then report
/// any alias that clashes with a locally-declared type/fn/capability/service/agent
/// (5b''). Returns the per-context alias maps; diagnostics go into `errors`.
fn phase_consumes_aliases(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
) -> HashMap<String, HashMap<String, String>> {
    let mut unit_consumes_aliases: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        if kind != UnitKind::Context {
            continue;
        }
        let mut aliases: HashMap<String, String> = HashMap::new();
        let mut alias_spans: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            for c in parsed[i].consumes() {
                let Some(alias) = &c.alias else { continue };
                let target = c.target.joined();
                if !unit_tables.contains_key(&target) {
                    // Already reported as unknown context above.
                    continue;
                }
                if let Some(prev_span) = alias_spans.get(&alias.name) {
                    errors.push_for(Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.consumes.alias_conflict",
                            alias.span,
                            format!(
                                "alias `{}` is used by more than one `consumes` clause in context `{}`",
                                alias.name, name
                            ),
                        )
                        .with_label(*prev_span, "previously defined here")
                        .with_note(
                            "each `consumes` clause may introduce at most one alias, and aliases must be unique within a context",
                        ),
                    );
                    continue;
                }
                aliases.insert(alias.name.clone(), target);
                alias_spans.insert(alias.name.clone(), alias.span);
            }
        }
        unit_consumes_aliases.insert(name.clone(), aliases);
    }

    // -- 5b''. Detect alias-vs-local-decl conflicts. An alias must not clash
    //          with any locally declared type/fn/capability/service/agent.
    for (name, aliases) in &unit_consumes_aliases {
        let Some(local) = unit_tables.get(name) else {
            continue;
        };
        for alias in aliases.keys() {
            let alias_site = parsed_alias_span(parsed, &groups[name], alias);
            let alias_span = alias_site.map(|(_, s)| s).unwrap_or_default();
            let alias_file = alias_site.map(|(i, _)| parsed[i].identity_path.clone());
            let conflict_kind = if local.types.contains_key(alias) {
                Some("type")
            } else if local.fns.contains_key(alias) {
                Some("function")
            } else if local.capabilities.contains_key(alias) {
                Some("capability")
            } else if local.services.contains_key(alias) {
                Some("service")
            } else if local.agents.contains_key(alias) {
                Some("agent")
            } else {
                None
            };
            if let Some(kind) = conflict_kind {
                errors.push_for(alias_file.as_deref(),
                    CompileError::new(
                        "bynk.consumes.alias_conflict",
                        alias_span,
                        format!(
                            "alias `{alias}` conflicts with a local {kind} of the same name in context `{name}`",
                        ),
                    )
                    .with_note(
                        "pick a different alias for the `consumes` clause, or rename the local declaration",
                    ),
                );
            }
        }
    }
    unit_consumes_aliases
}

/// Phase 6: for each unit, detect when two `uses`-imported commons declare the
/// same (non-shadowed) type or function name — an unrenamable conflict at the use
/// site. Diagnostics go into `errors`.
fn phase_uses_name_conflicts(
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    parsed: &[ParsedFile],
    groups: &HashMap<String, Vec<usize>>,
    errors: &mut ErrorSink,
) {
    for (name, targets) in unit_uses {
        let local = unit_tables.get(name).expect("unit table present");
        let mut imported: HashMap<String, String> = HashMap::new();
        for t in targets {
            let used = unit_tables.get(t).expect("used unit table present");
            for type_name in used.types.keys() {
                if local.types.contains_key(type_name) || local.fns.contains_key(type_name) {
                    continue;
                }
                if let Some(prev) = imported.get(type_name) {
                    let site = uses_span_of(parsed, &groups[name], t);
                    let span = site.map(|(_, s)| s).unwrap_or_default();
                    let file = site.map(|(i, _)| parsed[i].identity_path.clone());
                    errors.push_for(file.as_deref(),
                        CompileError::new(
                            "bynk.uses.name_conflict",
                            span,
                            format!(
                                "`{name}` uses two commons that both declare `{type_name}`: `{prev}` and `{t}`",
                            ),
                        )
                        .with_note(
                            "name conflicts at the use site are not yet renamable; remove or restructure one of the imports",
                        ),
                    );
                } else {
                    imported.insert(type_name.clone(), t.clone());
                }
            }
            for fn_name in used.fns.keys() {
                if local.types.contains_key(fn_name) || local.fns.contains_key(fn_name) {
                    continue;
                }
                if let Some(prev) = imported.get(fn_name) {
                    let site = uses_span_of(parsed, &groups[name], t);
                    let span = site.map(|(_, s)| s).unwrap_or_default();
                    let file = site.map(|(i, _)| parsed[i].identity_path.clone());
                    errors.push_for(file.as_deref(),
                        CompileError::new(
                            "bynk.uses.name_conflict",
                            span,
                            format!(
                                "`{name}` uses two commons that both declare `{fn_name}`: `{prev}` and `{t}`",
                            ),
                        )
                        .with_note(
                            "name conflicts at the use site are not yet renamable; remove or restructure one of the imports",
                        ),
                    );
                } else {
                    imported.insert(fn_name.clone(), t.clone());
                }
            }
        }
    }
}

/// Phase 6b: validate each context/adapter's `exports opaque/transparent { … }`
/// clauses — every name must be a locally-declared type, with no duplicates
/// within a clause or conflicting visibilities across clauses. Returns unit →
/// (type → visibility); diagnostics go into `errors` and export references into
/// `refs`.
fn phase_validate_type_exports(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) -> HashMap<String, HashMap<String, Visibility>> {
    let mut exports_visibility: HashMap<String, HashMap<String, Visibility>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        if kind != UnitKind::Context && kind != UnitKind::Adapter {
            // Commons may not have exports clauses (parsed grammar prevents it
            // at the parser level), but in case any sneak in, skip.
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, (Visibility, Span)> = HashMap::new();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path, name, parsed[i].synthetic);
            for clause in parsed[i].exports() {
                // v0.15: `exports capability { ... }` clauses are validated
                // separately (§4.1); 6b handles only type exports.
                let ExportKind::Type(clause_vis) = clause.kind else {
                    continue;
                };
                let mut within: HashMap<String, Span> = HashMap::new();
                for n in &clause.names {
                    if let Some(prev) = within.get(&n.name) {
                        errors.push_for(
                            Some(&parsed[i].identity_path),
                            CompileError::new(
                                "bynk.exports.duplicate_in_clause",
                                n.span,
                                format!(
                                    "type `{}` appears more than once in this exports clause",
                                    n.name
                                ),
                            )
                            .with_label(*prev, "previously listed here"),
                        );
                        continue;
                    }
                    within.insert(n.name.clone(), n.span);

                    if !local.types.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path),
                            CompileError::new(
                                "bynk.exports.undeclared_type",
                                n.span,
                                format!(
                                    "exports clause references `{}`, which is not a type declared in context `{}`",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "only types declared in the same context can appear in `exports` clauses",
                            ),
                        );
                        continue;
                    }
                    // v0.25: `exports opaque/transparent { T }` names the type.
                    refs.record(n.span, SymbolKind::Type, &n.name);

                    if let Some((prev_vis, prev_span)) = seen.get(&n.name) {
                        if *prev_vis == clause_vis {
                            errors.push_for(
                                Some(&parsed[i].identity_path),
                                CompileError::new(
                                    "bynk.exports.duplicate_export",
                                    n.span,
                                    format!("type `{}` is exported more than once", n.name),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        } else {
                            errors.push_for(Some(&parsed[i].identity_path),
                                CompileError::new(
                                    "bynk.exports.conflicting_visibility",
                                    n.span,
                                    format!(
                                        "type `{}` is exported with conflicting visibilities — pick `opaque` or `transparent`",
                                        n.name,
                                    ),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        }
                        continue;
                    }
                    seen.insert(n.name.clone(), (clause_vis, n.span));
                }
            }
        }
        let mut visibility_map: HashMap<String, Visibility> = HashMap::new();
        for (n, (v, _)) in seen {
            visibility_map.insert(n, v);
        }
        exports_visibility.insert(name.clone(), visibility_map);
    }
    exports_visibility
}

/// Phase 6b': validate each context/adapter's `exports capability { … }` clauses
/// (v0.15 §4.1) — every name must be a capability the unit declares *and*
/// provides, with no duplicate exports. Diagnostics go into `errors` and export
/// references into `refs`.
fn phase_validate_capability_exports(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) {
    for (name, indices) in groups {
        if kinds.get(name) != Some(&UnitKind::Context)
            && kinds.get(name) != Some(&UnitKind::Adapter)
        {
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path, name, parsed[i].synthetic);
            for clause in parsed[i].exports() {
                if !matches!(clause.kind, ExportKind::Capability) {
                    continue;
                }
                for n in &clause.names {
                    if let Some(prev) = seen.get(&n.name) {
                        errors.push_for(
                            Some(&parsed[i].identity_path),
                            CompileError::new(
                                "bynk.exports.duplicate_export",
                                n.span,
                                format!("capability `{}` is exported more than once", n.name),
                            )
                            .with_label(*prev, "previously exported here"),
                        );
                        continue;
                    }
                    seen.insert(n.name.clone(), n.span);
                    if local.capabilities.contains_key(&n.name) {
                        // v0.25: `exports capability { Cap }` names the
                        // capability.
                        refs.record(n.span, SymbolKind::Capability, &n.name);
                    }
                    if !local.capabilities.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path),
                            CompileError::new(
                                "bynk.exports.undeclared_capability",
                                n.span,
                                format!(
                                    "`exports capability` references `{}`, which is not a capability declared in context `{}`",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "only capabilities declared in the same context can appear in `exports capability` clauses",
                            ),
                        );
                        continue;
                    }
                    if !local.providers.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path),
                            CompileError::new(
                                "bynk.exports.capability_not_provided",
                                n.span,
                                format!(
                                    "exported capability `{}` has no provider in context `{}` — a consumer cannot instantiate it",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "add a `provides {n} = …` declaration so the capability can be wired into consumers",
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// Phase 6c: validate that every (non-external) provider matches its capability
/// exactly — each capability op has a provider op, and every provider op has a
/// matching capability op with the same parameter and return types. Diagnostics
/// go into `errors`.
fn phase_validate_providers(
    unit_tables: &HashMap<String, UnitTable>,
    // #696: the merged `UnitTable` has flattened a unit's files away, so provider
    // diagnostics need the group's files to recover which one declares each
    // provider and attribute the diagnostic to it.
    groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    errors: &mut ErrorSink,
) {
    for (name, table) in unit_tables {
        // Map each provided capability to the project-relative path of the file
        // that declares its provider — every diagnostic below carries a span into
        // that file.
        let provider_files: HashMap<&str, &Path> = groups
            .get(name)
            .map(|indices| {
                indices
                    .iter()
                    .flat_map(|&i| {
                        parsed[i].items().iter().filter_map(move |item| match item {
                            CommonsItem::Provider(p) => Some((
                                p.capability.name.as_str(),
                                parsed[i].identity_path.as_path(),
                            )),
                            _ => None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (cap_name, provider) in &table.providers {
            let provider_file = provider_files.get(cap_name.as_str()).copied();
            // v0.17: an external provider has no Bynk body to match against the
            // capability — its implementation is the binding, checked by `tsc`.
            if provider.external {
                continue;
            }
            let Some(cap) = table.capabilities.get(cap_name) else {
                errors.push_for(provider_file,
                    CompileError::new(
                        "bynk.provider.unknown_capability",
                        provider.capability.span,
                        format!(
                            "provider targets unknown capability `{}` — declare the capability in the same context",
                            cap_name
                        ),
                    ),
                );
                continue;
            };
            // 1) Every capability op has a provider op.
            for cap_op in &cap.ops {
                if !provider.ops.iter().any(|o| o.name.name == cap_op.name.name) {
                    errors.push_for(
                        provider_file,
                        CompileError::new(
                            "bynk.provider.missing_operation",
                            provider.span,
                            format!(
                                "provider `{}` for capability `{}` is missing operation `{}`",
                                provider.provider_name.name, cap_name, cap_op.name.name
                            ),
                        ),
                    );
                }
            }
            // 2) Every provider op corresponds to a capability op with the
            //    same signature (param types and return type).
            for prov_op in &provider.ops {
                let Some(cap_op) = cap.ops.iter().find(|o| o.name.name == prov_op.name.name) else {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.extra_operation",
                        prov_op.span,
                        format!(
                            "provider operation `{}.{}` does not match any operation in capability `{}`",
                            provider.provider_name.name, prov_op.name.name, cap_name
                        ),
                    ));
                    continue;
                };
                if cap_op.params.len() != prov_op.params.len() {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.signature_mismatch",
                        prov_op.span,
                        format!(
                            "provider operation `{}.{}` has {} parameter(s), but capability operation expects {}",
                            provider.provider_name.name,
                            prov_op.name.name,
                            prov_op.params.len(),
                            cap_op.params.len()
                        ),
                    ));
                    continue;
                }
                for (i, (cap_p, prov_p)) in
                    cap_op.params.iter().zip(prov_op.params.iter()).enumerate()
                {
                    if !type_refs_match(&cap_p.type_ref, &prov_p.type_ref) {
                        errors.push_for(provider_file, CompileError::new(
                            "bynk.provider.signature_mismatch",
                            prov_p.span,
                            format!(
                                "provider operation `{}.{}` parameter {} has type `{}`, but capability declares `{}`",
                                provider.provider_name.name,
                                prov_op.name.name,
                                i + 1,
                                ts_type_ref_display(&prov_p.type_ref),
                                ts_type_ref_display(&cap_p.type_ref)
                            ),
                        ));
                    }
                }
                if !type_refs_match(&cap_op.return_type, &prov_op.return_type) {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.signature_mismatch",
                        prov_op.return_type.span(),
                        format!(
                            "provider operation `{}.{}` returns `{}`, but capability declares `{}`",
                            provider.provider_name.name,
                            prov_op.name.name,
                            ts_type_ref_display(&prov_op.return_type),
                            ts_type_ref_display(&cap_op.return_type)
                        ),
                    ));
                }
            }
        }
    }
}

/// Phase 7: build each production unit's file-declaration index (which file in
/// the unit declares which name), for cross-file lookups in the back half.
fn phase_file_index(
    groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
) -> HashMap<String, FileDeclIndex> {
    let mut unit_file_index: HashMap<String, FileDeclIndex> = HashMap::new();
    for (name, indices) in groups {
        unit_file_index.insert(name.clone(), build_file_decl_index(indices, parsed));
    }
    unit_file_index
}

/// v0.29.4: the per-unit facets that the producer phases build as nine parallel
/// `HashMap<String, _>`s, all keyed on unit name. Assembling one record per unit
/// makes the "all these maps share one keyset" invariant structural: a single
/// lookup yields every facet as a field, so the per-column `.unwrap()`s on the
/// shared keyset disappear. Fields are total — `exports`/`aliases`/`flattened`
/// default to an empty map for a unit with no entry, reproducing the old
/// `.unwrap_or(empty)` read semantics without the dance.
struct UnitInfo {
    kind: UnitKind,
    table: UnitTable,
    uses: Vec<String>,
    consumes: Vec<String>,
    flattened: HashMap<String, String>,
    aliases: HashMap<String, String>,
    exports: HashMap<String, Visibility>,
    file_index: FileDeclIndex,
    files: Vec<usize>,
}

/// v0.29.4: fold the nine parallel per-unit maps into one `HashMap<String,
/// UnitInfo>`. Assembly is driven by the `groups` keyset (the authority), so
/// every group yields exactly one record. Facets that are genuinely optional in
/// the producer maps (`exports`/`aliases`/`flattened`, and `file_index` for a
/// unit with no declarations) default to empty — reproducing the old
/// `.unwrap_or(empty)` read semantics as a total field.
#[allow(clippy::too_many_arguments)]
fn assemble_unit_info(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_file_index: &HashMap<String, FileDeclIndex>,
) -> HashMap<String, UnitInfo> {
    groups
        .iter()
        .map(|(name, indices)| {
            let info = UnitInfo {
                kind: *kinds.get(name).unwrap(),
                table: unit_tables.get(name).unwrap().clone(),
                uses: unit_uses.get(name).cloned().unwrap_or_default(),
                consumes: unit_consumes.get(name).cloned().unwrap_or_default(),
                flattened: unit_flattened.get(name).cloned().unwrap_or_default(),
                aliases: unit_consumes_aliases.get(name).cloned().unwrap_or_default(),
                exports: exports_visibility.get(name).cloned().unwrap_or_default(),
                file_index: unit_file_index
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| FileDeclIndex {
                        types: HashMap::new(),
                        fns: HashMap::new(),
                        methods: HashMap::new(),
                    }),
                files: indices.clone(),
            };
            (name.clone(), info)
        })
        .collect()
}

/// Phase 8c: collect every method authored anywhere in one unit, keyed by its
/// attached type's name — so a type's methods surface in the file that declares
/// the type even when the method lives in a sibling file. The collection loop
/// has no `continue`s, so it lifts out whole.
fn collect_unit_methods(indices: &[usize], parsed: &[ParsedFile]) -> HashMap<String, Vec<FnDecl>> {
    let mut local_methods_for_type: HashMap<String, Vec<FnDecl>> = HashMap::new();
    for &j in indices {
        for item in parsed[j].items() {
            if let CommonsItem::Fn(f) = item
                && let FnName::Method { type_name, .. } = &f.name
            {
                local_methods_for_type
                    .entry(type_name.name.clone())
                    .or_default()
                    .push(f.clone());
            }
        }
    }
    local_methods_for_type
}

/// Phase 8b: merge one context's `consumes` exports into the composed symbol
/// space, recording visibility metadata in the returned `consumed_types`. The
/// per-export `continue`s (missing decl, name conflict) stay internal to the
/// loop, which lifts out whole; name conflicts are pushed into `errors` and the
/// caller's `group_error_baseline` guard reacts to them after this returns.
#[allow(clippy::too_many_arguments)]
fn merge_consumed_exports(
    name: &str,
    parsed: &[ParsedFile],
    unit_info: &HashMap<String, UnitInfo>,
    combined_types: &mut HashMap<String, TypeDecl>,
    combined_methods: &mut HashMap<String, ResolverMethodTable>,
    imported_from: &mut HashMap<String, String>,
    imported_from_kind: &mut HashMap<String, UnitKind>,
    errors: &mut ErrorSink,
) -> HashMap<String, ConsumedType> {
    // Names visible from `consumes` (read-only types from consumed contexts).
    // For each name we track:
    // - the type decl, with the consumed context's identity
    // - the visibility (opaque/transparent)
    // - the owning context's qualified name (for external-construction errors)
    let mut consumed_types: HashMap<String, ConsumedType> = HashMap::new();

    // Now process `consumes` for contexts: add exported types into the
    // symbol table with visibility metadata so the checker can enforce
    // construction / inspection rules.
    for t in unit_info.get(name).into_iter().flat_map(|i| &i.consumes) {
        let used = &unit_info.get(t).expect("consumed unit present").table;
        let used_exports = &unit_info[t].exports;
        for (type_name, vis) in used_exports {
            let Some(decl) = used.types.get(type_name) else {
                continue;
            };
            if combined_types.contains_key(type_name) {
                // Name conflict between local/uses and consumed export.
                let consumes_site = consumes_span_of(parsed, &unit_info[name].files, t);
                let consumes_span = consumes_site.map(|(_, s)| s).unwrap_or_default();
                let consumes_file = consumes_site.map(|(i, _)| parsed[i].identity_path.clone());
                errors.push_for(consumes_file.as_deref(),
                    CompileError::new(
                        "bynk.consumes.name_conflict",
                        consumes_span,
                        format!(
                            "context `{name}` consumes `{t}` which exports type `{type_name}`, but a type of the same name is already in scope",
                        ),
                    )
                    .with_note(
                        "rename one of the conflicting declarations or restructure the import",
                    ),
                );
                continue;
            }
            combined_types.insert(type_name.clone(), decl.clone());
            imported_from.insert(type_name.clone(), t.clone());
            imported_from_kind.insert(type_name.clone(), UnitKind::Context);
            consumed_types.insert(
                type_name.clone(),
                ConsumedType {
                    owning_context: t.clone(),
                    visibility: *vis,
                },
            );
            // Methods on transparently-exported types: they're emitted in
            // the owning context's output, but reading-side methods (like
            // user-declared instance methods) are callable from consumers.
            // For v0.4, we expose all instance methods on consumed types
            // so the checker can resolve method calls; the checker
            // separately enforces that constructors (.of/unsafe) aren't
            // callable externally.
            if let Some(mt) = used.methods.get(type_name) {
                let entry = combined_methods.entry(type_name.clone()).or_default();
                for (m, decl) in &mt.instance {
                    entry
                        .instance
                        .entry(m.clone())
                        .or_insert_with(|| decl.clone());
                }
                // We deliberately *don't* import static methods from
                // consumed contexts. Static methods can construct new
                // values, which is forbidden externally.
            }
        }
    }

    consumed_types
}

/// Phase 8a: compose one unit's symbol space — its local table plus a
/// one-level `uses` mixin (commons identity preserved). Returns the combined
/// type/fn/method tables and the `imported_from` provenance maps; the mixin
/// loop has no `continue`s, so it lifts out whole.
#[allow(clippy::type_complexity)]
fn compose_unit_symbols(
    name: &str,
    local_table: &UnitTable,
    unit_info: &HashMap<String, UnitInfo>,
) -> (
    HashMap<String, TypeDecl>,
    HashMap<String, FnDecl>,
    HashMap<String, ResolverMethodTable>,
    HashMap<String, String>,
    HashMap<String, UnitKind>,
) {
    // Compose: local + transitive (one level) uses. For commons, mixin
    // preserves type identity; for contexts, mixin produces per-context
    // nominal types. The resolver doesn't distinguish (the rebranding is
    // observable in emission); the symbol table union is the same.
    let mut combined_types = local_table.types.clone();
    let mut combined_fns = local_table.fns.clone();
    let mut combined_methods = local_table.methods.clone();
    let mut imported_from: HashMap<String, String> = HashMap::new();
    let mut imported_from_kind: HashMap<String, UnitKind> = HashMap::new();

    for t in unit_info.get(name).into_iter().flat_map(|i| &i.uses) {
        let used = &unit_info.get(t).expect("used unit present").table;
        for (type_name, decl) in &used.types {
            if !combined_types.contains_key(type_name) {
                combined_types.insert(type_name.clone(), decl.clone());
                imported_from.insert(type_name.clone(), t.clone());
                imported_from_kind.insert(type_name.clone(), UnitKind::Commons);
            }
        }
        for (fn_name, decl) in &used.fns {
            if !combined_fns.contains_key(fn_name) {
                combined_fns.insert(fn_name.clone(), decl.clone());
                imported_from.insert(fn_name.clone(), t.clone());
                imported_from_kind.insert(fn_name.clone(), UnitKind::Commons);
            }
        }
        for (type_name, mt) in &used.methods {
            let entry = combined_methods.entry(type_name.clone()).or_default();
            for (m, decl) in &mt.instance {
                entry
                    .instance
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
            for (m, decl) in &mt.statics {
                entry
                    .statics
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
        }
    }

    (
        combined_types,
        combined_fns,
        combined_methods,
        imported_from,
        imported_from_kind,
    )
}

/// v0.119 (ADR 0155): the agents a `for all run: History[Agent]` property drives,
/// scanned across every test suite in the project. `emit_agent` gates the
/// exported `__bynkDriveHistory_<Agent>` driver on membership, so a non-targeted
/// agent's emission is unchanged.
fn collect_history_target_agents(parsed: &[ParsedFile]) -> HashSet<String> {
    let mut set = HashSet::new();
    for pf in parsed {
        let Some(test) = pf.test() else { continue };
        for prop in &test.properties {
            for b in &prop.forall.bindings {
                if let TypeRef::History(inner, _) = &b.type_ref
                    && let TypeRef::Named(id) = &**inner
                {
                    set.insert(id.name.clone());
                }
            }
        }
    }
    set
}

/// Phase 8e: build the emitter context for one checked source file and render
/// its TypeScript, pushing the result onto `compiled`. Reached only in build
/// mode (the caller's analyse-mode `continue` gates this off); the block is
/// straight-line with no `continue`s of its own.
#[allow(clippy::too_many_arguments)]
fn emit_unit(
    name: &str,
    kind: UnitKind,
    i: usize,
    pf: &ParsedFile,
    indices: &[usize],
    parsed: &[ParsedFile],
    unit_info: &HashMap<String, UnitInfo>,
    imported_from: &HashMap<String, String>,
    imported_from_kind: &HashMap<String, UnitKind>,
    owning_context_for_emit: &Option<String>,
    consumed_types: &HashMap<String, ConsumedType>,
    cross_context_for_file: &resolver::CrossContextInfo,
    typed: &checker::TypedCommons,
    target: BuildTarget,
    import_ext: ImportExt,
    contracts: bool,
    agent_deps_plan: Option<&AgentDepsPlan>,
    compiled: &mut Vec<CompiledFile>,
) {
    // Build the emitter context.
    let info = &unit_info[name];
    // v0.132.1 (#481): gather the attached methods of every `uses`-imported type
    // (one level, matching the symbol-table merge). `emit_context_rebrands`
    // forwards these onto the consumer's rebranded const so a call like
    // `Cents.fromInt(n)` type-checks. Sorted by method name for deterministic
    // emission (the resolver stores instance/static methods in `HashMap`s).
    let mut imported_methods: HashMap<String, Vec<FnDecl>> = HashMap::new();
    for t in &info.uses {
        let Some(used) = unit_info.get(t) else {
            continue;
        };
        for (type_name, mt) in &used.table.methods {
            let entry = imported_methods.entry(type_name.clone()).or_default();
            entry.extend(mt.instance.values().cloned());
            entry.extend(mt.statics.values().cloned());
        }
    }
    let method_key = |f: &FnDecl| match &f.name {
        FnName::Method { method_name, .. } => method_name.name.clone(),
        FnName::Free(id) => id.name.clone(),
    };
    for decls in imported_methods.values_mut() {
        decls.sort_by_key(&method_key);
    }
    let mut imported_decl_paths: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
    for t in &info.uses {
        if let Some(target_info) = unit_info.get(t) {
            let target_index = &target_info.file_index;
            let mut paths: HashMap<String, PathBuf> = HashMap::new();
            for (n, p) in &target_index.types {
                paths.insert(n.clone(), p.clone());
            }
            for (n, p) in &target_index.fns {
                paths.insert(n.clone(), p.clone());
            }
            imported_decl_paths.insert(t.clone(), paths);
        }
    }
    for t in &info.consumes {
        if let Some(target_info) = unit_info.get(t) {
            let target_index = &target_info.file_index;
            let mut paths: HashMap<String, PathBuf> = HashMap::new();
            // Only expose exported names — the emitter needs to know
            // which file declares them so it can render the import.
            let exports_for_target = &target_info.exports;
            for n in exports_for_target.keys() {
                if let Some(p) = target_index.types.get(n) {
                    paths.insert(n.clone(), p.clone());
                }
            }
            imported_decl_paths.insert(t.clone(), paths);
        }
    }

    let exports_local = info.exports.clone();
    let exports_for_consumed = info
        .consumes
        .iter()
        .map(|t| {
            (
                t.clone(),
                unit_info
                    .get(t)
                    .map(|i| i.exports.clone())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let cross_context_info = cross_context_for_file.clone();

    // v0.8: in workers mode, a context's *output* lands under
    // workers/<dashes>/handlers.ts. Use that path as the synthetic
    // source_path so the emitter's depth/relative-path logic and
    // imported_decl_paths produce correct relative imports.
    let workers_mode = matches!(target, BuildTarget::Workers);
    let emit_source_path = if workers_mode && kind == UnitKind::Context {
        worker_handlers_source_path(name)
    } else {
        pf.source_path.clone()
    };
    let emit_local_files = if workers_mode && kind == UnitKind::Context {
        // Each context becomes one Worker; the body collapses into
        // one handlers.ts so there are no siblings to import from.
        Vec::new()
    } else {
        indices
            .iter()
            .filter_map(|&j| {
                if j == i {
                    None
                } else {
                    Some(parsed[j].source_path.clone())
                }
            })
            .collect()
    };

    // In workers mode, rewrite imported_decl_paths for consumed
    // contexts to point at the consumed Worker's handlers.ts.
    let mut imported_decl_paths_emit = imported_decl_paths.clone();
    if workers_mode {
        for (unit, decls) in imported_decl_paths.iter() {
            let target_kind = unit_info.get(unit).map(|i| i.kind);
            if target_kind == Some(UnitKind::Context) {
                let handlers_path = worker_handlers_source_path(unit);
                let mut rewritten = HashMap::new();
                for n in decls.keys() {
                    rewritten.insert(n.clone(), handlers_path.clone());
                }
                imported_decl_paths_emit.insert(unit.clone(), rewritten);
            }
        }
    }

    // v0.8: pre-compute boundary type owners so the emitter can
    // generate serialise/deserialise helper imports correctly. Only
    // relevant in workers mode for contexts.
    let boundary_type_owners = if workers_mode && kind == UnitKind::Context {
        compute_boundary_type_owners(name, unit_info, parsed)
    } else {
        HashMap::new()
    };

    let emit_ctx = EmitProjectCtx {
        source_path: emit_source_path,
        commons_name: name.to_string(),
        local_files: emit_local_files,
        file_decl_index: info.file_index.clone(),
        imported_from: imported_from.clone(),
        imported_from_kind: imported_from_kind.clone(),
        imported_decl_paths: imported_decl_paths_emit,
        commons_dir: commons_dir_for(name),
        unit_kind: kind,
        owning_context: owning_context_for_emit.clone(),
        exports_local,
        exports_for_consumed,
        imported_methods,
        consumed_types: consumed_types.clone(),
        cross_context: cross_context_info,
        is_consumed_by_others: unit_info
            .values()
            .any(|i| i.consumes.iter().any(|t| t == name)),
        target,
        boundary_type_owners,
        local_agents: info.table.agents.keys().cloned().collect(),
        agent_given_deps: agent_deps_plan.map(|p| p.exprs.clone()).unwrap_or_default(),
        extra_import_lines: agent_deps_plan
            .map(|p| p.imports.clone())
            .unwrap_or_default(),
        agent_method_givens: info
            .table
            .agents
            .iter()
            .map(|(agent, a)| {
                (
                    agent.clone(),
                    a.handlers
                        .iter()
                        .filter_map(|h| {
                            h.method_name
                                .as_ref()
                                .map(|m| (m.name.clone(), h.given.clone()))
                        })
                        .collect(),
                )
            })
            .collect(),
        // v0.47: the context's actors (merged across files), so the Bearer
        // verification seam resolves even when the actor and handler are in
        // different files of the same context.
        actors: info.table.actors.clone(),
        consumed_adapters: info
            .consumes
            .iter()
            .filter(|t| unit_info.get(*t).map(|i| i.kind) == Some(UnitKind::Adapter))
            .cloned()
            .collect(),
        import_ext,
        contracts,
        history_target_agents: collect_history_target_agents(parsed),
    };
    // v0.72: the map's `source` is the absolute path the compiler read the file
    // from, so an editor breakpoint set on the real `.bynk` resolves to the same
    // path the debugger loads (project-relative would resolve against the output
    // `.ts`'s directory — the wrong place). Synthetic units fall back to relative.
    let source_name = pf.map_source_name();
    let (ts, source_map) = emitter::emit_project(typed, &emit_ctx, &pf.source, &source_name);
    // Slice 3: the handler-label sidecar for this unit (ADR 0105) — names stack
    // frames by their Bynk operation. `None` for units with no handlers.
    let debug_metadata = emitter::collect_handler_labels(typed);
    let output_path = if workers_mode && kind == UnitKind::Context {
        worker_handlers_output_path(name)
    } else {
        ts_output_path(&pf.source_path)
    };
    compiled.push(CompiledFile {
        source_path: pf.source_path.clone(),
        output_path,
        typescript: ts,
        source_map,
        debug_metadata,
    });
}

/// Phase 8d/8e: resolve + check (and, in build mode, emit) every source file in
/// one production unit. The per-file `continue`s stay internal to this loop, so
/// a file that fails resolution/checking is skipped without abandoning the unit.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn check_unit_files(
    name: &str,
    kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    unit_info: &HashMap<String, UnitInfo>,
    combined_types: &HashMap<String, TypeDecl>,
    combined_fns: &HashMap<String, FnDecl>,
    combined_methods: &HashMap<String, ResolverMethodTable>,
    local_names: &HashSet<String>,
    local_methods_for_type: &HashMap<String, Vec<FnDecl>>,
    consumed_types: &HashMap<String, ConsumedType>,
    imported_from: &HashMap<String, String>,
    imported_from_kind: &HashMap<String, UnitKind>,
    owning_context_for_emit: &Option<String>,
    target: BuildTarget,
    import_ext: ImportExt,
    contracts: bool,
    agent_deps_plan: Option<&AgentDepsPlan>,
    mode: Mode,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    exprs: &mut ExprTypeSink,
    requirements: &mut RequirementSink,
    compiled: &mut Vec<CompiledFile>,
) {
    // v0.29.4: `build_cross_context_info` (and its `combined_types_for` helper)
    // is a general map-based function — the test-emission path calls it with
    // *synthetic* harness maps, not `unit_info` — so it keeps its parallel-map
    // signature. `check_unit_files` only has `unit_info`, so materialise the
    // four views that one call needs, once per unit ahead of the file loop.
    let unit_tables: HashMap<String, UnitTable> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.table.clone()))
        .collect();
    let unit_uses: HashMap<String, Vec<String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.uses.clone()))
        .collect();
    let unit_consumes: HashMap<String, Vec<String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.consumes.clone()))
        .collect();
    let unit_consumes_aliases: HashMap<String, HashMap<String, String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.aliases.clone()))
        .collect();

    for &i in indices {
        let pf = &parsed[i];

        let mut emit_items: Vec<CommonsItem> = Vec::new();
        let types_in_this_file: HashSet<String> = pf
            .items()
            .iter()
            .filter_map(|it| match it {
                CommonsItem::Type(t) => Some(t.name.name.clone()),
                _ => None,
            })
            .collect();
        for item in pf.items() {
            match item {
                CommonsItem::Type(t) => {
                    emit_items.push(CommonsItem::Type(t.clone()));
                }
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(_) => emit_items.push(CommonsItem::Fn(f.clone())),
                    FnName::Method { type_name, .. } => {
                        if types_in_this_file.contains(&type_name.name) {
                            emit_items.push(CommonsItem::Fn(f.clone()));
                        }
                    }
                },
                CommonsItem::Capability(c) => {
                    emit_items.push(CommonsItem::Capability(c.clone()));
                }
                CommonsItem::Provider(p) => {
                    emit_items.push(CommonsItem::Provider(p.clone()));
                }
                CommonsItem::Service(s) => {
                    emit_items.push(CommonsItem::Service(s.clone()));
                }
                CommonsItem::Agent(a) => {
                    emit_items.push(CommonsItem::Agent(a.clone()));
                }
                CommonsItem::Actor(a) => {
                    // Actors emit no standalone TS, but are carried so the
                    // emitter can read their schemes for the verification seam.
                    emit_items.push(CommonsItem::Actor(a.clone()));
                }
            }
        }
        for type_name in &types_in_this_file {
            if let Some(methods) = local_methods_for_type.get(type_name) {
                for m in methods {
                    let already = emit_items.iter().any(|it| match it {
                        CommonsItem::Fn(existing) => match &existing.name {
                            FnName::Method {
                                type_name: t,
                                method_name: n,
                            } => match &m.name {
                                FnName::Method {
                                    type_name: t2,
                                    method_name: n2,
                                } => t.name == t2.name && n.name == n2.name,
                                _ => false,
                            },
                            _ => false,
                        },
                        _ => false,
                    });
                    if !already {
                        emit_items.push(CommonsItem::Fn(m.clone()));
                    }
                }
            }
        }

        // Synthesize a "Commons-shaped" view of this file's items so we
        // can drive the existing resolver/checker without duplication.
        let synthetic_commons = pf.as_synthetic_commons(emit_items);

        // Cross-context info (v0.6) for contexts: consumed contexts,
        // aliases, services, and types. Computed once below; reused
        // for the resolver, checker, and emitter. v0.18: adapters get it
        // too, so an external provider's `given` resolves against the
        // adapter's flattened consumed capabilities (spec §4.5).
        let cross_context_for_file = if kind == UnitKind::Context || kind == UnitKind::Adapter {
            let mut cci = build_cross_context_info(
                name,
                &unit_consumes,
                &unit_consumes_aliases,
                &unit_uses,
                &unit_tables,
            );
            cci.flattened_caps = unit_info[name].flattened.clone();
            cci
        } else {
            resolver::CrossContextInfo::default()
        };

        let resolved = ResolvedCommons {
            commons: synthetic_commons,
            types: combined_types.clone(),
            fns: combined_fns.clone(),
            methods: combined_methods.clone(),
            local_type_names: local_names.clone(),
            cross_context: cross_context_for_file.clone(),
            agents: HashMap::new(),
            // ADR 0116 D6: provenance for the `bynk.list` deprecation lint.
            imported_from: imported_from.clone(),
        };
        refs.enter_file(&pf.identity_path, name, pf.synthetic);
        // v0.27: synthetic and test/integration files record no hints —
        // neither surfaces in an editor (the `assemble_index` rule).
        hints.enter_file(
            &pf.identity_path,
            pf.synthetic || matches!(pf.kind, UnitKind::Test | UnitKind::Integration),
        );
        // v0.31: locals serve completion/navigation in test files too — only
        // synthetic (toolchain-injected) files are muted.
        locals.enter_file(&pf.identity_path, pf.synthetic);
        // v0.99: capability requirements follow the inlay-hint muting rule —
        // synthetic and test/integration files surface none in an editor.
        requirements.enter_file(
            &pf.identity_path,
            pf.synthetic || matches!(pf.kind, UnitKind::Test | UnitKind::Integration),
        );
        if let Err(errs) = resolver::resolve_file_record(&resolved, refs) {
            errors.extend_for(Some(&pf.identity_path), errs);
            continue;
        }
        let rc = checker::check_record(resolved, refs, hints, locals, requirements);
        let typed = match rc.result {
            Ok(t) => {
                // v0.89 (ADR 0117): a unit that checks clean may still carry
                // non-failing warnings — push them into the (severity-aware)
                // sink, where they are classified as warnings and never gate.
                if !t.warnings.is_empty() {
                    errors.extend_for(Some(&pf.identity_path), t.warnings.clone());
                }
                t
            }
            Err(errs) => {
                errors.extend_for(Some(&pf.identity_path), errs);
                // ADR 0094: in Analyse mode, surface the best-effort partial types
                // the checker computed so `.`-member completion / signature help
                // work on a buffer with an unrelated error. Build bails (no
                // emission) as before.
                if mode == Mode::Analyse {
                    record_analyse_types(
                        exprs,
                        &pf.identity_path,
                        pf.synthetic,
                        &rc.partial_expr_types,
                    );
                }
                continue;
            }
        };

        // Run the context-specific checks: forbidden construction,
        // private-type references.
        if kind == UnitKind::Context {
            let context_check_errs = check_context_constraints(&typed, consumed_types, local_names);
            if !context_check_errs.is_empty() {
                errors.extend_for(Some(&pf.identity_path), context_check_errs);
                if mode == Mode::Analyse {
                    record_analyse_types(exprs, &pf.identity_path, pf.synthetic, &typed.expr_types);
                }
                continue;
            }
        }

        // v0.5: check capability/provider/service/agent declarations.
        // v0.18: adapters run these too — an external provider's `given`
        // resolves through the same path as a bodied provider's (the
        // service/agent checks are vacuous for adapters, which have none).
        let mut typed = typed;
        let unit_table_owned = unit_info.get(name).map(|i| i.table.clone());
        if (kind == UnitKind::Context || kind == UnitKind::Adapter)
            && let Some(table) = unit_table_owned.as_ref()
        {
            let decl_errs = check_context_declarations(
                &mut typed,
                table,
                &cross_context_for_file,
                refs,
                hints,
                locals,
                requirements,
            );
            if !decl_errs.is_empty() {
                // ADR 0117: a warning-severity declaration diagnostic (e.g. the
                // `@indexed` hygiene hints) must not block emission — only an
                // error does. Partition first, then gate on error severity alone.
                let blocks_emission = decl_errs.iter().any(|e| {
                    matches!(
                        bynk_syntax::Severity::for_error(e),
                        bynk_syntax::Severity::Error
                    )
                });
                errors.extend_for(Some(&pf.identity_path), decl_errs);
                if blocks_emission {
                    // ADR 0094: handler bodies are typed here — surface their
                    // best-effort types in Analyse mode even when a declaration check
                    // (e.g. a service/agent wiring error) fails for the file.
                    if mode == Mode::Analyse {
                        record_analyse_types(
                            exprs,
                            &pf.identity_path,
                            pf.synthetic,
                            &typed.expr_types,
                        );
                    }
                    continue;
                }
                // Warnings only: the declarations are valid — fall through to emit.
            }
        }

        // Analyse mode stops at checked: emission is build-only. Capture the
        // file's expression types on the way out (Ok path only — this point is
        // past every per-file error `continue`), for `.`-member completion.
        if mode == Mode::Analyse {
            record_analyse_types(exprs, &pf.identity_path, pf.synthetic, &typed.expr_types);
            continue;
        }
        emit_unit(
            name,
            kind,
            i,
            pf,
            indices,
            parsed,
            unit_info,
            imported_from,
            imported_from_kind,
            owning_context_for_emit,
            consumed_types,
            &cross_context_for_file,
            &typed,
            target,
            import_ext,
            contracts,
            agent_deps_plan,
            compiled,
        );
    }
}

/// The outcome of the shared check pipeline (regions 1+2's shared work),
/// before either entry point applies its own divergent exit. The two typed
/// entry points (`compile_project`, `analyse_project`) project this into a
/// `Result<ProjectOutput, ProjectFailure>` or a `ProjectAnalysis`.
#[allow(clippy::large_enum_variant)]
enum RunChecks {
    /// Discovery/parse failed, or (build mode) the structural gate bailed:
    /// only diagnostics, no checked program. Index is not assembled here.
    Bailed {
        errors: ErrorSink,
        snapshots: Vec<(PathBuf, String)>,
        hints: HintSink,
        locals: LocalsSink,
        exprs: ExprTypeSink,
        requirements: RequirementSink,
    },
    /// All phases ran (per-unit checks + tests + platform-lock done).
    Checked {
        errors: ErrorSink,
        snapshots: Vec<(PathBuf, String)>,
        refs: RefSink,
        hints: HintSink,
        locals: LocalsSink,
        exprs: ExprTypeSink,
        requirements: RequirementSink,
        parsed: Vec<ParsedFile>,
        compiled: Vec<CompiledFile>,
        runnable_tests: Vec<RunnableTest>,
        integration_outputs: Vec<CompiledFile>,
        integration_runnables: Vec<RunnableTest>,
        groups: HashMap<String, Vec<usize>>,
        kinds: HashMap<String, UnitKind>,
        unit_uses: HashMap<String, Vec<String>>,
        unit_consumes: HashMap<String, Vec<String>>,
        unit_consumes_aliases: HashMap<String, HashMap<String, String>>,
        unit_tables: HashMap<String, UnitTable>,
        unit_flattened: HashMap<String, HashMap<String, String>>,
        adapter_bindings: HashMap<String, AdapterBinding>,
        npm_deps: std::collections::BTreeMap<String, String>,
        target: BuildTarget,
    },
}

#[allow(clippy::too_many_arguments)]
fn run_checks(
    src_root: &Path,
    tests_root: &Path,
    // Slice 0: the primary tree's project-root-relative `include` prefix, empty
    // for a single root — builds `identity_path` alongside `tests_prefix`.
    src_prefix: &Path,
    tests_prefix: &Path,
    target: BuildTarget,
    platform: Platform,
    import_ext: ImportExt,
    mode: Mode,
    overlay: &HashMap<PathBuf, String>,
    // v0.113: absolute subtrees to skip during discovery (author `exclude` plus
    // the tool's `out`/`node_modules` caches). Empty for in-memory builds.
    excludes: &[PathBuf],
    // v0.108 (in-browser track, slice 3): when `Some`, the source files are
    // supplied directly — `(src_files, tests_files)` — and filesystem discovery
    // is skipped. The wasm/REPL entry feeds an in-memory single-module project
    // this way (the source itself rides in `overlay`); `None` keeps the on-disk
    // discovery walk for the CLI and the LSP.
    discovered: Option<(Vec<PathBuf>, Vec<PathBuf>)>,
    // v0.115: emit the function-contract call-site guard (dev/test profile).
    contracts: bool,
) -> RunChecks {
    let mut errors = ErrorSink::new();
    // v0.25 (ADR 0053): binding edges, recorded at the resolution sites and
    // assembled into the project index at the analyse exit.
    let mut refs = RefSink::new();
    // v0.27 (ADR 0056): inferred-type inlay hints, recorded at the checker's
    // binding sites. A sink (not part of the checker's Ok payload) so hints
    // survive the per-file error-`continue`s.
    let mut hints = HintSink::new();
    let mut locals = LocalsSink::new();
    // v0.99: the capability-requirement ledger — recorded at the checker's
    // capability-consuming sites, drained at the analyse exit for the LSP.
    let mut requirements = RequirementSink::new();
    // v0.30.2 (ADR 0063): per-file expression types, captured on the Ok path so
    // `.`-member completion can type a receiver. Carried like `hints`.
    let mut exprs = ExprTypeSink::new();
    let mut snapshots: Vec<(PathBuf, String)> = Vec::new();
    let split_mode = src_root != tests_root;

    // -- 1. Discovery (skipped when sources are supplied in memory). --
    let (src_files, tests_files) = match discovered {
        Some(files) => files,
        None => match phase_discovery(src_root, tests_root, split_mode, excludes, &mut errors) {
            Ok(files) => files,
            Err(()) => {
                return RunChecks::Bailed {
                    errors,
                    snapshots,
                    hints,
                    locals,
                    exprs,
                    requirements,
                };
            }
        },
    };

    // -- 2. Parse every file. --
    let (mut parsed, consumes_bynk, consumes_cloudflare) = match phase_parse(
        src_root,
        tests_root,
        src_prefix,
        tests_prefix,
        split_mode,
        &src_files,
        &tests_files,
        overlay,
        &mut errors,
        &mut snapshots,
    ) {
        Ok(out) => out,
        Err(()) => {
            return RunChecks::Bailed {
                errors,
                snapshots,
                hints,
                locals,
                exprs,
                requirements,
            };
        }
    };

    // -- 2b. Normalize service-level `by`/`given` defaults (v0.155). A service
    //        header default is injected into every handler that omits its own
    //        clause, so every downstream phase (grouping, checking, validation,
    //        emission) reads canonical handlers with no special-casing. `parsed`
    //        is indexed (not cloned) by later phases, so mutating it here reaches
    //        them all. The parsed AST that `bynk fmt` produces is untouched (it
    //        parses independently), so the terse inheriting source round-trips.
    normalize_service_defaults(&mut parsed);
    let parsed = parsed;

    // -- 3. Group by (name, kind) and validate per-directory consistency. --
    let (groups, kinds, test_groups, integration_groups, adapter_bindings, npm_deps) = phase_group(
        &parsed,
        src_root,
        platform,
        consumes_bynk,
        consumes_cloudflare,
        &mut errors,
    );

    // -- 4. Build per-unit combined symbol tables. --
    let unit_tables = phase_symbol_tables(&groups, &kinds, &parsed, &mut errors);

    // -- 5. Resolve `uses` clauses (target must exist + be a commons). --
    let unit_uses = phase_resolve_uses(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5b. Resolve `consumes` clauses (target must exist + be a context). --
    let (unit_consumes, unit_flattened) = phase_resolve_consumes(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 5b'. Collect `consumes` aliases (v0.6 §3.1). Each consuming context
    //         has an alias map: alias → consumed-context qualified name.
    //         Detect alias-alias conflicts here; alias-vs-local-decl conflicts
    //         are checked once the local symbol tables are built (step 6+).
    let unit_consumes_aliases =
        phase_consumes_aliases(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5b''. v0.173 (ADR 0196 D1): warn where a `bynk.Secrets` read names its
    //          secret with a computed expression. Non-failing — the program is
    //          correct, `deploy` simply cannot see the name (D1) — and raised
    //          here rather than at emit so it reaches the **editor**: `bynk
    //          check` and the LSP run this path and never build.
    //
    //          Gated on the Workers target because the whole consequence is
    //          about `bynk deploy`, which no other target has; warning a bundle
    //          project about a deploy plan it will never produce would be noise.
    //          Walked per **file** rather than per unit: `extend_for` attributes
    //          a diagnostic to a path, and a `UnitTable` has merged a context's
    //          files away — so a squiggle would land on the project instead of
    //          the call site.
    if target == BuildTarget::Workers {
        for (name, indices) in &groups {
            if kinds.get(name) != Some(&UnitKind::Context) {
                continue;
            }
            let Some(flattened) = unit_flattened.get(name) else {
                continue;
            };
            for &i in indices {
                let bynk_syntax::ast::SourceUnit::Context(ctx) = &parsed[i].unit else {
                    continue;
                };
                let handlers = ctx.items.iter().filter_map(|item| match item {
                    bynk_syntax::ast::CommonsItem::Service(s) => Some(s.handlers.iter()),
                    _ => None,
                });
                let (_, warnings) =
                    crate::emitter::secrets::secret_reads_of(handlers.flatten(), flattened);
                let rel = parsed[i].identity_path.clone();
                errors.extend_for(Some(&rel), warnings);
            }
        }
    }

    // -- 5c. Detect `consumes` cycles. --
    // #696: record a representative `consumes`-clause site (file + span) per unit
    // so a detected cycle anchors on a real clause and renders with source
    // context. First non-synthetic clause wins; synthetic units are left out so
    // their (snapshot-less) files never claim a diagnostic.
    let mut consumes_sites: HashMap<String, (PathBuf, Span)> = HashMap::new();
    for (name, indices) in &groups {
        for &i in indices {
            if parsed[i].synthetic {
                continue;
            }
            if let Some(c) = parsed[i].consumes().first() {
                consumes_sites
                    .entry(name.clone())
                    .or_insert_with(|| (parsed[i].identity_path.clone(), c.span));
            }
        }
    }
    let mut cycle_errors: Vec<(Option<PathBuf>, CompileError)> = Vec::new();
    detect_consumes_cycles(&unit_consumes, &consumes_sites, &mut cycle_errors);
    for (path, err) in cycle_errors {
        errors.push_for(path.as_deref(), err);
    }

    // -- 6. Name-conflict detection for uses imports (commons-only check). --
    phase_uses_name_conflicts(&unit_uses, &unit_tables, &parsed, &groups, &mut errors);

    // -- 6b. Validate exports clauses (each name is a locally-declared type;
    //         no duplicates within or across opaque/transparent). --
    let exports_visibility = phase_validate_type_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6b'. Validate `exports capability { … }` clauses (v0.15 §4.1): each
    //          name must be a capability the context declares *and* provides. --
    phase_validate_capability_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6c. Validate that providers match their capabilities exactly. --
    phase_validate_providers(&unit_tables, &groups, &parsed, &mut errors);

    if !errors.is_empty() && mode == Mode::Build {
        return RunChecks::Bailed {
            errors,
            snapshots,
            hints,
            locals,
            exprs,
            requirements,
        };
    }

    // -- 7. Build per-unit file index (which file declares which name). --
    let unit_file_index = phase_file_index(&groups, &parsed);

    // -- 7b (v0.29.4). Assemble the nine parallel per-unit maps into one record
    //          per unit. Driven by the `groups` keyset (the authority), so every
    //          group yields exactly one `UnitInfo` with all facets present. The
    //          producer maps are cloned, not moved, because the back half of the
    //          pipeline (tests, integration tests, platform-lock, composition
    //          root, the workers branch) still reads the originals.
    let unit_info = assemble_unit_info(
        &groups,
        &kinds,
        &unit_tables,
        &unit_uses,
        &unit_consumes,
        &unit_flattened,
        &unit_consumes_aliases,
        &exports_visibility,
        &unit_file_index,
    );

    // -- 8. For each unit, build the combined symbol space and run
    //       resolve+check per source file. --
    let mut compiled: Vec<CompiledFile> = Vec::new();

    for (name, info) in &unit_info {
        let kind = info.kind;
        let indices = info.files.as_slice();
        let local_table = &info.table;
        // v0.24: skip resolve/check only when THIS group's composition
        // failed. In build mode the sink is empty here (the structural gate
        // bailed), so the delta equals the old global is_empty check; in
        // analyse mode one broken unit no longer suppresses every other
        // unit's semantic diagnostics.
        let group_error_baseline = errors.len();

        let (
            mut combined_types,
            combined_fns,
            mut combined_methods,
            mut imported_from,
            mut imported_from_kind,
        ) = compose_unit_symbols(name, local_table, &unit_info);
        let consumed_types = merge_consumed_exports(
            name,
            &parsed,
            &unit_info,
            &mut combined_types,
            &mut combined_methods,
            &mut imported_from,
            &mut imported_from_kind,
            &mut errors,
        );

        if errors.len() > group_error_baseline {
            continue;
        }

        let local_names: HashSet<String> = local_table.types.keys().cloned().collect();

        let local_methods_for_type = collect_unit_methods(indices, &parsed);

        // Per-context view information for the emitter and checker.
        let owning_context_for_emit = if kind == UnitKind::Context {
            Some(name.clone())
        } else {
            None
        };

        // #527: workers contexts get a DO-side deps plan for their agents'
        // `given` capabilities (the wire cannot carry providers).
        let agent_deps_plan = if matches!(target, BuildTarget::Workers) && kind == UnitKind::Context
        {
            plan_agent_given_deps(name, &unit_info, &adapter_bindings)
        } else {
            None
        };

        check_unit_files(
            name,
            kind,
            indices,
            &parsed,
            &unit_info,
            &combined_types,
            &combined_fns,
            &combined_methods,
            &local_names,
            &local_methods_for_type,
            &consumed_types,
            &imported_from,
            &imported_from_kind,
            &owning_context_for_emit,
            target,
            import_ext,
            contracts,
            agent_deps_plan.as_ref(),
            mode,
            &mut errors,
            &mut refs,
            &mut hints,
            &mut locals,
            &mut exprs,
            &mut requirements,
            &mut compiled,
        );
    }

    // v0.7: process test declarations. Each `test commerce.X` group resolves
    // its target, validates mocks against the target's capability/consumed-
    // context shapes, type-checks bodies with the target's privileged view,
    // and emits a per-target TypeScript test module under `tests/`.
    let mut test_errors: Vec<CompileError> = Vec::new();
    // v0.132: barrel output paths emitted so far, shared across the unit- and
    // integration-test passes so a multi-file commons imported by both is
    // aggregated into `out/<name>.ts` exactly once.
    let mut emitted_barrels: HashSet<PathBuf> = HashSet::new();
    let (test_outputs, runnable_tests) = process_tests(
        &test_groups,
        &parsed,
        &kinds,
        &unit_tables,
        &exports_visibility,
        &unit_consumes,
        &unit_consumes_aliases,
        &unit_uses,
        &groups,
        tests_prefix,
        import_ext,
        contracts,
        &mut emitted_barrels,
        &mut test_errors,
        &mut refs,
    );
    // #696: test-suite diagnostics do have owning files, but attributing them
    // means threading a file through `process_tests`'s many internal push sites —
    // a separable follow-up. They render in the plain `[category]` form for now.
    errors.extend_for(None, test_errors);

    compiled.extend(test_outputs);

    // v0.16: process integration tests. Each `test integration "name"` suite
    // validates its `wires` participants, type-checks each case body as a
    // cross-context call from a synthetic harness root that consumes every
    // participant, and emits a TypeScript module that stands the participants
    // up as in-process Workers and exercises the flow across the real wire.
    let mut integration_errors: Vec<CompileError> = Vec::new();
    let (integration_outputs, integration_runnables) = process_integration_tests(
        &integration_groups,
        &parsed,
        &kinds,
        &unit_tables,
        &unit_consumes,
        &unit_consumes_aliases,
        &unit_uses,
        &groups,
        tests_prefix,
        &mut emitted_barrels,
        &mut integration_errors,
        &mut refs,
    );
    // #696: integration-suite diagnostics, like the unit-test ones above, stay
    // unattributed pending the same `process_integration_tests` threading.
    errors.extend_for(None, integration_errors);

    // v0.19 (decisions 0017/0024): platform-lock enforcement. A deployment
    // unit whose in-process closure reaches a platform-native capability is
    // locked to that platform; the selected `--platform` must match. Run only
    // on otherwise-clean programs: the closure walk recurses the provider
    // graph, whose acyclicity the earlier checks establish.
    if errors.is_empty() {
        let mut lock_errors: Vec<CompileError> = Vec::new();
        check_platform_lock(
            target,
            platform,
            &parsed,
            &groups,
            &kinds,
            &unit_tables,
            &unit_consumes,
            &unit_consumes_aliases,
            &unit_flattened,
            &mut lock_errors,
        );
        errors.extend_for(None, lock_errors);
    }

    // v0.176 (#642): the `Bytes`-at-a-workers-boundary guard (ADR 0142 D8) is
    // retired here. It existed because the workers boundary carried its own
    // codec dispatch, which cast a `Bytes` to `JsonValue` on the way out while
    // base64-decoding it on the way in — so a `Bytes` mis-round-tripped, and a
    // diagnostic was better than silent corruption. That dispatch is gone: every
    // wire position now routes through `serialisation.rs`, whose `Bytes` arm
    // base64-encodes. The restriction has no remaining cause, and ADR 0142 D8's
    // deferral to "the roadmap's typed cross-context boundary fix" is discharged.

    RunChecks::Checked {
        errors,
        snapshots,
        refs,
        hints,
        locals,
        exprs,
        requirements,
        parsed,
        compiled,
        runnable_tests,
        integration_outputs,
        integration_runnables,
        groups,
        kinds,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
        unit_tables,
        unit_flattened,
        adapter_bindings,
        npm_deps,
        target,
    }
}

/// Build-success tail (region 3): emit the composition/worker/runtime files
/// and assemble the final `ProjectOutput`. Reached only on build mode with a
/// clean error sink. Moved verbatim from the old pipeline; only the locals it
/// reads are now bound from the `Checked` variant.
#[allow(clippy::too_many_arguments)]
fn build_output(
    mut compiled: Vec<CompiledFile>,
    mut runnable_tests: Vec<RunnableTest>,
    integration_outputs: Vec<CompiledFile>,
    integration_runnables: Vec<RunnableTest>,
    groups: HashMap<String, Vec<usize>>,
    kinds: HashMap<String, UnitKind>,
    unit_consumes: HashMap<String, Vec<String>>,
    unit_consumes_aliases: HashMap<String, HashMap<String, String>>,
    unit_tables: HashMap<String, UnitTable>,
    // v0.177 (#643): needed to build each context's *own* combined type table,
    // so its contract hashes are computed from the same namespace a caller sees.
    unit_uses: HashMap<String, Vec<String>>,
    unit_flattened: HashMap<String, HashMap<String, String>>,
    adapter_bindings: HashMap<String, AdapterBinding>,
    npm_deps: std::collections::BTreeMap<String, String>,
    target: BuildTarget,
    import_ext: ImportExt,
) -> ProjectOutput {
    compiled.extend(integration_outputs);
    runnable_tests.extend(integration_runnables);

    // v0.67: the discovery manifest — built from the combined runnable set before
    // anything consumes it, so `--no-run --format json` lists suites/cases without
    // running. Ordered by the runner's sort key to match a run's suite order.
    let discovered = discovery_manifest(&runnable_tests);

    // v0.16: emit the combined top-level test runner once both passes are done,
    // so `tests/main.ts` aggregates unit and integration suites together.
    if !runnable_tests.is_empty() {
        let main_ts = emit_test_main(&runnable_tests, import_ext);
        compiled.push(CompiledFile {
            source_path: PathBuf::from("tests/main.test.bynk"),
            output_path: PathBuf::from("tests/main.ts"),
            typescript: main_ts,
            source_map: None,
            debug_metadata: None,
        });
    }

    // v0.19 (decision 0025): does any context's in-process closure reach a
    // platform-native unit? Drives env threading (bundle) and the per-Worker
    // Env/`wrangler.toml` resource derivation (workers).
    let context_native: HashMap<String, std::collections::BTreeMap<Platform, String>> = kinds
        .iter()
        .filter(|(_, k)| **k == UnitKind::Context)
        .filter_map(|(name, _)| {
            let table = unit_tables.get(name)?;
            let native = native_platforms_of_context(
                name,
                table,
                &unit_tables,
                &unit_consumes,
                &unit_consumes_aliases,
                &unit_flattened,
            );
            (!native.is_empty()).then(|| (name.clone(), native))
        })
        .collect();

    match target {
        BuildTarget::Bundle => {
            // v0.6 §6.3: emit a composition root when the project has at
            // least one context that consumes another context's service
            // surface. The compose file imports each context, instantiates
            // its providers, assembles its deps (capabilities + cross-
            // context surfaces), and exports the top-level service surface.
            if let Some(compose_ts) = emit_composition_root(
                &groups,
                &kinds,
                &unit_consumes,
                &unit_consumes_aliases,
                &unit_tables,
                &adapter_bindings,
                &unit_flattened,
                // D1: thread `env` through composeApp only when a native
                // resource is consumed, so native-free programs are
                // byte-identical to v0.18 output.
                !context_native.is_empty(),
            ) {
                compiled.push(CompiledFile {
                    source_path: PathBuf::from("compose.bynk"),
                    output_path: PathBuf::from("compose.ts"),
                    typescript: compose_ts,
                    source_map: None,
                    debug_metadata: None,
                });
            }
        }
        BuildTarget::Workers => {
            // v0.8 §2.3: per-Worker entry point, compose.ts, and wrangler
            // configuration. One Worker per context.
            for (ctx_name, kind) in &kinds {
                if *kind != UnitKind::Context {
                    continue;
                }
                let Some(table) = unit_tables.get(ctx_name) else {
                    continue;
                };
                let dashes = worker_dir_name(ctx_name);
                let consumes_targets = unit_consumes.get(ctx_name).cloned().unwrap_or_default();
                let aliases = unit_consumes_aliases
                    .get(ctx_name)
                    .cloned()
                    .unwrap_or_default();
                // v0.177 (#643): the callee's own view of each of its `on call`
                // contracts, hashed from *its own* namespace — the same table
                // (`combined_types_for`) a caller reaches through
                // `consumed_types[ctx_name]`, so the two sides cannot disagree.
                let own_types =
                    crate::project::symbols::combined_types_for(ctx_name, &unit_tables, &unit_uses);
                let own_contracts = own_contract_hashes(table, &own_types);
                let entry_ts = emitter::emit_worker_entry(ctx_name, table, &own_contracts);
                let binding_modules: HashMap<String, String> = adapter_bindings
                    .iter()
                    .map(|(n, b)| {
                        (
                            n.clone(),
                            emitter::ts_specifier(&b.output_path.with_extension("js")),
                        )
                    })
                    .collect();
                let flattened = unit_flattened.get(ctx_name).cloned().unwrap_or_default();
                // v0.19 (C1): this Worker needs the KV namespace binding when
                // its in-process closure reaches the cloudflare adapter.
                let needs_kv = context_native
                    .get(ctx_name)
                    .is_some_and(|n| n.values().any(|u| u == firstparty::CLOUDFLARE_UNIT));
                let compose_ts = emitter::emit_worker_compose(
                    ctx_name,
                    table,
                    &consumes_targets,
                    &aliases,
                    &unit_tables,
                    &binding_modules,
                    &flattened,
                    &unit_consumes,
                    &unit_consumes_aliases,
                    &unit_flattened,
                    needs_kv,
                );
                // Adapters are not Workers, so they get no Service Binding in
                // the consumer's wrangler config — drop them from the list.
                let service_consumes: Vec<String> = consumes_targets
                    .iter()
                    .filter(|t| !binding_modules.contains_key(*t))
                    .cloned()
                    .collect();
                let wrangler =
                    emitter::emit_wrangler_toml(ctx_name, table, &service_consumes, needs_kv);
                compiled.push(CompiledFile {
                    source_path: PathBuf::from(format!("workers/{dashes}/<index>")),
                    output_path: PathBuf::from(format!("workers/{dashes}/index.ts")),
                    typescript: entry_ts,
                    source_map: None,
                    debug_metadata: None,
                });
                compiled.push(CompiledFile {
                    source_path: PathBuf::from(format!("workers/{dashes}/<compose>")),
                    output_path: PathBuf::from(format!("workers/{dashes}/compose.ts")),
                    typescript: compose_ts,
                    source_map: None,
                    debug_metadata: None,
                });
                compiled.push(CompiledFile {
                    source_path: PathBuf::from(format!("workers/{dashes}/<wrangler>")),
                    output_path: PathBuf::from(format!("workers/{dashes}/wrangler.toml")),
                    typescript: wrangler,
                    source_map: None,
                    debug_metadata: None,
                });
                // v0.172 (ADR 0195 D5): the secret names this Worker's handlers
                // will read from `env`, for `deploy` to check before it pushes.
                // Emitted from the same seams the entry lowers, so the two
                // cannot describe different Workers.
                //
                // v0.173 (ADR 0196): plus the literal `bynk.Secrets` names it
                // reads, and whether that list is everything. The walk is here
                // rather than in the checker because it needs `unit_flattened`
                // to answer *whose* `Secrets` this is (D4) and the warning sink
                // to say when it cannot know a name — and `bynk-check` has
                // neither. Absent when there is nothing at all to say (D5).
                // The warnings half is dropped here: `run_checks` already raised
                // it, on the analyse path the editor shares.
                // v0.177 (#643): the contract hashes this context's entry
                // enforces, written where `deploy` can read them — so a skew is
                // refused at the push rather than discovered by live traffic.
                let own_types =
                    crate::project::symbols::combined_types_for(ctx_name, &unit_tables, &unit_uses);
                // What this context expects of each dependency — the same hash
                // it stamps at each call site, computed the same way: in the
                // *dependency's* namespace, from the dependency's own table.
                //
                // Only the services this context actually **calls**. Recording
                // everything the dependency provides would refuse a deploy over a
                // service this caller never touches — a skew its runtime check
                // could never fire on. See `called_cross_context_services`.
                let called = called_cross_context_services(
                    table,
                    unit_consumes
                        .get(ctx_name)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    &aliases,
                );
                let mut expects: std::collections::BTreeMap<
                    String,
                    std::collections::BTreeMap<String, String>,
                > = std::collections::BTreeMap::new();
                for (dep, services) in &called {
                    let Some(dep_table) = unit_tables.get(dep) else {
                        continue;
                    };
                    let dep_types =
                        crate::project::symbols::combined_types_for(dep, &unit_tables, &unit_uses);
                    let all = own_contract_hashes(dep_table, &dep_types);
                    let hashes: std::collections::BTreeMap<String, String> = all
                        .into_iter()
                        .filter(|(svc, _)| services.contains(svc))
                        .collect();
                    if !hashes.is_empty() {
                        expects.insert(dep.clone(), hashes);
                    }
                }
                if let Some(manifest) = emitter::contracts::emit_contracts_manifest(
                    &own_contract_hashes(table, &own_types),
                    &expects,
                ) {
                    compiled.push(CompiledFile {
                        source_path: PathBuf::from(format!("workers/{dashes}/<contracts>")),
                        output_path: PathBuf::from(format!(
                            "workers/{dashes}/{}",
                            emitter::contracts::CONTRACTS_MANIFEST
                        )),
                        typescript: manifest,
                        source_map: None,
                        debug_metadata: None,
                    });
                }

                let (reads, _) = emitter::secrets::secret_reads(table, &flattened);
                if let Some(manifest) = emitter::emit_secrets_manifest(table, &reads) {
                    compiled.push(CompiledFile {
                        source_path: PathBuf::from(format!("workers/{dashes}/<secrets>")),
                        output_path: PathBuf::from(format!(
                            "workers/{dashes}/{}",
                            emitter::secrets::SECRETS_MANIFEST
                        )),
                        typescript: manifest,
                        source_map: None,
                        debug_metadata: None,
                    });
                }
            }
        }
    }

    // v0.17: copy each adapter binding verbatim into the output, beside the
    // adapter's emitted interface module, so compose's import resolves and the
    // `tsc` gate checks the `implements` contract.
    let mut binding_names: Vec<&String> = adapter_bindings.keys().collect();
    binding_names.sort();
    for name in binding_names {
        let b = &adapter_bindings[name];
        compiled.push(CompiledFile {
            source_path: b.output_path.clone(),
            output_path: b.output_path.clone(),
            typescript: b.content.clone(),
            source_map: None,
            debug_metadata: None,
        });
    }

    // v0.17: emit `package.json` only when an adapter declares npm deps, so
    // existing (adapter-free) projects are unchanged.
    if !npm_deps.is_empty() {
        compiled.push(CompiledFile {
            source_path: PathBuf::from("<package.json>"),
            output_path: PathBuf::from("package.json"),
            typescript: render_package_json(&npm_deps),
            source_map: None,
            debug_metadata: None,
        });
    }

    // Runtime + tsconfig: emit once per project. The runtime sits at the
    // root of `out/` so every emitted file's `runtime.js` import resolves
    // relative to it. `tsconfig.json` is also at the root so `tsc -p out/
    // tsconfig.json` discovers every `.ts` file in the tree.
    compiled.push(CompiledFile {
        source_path: PathBuf::from("<runtime>"),
        output_path: PathBuf::from("runtime.ts"),
        typescript: emitter::emit_runtime_module(),
        source_map: None,
        debug_metadata: None,
    });
    compiled.push(CompiledFile {
        source_path: PathBuf::from("<tsconfig>"),
        output_path: PathBuf::from("tsconfig.json"),
        typescript: emitter::emit_tsconfig(),
        source_map: None,
        debug_metadata: None,
    });

    compiled.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    ProjectOutput {
        files: compiled,
        discovered,
        // Populated by `compile_project` from the run's warning sink (ADR 0117).
        warnings: Vec::new(),
    }
}

/// Build a project-level composition root that wires every context's
/// providers and cross-context surfaces together. Returns `None` if the
/// project has no cross-context wiring to glue.
/// Resolve a `given` prefix (alias or qualified context name) to a consumed
/// context, using one context's `consumes`/alias tables (v0.15).
fn resolve_consume_prefix(
    prefix: &str,
    consumed: &[String],
    aliases: &HashMap<String, String>,
) -> Option<String> {
    if let Some(q) = aliases.get(prefix) {
        return Some(q.clone());
    }
    if consumed.iter().any(|c| c == prefix) {
        return Some(prefix.to_string());
    }
    None
}

/// v0.15: the cross-context capabilities a context's **handlers** reference,
/// as `deps_key → consumed_context`. These become top-level deps fields.
fn handler_cross_caps(
    table: &UnitTable,
    consumed: &[String],
    aliases: &HashMap<String, String>,
    flattened: &HashMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut scan = |given: &[CapRef]| {
        for c in given {
            if let Some(p) = c.prefix() {
                if let Some(ctx) = resolve_consume_prefix(&p, consumed, aliases) {
                    out.entry(c.key().to_string()).or_insert(ctx);
                }
            } else if let Some(unit) = flattened.get(c.key()) {
                // v0.17: a bare flattened capability is provided by the unit it
                // was flattened from.
                out.entry(c.key().to_string())
                    .or_insert_with(|| unit.clone());
            }
        }
    };
    for s in table.services.values() {
        for h in &s.handlers {
            scan(&h.given);
        }
    }
    for a in table.agents.values() {
        for h in &a.handlers {
            scan(&h.given);
        }
    }
    out
}

/// v0.19 (decision 0017): the native platforms a context's **in-process
/// closure** commits it to: every unit whose provider its compose would
/// instantiate — local providers' `given` recursion plus the capabilities its
/// handlers reference — mapped through [`firstparty::platform_of`]. Each
/// platform carries an exemplar unit for the diagnostic message. Service
/// `consumes` edges (RPC under `workers`) do not contribute — only the
/// provider-instantiation walk, which is in-process by construction.
#[allow(clippy::too_many_arguments)]
fn native_platforms_of_context(
    ctx: &str,
    table: &UnitTable,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
) -> std::collections::BTreeMap<Platform, String> {
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for cap in table.providers.keys() {
        let _ = instantiate_provider_expr(
            ctx,
            cap,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_flattened,
            false,
            None,
            &mut referenced,
        );
    }
    let consumed = unit_consumes.get(ctx).cloned().unwrap_or_default();
    let aliases = unit_consumes_aliases.get(ctx).cloned().unwrap_or_default();
    let flattened = unit_flattened.get(ctx).cloned().unwrap_or_default();
    for (key, cctx) in handler_cross_caps(table, &consumed, &aliases, &flattened) {
        let _ = instantiate_provider_expr(
            &cctx,
            &key,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_flattened,
            false,
            None,
            &mut referenced,
        );
    }
    let mut out = std::collections::BTreeMap::new();
    for unit in referenced {
        if let Some(p) = bynk_check::firstparty::platform_of(&unit) {
            out.entry(p).or_insert(unit);
        }
    }
    out
}

/// #527: the DO-side deps plan for one workers context. Capability providers
/// cannot cross the DO wire (`{ args, deps }` is JSON — a provider's methods
/// die in serialisation), so the generated Durable Object reconstructs its
/// agents' `given` deps *inside* the DO from the same wiring compose uses.
#[derive(Debug, Default, Clone)]
pub struct AgentDepsPlan {
    /// agent name → TS object-literal expression for its `given` deps
    /// (e.g. `{ Clock: new bynk__binding.ClockProvider() }`).
    pub exprs: HashMap<String, String>,
    /// Import lines the expressions need in `handlers.ts` (binding modules,
    /// other Workers' handlers). Same relative depth as `compose.ts`.
    pub imports: Vec<String>,
}

/// Build the [`AgentDepsPlan`] for context `name`, or `None` when no local
/// agent has `given` capabilities.
fn plan_agent_given_deps(
    name: &str,
    unit_info: &HashMap<String, UnitInfo>,
    adapter_bindings: &HashMap<String, AdapterBinding>,
) -> Option<AgentDepsPlan> {
    let info = unit_info.get(name)?;
    info.table.agents.values().next()?;
    let unit_tables: HashMap<String, UnitTable> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.table.clone()))
        .collect();
    let unit_consumes: HashMap<String, Vec<String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.consumes.clone()))
        .collect();
    let unit_consumes_aliases: HashMap<String, HashMap<String, String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.aliases.clone()))
        .collect();
    let unit_flattened: HashMap<String, HashMap<String, String>> = unit_info
        .iter()
        .map(|(n, i)| (n.clone(), i.flattened.clone()))
        .collect();

    let mut referenced: BTreeSet<String> = BTreeSet::new();
    let mut exprs: HashMap<String, String> = HashMap::new();
    let mut agents: Vec<(&String, &bynk_syntax::ast::AgentDecl)> =
        info.table.agents.iter().collect();
    agents.sort_by_key(|(n, _)| (*n).clone());
    for (agent, a) in agents {
        let mut caps: std::collections::BTreeMap<String, bynk_syntax::ast::CapRef> =
            std::collections::BTreeMap::new();
        for h in &a.handlers {
            for g in &h.given {
                caps.entry(g.key().to_string()).or_insert_with(|| g.clone());
            }
        }
        if caps.is_empty() {
            continue;
        }
        let parts: Vec<String> = caps
            .iter()
            .map(|(key, g)| {
                let target_ctx = match g.prefix() {
                    Some(p) => resolve_consume_prefix(&p, &info.consumes, &info.aliases)
                        .unwrap_or_else(|| name.to_string()),
                    None => info
                        .flattened
                        .get(key.as_str())
                        .cloned()
                        .unwrap_or_else(|| name.to_string()),
                };
                let expr = instantiate_provider_expr(
                    &target_ctx,
                    key,
                    &unit_tables,
                    &unit_consumes,
                    &unit_consumes_aliases,
                    &unit_flattened,
                    true,
                    Some("env"),
                    &mut referenced,
                );
                format!("{key}: {expr}")
            })
            .collect();
        exprs.insert(agent.clone(), format!("{{ {} }}", parts.join(", ")));
    }
    if exprs.is_empty() {
        return None;
    }
    // Providers of *this* context live in the same module (`handlers.ts`), so
    // their compose-namespace prefix drops.
    let self_ns = format!("handlers_{}.", name.replace('.', "_"));
    for e in exprs.values_mut() {
        *e = e.replace(&self_ns, "");
    }
    referenced.remove(name);
    let mut imports = Vec::new();
    for u in &referenced {
        let ns = u.replace('.', "_");
        if let Some(b) = adapter_bindings.get(u) {
            let module = crate::emitter::ts_specifier(&b.output_path.with_extension("js"));
            imports.push(format!(
                "import * as {ns}__binding from \"../../{module}\";"
            ));
        } else {
            let dir = worker_dir_name(u);
            imports.push(format!(
                "import * as handlers_{ns} from \"../{dir}/handlers.js\";"
            ));
        }
    }
    Some(AgentDepsPlan { exprs, imports })
}

/// v0.15: build the TypeScript expression instantiating the provider of
/// capability `cap` declared in `provider_ctx`, recursively wiring its `given`
/// dependencies — local sibling providers and cross-context capability
/// providers alike. Stateless providers, so fresh instances per use are fine.
///
/// v0.18 (spec §4.5/§5.1): a *bare* `given` name resolves through the
/// provider's own unit's flattened-capability map (`Fetch` → `bynk`), falling
/// back to the unit itself; an *external* provider's deps are built the same
/// way and passed to the binding class constructor by name. Every unit whose
/// namespace the expression references is recorded in `referenced_units` so
/// the caller can emit the matching imports (the transitive given-closure).
///
/// `workers_ns` selects the namespace convention: a bodied provider's class
/// lives in `{ns}` under the bundle root but `handlers_{ns}` in a Worker
/// compose; external (binding) classes are `{ns}__binding` in both. When
/// `env_ident` is set (workers), env-taking first-party providers receive it
/// as a constructor argument.
#[allow(clippy::too_many_arguments)]
pub(crate) fn instantiate_provider_expr(
    provider_ctx: &str,
    cap: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    workers_ns: bool,
    env_ident: Option<&str>,
    referenced_units: &mut BTreeSet<String>,
) -> String {
    let ns = provider_ctx.replace('.', "_");
    let bodied_ns = if workers_ns {
        format!("handlers_{ns}")
    } else {
        ns.clone()
    };
    referenced_units.insert(provider_ctx.to_string());
    let Some(provider) = unit_tables
        .get(provider_ctx)
        .and_then(|t| t.providers.get(cap))
    else {
        return format!("new {bodied_ns}.{cap}()");
    };
    // Build the by-name deps object from the provider's `given`, if any.
    let deps_obj = if provider.given.is_empty() {
        None
    } else {
        let consumed = unit_consumes.get(provider_ctx).cloned().unwrap_or_default();
        let aliases = unit_consumes_aliases
            .get(provider_ctx)
            .cloned()
            .unwrap_or_default();
        let flattened = unit_flattened
            .get(provider_ctx)
            .cloned()
            .unwrap_or_default();
        let deps: Vec<String> = provider
            .given
            .iter()
            .map(|g| {
                let target_ctx = match g.prefix() {
                    Some(p) => resolve_consume_prefix(&p, &consumed, &aliases)
                        .unwrap_or_else(|| provider_ctx.to_string()),
                    None => flattened
                        .get(g.key())
                        .cloned()
                        .unwrap_or_else(|| provider_ctx.to_string()),
                };
                let expr = instantiate_provider_expr(
                    &target_ctx,
                    g.key(),
                    unit_tables,
                    unit_consumes,
                    unit_consumes_aliases,
                    unit_flattened,
                    workers_ns,
                    env_ident,
                    referenced_units,
                );
                format!("{}: {}", g.key(), expr)
            })
            .collect();
        Some(format!("{{ {} }}", deps.join(", ")))
    };
    let mut args: Vec<String> = deps_obj.into_iter().collect();
    // v0.18/v0.19: env-taking first-party providers (the bynk surface's
    // SecretsProvider; bynk.cloudflare's WorkersKv) receive the Worker `env`
    // explicitly — decisions 0021/0025. Keyed by (unit, class).
    if provider.external
        && bynk_check::firstparty::provider_takes_env(provider_ctx, &provider.provider_name.name)
        && let Some(env) = env_ident
    {
        args.push(env.to_string());
    }
    let class = &provider.provider_name.name;
    let args = args.join(", ");
    // v0.17: an external (adapter) provider's class lives in the binding module,
    // not the adapter's interface module — instantiate it from the binding
    // namespace (`<adapter>__binding`, imported by the composition root).
    if provider.external {
        format!("new {ns}__binding.{class}({args})")
    } else {
        format!("new {bodied_ns}.{class}({args})")
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_composition_root(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_tables: &HashMap<String, UnitTable>,
    adapter_bindings: &HashMap<String, AdapterBinding>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    // v0.19 (decision 0025, D1): when the program's closure reaches a
    // platform-native unit, composeApp takes an optional `env` and threads it
    // to env-taking first-party providers. A bundle on Cloudflare is a single
    // Worker with `env` at its entry; native-free programs emit the v0.18
    // no-parameter signature unchanged.
    thread_env: bool,
) -> Option<String> {
    // Identify contexts that consume something whose surface has services.
    let mut needs_compose = false;
    for (name, targets) in unit_consumes {
        if !targets.is_empty()
            && let Some(UnitKind::Context) = kinds.get(name)
        {
            for t in targets {
                if let Some(other) = unit_tables.get(t)
                    && !other.services.is_empty()
                {
                    needs_compose = true;
                }
            }
        }
    }
    // v0.15: also compose when a context uses a consumed context's capability
    // (in a handler or in a provider's `given`) — the consumer must instantiate
    // the provided capability's provider locally.
    if !needs_compose {
        for (name, kind) in kinds {
            if *kind != UnitKind::Context {
                continue;
            }
            let Some(table) = unit_tables.get(name) else {
                continue;
            };
            let consumed = unit_consumes.get(name).cloned().unwrap_or_default();
            let aliases = unit_consumes_aliases.get(name).cloned().unwrap_or_default();
            let flattened = unit_flattened.get(name).cloned().unwrap_or_default();
            if !handler_cross_caps(table, &consumed, &aliases, &flattened).is_empty()
                || table.providers.values().any(|p| {
                    p.given.iter().any(|g| {
                        g.is_cross_context()
                            // v0.18: a bare given flattened from `consumes U
                            // { Cap }` is cross-unit too — its provider lives
                            // in the consumed unit.
                            || (g.prefix().is_none() && flattened.contains_key(g.key()))
                    })
                })
            {
                needs_compose = true;
                break;
            }
        }
    }
    if !needs_compose {
        return None;
    }

    let mut contexts: Vec<&String> = groups
        .keys()
        .filter(|n| kinds.get(*n) == Some(&UnitKind::Context))
        .collect();
    contexts.sort();

    // The composeApp body is built first so the provider expressions can
    // record every unit namespace they reference (v0.18: an external
    // provider's `given` may pull in *another* adapter's binding — the
    // transitive given-closure — which must then be imported).
    let mut referenced_units: BTreeSet<String> = BTreeSet::new();
    let mut out = String::new();

    let (compose_params, env_ident) = if thread_env {
        ("env?: unknown", Some("env"))
    } else {
        ("", None)
    };
    out.push_str(&format!(
        "export function composeApp({compose_params}) {{\n"
    ));

    // Build each context's deps and surface in dependency-respecting order:
    // a context that consumes another must come after the consumed context,
    // so its `surface` field can reference the already-built surface.
    let mut ordered: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    fn visit(
        node: &str,
        unit_consumes: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        if visited.contains(node) {
            return;
        }
        visited.insert(node.to_string());
        if let Some(targets) = unit_consumes.get(node) {
            for t in targets {
                visit(t, unit_consumes, visited, out);
            }
        }
        out.push(node.to_string());
    }
    for c in &contexts {
        visit(c, unit_consumes, &mut visited, &mut ordered);
    }

    for ctx_name in &ordered {
        if kinds.get(ctx_name.as_str()) != Some(&UnitKind::Context) {
            continue;
        }
        let Some(table) = unit_tables.get(ctx_name.as_str()) else {
            continue;
        };
        // A context's deps object exists only to feed its `makeSurface`; a
        // capability-only context (no services) needs neither (v0.15).
        if table.services.is_empty() {
            continue;
        }
        let ns = ctx_name.replace('.', "_");

        let mut deps_entries: Vec<String> = table
            .providers
            .keys()
            .map(|cap| {
                format!(
                    "{cap}: {}",
                    instantiate_provider_expr(
                        ctx_name,
                        cap,
                        unit_tables,
                        unit_consumes,
                        unit_consumes_aliases,
                        unit_flattened,
                        false,
                        env_ident,
                        &mut referenced_units,
                    )
                )
            })
            .collect();
        // v0.15: cross-context capabilities used directly by handlers become
        // top-level deps fields, instantiated from the providing context.
        {
            let consumed = unit_consumes
                .get(ctx_name.as_str())
                .cloned()
                .unwrap_or_default();
            let aliases = unit_consumes_aliases
                .get(ctx_name.as_str())
                .cloned()
                .unwrap_or_default();
            let flattened = unit_flattened
                .get(ctx_name.as_str())
                .cloned()
                .unwrap_or_default();
            for (key, cctx) in handler_cross_caps(table, &consumed, &aliases, &flattened) {
                deps_entries.push(format!(
                    "{key}: {}",
                    instantiate_provider_expr(
                        &cctx,
                        &key,
                        unit_tables,
                        unit_consumes,
                        unit_consumes_aliases,
                        unit_flattened,
                        false,
                        env_ident,
                        &mut referenced_units,
                    )
                ));
            }
        }
        deps_entries.sort();

        let mut surface_entries: Vec<String> = Vec::new();
        if let Some(targets) = unit_consumes.get(ctx_name.as_str()) {
            let aliases = unit_consumes_aliases
                .get(ctx_name.as_str())
                .cloned()
                .unwrap_or_default();
            let mut alias_for: HashMap<String, String> = HashMap::new();
            for (alias, target) in &aliases {
                alias_for.insert(target.clone(), alias.clone());
            }
            let mut sorted_targets = targets.clone();
            sorted_targets.sort();
            for t in &sorted_targets {
                let Some(other) = unit_tables.get(t) else {
                    continue;
                };
                if other.services.is_empty() {
                    continue;
                }
                let surface_key = alias_for
                    .get(t)
                    .cloned()
                    .unwrap_or_else(|| t.rsplit('.').next().unwrap_or(t.as_str()).to_string());
                surface_entries.push(format!("{surface_key}: {}Surface", t.replace('.', "_")));
            }
        }
        if !surface_entries.is_empty() {
            deps_entries.push(format!("surface: {{ {} }}", surface_entries.join(", ")));
        }
        out.push_str(&format!(
            "  const {ns}Deps = {{ {} }};\n",
            deps_entries.join(", ")
        ));
        if !table.services.is_empty() {
            out.push_str(&format!(
                "  const {ns}Surface = {ns}.makeSurface({ns}Deps);\n",
            ));
        }
    }
    out.push('\n');

    // Export per-context surfaces under a top-level object.
    out.push_str("  return {\n");
    for ctx_name in &contexts {
        let Some(table) = unit_tables.get(ctx_name.as_str()) else {
            continue;
        };
        if table.services.is_empty() {
            continue;
        }
        let ns = ctx_name.replace('.', "_");
        let key = ctx_name.rsplit('.').next().unwrap_or(ctx_name.as_str());
        out.push_str(&format!("    {key}: {ns}Surface,\n"));
    }
    out.push_str("  };\n");
    out.push_str("}\n");

    // Assemble the header now that the body has recorded which units its
    // provider expressions reference.
    let mut header = String::new();
    header.push_str("// Generated by bynkc — do not edit by hand.\n");
    header.push_str("// composition root\n\n");

    // Import every context as a namespace.
    for ctx_name in &contexts {
        let dir = emitter::ts_specifier(&commons_dir_for(ctx_name));
        let ns = ctx_name.replace('.', "_");
        header.push_str(&format!("import * as {ns} from \"./{dir}.js\";\n"));
    }
    // v0.17: import each consumed adapter's binding module — the external
    // provider classes live there, not in the adapter's interface module.
    // v0.18: plus every adapter the provider expressions referenced through
    // the transitive given-closure (an adapter's external provider may depend
    // on another adapter's capability, spec §4.5).
    let mut consumed_adapters: Vec<String> = unit_consumes
        .iter()
        .filter(|(name, _)| kinds.get(*name) == Some(&UnitKind::Context))
        .flat_map(|(_, targets)| targets.iter().cloned())
        .chain(referenced_units.iter().cloned())
        .filter(|t| adapter_bindings.contains_key(t))
        .collect();
    consumed_adapters.sort();
    consumed_adapters.dedup();
    for adapter in &consumed_adapters {
        let ns = adapter.replace('.', "_");
        let module =
            emitter::ts_specifier(&adapter_bindings[adapter].output_path.with_extension("js"));
        header.push_str(&format!("import * as {ns}__binding from \"./{module}\";\n"));
    }
    header.push('\n');

    let out = format!("{header}{out}");

    Some(out)
}

// -- internals --

/// v0.8: collect the boundary-type owners visible to a given consuming
/// context. Every consumed-context type and every commons type referenced
/// in cross-context positions has an owner; that owner emits the
/// serialise/deserialise helpers.
fn compute_boundary_type_owners(
    consumer: &str,
    unit_info: &HashMap<String, UnitInfo>,
    parsed: &[ParsedFile],
) -> HashMap<String, BoundaryOwner> {
    let mut out: HashMap<String, BoundaryOwner> = HashMap::new();
    let Some(consumer_info) = unit_info.get(consumer) else {
        return out;
    };
    let _ = parsed;
    for t in &consumer_info.consumes {
        let Some(target_info) = unit_info.get(t) else {
            continue;
        };
        // Types declared in the consumed context (records, sums, refined,
        // opaque) — record them with the consumed context as owner.
        for type_name in target_info.table.types.keys() {
            out.insert(
                type_name.clone(),
                BoundaryOwner::Context { context: t.clone() },
            );
        }
        // Commons types `uses`-imported by the consumed context: their
        // file lookup is unit_file_index keyed by commons name.
    }
    // For consumer-side commons types (used in this context's exposed
    // signatures), look them up via this consumer's file index.
    let _ = &consumer_info.file_index;
    out
}

/// Context passed to the emitter so it can resolve cross-file and
/// cross-unit references into TypeScript import statements.
pub struct EmitProjectCtx {
    /// Source path of the file being emitted (relative to project root).
    pub source_path: PathBuf,
    /// Joined name of the commons or context this file belongs to.
    pub commons_name: String,
    /// Sibling files in the same unit (project-relative paths).
    pub local_files: Vec<PathBuf>,
    /// Which file declares each name in the local unit.
    pub file_decl_index: FileDeclIndex,
    /// For each imported name, the joined name of the unit it came from.
    pub imported_from: HashMap<String, String>,
    /// For each imported name, the kind (commons vs context) of the source unit.
    pub imported_from_kind: HashMap<String, UnitKind>,
    /// For each imported unit, the file path that declares each name.
    pub imported_decl_paths: HashMap<String, HashMap<String, PathBuf>>,
    /// The directory (project-relative) that holds this unit.
    pub commons_dir: PathBuf,
    /// What kind of unit this is.
    pub unit_kind: UnitKind,
    /// For contexts: this context's qualified name (used as the brand for
    /// rebranded mixed-in types and exported types).
    pub owning_context: Option<String>,
    /// For contexts: visibility of types declared in this context.
    pub exports_local: HashMap<String, Visibility>,
    /// For contexts: exports of each consumed context (so the emitter knows
    /// which names to import and how).
    pub exports_for_consumed: HashMap<String, HashMap<String, Visibility>>,
    /// For contexts: types imported via `consumes` clauses with their
    /// visibility and owning-context metadata.
    pub consumed_types: HashMap<String, ConsumedType>,
    /// For contexts: full cross-context information (consumed contexts,
    /// aliases, consumed services and types). Mirrors what the resolver
    /// and checker see (v0.6).
    pub cross_context: resolver::CrossContextInfo,
    /// True when *this* context's surface is consumed by another context in
    /// the project. Drives `makeSurface` emission (v0.6 §6.3).
    pub is_consumed_by_others: bool,
    /// v0.8 build target. Workers mode reroutes cross-context calls through
    /// Service Bindings and adds per-Worker entry/composition artefacts.
    pub target: BuildTarget,
    /// v0.8 (workers mode): for each cross-context type used in cross-context
    /// positions, the type's owning context's qualified name. Lets the
    /// emitter route serialise/deserialise helper imports to the owning
    /// module.
    pub boundary_type_owners: HashMap<String, BoundaryOwner>,
    /// Agent names declared in this unit. The body lowering uses this set
    /// to recognise `Agent(key)` construction and `agent_instance.method(...)`
    /// dispatch.
    pub local_agents: HashSet<String>,
    /// #527: for each local agent with `given` capabilities, the TS
    /// expression building those deps DO-side (workers contexts only; the DO
    /// wire cannot carry providers). Consumed by `emit_agent`'s fetch branch.
    pub agent_given_deps: HashMap<String, String>,
    /// #527: extra import lines `handlers.ts` needs for the expressions above.
    pub extra_import_lines: Vec<String>,
    /// #527: for each local agent, each `on call` method's `given` capability
    /// list. The lowering records which agent methods a handler body calls so
    /// the handler's emitted deps *type* carries the callee's capabilities —
    /// the runtime deps value (built by compose) always did.
    pub agent_method_givens: HashMap<String, HashMap<String, Vec<bynk_syntax::ast::CapRef>>>,
    /// v0.47: the context's actor declarations (merged across files), keyed by
    /// name. Used to resolve a handler's Bearer verification seam in `emit.rs`
    /// regardless of which file declares the actor.
    pub actors: HashMap<String, bynk_syntax::ast::ActorDecl>,
    /// v0.17: consumed unit names that are adapters. An adapter is not a Worker,
    /// so in workers mode its capability types are imported from its root module
    /// (`<adapter>.ts`), not from a per-Worker `handlers.ts`.
    pub consumed_adapters: HashSet<String>,
    /// Slice 2: the extension emitted import specifiers use (`.js` default; `.ts`
    /// for the `bynkc test --inspect` debug build). Consulted by `runtime_import_for`
    /// and the sibling/cross-commons specifier helpers.
    pub import_ext: ImportExt,
    /// v0.115 (testing track slice 3): emit the function-contract call-site guard
    /// (dev/test profile). Stripped in the deploy build for zero runtime cost.
    pub contracts: bool,
    /// v0.119 (testing track slice 7, ADR 0155): agent names a `for all run:
    /// History[Agent]` property in this project drives. Only these agents gain the
    /// exported `__bynkDriveHistory_<Agent>` test-support driver — every other
    /// agent's emission is byte-for-byte unchanged.
    pub history_target_agents: HashSet<String>,
    /// v0.132.1 (#481): for a context, the user-defined attached methods of each
    /// `uses`-imported refined/opaque type, keyed by the type's name and sorted
    /// by method name. The context's own `TypedCommons` merges the imported
    /// *types* but not their fn items, so `emit_context_rebrands` reads this to
    /// forward `Cents.fromInt(…)` and friends onto the rebranded const. Empty
    /// for commons units and for contexts with no such imports.
    pub imported_methods: HashMap<String, Vec<FnDecl>>,
}

/// Where a boundary-crossing type was declared.
#[derive(Debug, Clone)]
pub enum BoundaryOwner {
    /// Commons type. Path is project-relative to the `.bynk` file declaring it.
    Commons { source_path: PathBuf },
    /// Context type. Qualified context name (e.g., `commerce.payment`).
    Context { context: String },
}

impl EmitProjectCtx {
    pub fn commons_path(name: &str) -> PathBuf {
        commons_dir_for(name)
    }
}

#[allow(dead_code)]
fn _ensure_components_used(_p: &Path) {
    let _ = Component::CurDir;
}

/// v0.177 (#643, review of #658): the cross-context services a context actually
/// **calls**, as `consumed context → service names`.
///
/// This is not the same as "every service the dependency provides", and the
/// difference is the difference between a gate that reports what it *knows* is
/// skewed and one that reports what merely *differs*. If `payment` provides
/// `authorise` and `refund`, `orders` calls only `authorise`, and `refund`'s
/// contract changed, then recording `refund` in `orders`'s `expects` would refuse
/// `deploy --context orders` over a service `orders` never touches and whose
/// runtime check could never fire. ADR 0200 Decision E rejects a per-*context*
/// hash for exactly this reason — that it becomes a deployment tax — and a
/// per-context *gate* over per-service hashes would reintroduce it one layer up.
///
/// So the manifest's `expects` mirrors the runtime check's granularity: one entry
/// per call site, discovered the same way the lowering discovers it — an ident
/// chain on the receiver that resolves to a consumed context.
fn called_cross_context_services(
    table: &UnitTable,
    consumed: &[String],
    aliases: &HashMap<String, String>,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut out: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    if consumed.is_empty() {
        return out;
    }
    // Resolve a receiver chain to a consumed context: an alias, or the dotted
    // name itself. Mirrors `CrossContextInfo::resolve_prefix`, over the maps
    // `build_output` already holds.
    let resolve = |chain: &str| -> Option<String> {
        if let Some(target) = aliases.get(chain) {
            return Some(target.clone());
        }
        consumed.iter().find(|c| *c == chain).cloned()
    };
    let mut visit = |e: &bynk_syntax::ast::Expr| {
        if let bynk_syntax::ast::ExprKind::MethodCall {
            receiver, method, ..
        } = &e.kind
            && let Some(chain) = emitter::flatten_emit_ident_chain(receiver)
            && let Some(target) = resolve(&chain)
        {
            out.entry(target).or_default().insert(method.name.clone());
        }
    };
    for service in table.services.values() {
        for h in &service.handlers {
            emitter::walk_block_exprs(&h.body, &mut visit);
        }
    }
    for agent in table.agents.values() {
        for h in &agent.handlers {
            emitter::walk_block_exprs(&h.body, &mut visit);
        }
    }
    for provider in table.providers.values() {
        for op in &provider.ops {
            emitter::walk_block_exprs(&op.body, &mut visit);
        }
    }
    out
}

/// v0.177 (#643): a context's own `on call` contract hashes, keyed by service
/// name — the constants its Worker entry compares an incoming
/// `X-Bynk-Contract` against.
///
/// Built by projecting each local `on call` handler into the **same**
/// `CrossContextService` shape the resolver hands a *caller* for the same
/// service (`symbols.rs`'s `build_cross_context_info`), and hashing it from the
/// same combined type table. That symmetry is the whole correctness argument: a
/// caller and a callee compiled from one source tree must agree, or the check
/// fires on every call instead of only on real skew.
fn own_contract_hashes(
    table: &UnitTable,
    own_types: &HashMap<String, bynk_syntax::ast::TypeDecl>,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (sname, sdecl) in &table.services {
        let Some(handler) = sdecl
            .handlers
            .iter()
            .find(|h| matches!(h.kind, HandlerKind::Call))
        else {
            continue;
        };
        let svc = bynk_check::resolver::CrossContextService {
            name: sname.clone(),
            params: handler
                .params
                .iter()
                .map(|p| (p.name.name.clone(), p.type_ref.clone()))
                .collect(),
            return_type: handler.return_type.clone(),
            span: sdecl.span,
        };
        out.insert(
            sname.clone(),
            bynk_check::contract::service_contract_hash(&svc, own_types),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Slice 0: file identity is not the unit-validation path ---------------

    /// The defect, reproduced hermetically: two `include` roots each holding a
    /// file of the same name. Before slice 0 this yielded
    /// `["thing.bynk", "thing.bynk"]` — `parse_tree` stripped each tree's own
    /// root, so the two were indistinguishable and any consumer mapping by that
    /// key dropped one.
    ///
    /// This is the measurement from the track doc's §3.1, inverted into an
    /// assertion. It deliberately does **not** read `../examples/todo`:
    /// `bynk-emit` is published without an `exclude` list, so a test reaching
    /// outside the crate would fail a standalone `cargo test` on the released
    /// tarball. That `examples/todo` itself resolves is #647's regression
    /// fixture, where the LSP can actually observe it.
    #[test]
    fn split_roots_give_each_file_a_distinct_identity() {
        let root = scratch_project(
            "identity",
            &[
                ("bynk.toml", "[project]\nname = \"identity\"\n"),
                ("src/thing.bynk", "context thing\n"),
                ("tests/thing.bynk", "suite thing\n"),
            ],
        );
        let roots = Roots::Split {
            project_root: root.to_path_buf(),
            paths: read_project_paths(&root),
        };
        assert_eq!(
            roots.src_prefix().join("x"),
            PathBuf::from("src/x"),
            "the fixture must actually be two-rooted"
        );
        let (src_root, tests_root) = roots.resolve();
        let run = run_checks(
            &src_root,
            &tests_root,
            &roots.src_prefix(),
            &roots.tests_prefix(),
            BuildTarget::Bundle,
            Platform::default(),
            ImportExt::Js,
            Mode::Analyse,
            &HashMap::new(),
            &roots.excludes(),
            None,
            false,
        );
        let snapshots = match run {
            RunChecks::Bailed { snapshots, .. } => snapshots,
            RunChecks::Checked { snapshots, .. } => snapshots,
        };
        let mut keys: Vec<String> = snapshots
            .iter()
            .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["src/thing.bynk", "tests/thing.bynk"],
            "a file's identity must be project-relative and unique across include roots"
        );
    }

    /// A throwaway on-disk project, removed on drop — including when the test
    /// panics, which a trailing `remove_dir_all` would skip.
    struct Scratch(PathBuf);
    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Build a throwaway on-disk project. The e2e fixture suite cannot express
    /// these cases: `expected_error.txt` asserts *category strings only*, never
    /// a path, so no fixture there can pin attribution — which is precisely why
    /// the identity collision survived to slice 0.
    fn scratch_project(tag: &str, files: &[(&str, &str)]) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "bynk_slice0_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
        }
        Scratch(dir)
    }

    /// `AttributedError` is public API without a `Debug` impl; slice 0 is not
    /// the increment to add one, so tests render it themselves.
    fn render<'a>(errors: impl IntoIterator<Item = &'a AttributedError>) -> String {
        errors
            .into_iter()
            .map(|e| {
                format!(
                    "{} @ {}",
                    e.error.category,
                    e.source_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|| "<unattributed>".into())
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn analyse_split(root: &Path) -> Vec<AttributedError> {
        let roots = Roots::Split {
            project_root: root.to_path_buf(),
            paths: read_project_paths(root),
        };
        let (src_root, tests_root) = roots.resolve();
        let run = run_checks(
            &src_root,
            &tests_root,
            &roots.src_prefix(),
            &roots.tests_prefix(),
            BuildTarget::Bundle,
            Platform::default(),
            ImportExt::Js,
            Mode::Analyse,
            &HashMap::new(),
            &roots.excludes(),
            None,
            false,
        );
        match run {
            RunChecks::Bailed { errors, .. } => errors.into_all(),
            RunChecks::Checked { errors, .. } => errors.into_all(),
        }
    }

    /// The defect's user-visible half: a diagnostic in a secondary-root file
    /// must be attributed to *that* file. Before slice 0 both roots' files were
    /// named `thing.bynk`, so a consumer keying by the attributed path (the LSP
    /// does) folded the two together and one file's diagnostics vanished.
    #[test]
    fn a_secondary_root_diagnostic_is_attributed_to_the_secondary_root_file() {
        let root = scratch_project(
            "attr",
            &[
                ("bynk.toml", "[project]\nname = \"attr\"\n"),
                ("src/thing.bynk", "context thing\n"),
                // Same basename as the src file, different root — the collision.
                //
                // A *parse* error, deliberately: `parse_tree` attributes it as
                // the file is read, which is the path slice 0 changed. (A
                // checker-level error would not do: test bodies are checked by
                // `process_tests` during emit, not in `Mode::Analyse` — which is
                // also why `bynkc check` is silent on a broken `case`.)
                ("tests/thing.bynk", "suite thing\n\ncase {{{ \n"),
            ],
        );
        let errors = analyse_split(&root);
        let paths: Vec<String> = errors
            .iter()
            .filter_map(|e| e.source_path.as_ref())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            !paths.is_empty(),
            "the fixture must produce at least one attributed diagnostic; got [{}]",
            render(&errors),
        );
        assert!(
            paths.iter().all(|p| p == "tests/thing.bynk"),
            "a tests-root diagnostic must be attributed to `tests/thing.bynk`, \
             never the bare `thing.bynk` it shares with `src/` — got {paths:?}",
        );
    }

    /// The layout that ruled out the cheaper repair. Prefixing only the
    /// secondary tree would have worked for a `tests/` tree of suites — but
    /// ADR 0147 made test-ness *structural*, so `include[1]` may hold an
    /// ordinary unit. `check_path_name_alignment` reads `source_path`
    /// (tree-relative), so that unit must still validate: `spec/other.bynk`
    /// declaring `context other` is aligned, and prefixing its
    /// unit-validation path would have broken it.
    #[test]
    fn a_non_test_unit_in_the_secondary_root_still_validates() {
        let root = scratch_project(
            "nontest",
            &[
                (
                    "bynk.toml",
                    "[project]\nname = \"nontest\"\n\n[paths]\ninclude = [\"src\", \"spec\"]\n",
                ),
                ("src/thing.bynk", "context thing\n"),
                ("spec/other.bynk", "context other\n"),
            ],
        );
        let errors = analyse_split(&root);
        let alignment: Vec<&AttributedError> = errors
            .iter()
            .filter(|e| e.error.category == "bynk.project.inconsistent_commons_name")
            .collect();
        assert!(
            alignment.is_empty(),
            "a non-test unit in include[1] must still pass path/name alignment — \
             its unit-validation path stays tree-relative; got [{}]",
            render(alignment.iter().copied()),
        );
    }

    /// The prefix is the whole mechanism, and it is empty for a single root —
    /// which is *why* every existing single-root project's diagnostic paths are
    /// unchanged by construction rather than by care.
    #[test]
    fn single_root_prefixes_are_empty_so_identity_equals_source_path() {
        let single = Roots::Single(PathBuf::from("/p"));
        assert_eq!(single.src_prefix(), PathBuf::new());
        assert_eq!(single.tests_prefix(), PathBuf::new());
        // An empty prefix is a join identity: `"".join(rel) == rel`.
        assert_eq!(
            single.src_prefix().join("a/b.bynk"),
            PathBuf::from("a/b.bynk")
        );
    }

    /// The flat layout: `.bynk` at the project root, no `src/`, plus a
    /// `bynk.toml`. `ProjectPaths::conventional` yields `include = ["."]` and
    /// `bynk-driver` still selects `Roots::Split` (a manifest exists), so `.`
    /// reaches the prefix. It must normalise to empty: `Path::new(".")` only
    /// *looks* like a join identity — `".".join("x")` is `./x`, which is not
    /// equal to `x`, and would leak a `./` into every key and reported path.
    #[test]
    fn a_dot_include_normalises_to_an_empty_prefix() {
        // The premise, pinned: `.` is not a join identity but empty is.
        assert_ne!(
            PathBuf::from(".").join("thing.bynk"),
            PathBuf::from("thing.bynk")
        );
        assert_eq!(
            PathBuf::new().join("thing.bynk"),
            PathBuf::from("thing.bynk")
        );

        let roots = Roots::Split {
            project_root: PathBuf::from("/p"),
            paths: ProjectPaths {
                include: vec![PathBuf::from(".")],
                exclude: Vec::new(),
            },
        };
        assert_eq!(roots.src_prefix(), PathBuf::new(), "`.` must not leak in");
        assert_eq!(
            roots.src_prefix().join("thing.bynk"),
            PathBuf::from("thing.bynk"),
            "a flat project's identity is the bare path, never `./thing.bynk`"
        );
    }

    /// End-to-end over the flat layout `conventional()` actually produces, so
    /// the normalisation is pinned where it is reachable and not only on the
    /// accessor. No e2e fixture has this layout.
    #[test]
    fn a_flat_project_with_a_manifest_reports_unprefixed_paths() {
        let root = scratch_project(
            "flat",
            &[
                ("bynk.toml", "[project]\nname = \"flat\"\n"),
                // Parse error: `parse_tree` attributes it as the file is read.
                ("thing.bynk", "context thing\n\nfn {{{ \n"),
            ],
        );
        let paths = read_project_paths(&root);
        assert_eq!(
            paths.include,
            vec![PathBuf::from(".")],
            "the fixture must actually exercise the flat layout"
        );
        let errors = analyse_split(&root);
        let attributed: Vec<String> = errors
            .iter()
            .filter_map(|e| e.source_path.as_ref())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            !attributed.is_empty(),
            "the fixture must produce an attributed diagnostic; got [{}]",
            render(&errors),
        );
        assert!(
            attributed.iter().all(|p| p == "thing.bynk"),
            "a flat project's diagnostics must report `thing.bynk`, never \
             `./thing.bynk` — got {attributed:?}",
        );
    }

    /// A split project's prefixes are its `include` entries, in order.
    #[test]
    fn split_root_prefixes_are_the_include_entries() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/p"),
            paths: ProjectPaths {
                include: vec![PathBuf::from("lib"), PathBuf::from("spec")],
                exclude: Vec::new(),
            },
        };
        assert_eq!(roots.src_prefix(), PathBuf::from("lib"));
        assert_eq!(roots.tests_prefix(), PathBuf::from("spec"));
    }

    /// A single-entry `include` collapses to one walk (`split_mode` is
    /// `src_root != tests_root`), so the secondary prefix must not invent a
    /// second tree — it mirrors the primary.
    #[test]
    fn single_include_entry_resolves_both_roots_to_the_same_tree() {
        let roots = Roots::Split {
            project_root: PathBuf::from("/p"),
            paths: ProjectPaths {
                include: vec![PathBuf::from("src")],
                exclude: Vec::new(),
            },
        };
        let (src_root, tests_root) = roots.resolve();
        assert_eq!(src_root, tests_root, "one include entry is one tree");
        assert_eq!(roots.src_prefix(), PathBuf::from("src"));
    }

    /// v0.29.4: assembly yields exactly one `UnitInfo` per group, every facet
    /// present, with `exports`/`aliases`/`flattened` defaulting to empty for a
    /// unit absent from those (genuinely optional) producer maps — reproducing
    /// the old `.unwrap_or(empty)` read semantics as a total field.
    #[test]
    fn assemble_unit_info_yields_one_record_per_group_with_all_facets() {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        groups.insert("a.commons".to_string(), vec![0, 1]);
        groups.insert("a.context".to_string(), vec![2]);

        let mut kinds: HashMap<String, UnitKind> = HashMap::new();
        kinds.insert("a.commons".to_string(), UnitKind::Commons);
        kinds.insert("a.context".to_string(), UnitKind::Context);

        let mut unit_tables: HashMap<String, UnitTable> = HashMap::new();
        unit_tables.insert("a.commons".to_string(), UnitTable::default());
        unit_tables.insert("a.context".to_string(), UnitTable::default());

        let mut unit_uses: HashMap<String, Vec<String>> = HashMap::new();
        unit_uses.insert("a.context".to_string(), vec!["a.commons".to_string()]);

        let mut unit_consumes: HashMap<String, Vec<String>> = HashMap::new();
        unit_consumes.insert("a.context".to_string(), vec![]);

        // The genuinely-optional maps deliberately omit `a.commons` so the test
        // pins the empty-default behaviour.
        let mut unit_flattened: HashMap<String, HashMap<String, String>> = HashMap::new();
        unit_flattened.insert("a.context".to_string(), HashMap::new());
        let unit_consumes_aliases: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut exports_visibility: HashMap<String, HashMap<String, Visibility>> = HashMap::new();
        exports_visibility.insert("a.context".to_string(), HashMap::new());

        let mut unit_file_index: HashMap<String, FileDeclIndex> = HashMap::new();
        unit_file_index.insert(
            "a.commons".to_string(),
            FileDeclIndex {
                types: HashMap::new(),
                fns: HashMap::new(),
                methods: HashMap::new(),
            },
        );
        // `a.context` is absent from the file index → its `file_index` defaults.

        let info = assemble_unit_info(
            &groups,
            &kinds,
            &unit_tables,
            &unit_uses,
            &unit_consumes,
            &unit_flattened,
            &unit_consumes_aliases,
            &exports_visibility,
            &unit_file_index,
        );

        // One record per group, no more.
        assert_eq!(info.len(), 2);
        assert!(info.contains_key("a.commons"));
        assert!(info.contains_key("a.context"));

        // `files` mirrors the `groups` indices.
        assert_eq!(info["a.commons"].files, vec![0, 1]);
        assert_eq!(info["a.context"].files, vec![2]);

        // Non-optional facets are filled from their producer maps.
        assert_eq!(info["a.commons"].kind, UnitKind::Commons);
        assert_eq!(info["a.context"].kind, UnitKind::Context);
        assert_eq!(info["a.context"].uses, vec!["a.commons".to_string()]);

        // Optional facets default to empty for the unit with no entry.
        assert!(info["a.commons"].exports.is_empty());
        assert!(info["a.commons"].aliases.is_empty());
        assert!(info["a.commons"].flattened.is_empty());
        // And the absent `file_index` is an empty index, not a panic.
        assert!(info["a.context"].file_index.types.is_empty());
        assert!(info["a.context"].file_index.fns.is_empty());
        assert!(info["a.context"].file_index.methods.is_empty());
    }
}
