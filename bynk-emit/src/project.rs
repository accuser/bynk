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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::emitter;
use crate::ir::CapRefIr;
use crate::ir::EventSubscriberShape;
use crate::ir::FnSig;
use crate::ir::lower::{
    lower_attached_fn_sig_ir_from_types, lower_handler_given_ir, lower_provider_given_ir,
};
use bynk_check::actors::ActorDecl;
use bynk_check::check_pipeline::{self, prepare_unit_check_ctx};
use bynk_check::checker;
use bynk_check::checker::{ExprId, TyId, Types};
use bynk_check::expr_types::ExprTypeSink;
use bynk_check::firstparty::{self, Platform};
use bynk_check::hints::HintSink;
use bynk_check::index::{ProjectIndex, RefSink};
use bynk_check::locals::LocalsSink;
use bynk_check::project_model::{
    self, AdapterBinding, FnDecl, TypeDecl, UnitInfo, Visibility, handler_cross_caps,
    resolve_consume_prefix,
};
use bynk_check::requirements::RequirementSink;
use bynk_check::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use bynk_syntax::error::CompileError;
use bynk_syntax::lexer;
use bynk_syntax::parser;

// P4.0 (#1113): discovery, the unit graph, path resolution, and cross-unit
// consistency checks moved to `bynk-project` — this crate is now a
// dependent, not an owner, of that code (`design/tracks/project-model.md`
// §6 row P4.0). P4.1 (#1115): `symbols` and the per-file `context_checks`
// subset of `validate` moved to `bynk-check` for the same reason — this
// crate reaches them as `bynk_check::symbols`/`bynk_check::context_checks`
// now. P5.0-P5.3 (`design/tracks/semantics-in-the-checker.md` §6) emptied
// `validate` out the same way, project-wide check by project-wide check,
// including the reconciliation half of `schema_registry` (now
// `bynk_check::schema_registry`). P5.4 moved `tests_emit`'s checking half
// (target/participant resolution, `stub` resolution, case/property body
// type-checking) to `bynk_check::test_suites` the same way — `tests_emit`
// stays here as a caller of it, holding only TypeScript emission plus the
// two functions' unchanged public shape. P5.5 (§5, §6): `validate` reached
// empty (its last occupant, `check_platform_lock`, left at P5.3) and its
// remaining diagnostic-emitting sites — the `bynk.secrets.computed_name`
// warning and the `bynk.project.schema_registry_corrupt` construction, both
// outside the seven-category accounting — relocated too (§3.2's "eighth
// site" and §9's open risk respectively); `validate.rs` and its module
// declaration are deleted, this track's own completion criterion (§5).
// Pipeline-driving types (`diagnostics`'s `Mode`/`ErrorSink`/
// `ProjectAnalysis`/`ProjectFailure`) stay here too.
mod diagnostics;
mod schema_registry;
mod tests_emit;

use bynk_check::symbols::*;
use bynk_project::discovery::*;
use bynk_project::paths::*;
use diagnostics::*;
use tests_emit::*;

// External facade: items referenced as `crate::project::X` from outside this
// module (emitter, main, lib) must stay reachable at that path.
pub use bynk_check::project_model::BuildTarget;
pub use bynk_check::symbols::{FileDeclIndex, UnitTable};
pub use bynk_project::{
    AttributedError, ProjectPaths, ProjectPathsError, Roots, SchemaLock, UnitKind,
    discover_project_files, try_read_project_paths, try_read_project_paths_with, worker_dir_name,
    worker_handlers_output_path, worker_handlers_source_path,
};
pub use diagnostics::{ContextBoundaryInfo, ContextSequenceInfo, ProjectAnalysis, ProjectFailure};

/// A project's output, as a keyed set of typed documents (R7.8, #1309) —
/// replaces the old `ProjectOutput.files: Vec<CompiledFile>` outright, not
/// alongside it ("Done when": `CompiledFile`/`ProjectOutput::files` are
/// gone, not aliased). Keyed by output path (project-root-relative), so the
/// sibling-path lookup `bynk-driver::output` needs for `.map`/
/// `.bynkdbg.json` files is a direct map operation instead of a linear scan,
/// and so a `wrangler.toml` document carries its real [`crate::emitter::
/// toml_doc::TomlDocument`] all the way to the write boundary instead of
/// being stringified at construction (Decision E).
pub struct Artefacts {
    pub docs: BTreeMap<PathBuf, Document>,
}

/// One typed output document. No `String` at construction for TypeScript or
/// TOML content (R7.8) — `Json`/`Js`/`SourceMap`/`DebugSidecar` stay opaque
/// payloads because `bynk-emit` never re-parses them itself, not because
/// they're an escape hatch from the rule.
pub enum Document {
    /// A generated TypeScript module. Every `bynk-emit` construction site
    /// builds this today by wrapping its still-`String`-producing content in
    /// one `TsStmt::verbatim(VerbatimOrigin::NotYetConverted, ..)` — see that
    /// variant's own doc comment for why each site does this literally,
    /// rather than through one shared helper.
    Ts(bynk_ts::TsProgram),
    /// `wrangler.toml`, still as the real tree `emit_wrangler_toml` built —
    /// printed only at the `bynk-driver` write boundary.
    Toml(crate::emitter::toml_doc::TomlDocument),
    /// Generated JSON with no tree this compiler tracks yet (`package.json`,
    /// `tsconfig.json`, the contracts/secrets manifests).
    Json(String),
    /// Type-stripped JavaScript (`bynk-strip::strip_project_to_js`'s own
    /// output) — never produced by `bynk-emit` itself, which only ever
    /// builds `Ts`; kept distinct from `Ts` so a stripped artefact never
    /// round-trips back through the TypeScript printer/lint.
    Js(String),
    /// A source-map v3 document, split out of its originating file's own
    /// staged `source_map` at the sibling path `write_output` names it
    /// (`<file>.map`).
    SourceMap(String),
    /// The handler-label debug sidecar, split out the same way at
    /// `<file>.bynkdbg.json`.
    DebugSidecar(String),
}

impl Document {
    /// This document's own text, whichever kind it is — for read-side
    /// callers that only need bytes (golden fixtures, `bynk-strip`'s own
    /// need to feed `Ts` content through `strip_types`, `bynk-wasm`'s
    /// JS-facing API, in-process test helpers). `Ts`/`Toml` print through
    /// the same printer `bynk-driver::output::write_document` writes from
    /// (R7.3/R7.6) — this is a rendering, not a second construction path;
    /// nothing here builds a `Document` from a `String`.
    pub fn text(&self) -> String {
        match self {
            Document::Ts(program) => bynk_ts::print(program, "", "", "").text,
            Document::Toml(doc) => crate::emitter::print_toml_document(doc),
            Document::Json(s)
            | Document::Js(s)
            | Document::SourceMap(s)
            | Document::DebugSidecar(s) => s.clone(),
        }
    }
}

/// One generated document, staged during `build_output`/`emit_unit` before
/// the final split into [`Artefacts`] — see `build_output`'s own tail.
struct StagedFile {
    output_path: PathBuf,
    source_map: Option<String>,
    debug_metadata: Option<String>,
    document: Document,
}

/// Result of compiling a project.
pub struct ProjectOutput {
    /// P7.6 (#1309): every generated document, typed (R7.8).
    /// `bynk-driver::output::write_output` writes from this.
    pub artefacts: Artefacts,
    /// v0.89 (ADR 0117): non-failing warnings emitted on a successful build —
    /// surfaced (the CLI prints them, the LSP shows them) but not gating.
    pub warnings: Vec<AttributedError>,
    /// Per-file source snapshots, keyed the same way `ProjectFailure::snapshots`
    /// is — lets a warning render with real file/line/col context (ariadne or
    /// `path:line:col:`) instead of the position-free `warning[category]: …`
    /// fallback a successful build previously had no way to avoid.
    pub snapshots: Vec<(PathBuf, String)>,
    /// v0.67: the test manifest — every discovered suite and case, retained at
    /// emit time so `bynkc test --no-run --format json` can render a discovery
    /// document without running the suite. Built from the same names + spans the
    /// runner would emit at `suite-begin`/`case`, so a discovery document
    /// reconciles cleanly against a later run's document (same suite name/kind,
    /// same case names). Ordered to match the runner (`emit_test_main`).
    pub discovered: Vec<DiscoveredSuite>,
    /// #1078: the reconciled `bynk.schema.lock` content, `Some` whenever
    /// `CompileOptions::schema_registry` was `SchemaLock::On` for this build
    /// — `bynk-emit` computes it but never writes it; the caller (today,
    /// `bynk-driver`'s two wiring points) persists it atomically, exactly
    /// the discipline `schema_registry.rs` used to implement itself. `None`
    /// when the registry was off, regardless of whether the content would
    /// have changed — the unchanged-content no-op lives in the writer, not
    /// here, so a caller that reconciles the same shape twice in a row still
    /// gets `Some` both times.
    pub schema_lock: Option<String>,
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

// P4.1 (#1115): `AdapterBinding` and `BuildTarget` relocated to
// `bynk-check::project_model` alongside `phase_group`, which constructs
// them — see that module's own doc comment. `BuildTarget` is re-exported
// below at its old `crate::project::BuildTarget` path (part of this crate's
// public surface); `AdapterBinding` was never public here, so a plain `use`
// (above) is enough.

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
#[derive(Clone)]
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
    /// #57 (testing track): when `Some`, every file `roots` would otherwise
    /// discover on disk is instead read from here — keyed the same way
    /// `discovery::read_source`'s overlay is (a canonicalised absolute path,
    /// falling back to the literal path when the file has no on-disk
    /// counterpart to canonicalise). Filesystem discovery is skipped entirely;
    /// `roots` still supplies `src_root`/`tests_root` and their prefixes, so a
    /// `Roots::Split` project can drive both trees in-memory.
    ///
    /// #1077/#1081: `bynk-driver`'s `project_options`/`try_project_options`
    /// *do* set this now — the real CLI entry points walk and read every
    /// project file themselves and hand the result here, so `bynk-emit`
    /// itself no longer discovers or reads anything on disk for the CLI
    /// path. `bynkc`/the LSP still don't: `bynkc` routes through
    /// `bynk-driver`, and the LSP (`analyse_project_with`) has its own
    /// open-buffer overlay and depends on the on-disk fallback for the rest
    /// (#1079). `bynk-emit`'s own tests also set this, to exercise the full
    /// project pipeline (cross-context `uses`, multi-file layouts,
    /// workers-mode emission, …) without an on-disk fixture tree.
    pub sources: Option<HashMap<PathBuf, String>>,
    /// Events track, slice 3c (#980): reconcile against `bynk.schema.lock`.
    /// **Off by default** — `bynkc compile`'s directory branch and `bynk`'s
    /// deploy/dev build turn it on; every library/test caller (in-memory
    /// builds, `bynkc/tests/e2e.rs`'s in-place fixture compiles, `bynk-emit`'s
    /// own `sources`-driven tests, the LSP) leaves it off, so a compile never
    /// mutates a project tree it wasn't asked to.
    ///
    /// #1078: this carries the lock's pre-read content, not just an on/off
    /// switch — `bynk-emit` reads and writes no disk for this file (the same
    /// move #1077/#1081 already made for `.bynk` source content). See
    /// [`SchemaLock`]'s own doc for the read side; the reconciled content
    /// comes back out on [`ProjectOutput::schema_lock`] for the caller to
    /// persist.
    pub schema_registry: SchemaLock,
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
            sources: None,
            schema_registry: SchemaLock::Off,
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
            sources: None,
            schema_registry: SchemaLock::Off,
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

    /// #57 (testing track): supply every file in-memory instead of walking
    /// `roots` on disk — keyed the same way `discovery::read_source`'s
    /// overlay is (see the `sources` field's own doc).
    pub fn sources(mut self, sources: HashMap<PathBuf, String>) -> Self {
        self.sources = Some(sources);
        self
    }

    /// Events track, slice 3c (#980): turn on `bynk.schema.lock`
    /// reconciliation for this build, with its pre-read content (or
    /// verified-absent `None`, for a fresh project). See [`SchemaLock`] and
    /// the field's own doc for who calls this and why everyone else leaves
    /// it off.
    pub fn schema_registry(mut self, mode: SchemaLock) -> Self {
        self.schema_registry = mode;
        self
    }
}

/// #57: turn `options.sources` into the `(overlay, discovered)` pair
/// `run_checks` expects — the caller-supplied file list, partitioned across
/// `trees` the same way a real walk would (single-tree: every file lands in
/// the first tree's list, matching `compile_in_memory`'s own convention).
/// Shared by [`compile_project`] and [`check_project`] so the two can't drift
/// on this.
///
/// #1077/#1081 review: sorts every partition. `sources`'s own key order is a
/// `HashMap`'s — unspecified, randomised per process — but `phase_parse`
/// walks each tree's file list in order to assign sequential `FileId`s and
/// `ExprId`s (embedded in emitted spans/source maps) and `run_checks` pushes
/// diagnostics in file order, both of which a real disk walk already
/// guaranteed via `discover_bynk_files`'s own `out.sort()`. Without this, a
/// `sources`-driven compile (the CLI's own path as of #1081) would silently
/// vary its diagnostic order and `FileId` assignment run to run.
///
/// R3.9 (#1113): partitions across every `trees` entry, not a hardcoded
/// primary/secondary pair — a path that doesn't start with any tree's root
/// (shouldn't happen for a well-formed `sources` map) falls back to the
/// *last* tree, matching the pre-R3.9 two-tree `partition`'s fallback (an
/// unmatched key landed in `tests_files`, the second/last half of the pair).
/// Falling back to the *first* tree instead — silently tried during this
/// slice's initial cut — would move an unmatched file into whichever tree
/// `check_file_directory_conflicts` and `identity_path` treat as primary,
/// changing its attribution with no diagnostic raised either way; matching
/// the old convention at least keeps this rare path's behaviour unchanged by
/// the crate move.
type Overlay = HashMap<PathBuf, String>;
/// One file list per `trees` entry — the same shape `phase_discovery` and
/// `run_checks`'s own `discovered` parameter already use.
type Discovered = Vec<Vec<PathBuf>>;

fn sources_to_discovered(
    sources: &HashMap<PathBuf, String>,
    trees: &[(PathBuf, PathBuf)],
) -> (Overlay, Option<Discovered>) {
    let mut buckets: Vec<Vec<PathBuf>> = vec![Vec::new(); trees.len()];
    let mut keys: Vec<PathBuf> = sources.keys().cloned().collect();
    keys.sort();
    for p in keys {
        let idx = trees
            .iter()
            .position(|(root, _)| p.starts_with(root))
            .unwrap_or(trees.len().saturating_sub(1));
        buckets[idx].push(p);
    }
    (sources.clone(), Some(buckets))
}

/// Compile a Bynk project, keeping error attribution + snapshots on failure
/// (so the CLI can render project errors with source context, ADR 0052). Use
/// `.map_err(ProjectFailure::flatten)` for the flattened `Vec<CompileError>`
/// shape.
pub fn compile_project(options: &CompileOptions) -> Result<ProjectOutput, ProjectFailure> {
    // T3.6b (R4.1): one table per build, shared across every unit compiled.
    let tys = &Arc::new(Types::new());
    let trees = options.roots.trees();
    let excludes = options.roots.excludes();
    let (overlay, discovered) = match &options.sources {
        Some(sources) => sources_to_discovered(sources, &trees),
        None => (HashMap::new(), None),
    };
    let run = run_checks(
        &trees,
        options.target,
        options.platform,
        options.import_ext,
        Mode::Build,
        &overlay,
        &excludes,
        discovered,
        options.contracts,
        &options.schema_registry,
        options.roots.project_root(),
        tys,
    );
    // #1078: `bynk-emit` no longer writes `bynk.schema.lock` itself — the
    // reconciled content comes back on `ProjectOutput::schema_lock`
    // (`finish_build` populates it from `RunChecks::Checked`, only ever
    // constructed on the `Ok` path, i.e. only on a fully clean build — a
    // build that fails for any reason, including a schema mismatch
    // reconciliation itself just reported, produces `Err(ProjectFailure)`
    // instead and has no revised content for a caller to persist). The
    // caller (today, `bynk-driver`'s two wiring points) does the atomic
    // write.
    finish_build(run, options.import_ext)
}

/// Result of [`check_project`]: every diagnostic from a non-bailing project
/// analysis — errors *and* warnings together (ADR 0117), unconditionally,
/// unlike [`ProjectOutput`]/[`ProjectFailure`] where `errors`/`warnings` is
/// picked by which variant the caller got. `bynk check`'s exit code is
/// decided by [`Self::has_errors`], not by whether this was reached at all.
pub struct ProjectCheck {
    pub errors: Vec<AttributedError>,
    pub snapshots: Vec<(PathBuf, String)>,
}

impl ProjectCheck {
    /// Finding #64: `bynk check`'s exit-code gate is "does any error-severity
    /// diagnostic exist in the project" — not "did the pipeline reach the end
    /// without bailing", which is what `compile_project`'s `Result` encoded
    /// and why a `bynk.toml`-wide structural error anywhere silently hid every
    /// later diagnostic (including a test body's own type errors) from
    /// `bynk check`, even though the editor (via `analyse_project_with`, the
    /// same `Mode::Analyse` this runs) still reported them.
    pub fn has_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|ae| bynk_syntax::Severity::for_error(&ae.error) == bynk_syntax::Severity::Error)
    }
}

/// Check a project without building (finding #64) — never bails after
/// discovery (`Mode::Analyse`, the same mode `analyse_project_with` already
/// uses for the editor), so a diagnostic anywhere in the project does not
/// suppress diagnostics elsewhere. `compile_project`'s `Mode::Build` bails at
/// the first structural error and returns only what it collected up to that
/// point — correct for `build`/`test`, which must not emit past a real
/// error, but wrong for `check`, whose only job is to report everything.
/// `bynk check`'s directory path calls this instead of `compile_project`.
pub fn check_project(options: &CompileOptions) -> ProjectCheck {
    // T3.6b (R4.1): one table per check, shared across every unit.
    let tys = &Arc::new(Types::new());
    let trees = options.roots.trees();
    let excludes = options.roots.excludes();
    let (overlay, discovered) = match &options.sources {
        Some(sources) => sources_to_discovered(sources, &trees),
        None => (HashMap::new(), None),
    };
    let run = run_checks(
        &trees,
        options.target,
        options.platform,
        options.import_ext,
        Mode::Analyse,
        &overlay,
        &excludes,
        discovered,
        options.contracts,
        // `bynk check` never reconciles the schema registry, regardless of
        // `options.schema_registry` — pre-existing behaviour (finding #64's
        // own era), preserved as-is by #1078, not introduced by it.
        &SchemaLock::Off,
        options.roots.project_root(),
        tys,
    );
    match run {
        RunChecks::Bailed {
            errors, snapshots, ..
        }
        | RunChecks::Checked {
            errors, snapshots, ..
        } => ProjectCheck {
            errors: errors.into_all(),
            snapshots,
        },
    }
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
    // T3.6b (R4.1): one table for this virtual project's compile.
    let tys = &Arc::new(Types::new());
    // A single-tree (`src_root == tests_root`) virtual project rooted at `.`: the
    // one source file is supplied directly and its text layered in via the
    // overlay, so discovery and every other disk read are bypassed.
    let root = PathBuf::from(".");
    let path = in_memory_logical_path(source);
    let mut overlay = HashMap::new();
    overlay.insert(path.clone(), source.to_string());
    let trees = vec![(root.clone(), PathBuf::new())];
    let run = run_checks(
        &trees,
        target,
        platform,
        ImportExt::Js,
        Mode::Build,
        &overlay,
        &[],
        Some(vec![vec![path]]),
        false,
        &SchemaLock::Off,
        &root,
        tys,
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
    analyse_in_memory_with_types(source, target, platform).errors
}

/// The outcome of [`analyse_in_memory_with_types`]: diagnostics plus the
/// analysed file's `(span, type)` entries (span-sorted — see
/// [`ExprTypeSink::take_files`]), for a position→type query (#397, the
/// playground's hover), and its local bindings (#808, the playground's
/// completion — `bynk_check::locals::locals_at` over `locals` answers
/// "what's in scope at this offset").
pub struct InMemoryAnalysis {
    pub errors: Vec<AttributedError>,
    pub expr_types: Vec<(bynk_syntax::span::Span, TyId)>,
    /// T3.6b (R4.1): the table `expr_types`' ids resolve against.
    pub ty_intern: std::sync::Arc<bynk_check::checker::Types>,
    pub locals: Vec<bynk_check::locals::LocalBinding>,
}

/// Like [`analyse_in_memory`], but also exposes the expression-type map the
/// checker captured (ADR 0063's `expr_types` sink) — the same one
/// [`analyse_project_with`] drains — instead of discarding it. Per ADR 0094,
/// this is a best-effort **partial** map in `Analyse` mode: a function that
/// type-checked cleanly contributes its types even if a *different* function
/// in the same file has an error, so `expr_types` is empty only when the
/// expression at hand never typed at all (e.g. it sits in an unresolved
/// region, ADR 0094's "out of scope — the resolve gate"), not merely because
/// the file has some error somewhere.
pub fn analyse_in_memory_with_types(
    source: &str,
    target: BuildTarget,
    platform: Platform,
) -> InMemoryAnalysis {
    // T3.6b (R4.1): one table for the whole analysis — every unit it checks
    // interns into this, so the `TyId`s it hands back on `InMemoryAnalysis`
    // all resolve against the one table it also hands back.
    let tys = &Arc::new(Types::new());
    let root = PathBuf::from(".");
    let path = in_memory_logical_path(source);
    let mut overlay = HashMap::new();
    overlay.insert(path.clone(), source.to_string());
    let trees = vec![(root.clone(), PathBuf::new())];
    let run = run_checks(
        &trees,
        target,
        platform,
        ImportExt::Js,
        Mode::Analyse,
        &overlay,
        &[],
        Some(vec![vec![path.clone()]]),
        false,
        &SchemaLock::Off,
        &root,
        tys,
    );
    match run {
        RunChecks::Bailed {
            errors,
            mut exprs,
            mut locals,
            ..
        }
        | RunChecks::Checked {
            errors,
            mut exprs,
            mut locals,
            ..
        } => InMemoryAnalysis {
            errors: errors.into_all(),
            expr_types: exprs.take_files().remove(&path).unwrap_or_default(),
            ty_intern: Arc::clone(tys),
            locals: locals.take_files().remove(&path).unwrap_or_default(),
        },
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
        .map(|unit| unit.name().parts.iter().map(|i| i.name.clone()).collect());
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
            snapshots,
            parsed,
            compiled,
            runnable_tests,
            integration_outputs,
            integration_runnables,
            groups,
            kinds,
            unit_consumes,
            unit_consumes_aliases,
            unit_tables,
            unit_callees,
            unit_event_subscriber_shapes,
            unit_uses,
            unit_flattened,
            adapter_bindings,
            npm_deps,
            target,
            schema_registry,
            ..
        } => {
            let mut out = build_output(
                parsed,
                compiled,
                runnable_tests,
                integration_outputs,
                integration_runnables,
                groups,
                kinds,
                unit_consumes,
                unit_consumes_aliases,
                unit_tables,
                unit_callees,
                unit_event_subscriber_shapes,
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
            out.snapshots = snapshots;
            // #1078: the reconciled registry, if this build had one on —
            // bynk-emit computes it, the caller persists it.
            out.schema_lock = schema_registry.map(|reg| schema_registry::serialize(&reg));
            Ok(out)
        }
    }
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
    // T3.6b (R4.1): see `analyse_in_memory_with_types` — one table per
    // analysis, shared by every unit, carried out on the result.
    let tys = &Arc::new(Types::new());
    // Resolved exactly as `compile_project` does — one project model, not two.
    let trees = roots.trees();
    let excludes = roots.excludes();
    match run_checks(
        &trees,
        BuildTarget::Bundle,
        Platform::default(),
        ImportExt::Js,
        Mode::Analyse,
        overlay,
        &excludes,
        None,
        false,
        // The LSP never reconciles the schema registry (#1079's open scope
        // covers the editor becoming a real file-content owner generally;
        // this specific flag was already, and stays, hardcoded off).
        &SchemaLock::Off,
        roots.project_root(),
        tys,
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
            ty_intern: Arc::clone(tys),
            requirements: requirements.take_files(),
            // No parsed tree on the bail path — the map stays empty (ADR 0095).
            unit_sources: HashMap::new(),
            // #846: same bail rule as `unit_sources` — nothing was resolved.
            sequence_info: HashMap::new(),
            // #855: same bail rule — nothing was resolved.
            boundary_info: HashMap::new(),
            // #848: no parsed tree on the bail path either.
            doc_scope: HashMap::new(),
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
            unit_consumes_aliases,
            unit_tables,
            unit_flattened,
            kinds,
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
                if pf.is_synthetic() {
                    continue;
                }
                unit_sources
                    .entry(pf.unit().name().joined())
                    .or_default()
                    .push(pf.identity_path());
            }
            // #846: qualified context/adapter unit name → the cross-context +
            // agent tables the sequence-diagram classifier needs. Rebuilt from
            // the same retained per-project tables the per-file checking pass
            // used to build its own transient `cross_context_for_file` (see
            // the call site of `build_cross_context_info` above, in the
            // per-file loop) — that transient value is never itself kept
            // around, so this re-derives it once per unit instead of once per
            // file, from data `run_checks` already retains.
            let mut sequence_info: HashMap<String, ContextSequenceInfo> = HashMap::new();
            // #855: qualified context/adapter unit name → the combined type
            // table plus service/agent tables the wire-contract peek needs —
            // built alongside `sequence_info` in the same loop iteration so
            // `table`/`unit_tables`/`unit_uses` are already in scope. Uses
            // `combined_types_for`, the same table `own_contract_hashes`
            // hashes through, so the peek's hash and the emitted
            // `X-Bynk-Contract` constant cannot disagree.
            let mut boundary_info: HashMap<String, ContextBoundaryInfo> = HashMap::new();
            for (name, kind) in &kinds {
                if !matches!(kind, UnitKind::Context | UnitKind::Adapter) {
                    continue;
                }
                let Some(table) = unit_tables.get(name) else {
                    continue;
                };
                let mut cross_context = build_cross_context_info(
                    name,
                    &unit_consumes,
                    &unit_consumes_aliases,
                    &unit_uses,
                    &unit_tables,
                );
                cross_context.flattened_caps =
                    unit_flattened.get(name).cloned().unwrap_or_default();
                sequence_info.insert(
                    name.clone(),
                    ContextSequenceInfo {
                        cross_context,
                        agents: table.agents.clone(),
                    },
                );
                boundary_info.insert(
                    name.clone(),
                    ContextBoundaryInfo {
                        types: bynk_check::symbols::combined_types_for(
                            name,
                            &unit_tables,
                            &unit_uses,
                        ),
                        services: table.services.clone(),
                        agents: table.agents.clone(),
                    },
                );
            }
            // #848: doc_scope reuses unit_sources' key set (production units —
            // the only files a doc comment can live in) and the
            // unit_uses/unit_consumes already destructured above for
            // assemble_index — itself first, then its `uses` targets, then
            // its `consumes` targets, mirroring IndexBuilder::qualify_with's
            // bare-name search order.
            let mut doc_scope: HashMap<String, Vec<String>> = HashMap::new();
            for name in unit_sources.keys() {
                let mut scope = vec![name.clone()];
                scope.extend(unit_uses.get(name).cloned().unwrap_or_default());
                scope.extend(unit_consumes.get(name).cloned().unwrap_or_default());
                doc_scope.insert(name.clone(), scope);
            }
            ProjectAnalysis {
                snapshots,
                errors: errors.into_all(),
                index,
                hints: hints.take_files(),
                locals: locals.take_files(),
                expr_types: exprs.take_files(),
                ty_intern: Arc::clone(tys),
                requirements: requirements.take_files(),
                unit_sources,
                sequence_info,
                boundary_info,
                doc_scope,
            }
        }
    }
}

// P4.1 (#1115): `normalize_service_defaults`/`inject_service_defaults`
// relocated to `bynk-check::project_model` (called from both `run_checks`
// here and the new `bynk-check`-native analysis entry point, ahead of
// `phase_group` in both).

/// v0.54 (#655): whether a context's services declare an `on call … by c: Caller`
/// handler, whose emitted `deps` carries the calling context's qualified name as
/// its `CallerId` identity (ADR 0092); in bundle mode the compose root supplies
/// that name to `makeSurface`, mirroring the `X-Bynk-Caller` header a Worker
/// reads at its entry. Delegates to the *same*
/// [`any_service_binds_caller`](crate::emitter::any_service_binds_caller) the
/// emitter's `emit_make_surface` calls, so the compose root and the surface can
/// never disagree on which providers take the extra `__caller` argument.
fn context_binds_caller(table: &UnitTable) -> bool {
    crate::emitter::any_service_binds_caller(table.services.values(), &table.actors)
}

/// A double-quoted, escaped TypeScript string literal for `s` (a qualified
/// context name at the compose-root caller sites).
fn ts_string_literal(s: &str) -> String {
    format!("\"{}\"", crate::emitter::escape_ts_string(s))
}

// P4.1 (#1115): `record_analyse_types` relocated to
// `bynk-check::check_pipeline` (called from `check_file_core`'s own
// error-path exits, plus every clean-path caller — `check_unit_files`'s
// `Mode::Analyse` branch below and the new entry point's own).

// P4.1 (#1115): the whole discovery->parse->group->resolve pipeline
// (`phase_discovery` through `phase_file_index`/`assemble_unit_info`, plus
// per-unit symbol composition — `compose_unit_symbols`/
// `merge_consumed_exports`/`collect_unit_methods`) relocated to
// `bynk-check::project_model` — see that module's own doc comment for why
// (the same `extract, don't duplicate` move `bynk-project` itself was, P4.0).
// `run_checks`, below, is now a caller of `project_model::phase_*` instead of
// owning this logic inline.

/// v0.119 (ADR 0155): the agents a `for all run: History[Agent]` property drives,
/// scanned across every test suite in the project. `emit_agent` gates the
/// exported `__bynkDriveHistory_<Agent>` driver on membership, so a non-targeted
/// agent's emission is unchanged.
fn collect_history_target_agents(parsed: &[ParsedFile]) -> HashSet<String> {
    parsed
        .iter()
        .flat_map(|pf| pf.history_target_agent_names())
        .map(String::from)
        .collect()
}

/// Phase 8e: build the emitter context for one checked source file and render
/// its TypeScript, pushing the result onto `compiled`. Reached only in build
/// mode (the caller's analyse-mode `continue` gates this off); the block is
/// straight-line with no `continue`s of its own.
#[allow(clippy::too_many_arguments)]
/// Emit-prologue tables that depend only on the *unit* (`name`/`unit_info`/
/// `target`) — never on which file within the unit is being emitted. Building
/// one of these once per unit, ahead of the per-file loop, replaces what used
/// to be an identical rebuild (several nested nested loops over `unit_info`)
/// on every emitted file of a multi-file context.
struct EmitUnitCtx {
    imported_methods: HashMap<String, Vec<FnSig>>,
    /// The workers-mode-rewritten view is the only one `emit_unit` reads —
    /// the pre-rewrite table is an intermediate of computing it, not exposed
    /// separately.
    imported_decl_paths_emit: HashMap<String, HashMap<String, PathBuf>>,
    exports_for_consumed: HashMap<String, HashMap<String, Visibility>>,
    file_decl_index: FileDeclIndex,
}

fn build_emit_unit_ctx(
    name: &str,
    unit_info: &BTreeMap<String, UnitInfo>,
    target: BuildTarget,
    tys: &Arc<Types>,
) -> EmitUnitCtx {
    let info = &unit_info[name];
    // v0.132.1 (#481): gather the attached methods of every `uses`-imported type
    // (one level, matching the symbol-table merge). `emit_context_rebrands`
    // forwards these onto the consumer's rebranded const so a call like
    // `Cents.fromInt(n)` type-checks. Sorted by method name for deterministic
    // emission (the resolver stores instance/static methods in `HashMap`s).
    //
    // P6.18: each method's own `params`/`return_type` now resolve to a real
    // `TyId` (`ir::lower::lower_attached_fn_sig_ir_from_types`) against the
    // *declaring* unit's own visible types, rather than carrying the raw
    // `FnDecl` (and its unresolved `TypeRef`s) all the way to
    // `emit_forwarded_methods`. `Free`-named entries (never present in
    // practice — `ResolverMethodTable` only ever collects attached methods,
    // `bynk-check/src/resolver.rs:47`) are skipped, matching
    // `emit_forwarded_methods`'s own pre-existing `FnName::Method` filter
    // one step earlier rather than lowering a signature nothing renders.
    let mut imported_methods: HashMap<String, Vec<FnSig>> = HashMap::new();
    for t in &info.uses {
        let Some(used) = unit_info.get(t) else {
            continue;
        };
        let used_types = bynk_check::symbols::combined_types_for_unit_info(t, unit_info);
        for (type_name, mt) in &used.table.methods {
            let entry = imported_methods.entry(type_name.clone()).or_default();
            entry.extend(lower_attached_fn_sig_ir_from_types(mt, &used_types, tys));
        }
    }
    for decls in imported_methods.values_mut() {
        decls.sort_by_key(|sig| sig.name.clone());
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

    // In workers mode, rewrite imported_decl_paths for consumed
    // contexts to point at the consumed Worker's handlers.ts.
    let mut imported_decl_paths_emit = imported_decl_paths.clone();
    if matches!(target, BuildTarget::Workers) {
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

    EmitUnitCtx {
        imported_methods,
        imported_decl_paths_emit,
        exports_for_consumed,
        file_decl_index: info.file_index.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_unit(
    name: &str,
    kind: UnitKind,
    pf: &ParsedFile,
    unit_ctx: &EmitUnitCtx,
    history_target_agents: &HashSet<String>,
    unit_info: &BTreeMap<String, UnitInfo>,
    imported_from: &HashMap<String, String>,
    imported_from_kind: &HashMap<String, UnitKind>,
    owning_context_for_emit: &Option<String>,
    cross_context_for_file: &resolver::CrossContextInfo,
    program: &checker::CheckedProgram,
    target: BuildTarget,
    import_ext: ImportExt,
    contracts: bool,
    agent_deps_plan: Option<&AgentDepsPlan>,
    compiled: &mut Vec<StagedFile>,
    schema_effective_versions: &HashMap<String, i64>,
) {
    let typed = program.program();
    // Build the emitter context.
    let info = &unit_info[name];
    let cross_context_info = cross_context_for_file.clone();

    // v0.8: in workers mode, a context's *output* lands under
    // workers/<dashes>/handlers.ts. Use that path as the synthetic
    // source_path so the emitter's depth/relative-path logic and
    // imported_decl_paths produce correct relative imports.
    let workers_mode = matches!(target, BuildTarget::Workers);
    let emit_source_path = if workers_mode && kind == UnitKind::Context {
        worker_handlers_source_path(name)
    } else {
        pf.source_path()
    };

    // message-bundles slice 1 (#859): a `messages` block's generated `render`
    // needs `bynk.locale`'s own `render` in scope for its fallback rung, but
    // under a private alias — this file's own `export function render` would
    // otherwise collide with a plain `import { render }`. Injected as a hand-
    // written extra import line rather than through the usual reference-
    // collection path (`collect_external_references`/`record_name_ref`),
    // which has no per-name aliasing of its own and would emit a colliding,
    // unaliased `render`.
    let mut extra_import_lines: Vec<String> = agent_deps_plan
        .map(|p| p.imports.clone())
        .unwrap_or_default();
    if pf.declares_messages() {
        let render_path = unit_ctx
            .imported_decl_paths_emit
            .get("bynk.locale")
            .and_then(|m| m.get("render"))
            .cloned()
            .unwrap_or_else(|| EmitProjectCtx::commons_path("bynk.locale"));
        let import = emitter::cross_commons_import_specifier_for_path(
            &emit_source_path,
            &render_path,
            import_ext,
        );
        extra_import_lines.push(format!(
            "import {{ render as __bynkLocaleRender, renderArg }} from \"{import}\";"
        ));
    }

    let emit_ctx = EmitProjectCtx {
        source_path: emit_source_path,
        commons_name: name.to_string(),
        file_decl_index: unit_ctx.file_decl_index.clone(),
        imported_from: imported_from.clone(),
        imported_from_kind: imported_from_kind.clone(),
        imported_decl_paths: unit_ctx.imported_decl_paths_emit.clone(),
        unit_kind: kind,
        owning_context: owning_context_for_emit.clone(),
        exports_for_consumed: unit_ctx.exports_for_consumed.clone(),
        imported_methods: unit_ctx.imported_methods.clone(),
        cross_context: cross_context_info,
        target,
        local_agents: info.table.agents.keys().cloned().collect(),
        agent_given_deps: agent_deps_plan.map(|p| p.exprs.clone()).unwrap_or_default(),
        extra_import_lines,
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
                                .map(|m| (m.name.clone(), lower_handler_given_ir(h)))
                        })
                        .collect(),
                )
            })
            .collect(),
        // v0.47: the context's actors (merged across files), so the Bearer
        // verification seam resolves even when the actor and handler are in
        // different files of the same context.
        actors: info.table.actors.clone(),
        // Events slice 3b (#978), verified by slice 3c (#980): resolved once
        // per unit, merged across files the same way `actors` is above. The
        // registry's reconciled version wins when present (it is the
        // auto-bumped or `@schema(N)`-verified truth); `decl.schema_version()`
        // is the fallback for when the registry is off, matching every
        // event's pre-3c behaviour exactly.
        event_schema_versions: info
            .table
            .events
            .iter()
            .map(|(event_name, decl)| {
                let key = format!("{name}.{event_name}");
                let version = schema_effective_versions
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| decl.schema_version());
                (event_name.clone(), version)
            })
            .collect(),
        consumed_adapters: info
            .consumes
            .iter()
            .filter(|t| unit_info.get(*t).map(|i| i.kind) == Some(UnitKind::Adapter))
            .cloned()
            .collect(),
        import_ext,
        contracts,
        history_target_agents: history_target_agents.clone(),
        runtime_use: Default::default(),
    };
    // v0.72: the map's `source` is the absolute path the compiler read the file
    // from, so an editor breakpoint set on the real `.bynk` resolves to the same
    // path the debugger loads (project-relative would resolve against the output
    // `.ts`'s directory — the wrong place). Synthetic units fall back to relative.
    let source_name = pf.map_source_name();
    let (ts, source_map) = emitter::emit_project(program, &emit_ctx, pf.source(), &source_name);
    // Slice 3: the handler-label sidecar for this unit (ADR 0105) — names stack
    // frames by their Bynk operation. `None` for units with no handlers.
    let debug_metadata = emitter::collect_handler_labels(typed);
    let output_path = if workers_mode && kind == UnitKind::Context {
        worker_handlers_output_path(name)
    } else {
        ts_output_path(&pf.source_path())
    };
    compiled.push(StagedFile {
        output_path,
        document: Document::Ts(bynk_ts::TsProgram {
            stmts: vec![bynk_ts::TsStmt::verbatim(
                bynk_ts::VerbatimOrigin::NotYetConverted,
                ts,
                None,
            )],
        }),
        source_map,
        debug_metadata,
    });
}

/// Phase 8d/8e: resolve + check (and, in build mode, emit) every source file in
/// one production unit. The per-file `continue`s stay internal to this loop, so
/// a file that fails resolution/checking is skipped without abandoning the unit.
///
/// P4.1 (#1115): the resolve+check+context-checks core — identical for both
/// `Mode`s except for the four `record_analyse_types` call sites — moved to
/// `bynk_check::check_pipeline::check_file_core` (see that module's own doc
/// comment), used by both this function and the new `bynk-check`-native
/// analysis entry point. This function now owns only: the per-unit
/// `EmitUnitCtx`/`prepare_unit_check_ctx` prelude, the `Mode`-conditional
/// exit (`Mode::Analyse` records and stops; `Mode::Build` proceeds to
/// `certify`+`emit_unit`) and the emission tail itself.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn check_unit_files(
    name: &str,
    kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    unit_info: &BTreeMap<String, UnitInfo>,
    combined_types: &HashMap<String, Arc<TypeDecl>>,
    combined_fns: &HashMap<String, Arc<FnDecl>>,
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
    history_target_agents: &HashSet<String>,
    mode: Mode,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    exprs: &mut ExprTypeSink,
    requirements: &mut RequirementSink,
    compiled: &mut Vec<StagedFile>,
    // Events track, slice 3c (#980): each locally-declared event's *effective*
    // schema version, keyed `<unit>.<EventName>` — the schema registry's
    // reconciled value when the registry is on, empty (so every lookup falls
    // through to `EventDecl::schema_version()`) when it is off.
    schema_effective_versions: &HashMap<String, i64>,
    tys: &Arc<Types>,
    // #1187's slice 6 plumbing — this unit's own accumulator; merged into
    // per file below, from each file's own certified `CheckedProgram`
    // (`RunChecks::Checked::unit_callees`'s own doc comment has the full
    // grounding for why this exists).
    unit_callees: &mut HashMap<ExprId, bynk_check::checker::Callee>,
    // P6.x (#1232): this unit's own declared event-subscriber service
    // shapes, keyed by service name — see `EventSubscriberShape`'s own doc
    // comment. Populated the same way as `unit_callees` above: merged from
    // each file's own `CheckedProgram` before it is dropped at the end of
    // this loop's iteration.
    event_subscriber_shapes: &mut HashMap<String, EventSubscriberShape>,
) {
    // Emit-prologue tables invariant across every file of this unit — built
    // once here rather than once per file (see `EmitUnitCtx`).
    let unit_ctx = build_emit_unit_ctx(name, unit_info, target, tys);
    let check_ctx = prepare_unit_check_ctx(kind, unit_info, combined_types, imported_from_kind);

    for &i in indices {
        let pf = &parsed[i];
        let Some(check_pipeline::FileCheckResult {
            typed,
            cross_context: cross_context_for_file,
        }) = check_pipeline::check_file_core(
            name,
            kind,
            pf,
            unit_info,
            combined_types,
            combined_fns,
            combined_methods,
            local_names,
            local_methods_for_type,
            consumed_types,
            imported_from,
            &check_ctx,
            errors,
            refs,
            hints,
            locals,
            exprs,
            requirements,
            tys,
        )
        else {
            continue;
        };

        // Analyse mode stops at checked: emission is build-only. Capture the
        // file's expression types on the way out (Ok path only — this point is
        // past every per-file error exit inside `check_file_core`), for
        // `.`-member completion.
        if mode == Mode::Analyse {
            check_pipeline::record_analyse_types(
                exprs,
                &pf.identity_path(),
                pf.is_synthetic(),
                &typed.expr_types,
            );
            continue;
        }
        // T3.7b (R3.10): every per-unit gate above already ran (check_record's
        // Ok path, check_context_constraints, check_context_declarations's
        // blocks_emission, all inside `check_file_core`) — certify makes that
        // structural, the same way T3.7a did for the single-file path, rather
        // than relying on every future call site remembering to check all
        // three before reaching emission. A later, unrelated diagnostic (e.g.
        // check_platform_lock, which runs after this whole loop) can still
        // bail the entire build via finish_build's separate
        // errors.is_empty() gate — that's whole-build atomicity (already
        // correct, already unconditional), orthogonal to this unit's own
        // certification here.
        let program = checker::certify(typed, Vec::new()).unwrap_or_else(|_| {
            panic!("bynk internal error: unit already passed every per-unit gate above")
        });
        // #1187's slice 6 plumbing: merge this file's own resolved `Callee`
        // classification into the unit's accumulator before `program` (and
        // the `TypedCommons` it wraps) is dropped at the end of this
        // iteration — the only point in this pipeline that ever holds it.
        // Filtered to the two variants either reader actually matches on
        // (review of #1202): every other `Callee` variant would otherwise
        // sit retained project-wide, for the rest of the build, to answer
        // two boolean-ish questions — a real `String`/`Arc` cost on a large
        // project with nothing reading the rest yet. Widen this filter (or
        // drop it) the moment a future reader needs a different variant.
        unit_callees.extend(program.program().callees.iter().filter_map(|(id, c)| {
            let keep = match c {
                bynk_check::checker::Callee::Cross { .. } => true,
                bynk_check::checker::Callee::Capability { cap, op } => {
                    cap == "Events" && op == "emit"
                }
                _ => false,
            };
            keep.then(|| (*id, c.clone()))
        }));
        // P6.23 (review of #1254): captures this file's own
        // event-subscriber service shapes the same way as before —
        // before `program` is dropped at the end of this iteration, same
        // insertion point as `unit_callees.extend` above. P6.47 (#1137):
        // the walk itself (including the `ServiceProtocol::Events`
        // pre-filter guarding `lower_service_item_ir` — see that
        // function's own doc comment for why the guard stays) relocated to
        // `ir::lower::lower_event_subscriber_shapes_ir`, an excluded file.
        event_subscriber_shapes
            .extend(crate::ir::lower::lower_event_subscriber_shapes_ir(&program));
        emit_unit(
            name,
            kind,
            pf,
            &unit_ctx,
            history_target_agents,
            unit_info,
            imported_from,
            imported_from_kind,
            owning_context_for_emit,
            &cross_context_for_file,
            &program,
            target,
            import_ext,
            contracts,
            agent_deps_plan,
            compiled,
            schema_effective_versions,
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
        compiled: Vec<StagedFile>,
        runnable_tests: Vec<RunnableTest>,
        integration_outputs: Vec<StagedFile>,
        integration_runnables: Vec<RunnableTest>,
        groups: BTreeMap<String, Vec<usize>>,
        kinds: BTreeMap<String, UnitKind>,
        unit_uses: HashMap<String, Vec<String>>,
        unit_consumes: HashMap<String, Vec<String>>,
        unit_consumes_aliases: HashMap<String, HashMap<String, String>>,
        unit_tables: HashMap<String, UnitTable>,
        // #1187's slice 6 plumbing: each unit's own `Callee` classification,
        // merged across its files (`ExprId` is a single project-wide
        // counter, `project_model.rs`'s `next_expr_id`, so merging different
        // files' maps never collides) — checked, resolved data the pre-check
        // `unit_tables` above cannot carry. Exists so a later, project-wide
        // pass (`build_output`/`emit_composition_root`) can read an
        // already-resolved `Callee::Capability`/`Callee::Cross` instead of
        // re-deriving the same fact by walking raw AST method-call syntax —
        // `check_unit_files`'s own per-file `CheckedProgram` was previously
        // built and dropped before any such later pass ever ran. Filtered at
        // merge time (`check_unit_files`'s own `unit_callees.extend` call,
        // review of #1202) to only `Callee::Cross` and
        // `Callee::Capability{cap:"Events",op:"emit"}` — the two variants
        // `unit_table_uses_emit`/`called_cross_context_services` actually
        // read today; widen the filter (or drop it) the moment a future
        // reader needs a different variant, rather than paying to retain
        // every call site's full classification project-wide for the rest
        // of the build on spec.
        unit_callees: HashMap<String, HashMap<ExprId, bynk_check::checker::Callee>>,
        // P6.x (#1232): each unit's own declared event-subscriber service
        // shapes, merged across its files — see `EventSubscriberShape`'s own
        // doc comment. Threaded the same way as `unit_callees` immediately
        // above.
        unit_event_subscriber_shapes: HashMap<String, HashMap<String, EventSubscriberShape>>,
        unit_flattened: HashMap<String, HashMap<String, String>>,
        adapter_bindings: HashMap<String, AdapterBinding>,
        npm_deps: std::collections::BTreeMap<String, String>,
        target: BuildTarget,
        // Events track, slice 3c (#980): the reconciled registry document,
        // ready for `finish_build` to serialize onto
        // `ProjectOutput::schema_lock`. `None` when `schema_registry` was
        // `SchemaLock::Off` (#1078) — nothing for a caller to persist.
        schema_registry: Option<schema_registry::SchemaRegistry>,
    },
}

#[allow(clippy::too_many_arguments)]
fn run_checks(
    // R3.9 (#1113): one `(root, prefix)` pair per `Roots::trees` entry, not a
    // hardcoded primary/secondary pair — every `include` tree is walked, not
    // just the first one or two.
    trees: &[(PathBuf, PathBuf)],
    target: BuildTarget,
    platform: Platform,
    import_ext: ImportExt,
    mode: Mode,
    overlay: &HashMap<PathBuf, String>,
    // v0.113: absolute subtrees to skip during discovery (author `exclude` plus
    // the tool's `out`/`node_modules` caches). Empty for in-memory builds.
    excludes: &[PathBuf],
    // v0.108 (in-browser track, slice 3): when `Some`, the source files are
    // supplied directly, one file list per `trees` entry — and filesystem
    // discovery is skipped. The wasm/REPL entry feeds an in-memory
    // single-module project this way (the source itself rides in `overlay`);
    // `None` keeps the on-disk discovery walk for the CLI and the LSP.
    discovered: Option<Vec<Vec<PathBuf>>>,
    // v0.115: emit the function-contract call-site guard (dev/test profile).
    contracts: bool,
    // Events track, slice 3c (#980): `On` turns on `bynk.schema.lock`
    // reconciliation, with its pre-read content; `Off` (every in-memory/
    // test/LSP caller) skips it entirely. See `CompileOptions::schema_registry`
    // and `SchemaLock`. #1078: no disk access here — the caller pre-reads.
    schema_registry: &SchemaLock,
    // #1085 review: only for `schema_registry::parse`'s corruption message —
    // naming *which* project's lock file is corrupt, now that #1078 made
    // `bynk-emit` disk-free (and so path-blind) for this file.
    project_root: &Path,
    tys: &Arc<Types>,
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

    // -- 1. Discovery (skipped when sources are supplied in memory). --
    let file_lists = match discovered {
        Some(files) => files,
        None => match project_model::phase_discovery(trees, excludes, &mut errors) {
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
    // #1077/#1081 review: `no_sources`/file-directory-conflict checks run on
    // every tree's file list regardless of provenance — see
    // `check_discovered_files`'s own doc.
    if project_model::check_discovered_files(trees, &file_lists, &mut errors).is_err() {
        return RunChecks::Bailed {
            errors,
            snapshots,
            hints,
            locals,
            exprs,
            requirements,
        };
    }

    // -- 2. Parse every file. --
    let (mut parsed, consumes_bynk, consumes_cloudflare) = match project_model::phase_parse(
        trees,
        &file_lists,
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
    project_model::normalize_service_defaults(&mut parsed);
    let parsed = parsed;

    // -- 3. Group by (name, kind) and validate per-directory consistency.
    //       P5.2 (`design/tracks/semantics-in-the-checker.md` §6):
    //       `phase_group` now also confines function types to non-boundary
    //       positions directly, at the point its old optional hook used to
    //       fire — see that function's own doc comment. --
    let (groups, kinds, test_groups, integration_groups, adapter_bindings, npm_deps) =
        project_model::phase_group(
            &parsed,
            trees,
            platform,
            consumes_bynk,
            consumes_cloudflare,
            overlay,
            &mut errors,
        );

    // -- 4. Build per-unit combined symbol tables. --
    let unit_tables = project_model::phase_symbol_tables(&groups, &kinds, &parsed, &mut errors);

    // -- 5. Resolve `uses` clauses (target must exist + be a commons). --
    let unit_uses =
        project_model::phase_resolve_uses(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5b. Resolve `consumes` clauses (target must exist + be a context). --
    let (unit_consumes, unit_flattened) = project_model::phase_resolve_consumes(
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
        project_model::phase_consumes_aliases(&groups, &kinds, &parsed, &unit_tables, &mut errors);

    // -- 5b''. v0.173 (ADR 0196 D1): warn where a `bynk.Secrets` read names its
    //          secret with a computed expression. P5.5
    //          (`design/tracks/semantics-in-the-checker.md` §6, §9): relocated
    //          to `bynk_check::project_model::phase_secrets_computed_name` —
    //          this is now a caller, not an owner, the same move as this
    //          function's neighbours above. See that function's own doc for
    //          why raising it here no longer reaches the editor by itself.
    project_model::phase_secrets_computed_name(
        target,
        &parsed,
        &groups,
        &kinds,
        &unit_flattened,
        &mut errors,
    );

    // -- 5c. Detect `consumes` cycles. --
    project_model::phase_detect_consumes_cycles(&groups, &parsed, &unit_consumes, &mut errors);

    // -- 6. Name-conflict detection for uses imports (commons-only check). --
    project_model::phase_uses_name_conflicts(
        &unit_uses,
        &unit_tables,
        &parsed,
        &groups,
        &mut errors,
    );

    // -- 6a'. message-bundles slice 1 (#859): messages-block legality,
    //         @reference cardinality, within-block duplicate codes, and the
    //         `uses bynk.locale` dependency. Runs here (not in phase_group)
    //         because it needs `unit_uses`, resolved just above.
    //
    //         P5.0 (#1128, `design/tracks/semantics-in-the-checker.md` §6):
    //         relocated to `bynk-check::project_model` alongside the rest of
    //         this pipeline (P4.1's own move) — this is now a caller, not an
    //         owner, the same way P4.0/P4.1 turned this function into a
    //         caller of `bynk-project`/`bynk-check`.
    project_model::phase_messages_bundles(&parsed, &groups, &kinds, &unit_uses, &mut errors);

    // -- 6a''. Locale capability track, slice 2 (#882): a context reaching
    //          two or more message-bundle commons while consuming `Locale`
    //          has no single bundle to negotiate against. P5.0: relocated,
    //          see above.
    project_model::phase_locale_bundle_ambiguity(
        &parsed,
        &groups,
        &kinds,
        &unit_uses,
        &unit_flattened,
        &mut errors,
    );

    // -- 6a'''. Events track, slice 0 (spine #936): a `from Events(E)`
    //           subscription must name a real, declared event — needs
    //           `unit_tables` + `unit_consumes` together, so it runs here
    //           rather than in the per-context `check_service_protocols`.
    //
    //           P5.1 (#1130, `design/tracks/semantics-in-the-checker.md` §6):
    //           relocated to `bynk-check::project_model`, same move as
    //           P5.0's neighbours above.
    project_model::phase_event_subscriptions(
        &parsed,
        &groups,
        &kinds,
        &unit_tables,
        &unit_consumes,
        &unit_uses,
        &mut errors,
    );

    // -- 6b. Validate exports clauses (each name is a locally-declared type;
    //         no duplicates within or across opaque/transparent). --
    let exports_visibility = project_model::phase_validate_type_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6b'. Validate `exports capability { … }` clauses (v0.15 §4.1): each
    //          name must be a capability the context declares *and* provides. --
    project_model::phase_validate_capability_exports(
        &groups,
        &kinds,
        &parsed,
        &unit_tables,
        &mut errors,
        &mut refs,
    );

    // -- 6c. Validate that providers match their capabilities exactly. --
    project_model::phase_validate_providers(&unit_tables, &groups, &parsed, &mut errors, tys);

    // -- 6d. Events track, slice 3c (#980): reconcile every event's shape
    //        against the committed schema registry. `schema_registry` is
    //        `SchemaLock::Off` for every in-memory/test/LSP/fixture caller
    //        (opt-in — see `CompileOptions::schema_registry`'s doc), in which
    //        case this is a no-op and every event falls back to today's
    //        `@schema(N)`-or-`1` behaviour. Must run before the per-unit loop
    //        below: `emit_unit` needs `schema_effective_versions` to mint the
    //        right `schemaVersion`, and by the time `RunChecks` reaches
    //        `finish_build` the TypeScript is already emitted. Only the
    //        *write* is deferred — to the caller, gated on a fully clean
    //        build (#1078: `bynk-emit` computes, never writes) — reconciliation
    //        itself happens here. P5.3: `reconcile` itself now lives in
    //        `bynk_check::schema_registry` (this crate is a caller, not an
    //        owner) — `SchemaRegistry` stays re-exported from this crate's
    //        own `schema_registry` module. P5.5: the corrupt-file diagnostic
    //        moved too — `bynk_check::schema_registry::parse_or_diagnose` is
    //        now this crate's caller-side of both the parse and the
    //        `bynk.project.schema_registry_corrupt` construction (§3.2's
    //        "eighth site").
    let mut schema_effective_versions: HashMap<String, i64> = HashMap::new();
    let mut schema_registry_doc: Option<schema_registry::SchemaRegistry> = None;
    if let SchemaLock::On { existing } = schema_registry {
        match bynk_check::schema_registry::parse_or_diagnose(existing.as_deref(), project_root) {
            Ok(existing_reg) => {
                let mut schema_errors: Vec<CompileError> = Vec::new();
                let (updated, effective) = bynk_check::schema_registry::reconcile(
                    &existing_reg,
                    &unit_tables,
                    &mut schema_errors,
                );
                errors.extend_for(None, schema_errors);
                schema_effective_versions = effective;
                schema_registry_doc = Some(updated);
            }
            Err(err) => {
                errors.push_for(None, err);
            }
        }
    }

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
    let unit_file_index = project_model::phase_file_index(&groups, &parsed);

    // -- 7b (v0.29.4). Assemble the nine parallel per-unit maps into one record
    //          per unit. Driven by the `groups` keyset (the authority), so every
    //          group yields exactly one `UnitInfo` with all facets present. The
    //          producer maps are cloned, not moved, because the back half of the
    //          pipeline (tests, integration tests, platform-lock, composition
    //          root, the workers branch) still reads the originals.
    let unit_info = project_model::assemble_unit_info(
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
    let mut compiled: Vec<StagedFile> = Vec::new();
    // #1187's slice 6 plumbing (see `RunChecks::Checked::unit_callees`'s own
    // doc comment) — one `Callee` map per unit, merged across that unit's
    // own files inside the loop below.
    let mut unit_callees: HashMap<String, HashMap<ExprId, bynk_check::checker::Callee>> =
        HashMap::new();
    // P6.x (#1232): one `EventSubscriberShape` map per unit, merged across
    // that unit's own files inside the loop below — same shape as
    // `unit_callees` immediately above.
    let mut unit_event_subscriber_shapes: HashMap<String, HashMap<String, EventSubscriberShape>> =
        HashMap::new();

    // v0.119 (testing track slice 7, ADR 0155): a project-wide fold over every
    // parsed file, producing the identical `HashSet` regardless of which unit
    // or file is currently emitting — computed once here rather than once per
    // emitted file (`collect_history_target_agents` used to be called from
    // inside the per-file emit prologue).
    let history_target_agents = collect_history_target_agents(&parsed);

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
        ) = project_model::compose_unit_symbols(name, local_table, &unit_info);
        let consumed_types = project_model::merge_consumed_exports(
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

        let local_methods_for_type = project_model::collect_unit_methods(indices, &parsed);

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
            &history_target_agents,
            mode,
            &mut errors,
            &mut refs,
            &mut hints,
            &mut locals,
            &mut exprs,
            &mut requirements,
            &mut compiled,
            &schema_effective_versions,
            tys,
            unit_callees.entry(name.clone()).or_default(),
            unit_event_subscriber_shapes
                .entry(name.clone())
                .or_default(),
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
        &unit_flattened,
        &groups,
        import_ext,
        contracts,
        &mut emitted_barrels,
        &mut test_errors,
        &mut refs,
        tys,
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
        &mut emitted_barrels,
        &mut integration_errors,
        &mut refs,
        tys,
    );
    // #696: integration-suite diagnostics, like the unit-test ones above, stay
    // unattributed pending the same `process_integration_tests` threading.
    errors.extend_for(None, integration_errors);

    // v0.19 (decisions 0017/0024): platform-lock enforcement. A deployment
    // unit whose in-process closure reaches a platform-native capability is
    // locked to that platform; the selected `--platform` must match. Run only
    // on otherwise-clean programs: the closure walk recurses the provider
    // graph, whose acyclicity the earlier checks establish. P5.3: relocated
    // to `bynk_check::project_model::phase_platform_lock` — this crate is a
    // caller, not an owner.
    if errors.is_empty() {
        project_model::phase_platform_lock(
            target,
            platform,
            &parsed,
            &groups,
            &kinds,
            &unit_tables,
            &unit_consumes,
            &unit_consumes_aliases,
            &unit_flattened,
            &mut errors,
        );
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
        unit_callees,
        unit_event_subscriber_shapes,
        unit_flattened,
        adapter_bindings,
        npm_deps,
        target,
        schema_registry: schema_registry_doc,
    }
}

/// `<name>.<suffix>`, matching `bynk-driver::output.rs`'s own sibling-name
/// derivation for `.map`/`.bynkdbg.json` files exactly (P7.6, #1309).
pub fn sibling_path(output_path: &Path, suffix: &str) -> PathBuf {
    let name = output_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    output_path.with_file_name(format!("{name}.{suffix}"))
}

/// Build-success tail (region 3): emit the composition/worker/runtime files
/// and assemble the final `ProjectOutput`. Reached only on build mode with a
/// clean error sink. Moved verbatim from the old pipeline; only the locals it
/// reads are now bound from the `Checked` variant.
#[allow(clippy::too_many_arguments)]
fn build_output(
    parsed: Vec<ParsedFile>,
    mut compiled: Vec<StagedFile>,
    mut runnable_tests: Vec<RunnableTest>,
    integration_outputs: Vec<StagedFile>,
    integration_runnables: Vec<RunnableTest>,
    groups: BTreeMap<String, Vec<usize>>,
    kinds: BTreeMap<String, UnitKind>,
    unit_consumes: HashMap<String, Vec<String>>,
    unit_consumes_aliases: HashMap<String, HashMap<String, String>>,
    unit_tables: HashMap<String, UnitTable>,
    unit_callees: HashMap<String, HashMap<ExprId, bynk_check::checker::Callee>>,
    // P6.x (#1232): see `EventSubscriberShape`'s own doc comment.
    unit_event_subscriber_shapes: HashMap<String, HashMap<String, EventSubscriberShape>>,
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
        compiled.push(StagedFile {
            output_path: PathBuf::from("tests/main.ts"),
            document: Document::Ts(bynk_ts::TsProgram {
                stmts: vec![bynk_ts::TsStmt::verbatim(
                    bynk_ts::VerbatimOrigin::NotYetConverted,
                    main_ts,
                    None,
                )],
            }),
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

    // Events track, slice 0 (spine #936): project-wide "who subscribes to
    // what", built once and shared by both targets — Bundle mode's
    // `composeApp` dispatches in-process; Workers mode uses it to size each
    // publishing context's fan-out DO routing table and wrangler.toml
    // Service Bindings.
    let event_subscribers =
        bynk_check::symbols::discover_event_subscribers(&unit_tables, &unit_consumes);

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
                &unit_callees,
                &unit_event_subscriber_shapes,
                &adapter_bindings,
                &unit_flattened,
                // D1: thread `env` through composeApp only when a native
                // resource is consumed, so native-free programs are
                // byte-identical to v0.18 output.
                !context_native.is_empty(),
                &event_subscribers,
            ) {
                compiled.push(StagedFile {
                    output_path: PathBuf::from("compose.ts"),
                    document: Document::Ts(bynk_ts::TsProgram {
                        stmts: vec![bynk_ts::TsStmt::verbatim(
                            bynk_ts::VerbatimOrigin::NotYetConverted,
                            compose_ts,
                            None,
                        )],
                    }),
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
                    bynk_check::symbols::combined_types_for(ctx_name, &unit_tables, &unit_uses);
                let own_contracts = bynk_check::contract::own_contract_hashes(table, &own_types);
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
                // Locale capability track, slice 2 (#882, Decision A): real
                // negotiation is Cloudflare-only — `BuildTarget::Workers`
                // isn't itself restricted to `Platform::Cloudflare`, so a
                // hypothetical `--target workers --platform node` project
                // must not try to pass 3 args to Node's still-0-arg
                // `LocaleProvider`.
                let is_cloudflare_binding = adapter_bindings
                    .get(firstparty::BYNK_UNIT)
                    .is_some_and(|b| {
                        b.output_path.file_name()
                            == Some(std::ffi::OsStr::new(
                                firstparty::Platform::Cloudflare.bynk_binding_filename(),
                            ))
                    });
                let bundle = bynk_check::symbols::detect_context_message_bundle(
                    ctx_name, &unit_uses, &groups, &kinds, &parsed,
                );
                let locale_bundle_info = match &bundle {
                    bynk_check::symbols::ContextMessageBundle::One(info)
                        if is_cloudflare_binding =>
                    {
                        Some(info)
                    }
                    _ => None,
                };
                // #1187's slice 6 plumbing: computed once, reused by every
                // Workers-target emitter below that needs it.
                let ctx_uses_emit = unit_table_uses_emit(table, unit_callees.get(ctx_name));
                let (compose_ts, needs_locale_request) = emitter::emit_worker_compose(
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
                    locale_bundle_info,
                    import_ext,
                    ctx_uses_emit,
                );
                let entry_ts = emitter::emit_worker_entry(
                    ctx_name,
                    table,
                    &own_contracts,
                    needs_locale_request,
                    ctx_uses_emit,
                );
                // Adapters are not Workers, so they get no Service Binding in
                // the consumer's wrangler config — drop them from the list.
                let mut service_consumes: BTreeSet<String> = consumes_targets
                    .iter()
                    .filter(|t| !binding_modules.contains_key(*t))
                    .cloned()
                    .collect();
                // Events track, slice 0 (spine #936, ADR 0284): this
                // context's own published events → their subscribers,
                // sliced from the project-wide table. A subscriber
                // `consumes` the publisher for the event *type*; nothing
                // upstream gives the publisher a binding back to the
                // subscriber, so its Worker needs one added here — the
                // reverse direction of an ordinary `consumes` edge.
                let own_event_routes: BTreeMap<String, Vec<(String, String)>> = event_subscribers
                    .iter()
                    .filter(|((owner, _), _)| owner == ctx_name)
                    .map(|((_, name), subs)| (name.clone(), subs.clone()))
                    .collect();
                service_consumes.extend(
                    own_event_routes
                        .values()
                        .flatten()
                        .map(|(sub_ctx, _)| sub_ctx.clone()),
                );
                let service_consumes: Vec<String> = service_consumes.into_iter().collect();
                // P6.x cutover slice 2 (#1191): collected here, not inside
                // `emit_wrangler_toml` itself, so that function's own file
                // needs no raw-AST match. P6.46 (#1137): the walk itself
                // relocated to `bynk_check::symbols::cron_and_queue_triggers`
                // — a pure function of `table`, `project.rs`'s own remaining
                // AST contact here was incidental to where the loop happened
                // to be written, not structural.
                let (crons, queues) = bynk_check::symbols::cron_and_queue_triggers(table);
                let wrangler_doc = emitter::emit_wrangler_toml(
                    ctx_name,
                    table,
                    &service_consumes,
                    needs_kv,
                    &crons,
                    &queues,
                    ctx_uses_emit,
                );
                // P7.3 (#1303): `emit_wrangler_toml` builds a typed
                // `TomlDocument`. P7.6 (#1309, Decision E): the tree moves
                // straight into `Document::Toml`, unstringified — printed
                // only at the `bynk-driver` write boundary.
                compiled.push(StagedFile {
                    output_path: PathBuf::from(format!("workers/{dashes}/index.ts")),
                    document: Document::Ts(bynk_ts::TsProgram {
                        stmts: vec![bynk_ts::TsStmt::verbatim(
                            bynk_ts::VerbatimOrigin::NotYetConverted,
                            entry_ts,
                            None,
                        )],
                    }),
                    source_map: None,
                    debug_metadata: None,
                });
                // Events track, slice 0: this context's fan-out Durable
                // Object — emitted only when it actually publishes (mirrors
                // `emit_worker_compose`'s own `unit_table_uses_emit` gate on
                // `deps.__eventsDispatch`, so the two never disagree about
                // whether `env.EVENTS_FANOUT` is real).
                if ctx_uses_emit {
                    let fanout_ts = emitter::emit_events_fanout_do(ctx_name, &own_event_routes);
                    compiled.push(StagedFile {
                        output_path: PathBuf::from(format!("workers/{dashes}/events_fanout.ts")),
                        document: Document::Ts(bynk_ts::TsProgram {
                            stmts: vec![bynk_ts::TsStmt::verbatim(
                                bynk_ts::VerbatimOrigin::NotYetConverted,
                                fanout_ts,
                                None,
                            )],
                        }),
                        source_map: None,
                        debug_metadata: None,
                    });
                }
                compiled.push(StagedFile {
                    output_path: PathBuf::from(format!("workers/{dashes}/compose.ts")),
                    document: Document::Ts(bynk_ts::TsProgram {
                        stmts: vec![bynk_ts::TsStmt::verbatim(
                            bynk_ts::VerbatimOrigin::NotYetConverted,
                            compose_ts,
                            None,
                        )],
                    }),
                    source_map: None,
                    debug_metadata: None,
                });
                compiled.push(StagedFile {
                    output_path: PathBuf::from(format!("workers/{dashes}/wrangler.toml")),
                    document: Document::Toml(wrangler_doc),
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
                    bynk_check::symbols::combined_types_for(ctx_name, &unit_tables, &unit_uses);
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
                    unit_callees.get(ctx_name),
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
                        bynk_check::symbols::combined_types_for(dep, &unit_tables, &unit_uses);
                    let all = bynk_check::contract::own_contract_hashes(dep_table, &dep_types);
                    let hashes: std::collections::BTreeMap<String, String> = all
                        .into_iter()
                        .filter(|(svc, _)| services.contains(svc))
                        .collect();
                    if !hashes.is_empty() {
                        expects.insert(dep.clone(), hashes);
                    }
                }
                if let Some(manifest) = emitter::contracts::emit_contracts_manifest(
                    &bynk_check::contract::own_contract_hashes(table, &own_types),
                    &expects,
                ) {
                    compiled.push(StagedFile {
                        output_path: PathBuf::from(format!(
                            "workers/{dashes}/{}",
                            emitter::contracts::CONTRACTS_MANIFEST
                        )),
                        document: Document::Json(manifest),
                        source_map: None,
                        debug_metadata: None,
                    });
                }

                let (reads, _) = emitter::secrets::secret_reads(table, &flattened);
                if let Some(manifest) = emitter::emit_secrets_manifest(table, &reads) {
                    compiled.push(StagedFile {
                        output_path: PathBuf::from(format!(
                            "workers/{dashes}/{}",
                            emitter::secrets::SECRETS_MANIFEST
                        )),
                        document: Document::Json(manifest),
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
        compiled.push(StagedFile {
            output_path: b.output_path.clone(),
            document: Document::Ts(bynk_ts::TsProgram {
                stmts: vec![bynk_ts::TsStmt::verbatim(
                    bynk_ts::VerbatimOrigin::NotYetConverted,
                    b.content.clone(),
                    None,
                )],
            }),
            source_map: None,
            debug_metadata: None,
        });
    }

    // v0.17: emit `package.json` only when an adapter declares npm deps, so
    // existing (adapter-free) projects are unchanged.
    if !npm_deps.is_empty() {
        compiled.push(StagedFile {
            output_path: PathBuf::from("package.json"),
            document: Document::Json(render_package_json(&npm_deps)),
            source_map: None,
            debug_metadata: None,
        });
    }

    // Runtime + tsconfig: emit once per project. The runtime sits at the
    // root of `out/` so every emitted file's `runtime.js` import resolves
    // relative to it. `tsconfig.json` is also at the root so `tsc -p out/
    // tsconfig.json` discovers every `.ts` file in the tree.
    compiled.push(StagedFile {
        output_path: PathBuf::from("runtime.ts"),
        document: Document::Ts(bynk_ts::TsProgram {
            stmts: vec![bynk_ts::TsStmt::verbatim(
                bynk_ts::VerbatimOrigin::NotYetConverted,
                emitter::emit_runtime_module(),
                None,
            )],
        }),
        source_map: None,
        debug_metadata: None,
    });
    compiled.push(StagedFile {
        output_path: PathBuf::from("tsconfig.json"),
        document: Document::Json(emitter::emit_tsconfig()),
        source_map: None,
        debug_metadata: None,
    });

    // `Artefacts.docs` is a `BTreeMap`, so it iterates in `output_path` order
    // naturally (Decision D, #1309) — no explicit sort needed here the way
    // the old `Vec<CompiledFile>` (sorted by `source_path`) required.
    let mut docs: BTreeMap<PathBuf, Document> = BTreeMap::new();
    for f in compiled {
        if let Some(sm) = &f.source_map {
            docs.insert(
                sibling_path(&f.output_path, "map"),
                Document::SourceMap(sm.clone()),
            );
        }
        if let Some(dbg) = &f.debug_metadata {
            docs.insert(
                sibling_path(&f.output_path, "bynkdbg.json"),
                Document::DebugSidecar(dbg.clone()),
            );
        }
        docs.insert(f.output_path, f.document);
    }
    ProjectOutput {
        artefacts: Artefacts { docs },
        discovered,
        // Populated by `compile_project` from the run's warning sink (ADR 0117).
        warnings: Vec::new(),
        // Populated by `finish_build` from the same `RunChecks::Checked` this
        // whole `ProjectOutput` was built from.
        snapshots: Vec::new(),
        // Likewise (#1078) — `Some` only when the registry was on.
        schema_lock: None,
    }
}

// P5.3 review (#1133): `resolve_consume_prefix` and `handler_cross_caps` used
// to have their own copies here, byte-identical to `bynk-check::project_model`'s
// (neither builds TypeScript, so neither had the codegen coupling that keeps
// `instantiate_provider_expr`/`native_platforms_of_context` below in this
// crate) — deleted, every call site repointed at
// `project_model::{resolve_consume_prefix, handler_cross_caps}`.

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
    unit_info: &BTreeMap<String, UnitInfo>,
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
    // P6.35 (design/tracks/the-ir.md §6a): no explicit `&AgentDecl` annotation
    // needed — the loop body's own `a.handlers` field access below already
    // pins the element type through inference.
    let mut agents: Vec<_> = info.table.agents.iter().collect();
    agents.sort_by_key(|(n, _)| (*n).clone());
    for (agent, a) in agents {
        // #1187's slice 6 (Agent/Service given wiring): reads bynk-emit::ir's
        // own CapRefIr (lower_handler_given_ir — a standalone reader mirroring
        // lower_provider_given_ir, #1200) instead of walking the raw AST
        // CapRef directly. `caps` stays keyed by bare name (declaration-order-
        // first dedup across every handler), just storing CapRefIr instead of
        // CapRef.
        let mut caps: std::collections::BTreeMap<String, CapRefIr> =
            std::collections::BTreeMap::new();
        for h in &a.handlers {
            for g in lower_handler_given_ir(h) {
                // Events track, slice 0 (spine #936): see the matching skip
                // in `handler_cross_caps` — no `EventsProvider` exists for
                // compose (or a synthesised DO's own reconstructed deps) to
                // build.
                if g.name == "Events"
                    && info.flattened.get(g.name.as_str()).map(String::as_str) == Some("bynk")
                {
                    continue;
                }
                caps.entry(g.name.clone()).or_insert(g);
            }
        }
        if caps.is_empty() {
            continue;
        }
        let parts: Vec<String> = caps
            .iter()
            .map(|(key, g)| {
                let target_ctx = match &g.context {
                    Some(p) => resolve_consume_prefix(p, &info.consumes, &info.aliases)
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
                    None,
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
/// Locale capability track, slice 2 (#882, Decision C): the three extra
/// constructor arguments `LocaleProvider` receives when its composing
/// context has a uniquely-detected message bundle — the JS expressions
/// themselves (an identifier for `request`, and the two cross-commons-
/// imported bundle constants), not raw data, since they're spliced directly
/// into the generated `new bynk__binding.LocaleProvider(...)` call.
pub(crate) struct LocaleNegotiationArgs {
    pub(crate) request_expr: String,
    pub(crate) declared_locales_expr: String,
    pub(crate) reference_locale_expr: String,
}

/// `workers_ns` selects the namespace convention: a bodied provider's class
/// lives in `{ns}` under the bundle root but `handlers_{ns}` in a Worker
/// compose; external (binding) classes are `{ns}__binding` in both. When
/// `env_ident` is set (workers), env-taking first-party providers receive it
/// as a constructor argument.
///
/// Locale capability track, slice 2 (#882): `locale_negotiation`, when
/// `Some`, is threaded to exactly the `(bynk, LocaleProvider)` pair, the same
/// way `env_ident` is threaded to `provider_takes_env`'s pairs — a small,
/// closed set of first-party providers that need ambient, request-scoped
/// construction data no ordinary `given` clause could express.
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
    locale_negotiation: Option<&LocaleNegotiationArgs>,
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
    // #1187's Provider given/deps-wiring slice: reads bynk-emit::ir's own
    // CapRefIr (lower_provider_given_ir — a standalone reader, never a full
    // IrItem::Provider; see that function's own doc comment for why) instead
    // of walking the raw AST CapRef directly.
    let given: Vec<CapRefIr> = lower_provider_given_ir(provider);
    let deps_obj = if given.is_empty() {
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
        let deps: Vec<String> = given
            .iter()
            .map(|g| {
                let target_ctx = match &g.context {
                    Some(p) => resolve_consume_prefix(p, &consumed, &aliases)
                        .unwrap_or_else(|| provider_ctx.to_string()),
                    None => flattened
                        .get(&g.name)
                        .cloned()
                        .unwrap_or_else(|| provider_ctx.to_string()),
                };
                let expr = instantiate_provider_expr(
                    &target_ctx,
                    &g.name,
                    unit_tables,
                    unit_consumes,
                    unit_consumes_aliases,
                    unit_flattened,
                    workers_ns,
                    env_ident,
                    locale_negotiation,
                    referenced_units,
                );
                format!("{}: {}", g.name, expr)
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
    // Locale capability track, slice 2 (#882, Decision C): only the
    // `(bynk, LocaleProvider)` pair ever receives these — every other
    // provider's construction is unaffected since every other call site
    // passes `None`.
    if provider.external
        && provider_ctx == bynk_check::firstparty::BYNK_UNIT
        && provider.provider_name.name == "LocaleProvider"
        && let Some(loc) = locale_negotiation
    {
        args.push(loc.request_expr.clone());
        args.push(loc.declared_locales_expr.clone());
        args.push(loc.reference_locale_expr.clone());
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
/// Events track, slice 0 (spine #936): does any handler in this unit emit —
/// the `UnitTable`-level analogue of `emitter::commons_uses_emit`, needed
/// here because compose works from the project-wide `UnitTable` map, not a
/// single unit's `TypedCommons`. #1187's slice 6 plumbing: reads the
/// checker's own already-resolved `Callee::Capability{cap:"Events",
/// op:"emit"}` (`Events.emit[...]` dispatches through the ordinary
/// capability-call path, `bynk-check/src/checker/calls.rs`) instead of
/// `emitter::block_uses_emit`'s bare-`Ident("Events")`-receiver name match.
/// `callees` is `None` only defensively (a unit whose own check never ran) —
/// every call site this function actually reaches has already certified
/// (review of #1202: traced live, confirmed unreachable on the build path
/// today). A silent `false` here disables four emission gates at once (no
/// fan-out DO, no `dispatchToEventsFanout` import, no `EVENTS_FANOUT`
/// binding, no `__eventsDispatch` field) with no diagnostic — `debug_assert`
/// makes that invariant enforced, not just documented, so a future caller
/// that violates it fails loudly in tests rather than shipping a publishing
/// context that silently drops every emitted event.
///
/// `emitter::block_uses_emit` — the per-*handler* twin deciding
/// `emit_service`/`emit_agent`'s own `deps.__eventsDispatch` *parameter*
/// threading — reads the same resolved `Callee` now too (its own doc
/// comment has the story: the two checks briefly disagreed on a
/// locally-shadowed `Events` type between this function converting and
/// that one following, confirmed by a fixture that failed `tsc --strict` in
/// between, `1204_events_emit_shadowed_by_local_type`), so the two stay in
/// agreement on every input, not just the ones existing fixtures cover.
pub(crate) fn unit_table_uses_emit(
    table: &UnitTable,
    callees: Option<&HashMap<ExprId, bynk_check::checker::Callee>>,
) -> bool {
    let Some(callees) = callees else {
        debug_assert!(
            false,
            "unit_table_uses_emit: no Callee map for a checked unit"
        );
        return false;
    };
    let mut found = false;
    emitter::walk_unit_table_bodies(table, &mut |e| {
        if !found
            && matches!(
                callees.get(&e.id),
                Some(bynk_check::checker::Callee::Capability { cap, op })
                    if cap == "Events" && op == "emit"
            )
        {
            found = true;
        }
    });
    found
}

#[allow(clippy::too_many_arguments)]
fn emit_composition_root(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_callees: &HashMap<String, HashMap<ExprId, bynk_check::checker::Callee>>,
    // P6.x (#1232): see `EventSubscriberShape`'s own doc comment. Read by
    // `wants_envelope` below instead of walking a *different, already-
    // consumed* unit's raw `UnitTable` directly.
    unit_event_subscriber_shapes: &HashMap<String, HashMap<String, EventSubscriberShape>>,
    adapter_bindings: &HashMap<String, AdapterBinding>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    // v0.19 (decision 0025, D1): when the program's closure reaches a
    // platform-native unit, composeApp takes an optional `env` and threads it
    // to env-taking first-party providers. A bundle on Cloudflare is a single
    // Worker with `env` at its entry; native-free programs emit the v0.18
    // no-parameter signature unchanged.
    thread_env: bool,
    // Events track, slice 0 (spine #936): the project-wide subscriber table,
    // computed once by the caller (Workers mode needs the same table for its
    // own per-Worker fan-out wiring, so it is shared rather than rebuilt).
    event_subscribers: &BTreeMap<(String, String), Vec<(String, String)>>,
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
                // Events track, slice 0 (spine #936): a context whose
                // handlers emit needs its own `__eventsDispatch` closure
                // built by compose (§ `discover_event_subscribers`) even
                // when it consumes nothing and no other context consumes
                // it — `Events` is filtered out of `handler_cross_caps`
                // (there is no `EventsProvider`), so without this check a
                // publish-only context would never get a compose entry and
                // its service would simply never be called.
                || unit_table_uses_emit(table, unit_callees.get(name))
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
                        None, // Bundle mode has no inbound request (Decision A)
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
                        None, // Bundle mode has no inbound request (Decision A)
                        &mut referenced_units,
                    )
                ));
            }
        }
        // Events track, slice 0 (spine #936): a context whose handlers emit
        // gets an `__eventsDispatch` closure built here — Bundle/node mode
        // has no isolate boundary to cross, so this dispatches in-process
        // directly into each subscriber's own `on event` handler (`.event`,
        // the object method `emit_service` gives `HandlerKind::Event`),
        // reusing whatever deps that subscriber context already builds in
        // this same loop (referenced by name; the arrow function body isn't
        // evaluated until well after every `const ...Deps` in `composeApp`
        // has run, so declaration order here doesn't matter). A publisher
        // with no subscribers still gets the field — its type is required —
        // just with an empty switch.
        if unit_table_uses_emit(table, unit_callees.get(ctx_name)) {
            let mut cases = String::new();
            for name in table.events.keys() {
                let Some(subs) = event_subscribers.get(&(ctx_name.clone(), name.clone())) else {
                    continue;
                };
                // ADR 0284: subscriber failure isolation — one subscriber's
                // throw is caught and logged, not left to abort delivery to
                // its siblings or propagate into the already-committed
                // publishing handler. Mirrors the Cloudflare fan-out DO's own
                // per-subscriber try/catch (`emit_events_fanout_do`) so the
                // two targets agree on this guarantee, not just on delivery.
                let calls: Vec<String> = subs
                    .iter()
                    .map(|(sub_ctx, sub_svc)| {
                        let sub_ns = sub_ctx.replace('.', "_");
                        // Events track, slice 2 (spine #936): the envelope
                        // is only forwarded to a subscriber that declared
                        // the optional second `env: EventEnvelope`
                        // parameter — a subscriber that kept `on event(e:
                        // E)` sees no change to its call at all. Slice 4
                        // (#985): also forwarded when the subscriber's
                        // protocol carries a `via schema(N)` clause, even if
                        // undeclared — `emit_service` inserts a synthetic
                        // `env` parameter in that case, and needs the value
                        // to line up positionally.
                        let wants_envelope = unit_event_subscriber_shapes
                            .get(sub_ctx)
                            .and_then(|m| m.get(sub_svc))
                            .is_some_and(|shape| shape.two_param_handler || shape.schema_dispatch);
                        // P7.2: deferred, not narrowed — a first attempt used the
                        // event's own bare name directly (`EventDecl::as_type_decl`
                        // keys the synthetic `TypeDecl` on it), on the theory that
                        // an event doubles as a named type. It broke real
                        // `tsc --strict` fixtures ("Cannot find name") — the bare
                        // name isn't necessarily in scope at this dispatch site
                        // (cross-context: publisher and subscriber are different
                        // units), the same qualification problem
                        // `ts_type_ref_qualified` exists to solve for handler
                        // wrappers elsewhere in this crate. Needs the same kind of
                        // scoped qualification, not a bare name — more than a
                        // same-line fix.
                        let call_args = if wants_envelope {
                            "ev.payload as any, ev.envelope".to_string()
                        } else {
                            "ev.payload as any".to_string()
                        };
                        format!(
                            "try {{ await {sub_ns}.{sub_svc}.event({call_args}, {sub_ns}Deps); }} catch (e) {{ console.error(\"EventsFanout delivery failed\", {{ event: ev.type, service: {sub_svc:?}, error: String(e) }}); }}"
                        )
                    })
                    .collect();
                cases.push_str(&format!("case {name:?}: {{ {} break; }} ", calls.join(" ")));
            }
            deps_entries.push(format!(
                "__eventsDispatch: async (events: Array<{}>) => {{ for (const ev of events) {{ switch (ev.type) {{ {cases}}} }} }}",
                crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
            ));
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
                let t_ns = t.replace('.', "_");
                // v0.54 (#655): a consumed context with an `on call … by c: Caller`
                // handler needs the *caller's* qualified name (this context) threaded
                // into that handler's deps as its `CallerId` identity (ADR 0092). The
                // shared `{t_ns}Surface` (built for the top-level entry with the
                // provider's own name) would carry the wrong caller, so build a
                // per-consumer surface instead. A caller-free provider keeps the
                // shared instance — byte-unchanged.
                let entry = if context_binds_caller(other) {
                    format!(
                        "{surface_key}: {t_ns}.makeSurface({t_ns}Deps, {})",
                        ts_string_literal(ctx_name)
                    )
                } else {
                    format!("{surface_key}: {t_ns}Surface")
                };
                surface_entries.push(entry);
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
            // The top-level entry addresses the context directly; there is no
            // calling context, so a `by c: Caller` handler reached this way reads
            // the context's own qualified name (a stable, non-empty `CallerId`
            // within the single-trust-domain bundle).
            let caller_arg = if context_binds_caller(table) {
                format!(", {}", ts_string_literal(ctx_name))
            } else {
                String::new()
            };
            out.push_str(&format!(
                "  const {ns}Surface = {ns}.makeSurface({ns}Deps{caller_arg});\n",
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

/// Context passed to the emitter so it can resolve cross-file and
/// cross-unit references into TypeScript import statements.
pub(crate) struct EmitProjectCtx {
    /// Source path of the file being emitted (relative to project root).
    pub source_path: PathBuf,
    /// Joined name of the commons or context this file belongs to.
    pub commons_name: String,
    /// Which file declares each name in the local unit.
    pub file_decl_index: FileDeclIndex,
    /// For each imported name, the joined name of the unit it came from.
    pub imported_from: HashMap<String, String>,
    /// For each imported name, the kind (commons vs context) of the source unit.
    pub imported_from_kind: HashMap<String, UnitKind>,
    /// For each imported unit, the file path that declares each name.
    pub imported_decl_paths: HashMap<String, HashMap<String, PathBuf>>,
    /// What kind of unit this is.
    pub unit_kind: UnitKind,
    /// For contexts: this context's qualified name (used as the brand for
    /// rebranded mixed-in types and exported types).
    pub owning_context: Option<String>,
    /// For contexts: exports of each consumed context (so the emitter knows
    /// which names to import and how).
    pub exports_for_consumed: HashMap<String, HashMap<String, Visibility>>,
    /// For contexts: full cross-context information (consumed contexts,
    /// aliases, consumed services and types). Mirrors what the resolver
    /// and checker see (v0.6).
    pub cross_context: resolver::CrossContextInfo,
    /// v0.8 build target. Workers mode reroutes cross-context calls through
    /// Service Bindings and adds per-Worker entry/composition artefacts.
    pub target: BuildTarget,
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
    pub agent_method_givens: HashMap<String, HashMap<String, Vec<CapRefIr>>>,
    /// v0.47: the context's actor declarations (merged across files), keyed by
    /// name. Used to resolve a handler's Bearer verification seam in `emit.rs`
    /// regardless of which file declares the actor.
    pub actors: HashMap<String, ActorDecl>,
    /// Events slice 3b (#978): each locally-declared event's resolved
    /// `@schema(N)` version (or `1` if absent), merged across files the same
    /// way `actors` is above — `Events.emit[E]`'s lowering site only has
    /// `E`'s bare name (the turbofish type argument), never its declaration,
    /// so this is threaded down to `ModuleCtx`/`LowerCtx` rather than
    /// re-derived from the per-file synthetic `Commons` `lower.rs` otherwise
    /// sees (which would silently miss an event declared in a sibling file).
    pub event_schema_versions: HashMap<String, i64>,
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
    ///
    /// P6.18: each entry is a resolved [`FnSig`] (the declaring unit's own
    /// `params`/`return_ty`, already `TyId`-typed), not a raw `FnDecl` —
    /// see [`build_emit_unit_ctx`]'s own doc comment for why.
    pub imported_methods: HashMap<String, Vec<FnSig>>,
    /// Which conditional `runtime.ts` helpers this file's emission referenced.
    ///
    /// Unlike every field above, this is an **output**, not an input: emission
    /// writes it (through `&self`, via interior mutability) and the header /
    /// import post-pass reads it back. It rides on the context because the
    /// producers — the `Bytes` kernel in `lower`, the boundary codecs in
    /// `serialisation`, the ICU formatters in `emit` — already receive `&ctx`,
    /// so no other signature has to change to carry the fact up.
    ///
    /// One `EmitProjectCtx` is built per emitted file, immediately before its
    /// `emit_project` call, so the flags cannot leak between files. Replaces a
    /// substring scan of the generated text; see `emitter::runtime_use`.
    pub runtime_use: crate::emitter::RuntimeUse,
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
    // #1187's slice 6 plumbing: reads the checker's own already-resolved
    // `Callee::Cross { unit, service }` (`RunChecks::Checked::unit_callees`'s
    // own doc comment has the full grounding) instead of re-deriving
    // cross-context-ness by flattening a receiver's own ident chain and
    // string-matching it against `consumed`/`aliases` — the identical
    // resolution `CrossContextInfo::resolve_prefix` already did once, at
    // check time, per call site. `consumed` stays, purely as the cheap
    // early-out below: an empty `consumes` list means no `Callee::Cross`
    // could exist in this unit's own bodies regardless, so skip the walk.
    callees: Option<&HashMap<ExprId, bynk_check::checker::Callee>>,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut out: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    if consumed.is_empty() {
        return out;
    }
    // See `unit_table_uses_emit`'s own matching `debug_assert` (review of
    // #1202) — `consumed` non-empty means this unit certified with a real
    // `consumes`, so `callees` missing here is the same "invariant broke a
    // thousand lines away" case, just silently thinning the contracts
    // manifest's `expects` instead of silently disabling emission.
    let Some(callees) = callees else {
        debug_assert!(
            false,
            "called_cross_context_services: no Callee map for a checked unit with a non-empty \
             consumes list"
        );
        return out;
    };
    emitter::walk_unit_table_bodies(table, &mut |e| {
        if let Some(bynk_check::checker::Callee::Cross { unit, service }) = callees.get(&e.id) {
            out.entry(unit.clone()).or_default().insert(service.clone());
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Regression (code review of #1114): a `sources` key that matches none
    /// of `trees`'s roots (shouldn't happen for a well-formed map — see
    /// `sources_to_discovered`'s own doc) must fall back to the *last* tree,
    /// matching the pre-R3.9 two-tree `partition`'s fallback, not silently
    /// switch to the first.
    #[test]
    fn sources_to_discovered_unmatched_key_falls_back_to_the_last_tree() {
        let trees = vec![
            (PathBuf::from("/proj/src"), PathBuf::from("src")),
            (PathBuf::from("/proj/tests"), PathBuf::from("tests")),
        ];
        let mut sources = HashMap::new();
        sources.insert(PathBuf::from("/proj/src/a.bynk"), "commons a\n".to_string());
        sources.insert(
            PathBuf::from("/elsewhere/stray.bynk"),
            "commons stray\n".to_string(),
        );
        let (_, discovered) = sources_to_discovered(&sources, &trees);
        let buckets = discovered.expect("a sources map always yields Some(Discovered)");
        assert_eq!(buckets[0], vec![PathBuf::from("/proj/src/a.bynk")]);
        assert_eq!(
            buckets[1],
            vec![PathBuf::from("/elsewhere/stray.bynk")],
            "an unmatched key must land in the last tree, not the first"
        );
    }

    /// Content-ownership track (#1086) slice 4: this crate's own
    /// `#[cfg(test)]` module can't depend on the cross-crate `bynk-testkit`
    /// (that crate depends on `bynk-emit` — a cyclic dev-dependency, the
    /// same class of issue slice 3 found for `bynk-ide`). Mirrors
    /// `bynk-testkit::compile_options_split` in-crate instead, directly
    /// against this crate's own `Roots`/`discover_project_files` — no second
    /// resolution to drift from the first. Keyed by the literal discovered
    /// path, not canonicalised, matching `bynk-testkit`'s own convention
    /// (canonicalising broke a project-consistency check the hard way in
    /// slice 3).
    /// Content-ownership track (#1086) slice 5: this crate's own tests can't
    /// depend on `bynk-testkit` (cyclic — `bynk-testkit` depends on
    /// `bynk-emit`), so its handful of sites that build a `Roots` and need
    /// real disk content for it mirror `bynk-testkit`'s own read, in-crate.
    fn read_disk_sources(roots: &Roots) -> HashMap<PathBuf, String> {
        discover_project_files(roots)
            .into_iter()
            .filter_map(|p| {
                let content = std::fs::read_to_string(&p).ok()?;
                Some((p, content))
            })
            .collect()
    }

    fn compile_options_split_with_sources(
        project_root: PathBuf,
        paths: ProjectPaths,
    ) -> CompileOptions {
        let roots = Roots::Split {
            project_root: project_root.clone(),
            paths: paths.clone(),
        };
        let sources = read_disk_sources(&roots);
        CompileOptions::split(project_root, paths).sources(sources)
    }

    // -- Finding #55/#65: memoized first-party parse must not leak gating ----

    /// The first-party parse cache (`firstparty_parsed`) is keyed per-source,
    /// not per-project — `phase_parse`'s `consumes`/`uses` gating still runs
    /// fresh for every project. Two in-memory projects that gate in
    /// *different* first-party units, compiled back-to-back in the same
    /// process (so both share the same cache), must each see exactly their
    /// own gated-in set: `bynk.map` (which itself `uses bynk.list`) for the
    /// first, `bynk.string` alone for the second — never the other's.
    #[test]
    fn firstparty_cache_does_not_leak_gating_across_projects() {
        let out = compile_in_memory(
            "commons app.only_map\n\nuses bynk.map\n\nfn f() -> Int { 1 }\n",
            BuildTarget::Bundle,
            Default::default(),
        )
        .unwrap_or_else(|_| panic!("`uses bynk.map` should compile"));
        let paths: Vec<String> = out
            .artefacts
            .docs
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(paths.iter().any(|p| p == "bynk/map.ts"), "{paths:?}");
        assert!(
            paths.iter().any(|p| p == "bynk/list.ts"),
            "bynk.map itself uses bynk.list, so list must be injected too: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("bynk/string")),
            "a project that never uses bynk.string must not gain it: {paths:?}"
        );

        let out2 = compile_in_memory(
            "commons app.only_string\n\nuses bynk.string\n\nfn f() -> String { \"x\" }\n",
            BuildTarget::Bundle,
            Default::default(),
        )
        .unwrap_or_else(|_| panic!("`uses bynk.string` should compile"));
        let paths2: Vec<String> = out2
            .artefacts
            .docs
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(paths2.iter().any(|p| p == "bynk/string.ts"), "{paths2:?}");
        assert!(
            !paths2.iter().any(|p| p.starts_with("bynk/map")),
            "the shared first-party parse cache must not leak the first \
             project's gating into this one: {paths2:?}"
        );
    }

    // -- Finding #64: `check_project` must not bail past an earlier error --

    /// `compile_project`'s `Mode::Build` bails at the first structural error
    /// (here, `exports capability` naming an undeclared capability) and never
    /// reaches the per-unit checking pass — so a completely separate file's
    /// test-body type error is silently dropped. `check_project`'s
    /// `Mode::Analyse` must report both.
    #[test]
    fn check_project_reports_a_test_body_error_past_an_earlier_structural_error() {
        let root = scratch_project(
            "check_past_error",
            &[
                ("bynk.toml", "[project]\nname = \"c\"\n"),
                (
                    "src/greet.bynk",
                    "context greet {\n  exports capability { Bogus }\n}\n",
                ),
                (
                    "src/math.bynk",
                    "commons math {\n  fn double(n: Int) -> Int { n * 2 }\n}\n",
                ),
                (
                    "tests/math_test.bynk",
                    "suite math\n\ncase \"broken\" {\n  let x: Int = \"not an int\"\n  expect x == 1\n}\n",
                ),
            ],
        );
        let options = compile_options_split_with_sources(
            root.to_path_buf(),
            try_read_project_paths(&root).expect("well-formed fixture manifest"),
        );

        let check = check_project(&options);
        assert!(check.has_errors());
        let categories: Vec<&str> = check.errors.iter().map(|ae| ae.error.category).collect();
        assert!(
            categories.contains(&"bynk.exports.undeclared_capability"),
            "{categories:?}"
        );
        assert!(
            categories.contains(&"bynk.types.let_annotation_mismatch"),
            "check_project must still report the test body's own type error \
             past the earlier structural error: {categories:?}"
        );

        // The contrast: `compile_project`'s bail-fast `Mode::Build` is the
        // defect `check_project` exists to route `bynk check` around.
        let failure = match compile_project(&options) {
            Err(f) => f,
            Ok(_) => panic!("the structural error must still fail a real build"),
        };
        let failure_categories: Vec<&str> =
            failure.errors.iter().map(|ae| ae.error.category).collect();
        assert!(
            !failure_categories.contains(&"bynk.types.let_annotation_mismatch"),
            "compile_project must still bail before the test-body check runs \
             (documents why check_project is a separate entry point): {failure_categories:?}"
        );
    }

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
            paths: try_read_project_paths(&root).expect("well-formed fixture manifest"),
        };
        let trees = roots.trees();
        assert_eq!(
            trees,
            vec![
                (root.join("src"), PathBuf::from("src")),
                (root.join("tests"), PathBuf::from("tests")),
            ],
            "the fixture must actually be two-rooted"
        );
        let sources = read_disk_sources(&roots);
        let run = run_checks(
            &trees,
            BuildTarget::Bundle,
            Platform::default(),
            ImportExt::Js,
            Mode::Analyse,
            &sources,
            &roots.excludes(),
            None,
            false,
            &SchemaLock::Off,
            roots.project_root(),
            &Arc::new(Types::new()),
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

    /// Regression (code review of #1114): an adapter declared outside the
    /// first `include` tree used to have its `binding` module resolved
    /// against `trees[0]` unconditionally (`phase_group`'s old `src_root:
    /// &Path` parameter) — a project with `include = ["src", "adapters",
    /// "tests"]` and an adapter under `adapters/` would look for its binding
    /// under `src/`, fail to find it, and report `bynk.adapter.no_binding`
    /// even though the binding file exists right beside the adapter.
    #[test]
    fn adapter_binding_resolves_against_its_own_include_tree() {
        let root = scratch_project(
            "adapter_binding_tree",
            &[
                (
                    "bynk.toml",
                    "[project]\nname = \"a\"\n\n[paths]\ninclude = [\"src\", \"adapters\", \"tests\"]\n",
                ),
                (
                    "src/math.bynk",
                    "commons math {\n  fn double(n: Int) -> Int { n * 2 }\n}\n",
                ),
                (
                    "adapters/payments.bynk",
                    "adapter payments {\n  binding \"./payments.binding.ts\"\n\n  exports capability { Pay }\n\n  capability Pay {\n    fn charge(amount: Int) -> Effect[String]\n  }\n\n  provides Pay = RealPay\n}\n",
                ),
                (
                    "adapters/payments.binding.ts",
                    "import type { Pay } from \"./payments.js\";\n\nexport class RealPay implements Pay {\n  async charge(amount: number): Promise<string> {\n    return \"ok\";\n  }\n}\n",
                ),
            ],
        );
        let options = compile_options_split_with_sources(
            root.to_path_buf(),
            try_read_project_paths(&root).expect("well-formed fixture manifest"),
        );
        let out = compile_project(&options).unwrap_or_else(|f| {
            panic!(
                "adapter with a binding in a non-first include tree must compile: {}",
                render(&f.errors)
            )
        });
        let names: Vec<String> = out
            .artefacts
            .docs
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            names.contains(&"payments.binding.ts".to_string()),
            "expected the binding to be copied into the output among {names:?}"
        );
    }

    /// Regression (code review of the #1114 fix itself): `tree_root_for`
    /// compared `pf.abs_path()` (always absolute, via `std::path::absolute`)
    /// against `trees`' roots as-is — for the ordinary CLI shape (a relative
    /// project root, e.g. `bynkc build .`), every tree root stays relative,
    /// so `starts_with` never matched and this silently fell back to
    /// `trees[0]` for every file, reproducing the exact bug the fix above
    /// exists to close. `scratch_project`-based tests never caught this
    /// because their project root is always an absolute temp path.
    #[test]
    fn tree_root_for_matches_against_a_relative_tree_root() {
        let trees = vec![
            (PathBuf::from("src"), PathBuf::from("src")),
            (PathBuf::from("adapters"), PathBuf::from("adapters")),
        ];
        let root = Path::new("adapters");
        // Relative, exactly as `discover_bynk_files`/`phase_parse` would pass
        // it when `Roots::Split.project_root` is itself relative.
        let rel_path = root.join("payments.bynk");
        let (parsed, _warnings) = parse_sources(
            root,
            Path::new("adapters"),
            &rel_path,
            "adapter payments {\n  binding \"./payments.binding.ts\"\n\n  exports capability { Pay }\n\n  capability Pay {\n    fn charge(amount: Int) -> Effect[String]\n  }\n\n  provides Pay = RealPay\n}\n".to_string(),
            &mut 0,
            &mut 0,
        )
        .expect("trivial adapter source must parse");
        assert_eq!(
            project_model::tree_root_for(&trees, &parsed[0]),
            Path::new("adapters"),
            "must resolve to the adapter's own (relative) tree root, not trees[0] (\"src\")"
        );
    }

    /// Regression (code review of #1114): the emitted test module's
    /// discovered-case location used to key off `trees.get(1)`'s prefix
    /// unconditionally (`tests_prefix` in `process_tests`/
    /// `emit_test_module`) — a project with `include = ["src", "examples",
    /// "tests"]` would prefix every discovered case's location with
    /// `examples/` (the second tree) even though the suite actually lives
    /// under `tests/` (the third).
    #[test]
    fn discovered_case_location_uses_the_suite_files_own_tree() {
        let root = scratch_project(
            "test_tree_prefix",
            &[
                (
                    "bynk.toml",
                    "[project]\nname = \"t\"\n\n[paths]\ninclude = [\"src\", \"examples\", \"tests\"]\n",
                ),
                (
                    "src/math.bynk",
                    "commons math {\n  fn double(n: Int) -> Int { n * 2 }\n}\n",
                ),
                (
                    "tests/math_test.bynk",
                    "suite math\n\ncase \"doubles\" {\n  expect double(2) == 4\n}\n",
                ),
            ],
        );
        let options = compile_options_split_with_sources(
            root.to_path_buf(),
            try_read_project_paths(&root).expect("well-formed fixture manifest"),
        );
        let out = compile_project(&options).unwrap_or_else(|f| {
            panic!(
                "a suite in the third include tree must compile: {}",
                render(&f.errors)
            )
        });
        let locations: Vec<String> = out
            .discovered
            .iter()
            .flat_map(|s| &s.cases)
            .filter_map(|c| c.location.as_ref())
            .map(|l| l.path.clone())
            .collect();
        assert!(
            locations
                .iter()
                .all(|p| p.starts_with("tests/") && !p.starts_with("examples/")),
            "case locations must key off the suite file's own tree (`tests/`), not the \
             second `include` tree (`examples/`): {locations:?}"
        );
    }

    /// #57 (testing track): a two-file, cross-referencing project compiled
    /// entirely through the public `compile_project` API with no on-disk
    /// tree at all — `CompileOptions::sources` replaces what
    /// `scratch_project` below has to fake with real temp-directory I/O.
    /// Before this seam, exercising `uses` across two units from inside
    /// `bynk-emit`'s own tests meant either a `scratch_project` (real files,
    /// cleaned up on drop) or `bynkc`'s on-disk fixtures one crate up.
    #[test]
    fn compile_project_with_in_memory_sources_resolves_a_cross_unit_uses() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("shapes.bynk"),
            "commons shapes\n\ntype Circle = { radius: Int }\n".to_string(),
        );
        sources.insert(
            PathBuf::from("app.bynk"),
            "commons app\n\nuses shapes\n\nfn area(c: Circle) -> Int {\n  c.radius * c.radius\n}\n"
                .to_string(),
        );
        let options = CompileOptions::single(".").sources(sources);
        let out = compile_project(&options).unwrap_or_else(|f| {
            panic!(
                "in-memory sources project should compile: {:?}",
                ProjectFailure::flatten(f)
            )
        });
        let names: Vec<String> = out
            .artefacts
            .docs
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            names.contains(&"shapes.ts".to_string()),
            "expected shapes.ts among {names:?}"
        );
        assert!(
            names.contains(&"app.ts".to_string()),
            "expected app.ts among {names:?}"
        );
        let app_ts = out.artefacts.docs.get(Path::new("app.ts")).unwrap().text();
        assert!(
            app_ts.contains("radius"),
            "app.ts should reference the cross-unit Circle field:\n{app_ts}"
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
            paths: try_read_project_paths(root).expect("well-formed fixture manifest"),
        };
        let trees = roots.trees();
        let sources = read_disk_sources(&roots);
        let run = run_checks(
            &trees,
            BuildTarget::Bundle,
            Platform::default(),
            ImportExt::Js,
            Mode::Analyse,
            &sources,
            &roots.excludes(),
            None,
            false,
            &SchemaLock::Off,
            roots.project_root(),
            &Arc::new(Types::new()),
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
        let paths = try_read_project_paths(&root).expect("well-formed fixture manifest");
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

    /// v0.29.4: assembly yields exactly one `UnitInfo` per group, every facet
    /// present, with `exports`/`aliases`/`flattened` defaulting to empty for a
    /// unit absent from those (genuinely optional) producer maps — reproducing
    /// the old `.unwrap_or(empty)` read semantics as a total field.
    #[test]
    fn assemble_unit_info_yields_one_record_per_group_with_all_facets() {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        groups.insert("a.commons".to_string(), vec![0, 1]);
        groups.insert("a.context".to_string(), vec![2]);

        let mut kinds: BTreeMap<String, UnitKind> = BTreeMap::new();
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

        let info = project_model::assemble_unit_info(
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

    // -- #397: analyse_in_memory_with_types exposes expr_types (ADR 0094) -----

    #[test]
    fn analyse_in_memory_with_types_reports_expr_types_for_clean_source() {
        let src = "commons app.demo\n\nfn good() -> Int {\n  42\n}\n";
        let out = analyse_in_memory_with_types(src, BuildTarget::Bundle, Platform::default());
        assert!(
            out.errors.is_empty(),
            "clean source should have no errors: {:?}",
            out.errors
                .iter()
                .map(|e| &e.error.message)
                .collect::<Vec<_>>()
        );
        let offset = src.find("42").expect("source mentions 42");
        let ty = bynk_check::expr_types::type_at_offset(&out.expr_types, offset);
        assert_eq!(
            ty.map(|t| t.display(&out.ty_intern)),
            Some("Int".to_string())
        );
    }

    #[test]
    fn analyse_in_memory_with_types_is_partial_under_a_sibling_error() {
        // ADR 0094: a function that types cleanly still contributes its
        // `expr_types` even though a *different* function in the same file
        // has an error — `check_record`'s pre-ADR-0094 all-or-nothing gate
        // applied per-file, not per-function, and this is the change that
        // relaxed it. Hover (#397) depends on this: it must not go blank
        // over a well-typed expression just because some other function in
        // the buffer is mid-edit and broken.
        let src = "commons app.demo\n\n\
            fn good() -> Int {\n  42\n}\n\n\
            fn bad() -> Int {\n  \"oops\"\n}\n";
        let out = analyse_in_memory_with_types(src, BuildTarget::Bundle, Platform::default());
        assert!(
            !out.errors.is_empty(),
            "the broken function must still be reported"
        );
        let offset = src.find("42").expect("source mentions 42");
        let ty = bynk_check::expr_types::type_at_offset(&out.expr_types, offset);
        assert_eq!(
            ty.map(|t| t.display(&out.ty_intern)),
            Some("Int".to_string()),
            "the clean function's types must survive the sibling error"
        );
    }

    #[test]
    fn analyse_in_memory_still_returns_exactly_the_typed_variants_errors() {
        // Pins the refactor: `analyse_in_memory` must keep delegating to
        // `analyse_in_memory_with_types` rather than drift into a second,
        // independently-maintained `run_checks` call.
        let src = "commons app.demo\n\nfn bad() -> Int {\n  \"oops\"\n}\n";
        let errs = analyse_in_memory(src, BuildTarget::Bundle, Platform::default());
        let typed = analyse_in_memory_with_types(src, BuildTarget::Bundle, Platform::default());
        assert_eq!(errs.len(), typed.errors.len());
        assert!(!errs.is_empty());
    }

    // -- T3.3b: `expr_types` is total (R4.3, R2.5, R4.9) -----------------

    #[test]
    fn a_diagnosed_resolution_failure_records_ty_error_instead_of_nothing() {
        // An empty list literal with no expected element type to infer from
        // (`bynk.types.uninferable_element_type`, `checker.rs`'s `type_of`
        // `ExprKind::ListLit` arm) is a genuine, diagnosed `type_of` failure
        // reachable from a plain `fn` — no resolver/handler-body plumbing
        // needed to reproduce it.
        let src = "commons app.demo\n\nfn bad() -> Int {\n  []\n}\n";
        let out = analyse_in_memory_with_types(src, BuildTarget::Bundle, Platform::default());
        assert!(
            out.errors
                .iter()
                .any(|e| e.error.category == "bynk.types.uninferable_element_type"),
            "expected the uninferable-element-type diagnostic: {:?}",
            out.errors
                .iter()
                .map(|e| &e.error.message)
                .collect::<Vec<_>>()
        );
        let offset = src.find("[]").expect("source mentions []");
        let ty = bynk_check::expr_types::type_at_offset(&out.expr_types, offset);
        assert_eq!(
            ty.map(|t| t.display(&out.ty_intern)),
            Some("<type error>".to_string()),
            "T3.3b: a diagnosed type_of failure must record Ty::Error, not leave the span \
             unrecorded — {:?}",
            ty
        );
    }
}
