//! Multi-file project compilation (v0.3 §3.2 and §3.3, v0.4 §3.5).
//!
//! A "project" is a directory tree of `.karn` source files. The dotted name
//! of a commons or context (e.g., `karn.time`, `commerce.orders`) maps to a
//! path under the project root — either a single file (`karn/time.karn`) or
//! a directory of files all sharing the same header (`karn/time/*.karn`).
//!
//! v0.4: each file is one of two kinds — commons or context. Both kinds share
//! the same multi-file directory machinery; they differ in body content
//! (contexts have `consumes`/`exports`, types are nominally per-context), in
//! visibility (contexts export only the types listed), and in TypeScript
//! emission (contexts re-brand types from used commons).
//!
//! Compilation proceeds in two passes:
//!   1. **Discover and parse** every `.karn` file. Group by qualified name
//!      and kind. Build a global symbol table where each unit contributes
//!      its declarations.
//!   2. **Resolve, type-check, and emit** each unit with full visibility of
//!      the units it transitively `uses` or `consumes`. Two passes keep
//!      `uses` cycles trivial — there is no order-of-evaluation, only
//!      declarative mixin.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::ast::*;
use crate::checker;
use crate::checker::{CapabilityInfo, CapabilityOpInfo, Ty};
use crate::emitter;
use crate::error::CompileError;
use crate::firstparty::{self, Platform};
use crate::hints::{FileHints, HintSink};
use crate::index::{IndexBuilder, ProjectIndex, RefSink, SiteRef, SymbolKind};
use crate::lexer;
use crate::parser;
use crate::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use crate::span::Span;

mod consistency;
mod diagnostics;
mod discovery;
mod graph;
mod paths;
mod symbols;
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
pub(crate) use validate::check_function_type_boundary_items;

/// One generated TypeScript file.
pub struct CompiledFile {
    /// The originating Karn source file, relative to the project root.
    pub source_path: PathBuf,
    /// Where the TS output should be written, relative to the output root.
    /// Mirrors the source tree, with `.karn` rewritten to `.ts`.
    pub output_path: PathBuf,
    /// The emitted TypeScript content.
    pub typescript: String,
}

/// Result of compiling a project.
pub struct ProjectOutput {
    pub files: Vec<CompiledFile>,
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

/// Compile a Karn project rooted at `root`, defaulting to the bundle build
/// target. Use [`compile_project_with_target`] to select a target.
pub fn compile_project(root: &Path) -> Result<ProjectOutput, Vec<CompileError>> {
    compile_project_with_target(root, BuildTarget::Bundle)
}

/// Compile a Karn project rooted at `root` with an explicit build target.
///
/// In `BuildTarget::Bundle` (default) the output is the existing v0.6+
/// single-bundle layout. In `BuildTarget::Workers` (v0.8) each context
/// becomes a Cloudflare Worker under `out/workers/<context-with-dashes>/`.
pub fn compile_project_with_target(
    root: &Path,
    target: BuildTarget,
) -> Result<ProjectOutput, Vec<CompileError>> {
    compile_project_inner(root, root, target, Platform::default())
}

/// v0.17: compile with an explicit deploy [`Platform`] (selects the `karn`
/// surface binding). The MVP ships `cloudflare` only.
pub fn compile_project_with_platform(
    root: &Path,
    target: BuildTarget,
    platform: Platform,
) -> Result<ProjectOutput, Vec<CompileError>> {
    compile_project_inner(root, root, target, platform)
}

/// v0.9.1: compile a Karn project where source and test units live in
/// separate subdirectories under `project_root`, configured via the supplied
/// [`ProjectPaths`]. Source-unit identity is rooted at `<project_root>/<src>`
/// and test-unit identity at `<project_root>/<tests>`; both kinds of paths
/// are validated through the same logic. Use this from `karnc test` so its
/// rooting matches `karnc compile`'s.
pub fn compile_project_with_split_paths(
    project_root: &Path,
    target: BuildTarget,
    paths: &ProjectPaths,
) -> Result<ProjectOutput, Vec<CompileError>> {
    compile_project_with_split_paths_full(project_root, target, paths)
        .map_err(ProjectFailure::flatten)
}

/// v0.24: split-paths build keeping attribution + snapshots on failure.
pub fn compile_project_with_split_paths_full(
    project_root: &Path,
    target: BuildTarget,
    paths: &ProjectPaths,
) -> Result<ProjectOutput, ProjectFailure> {
    let src_root = project_root.join(&paths.src);
    let tests_root = project_root.join(&paths.tests);
    compile_project_full(&src_root, &tests_root, target, Platform::default())
}

/// v0.24: analyse a project without building — non-bailing, overlay-aware,
/// file-attributed (ADR 0052). `overlay` maps canonicalised absolute paths
/// to buffer text layered over disk reads (unsaved editor buffers).
pub fn analyse_project(root: &Path, overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis {
    match compile_project_pipeline(
        root,
        root,
        BuildTarget::Bundle,
        Platform::default(),
        Mode::Analyse,
        overlay,
    ) {
        PipelineResult::Analysis(a) => a,
        PipelineResult::Build(_) => unreachable!("analyse mode returned a build result"),
    }
}

fn compile_project_inner(
    src_root: &Path,
    tests_root: &Path,
    target: BuildTarget,
    platform: Platform,
) -> Result<ProjectOutput, Vec<CompileError>> {
    compile_project_full(src_root, tests_root, target, platform).map_err(ProjectFailure::flatten)
}

/// v0.24: the build entry point that keeps attribution + snapshots on
/// failure, so the CLI can render project errors with source context.
pub fn compile_project_full(
    src_root: &Path,
    tests_root: &Path,
    target: BuildTarget,
    platform: Platform,
) -> Result<ProjectOutput, ProjectFailure> {
    match compile_project_pipeline(
        src_root,
        tests_root,
        target,
        platform,
        Mode::Build,
        &HashMap::new(),
    ) {
        PipelineResult::Build(r) => r,
        PipelineResult::Analysis(_) => unreachable!("build mode returned an analysis"),
    }
}

/// Phase 1: discover the `.karn` files under the source (and, in split mode,
/// the tests) root, and run the file-vs-directory conflict checks. Pushes any
/// discovery errors into `errors` and signals a pipeline bail via `Err(())`
/// (the caller terminates with `finish`); otherwise returns the discovered
/// `(src_files, tests_files)`.
fn phase_discovery(
    src_root: &Path,
    tests_root: &Path,
    split_mode: bool,
    errors: &mut ErrorSink,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), ()> {
    let src_files = match discover_karn_files(src_root) {
        Ok(f) => f,
        Err(e) => {
            errors.push_for(None, e);
            return Err(());
        }
    };
    let tests_files = if split_mode {
        // Tests directory is optional in split mode — a project may have no
        // tests yet. Missing directory is not an error.
        if tests_root.exists() {
            match discover_karn_files(tests_root) {
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
                "karn.project.no_sources",
                Span::default(),
                format!("no `.karn` source files found under {}", src_root.display()),
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
/// Then inject the first-party synthetic units (the `karn`/`karn.cloudflare`
/// adapters and the `karn.{list,map,string}` commons) that the project
/// consumes/uses. Returns the parsed units plus whether the `karn` and
/// `karn.cloudflare` adapters were injected; signals a pipeline bail via
/// `Err(())` when parsing produced errors and yielded no units at all.
#[allow(clippy::too_many_arguments)]
fn phase_parse(
    src_root: &Path,
    tests_root: &Path,
    split_mode: bool,
    src_files: &[PathBuf],
    tests_files: &[PathBuf],
    overlay: &HashMap<PathBuf, String>,
    errors: &mut ErrorSink,
    snapshots: &mut Vec<(PathBuf, String)>,
) -> Result<(Vec<ParsedFile>, bool, bool), ()> {
    let mut parsed: Vec<ParsedFile> = Vec::new();
    let parse_tree = |root: &Path,
                      files: &[PathBuf],
                      parsed: &mut Vec<ParsedFile>,
                      errors: &mut ErrorSink,
                      snapshots: &mut Vec<(PathBuf, String)>| {
        for path in files {
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            let source = match read_source(path, overlay) {
                Ok(s) => s,
                Err(e) => {
                    errors.push_for(
                        Some(&rel),
                        CompileError::new(
                            "karn.project.read_failed",
                            Span::default(),
                            format!("could not read `{}`: {e}", path.display()),
                        ),
                    );
                    continue;
                }
            };
            snapshots.push((rel.clone(), source.clone()));
            match parse_source(root, path, source) {
                Ok(pf) => parsed.push(pf),
                Err(errs) => errors.extend_for(Some(&rel), errs),
            }
        }
    };
    parse_tree(src_root, src_files, &mut parsed, errors, snapshots);
    if split_mode {
        parse_tree(tests_root, tests_files, &mut parsed, errors, snapshots);
    }
    if !errors.is_empty() && parsed.is_empty() {
        return Err(());
    }

    // v0.17: if any user unit consumes the first-party `karn` surface, inject it
    // as a synthetic adapter so it flows through the normal pipeline (tables,
    // exports, emission, compose). Its binding is supplied by the toolchain for
    // the selected platform (§4.2). Injected only when consumed, so adapter-free
    // projects are unchanged.
    let consumes_karn = parsed.iter().any(|pf| {
        pf.consumes()
            .iter()
            .any(|c| c.target.joined() == firstparty::KARN_UNIT)
    });
    if consumes_karn {
        match lexer::tokenize(firstparty::KARN_ADAPTER_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::KARN_ADAPTER_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                source_path: PathBuf::from("karn.karn"),
                source: firstparty::KARN_ADAPTER_SRC.to_string(),
                unit,
                kind: UnitKind::Adapter,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.19: likewise the first-party `karn.cloudflare` platform adapter —
    // injected only when consumed, binding supplied by the toolchain. The
    // unit name sits inside the reserved `karn.*` prefix (decision 0026).
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
                source_path: PathBuf::from("karn/cloudflare.karn"),
                source: firstparty::CLOUDFLARE_ADAPTER_SRC.to_string(),
                unit,
                kind: UnitKind::Adapter,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.20b: the first-party collection commons. Unlike the adapters above
    // these are *library* units — plain Karn commons of generic functions —
    // imported via `uses` rather than `consumes`, and injected the same way
    // so they flow through the ordinary commons pipeline (tables, uses
    // resolution, emission). `karn.map` itself `uses karn.list`, so using
    // the former injects both.
    let uses_unit = |parsed: &[ParsedFile], unit: &str| {
        parsed
            .iter()
            .any(|pf| pf.uses().iter().any(|u| u.target.joined() == unit))
    };
    let uses_map = uses_unit(&parsed, firstparty::MAP_UNIT);
    if uses_map {
        match lexer::tokenize(firstparty::KARN_MAP_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::KARN_MAP_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                source_path: PathBuf::from("karn/map.karn"),
                source: firstparty::KARN_MAP_SRC.to_string(),
                unit,
                kind: UnitKind::Commons,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    if uses_map || uses_unit(&parsed, firstparty::LIST_UNIT) {
        match lexer::tokenize(firstparty::KARN_LIST_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::KARN_LIST_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                source_path: PathBuf::from("karn/list.karn"),
                source: firstparty::KARN_LIST_SRC.to_string(),
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
        match lexer::tokenize(firstparty::KARN_STRING_SRC)
            .map_err(|e| vec![e])
            .and_then(|toks| parser::parse_unit(&toks, firstparty::KARN_STRING_SRC))
        {
            Ok(unit) => parsed.push(ParsedFile {
                source_path: PathBuf::from("karn/string.karn"),
                source: firstparty::KARN_STRING_SRC.to_string(),
                unit,
                kind: UnitKind::Commons,
                synthetic: true,
            }),
            Err(errs) => errors.extend_for(None, errs),
        }
    }

    Ok((parsed, consumes_karn, consumes_cloudflare))
}

/// Phase 3: group the parsed units by qualified name (production units, unit
/// tests, and integration suites tracked separately), run the per-directory
/// and path/name consistency checks, enforce the reserved `karn` namespace and
/// the adapter `binding` rules, resolve each adapter's binding module, and fold
/// the adapters' pinned npm dependencies. Pushes diagnostics into `errors` and
/// returns the production `groups`/`kinds`, the `test`/`integration` groups, the
/// resolved `adapter_bindings`, and the collected `npm_deps`.
#[allow(clippy::type_complexity)]
fn phase_group(
    parsed: &[ParsedFile],
    src_root: &Path,
    split_mode: bool,
    platform: Platform,
    consumes_karn: bool,
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
    if let Err(e) = check_directory_name_consistency(parsed) {
        errors.extend_for(None, e);
    }
    if let Err(e) = check_directory_kind_consistency(parsed) {
        errors.extend_for(None, e);
    }
    // A group must agree on kind across all its files (different name but
    // same kind is fine; same name but different kind is an error).
    if let Err(e) = check_group_kind_consistency(parsed, &groups) {
        errors.extend_for(None, e);
    }
    // Each file's path must match its declared qualified name.
    if let Err(e) = check_path_name_alignment(parsed) {
        errors.extend_for(None, e);
    }
    // v0.9.1: in split-paths mode, also align test-file paths against the
    // target qualified name. In single-tree mode tests live wherever the
    // user puts them, so the check doesn't apply.
    if split_mode && let Err(e) = check_test_path_alignment(parsed) {
        errors.extend_for(None, e);
    }

    // v0.20a: function types are confined to non-boundary positions.
    let mut fn_boundary_errors: Vec<CompileError> = Vec::new();
    check_function_type_boundaries(parsed, &mut fn_boundary_errors);
    errors.extend_for(None, fn_boundary_errors);

    // v0.17: the `karn` root namespace is reserved for the toolchain. No user
    // unit of any kind may be named `karn` or `karn.*` (§3.4).
    for pf in parsed {
        if pf.synthetic {
            continue;
        }
        let qn = pf.unit.name();
        if qn.parts.first().is_some_and(|p| p.name == "karn") {
            errors.push_for(None,
                CompileError::new(
                    "karn.namespace.reserved",
                    qn.span,
                    format!(
                        "`{}` uses the reserved `karn` namespace — the `karn` root is reserved for the toolchain's conformance surface",
                        qn.joined()
                    ),
                )
                .with_note("rename the unit so its first segment is not `karn`"),
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
                errors.push_for(None,
                    CompileError::new(
                        "karn.adapter.no_binding",
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
    // v0.17: the toolchain supplies the `karn` surface's binding, platform-keyed.
    if consumes_karn {
        adapter_bindings.insert(
            firstparty::KARN_UNIT.to_string(),
            AdapterBinding {
                output_path: PathBuf::from(platform.karn_binding_filename()),
                content: platform.karn_binding_source().to_string(),
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
                errors.push_for(None,
                    CompileError::new(
                        "karn.adapter.no_binding",
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
                errors.push_for(None,
                    CompileError::new(
                        "karn.requires.unpinned_dependency",
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
        let mut table_errors: Vec<CompileError> = Vec::new();
        let table = build_unit_table(name, kind, indices, parsed, &mut table_errors);
        errors.extend_for(None, table_errors);
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
                        None,
                        CompileError::new(
                            "karn.uses.unknown_commons",
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
                    errors.push_for(None,
                        CompileError::new(
                            "karn.uses.target_is_context",
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
                        None,
                        CompileError::new(
                            "karn.uses.self_reference",
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
            refs.enter_file(&parsed[i].source_path, name, parsed[i].synthetic);
            for c in parsed[i].consumes() {
                let target = c.target.joined();
                if kind != UnitKind::Context && kind != UnitKind::Adapter {
                    errors.push_for(None,
                        CompileError::new(
                            "karn.consumes.in_commons",
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
                    errors.push_for(None,
                        CompileError::new(
                            "karn.adapter.consumes_requires_selection",
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
                        None,
                        CompileError::new(
                            "karn.consumes.unknown_context",
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
                    errors.push_for(None,
                        CompileError::new(
                            "karn.consumes.target_is_commons",
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
                    errors.push_for(None,
                        CompileError::new(
                            "karn.adapter.consumes_context",
                            c.span,
                            format!(
                                "adapter `{name}` cannot `consumes` the context `{target}` — adapter dependencies are adapter-to-adapter"
                            ),
                        )
                        .with_note(
                            "an adapter may only depend on capabilities exported by other adapters (e.g. the `karn` surface)",
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
                        None,
                        CompileError::new(
                            "karn.consumes.self_reference",
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
                                None,
                                CompileError::new(
                                    "karn.given.cross_context_unknown_capability",
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
                            errors.push_for(None, CompileError::new(
                                "karn.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "flattened capability `{}` clashes with a capability declared locally — use qualified `given {target}.{}` instead",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
                        if let Some(prev) = flattened.get(&cap.name) {
                            errors.push_for(None, CompileError::new(
                                "karn.consumes.capability_name_clash",
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
                    errors.push_for(None,
                        CompileError::new(
                            "karn.consumes.alias_conflict",
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
            let alias_span = parsed_alias_span(parsed, &groups[name], alias).unwrap_or_default();
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
                errors.push_for(None,
                    CompileError::new(
                        "karn.consumes.alias_conflict",
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
                    let span = uses_span_of(parsed, &groups[name], t).unwrap_or_default();
                    errors.push_for(None,
                        CompileError::new(
                            "karn.uses.name_conflict",
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
                    let span = uses_span_of(parsed, &groups[name], t).unwrap_or_default();
                    errors.push_for(None,
                        CompileError::new(
                            "karn.uses.name_conflict",
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
            refs.enter_file(&parsed[i].source_path, name, parsed[i].synthetic);
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
                            None,
                            CompileError::new(
                                "karn.exports.duplicate_in_clause",
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
                        errors.push_for(None,
                            CompileError::new(
                                "karn.exports.undeclared_type",
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
                                None,
                                CompileError::new(
                                    "karn.exports.duplicate_export",
                                    n.span,
                                    format!("type `{}` is exported more than once", n.name),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        } else {
                            errors.push_for(None,
                                CompileError::new(
                                    "karn.exports.conflicting_visibility",
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
            refs.enter_file(&parsed[i].source_path, name, parsed[i].synthetic);
            for clause in parsed[i].exports() {
                if !matches!(clause.kind, ExportKind::Capability) {
                    continue;
                }
                for n in &clause.names {
                    if let Some(prev) = seen.get(&n.name) {
                        errors.push_for(
                            None,
                            CompileError::new(
                                "karn.exports.duplicate_export",
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
                        errors.push_for(None,
                            CompileError::new(
                                "karn.exports.undeclared_capability",
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
                        errors.push_for(None,
                            CompileError::new(
                                "karn.exports.capability_not_provided",
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
fn phase_validate_providers(unit_tables: &HashMap<String, UnitTable>, errors: &mut ErrorSink) {
    for (name, table) in unit_tables {
        let _ = name;
        for (cap_name, provider) in &table.providers {
            // v0.17: an external provider has no Karn body to match against the
            // capability — its implementation is the binding, checked by `tsc`.
            if provider.external {
                continue;
            }
            let Some(cap) = table.capabilities.get(cap_name) else {
                errors.push_for(None,
                    CompileError::new(
                        "karn.provider.unknown_capability",
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
                        None,
                        CompileError::new(
                            "karn.provider.missing_operation",
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
                    errors.push_for(None, CompileError::new(
                        "karn.provider.extra_operation",
                        prov_op.span,
                        format!(
                            "provider operation `{}.{}` does not match any operation in capability `{}`",
                            provider.provider_name.name, prov_op.name.name, cap_name
                        ),
                    ));
                    continue;
                };
                if cap_op.params.len() != prov_op.params.len() {
                    errors.push_for(None, CompileError::new(
                        "karn.provider.signature_mismatch",
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
                        errors.push_for(None, CompileError::new(
                            "karn.provider.signature_mismatch",
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
                    errors.push_for(None, CompileError::new(
                        "karn.provider.signature_mismatch",
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
    groups: &HashMap<String, Vec<usize>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    empty_exports: &HashMap<String, Visibility>,
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
    for t in unit_consumes.get(name).into_iter().flatten() {
        let used = unit_tables.get(t).expect("consumed unit table present");
        let used_exports = exports_visibility.get(t).unwrap_or(empty_exports);
        for (type_name, vis) in used_exports {
            let Some(decl) = used.types.get(type_name) else {
                continue;
            };
            if combined_types.contains_key(type_name) {
                // Name conflict between local/uses and consumed export.
                let consumes_span = consumes_span_of(parsed, &groups[name], t).unwrap_or_default();
                errors.push_for(None,
                    CompileError::new(
                        "karn.consumes.name_conflict",
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
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
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

    for t in unit_uses.get(name).into_iter().flatten() {
        let used = unit_tables.get(t).expect("used unit table present");
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
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_file_index: &HashMap<String, FileDeclIndex>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    empty_exports: &HashMap<String, Visibility>,
    imported_from: &HashMap<String, String>,
    imported_from_kind: &HashMap<String, UnitKind>,
    owning_context_for_emit: &Option<String>,
    consumed_types: &HashMap<String, ConsumedType>,
    cross_context_for_file: &resolver::CrossContextInfo,
    typed: &checker::TypedCommons,
    target: BuildTarget,
    compiled: &mut Vec<CompiledFile>,
) {
    // Build the emitter context.
    let mut imported_decl_paths: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
    for t in unit_uses.get(name).into_iter().flatten() {
        if let Some(target_index) = unit_file_index.get(t) {
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
    for t in unit_consumes.get(name).into_iter().flatten() {
        if let Some(target_index) = unit_file_index.get(t) {
            let mut paths: HashMap<String, PathBuf> = HashMap::new();
            // Only expose exported names — the emitter needs to know
            // which file declares them so it can render the import.
            let exports_for_target = exports_visibility.get(t).unwrap_or(empty_exports);
            for n in exports_for_target.keys() {
                if let Some(p) = target_index.types.get(n) {
                    paths.insert(n.clone(), p.clone());
                }
            }
            imported_decl_paths.insert(t.clone(), paths);
        }
    }

    let exports_local = exports_visibility.get(name).cloned().unwrap_or_default();
    let exports_for_consumed = unit_consumes
        .get(name)
        .into_iter()
        .flatten()
        .map(|t| {
            (
                t.clone(),
                exports_visibility.get(t).cloned().unwrap_or_default(),
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
            let target_kind = kinds.get(unit).copied();
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
        compute_boundary_type_owners(name, unit_consumes, unit_tables, parsed, unit_file_index)
    } else {
        HashMap::new()
    };

    let emit_ctx = EmitProjectCtx {
        source_path: emit_source_path,
        commons_name: name.to_string(),
        local_files: emit_local_files,
        file_decl_index: unit_file_index
            .get(name)
            .cloned()
            .unwrap_or_else(|| FileDeclIndex {
                types: HashMap::new(),
                fns: HashMap::new(),
                methods: HashMap::new(),
            }),
        imported_from: imported_from.clone(),
        imported_from_kind: imported_from_kind.clone(),
        imported_decl_paths: imported_decl_paths_emit,
        commons_dir: commons_dir_for(name),
        unit_kind: kind,
        owning_context: owning_context_for_emit.clone(),
        exports_local,
        exports_for_consumed,
        consumed_types: consumed_types.clone(),
        cross_context: cross_context_info,
        is_consumed_by_others: unit_consumes
            .iter()
            .any(|(_, targets)| targets.iter().any(|t| t == name)),
        target,
        boundary_type_owners,
        local_agents: unit_tables
            .get(name)
            .map(|t| t.agents.keys().cloned().collect())
            .unwrap_or_default(),
        consumed_adapters: unit_consumes
            .get(name)
            .into_iter()
            .flatten()
            .filter(|t| kinds.get(*t) == Some(&UnitKind::Adapter))
            .cloned()
            .collect(),
    };
    let ts = emitter::emit_project(typed, &emit_ctx);
    let output_path = if workers_mode && kind == UnitKind::Context {
        worker_handlers_output_path(name)
    } else {
        ts_output_path(&pf.source_path)
    };
    compiled.push(CompiledFile {
        source_path: pf.source_path.clone(),
        output_path,
        typescript: ts,
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
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    unit_file_index: &HashMap<String, FileDeclIndex>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    empty_exports: &HashMap<String, Visibility>,
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
    mode: Mode,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
    hints: &mut HintSink,
    compiled: &mut Vec<CompiledFile>,
) {
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
                unit_consumes,
                unit_consumes_aliases,
                unit_uses,
                unit_tables,
            );
            cci.flattened_caps = unit_flattened.get(name).cloned().unwrap_or_default();
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
        };
        refs.enter_file(&pf.source_path, name, pf.synthetic);
        // v0.27: synthetic and test/integration files record no hints —
        // neither surfaces in an editor (the `assemble_index` rule).
        hints.enter_file(
            &pf.source_path,
            pf.synthetic || matches!(pf.kind, UnitKind::Test | UnitKind::Integration),
        );
        if let Err(errs) = resolver::resolve_file_record(&resolved, refs) {
            errors.extend_for(Some(&pf.source_path), errs);
            continue;
        }
        let typed = match checker::check_record(resolved, refs, hints) {
            Ok(t) => t,
            Err(errs) => {
                errors.extend_for(Some(&pf.source_path), errs);
                continue;
            }
        };

        // Run the context-specific checks: forbidden construction,
        // private-type references.
        if kind == UnitKind::Context {
            let context_check_errs = check_context_constraints(&typed, consumed_types, local_names);
            if !context_check_errs.is_empty() {
                errors.extend_for(Some(&pf.source_path), context_check_errs);
                continue;
            }
        }

        // v0.5: check capability/provider/service/agent declarations.
        // v0.18: adapters run these too — an external provider's `given`
        // resolves through the same path as a bodied provider's (the
        // service/agent checks are vacuous for adapters, which have none).
        let mut typed = typed;
        let unit_table_owned = unit_tables.get(name).cloned();
        if (kind == UnitKind::Context || kind == UnitKind::Adapter)
            && let Some(table) = unit_table_owned.as_ref()
        {
            let v0_5_errs =
                check_v0_5_declarations(&mut typed, table, &cross_context_for_file, refs, hints);
            if !v0_5_errs.is_empty() {
                errors.extend_for(Some(&pf.source_path), v0_5_errs);
                continue;
            }
        }

        // Analyse mode stops at checked: emission is build-only.
        if mode == Mode::Analyse {
            continue;
        }
        emit_unit(
            name,
            kind,
            i,
            pf,
            indices,
            parsed,
            kinds,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_file_index,
            exports_visibility,
            empty_exports,
            imported_from,
            imported_from_kind,
            owning_context_for_emit,
            consumed_types,
            &cross_context_for_file,
            &typed,
            target,
            compiled,
        );
    }
}

fn compile_project_pipeline(
    src_root: &Path,
    tests_root: &Path,
    target: BuildTarget,
    platform: Platform,
    mode: Mode,
    overlay: &HashMap<PathBuf, String>,
) -> PipelineResult {
    let mut errors = ErrorSink::new();
    // v0.25 (ADR 0053): binding edges, recorded at the resolution sites and
    // assembled into the project index at the analyse exit.
    let mut refs = RefSink::new();
    // v0.27 (ADR 0056): inferred-type inlay hints, recorded at the checker's
    // binding sites. A sink (not part of the checker's Ok payload) so hints
    // survive the per-file error-`continue`s.
    let mut hints = HintSink::new();
    let mut snapshots: Vec<(PathBuf, String)> = Vec::new();
    let split_mode = src_root != tests_root;

    // -- 1. Discovery. --
    let (src_files, tests_files) =
        match phase_discovery(src_root, tests_root, split_mode, &mut errors) {
            Ok(files) => files,
            Err(()) => {
                return finish(
                    mode,
                    errors,
                    snapshots,
                    ProjectIndex::default(),
                    hints.take_files(),
                );
            }
        };

    // -- 2. Parse every file. --
    let (parsed, consumes_karn, consumes_cloudflare) = match phase_parse(
        src_root,
        tests_root,
        split_mode,
        &src_files,
        &tests_files,
        overlay,
        &mut errors,
        &mut snapshots,
    ) {
        Ok(out) => out,
        Err(()) => {
            return finish(
                mode,
                errors,
                snapshots,
                ProjectIndex::default(),
                hints.take_files(),
            );
        }
    };

    // -- 3. Group by (name, kind) and validate per-directory consistency. --
    let (groups, kinds, test_groups, integration_groups, adapter_bindings, npm_deps) = phase_group(
        &parsed,
        src_root,
        split_mode,
        platform,
        consumes_karn,
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

    // -- 5c. Detect `consumes` cycles. --
    let mut cycle_errors: Vec<CompileError> = Vec::new();
    detect_consumes_cycles(&unit_consumes, &mut cycle_errors);
    errors.extend_for(None, cycle_errors);

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
    phase_validate_providers(&unit_tables, &mut errors);

    if !errors.is_empty() && mode == Mode::Build {
        return finish(
            mode,
            errors,
            snapshots,
            ProjectIndex::default(),
            hints.take_files(),
        );
    }

    // -- 7. Build per-unit file index (which file declares which name). --
    let unit_file_index = phase_file_index(&groups, &parsed);

    // -- 8. For each unit, build the combined symbol space and run
    //       resolve+check per source file. --
    let mut compiled: Vec<CompiledFile> = Vec::new();
    let empty_exports = HashMap::new();

    for (name, indices) in &groups {
        let kind = *kinds.get(name).unwrap();
        let local_table = unit_tables.get(name).unwrap();
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
        ) = compose_unit_symbols(name, local_table, &unit_uses, &unit_tables);
        let consumed_types = merge_consumed_exports(
            name,
            &parsed,
            &groups,
            &unit_consumes,
            &unit_tables,
            &exports_visibility,
            &empty_exports,
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

        check_unit_files(
            name,
            kind,
            indices,
            &parsed,
            &kinds,
            &unit_tables,
            &unit_uses,
            &unit_consumes,
            &unit_consumes_aliases,
            &unit_flattened,
            &unit_file_index,
            &exports_visibility,
            &empty_exports,
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
            mode,
            &mut errors,
            &mut refs,
            &mut hints,
            &mut compiled,
        );
    }

    // v0.7: process test declarations. Each `test commerce.X` group resolves
    // its target, validates mocks against the target's capability/consumed-
    // context shapes, type-checks bodies with the target's privileged view,
    // and emits a per-target TypeScript test module under `tests/`.
    let mut test_errors: Vec<CompileError> = Vec::new();
    let (test_outputs, mut runnable_tests) = process_tests(
        &test_groups,
        &parsed,
        &kinds,
        &unit_tables,
        &exports_visibility,
        &unit_consumes,
        &unit_consumes_aliases,
        &unit_uses,
        &mut test_errors,
        &mut refs,
    );
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
        &mut integration_errors,
        &mut refs,
    );
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

    if mode == Mode::Analyse || !errors.is_empty() {
        let index = assemble_index(
            &parsed,
            &unit_uses,
            &unit_consumes,
            std::mem::take(&mut refs),
        );
        return finish(mode, errors, snapshots, index, hints.take_files());
    }

    compiled.extend(integration_outputs);
    runnable_tests.extend(integration_runnables);

    // v0.16: emit the combined top-level test runner once both passes are done,
    // so `tests/main.ts` aggregates unit and integration suites together.
    if !runnable_tests.is_empty() {
        let main_ts = emit_test_main(&runnable_tests);
        compiled.push(CompiledFile {
            source_path: PathBuf::from("tests/main.test.karn"),
            output_path: PathBuf::from("tests/main.ts"),
            typescript: main_ts,
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
                    source_path: PathBuf::from("compose.karn"),
                    output_path: PathBuf::from("compose.ts"),
                    typescript: compose_ts,
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
                let entry_ts = emitter::emit_worker_entry(ctx_name, table);
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
                });
                compiled.push(CompiledFile {
                    source_path: PathBuf::from(format!("workers/{dashes}/<compose>")),
                    output_path: PathBuf::from(format!("workers/{dashes}/compose.ts")),
                    typescript: compose_ts,
                });
                compiled.push(CompiledFile {
                    source_path: PathBuf::from(format!("workers/{dashes}/<wrangler>")),
                    output_path: PathBuf::from(format!("workers/{dashes}/wrangler.toml")),
                    typescript: wrangler,
                });
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
        });
    }

    // v0.17: emit `package.json` only when an adapter declares npm deps, so
    // existing (adapter-free) projects are unchanged.
    if !npm_deps.is_empty() {
        compiled.push(CompiledFile {
            source_path: PathBuf::from("<package.json>"),
            output_path: PathBuf::from("package.json"),
            typescript: render_package_json(&npm_deps),
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
    });
    compiled.push(CompiledFile {
        source_path: PathBuf::from("<tsconfig>"),
        output_path: PathBuf::from("tsconfig.json"),
        typescript: emitter::emit_tsconfig(),
    });

    compiled.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    PipelineResult::Build(Ok(ProjectOutput { files: compiled }))
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
        if let Some(p) = crate::firstparty::platform_of(&unit) {
            out.entry(p).or_insert(unit);
        }
    }
    out
}

/// v0.15: build the TypeScript expression instantiating the provider of
/// capability `cap` declared in `provider_ctx`, recursively wiring its `given`
/// dependencies — local sibling providers and cross-context capability
/// providers alike. Stateless providers, so fresh instances per use are fine.
///
/// v0.18 (spec §4.5/§5.1): a *bare* `given` name resolves through the
/// provider's own unit's flattened-capability map (`Fetch` → `karn`), falling
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
    // v0.18/v0.19: env-taking first-party providers (the karn surface's
    // SecretsProvider; karn.cloudflare's WorkersKv) receive the Worker `env`
    // explicitly — decisions 0021/0025. Keyed by (unit, class).
    if provider.external
        && crate::firstparty::provider_takes_env(provider_ctx, &provider.provider_name.name)
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
    header.push_str("// Generated by karnc — do not edit by hand.\n");
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
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    parsed: &[ParsedFile],
    unit_file_index: &HashMap<String, FileDeclIndex>,
) -> HashMap<String, BoundaryOwner> {
    let mut out: HashMap<String, BoundaryOwner> = HashMap::new();
    let Some(targets) = unit_consumes.get(consumer) else {
        return out;
    };
    let _ = parsed;
    for t in targets {
        let Some(table) = unit_tables.get(t) else {
            continue;
        };
        // Types declared in the consumed context (records, sums, refined,
        // opaque) — record them with the consumed context as owner.
        for type_name in table.types.keys() {
            out.insert(
                type_name.clone(),
                BoundaryOwner::Context { context: t.clone() },
            );
        }
        // Commons types `uses`-imported by the consumed context: their
        // file lookup is unit_file_index keyed by commons name.
    }
    // For consumer-side commons types (used in this context's exposed
    // signatures), look them up via this consumer's unit_file_index.
    if let Some(idx) = unit_file_index.get(consumer) {
        let _ = idx;
    }
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
    /// v0.17: consumed unit names that are adapters. An adapter is not a Worker,
    /// so in workers mode its capability types are imported from its root module
    /// (`<adapter>.ts`), not from a per-Worker `handlers.ts`.
    pub consumed_adapters: HashSet<String>,
}

/// Where a boundary-crossing type was declared.
#[derive(Debug, Clone)]
pub enum BoundaryOwner {
    /// Commons type. Path is project-relative to the `.karn` file declaring it.
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
