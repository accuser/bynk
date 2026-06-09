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

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::ast::*;
use crate::checker;
use crate::checker::{CapabilityInfo, CapabilityOpInfo, Ty};
use crate::emitter;
use crate::error::CompileError;
use crate::firstparty::{self, Platform};
use crate::lexer;
use crate::parser;
use crate::resolver::{self, MethodTable as ResolverMethodTable, ResolvedCommons};
use crate::span::Span;

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

/// v0.17 [DECISION L] stub: a version range is *unpinned* — and rejected — when
/// it is empty, `*`/`x`/`latest`, or otherwise carries no concrete version
/// number. A pinned range names at least one digit (`^5`, `~1.2`, `1.2.3`,
/// `>=1.0 <2`). No allow-list or registry check yet.
fn is_unpinned_range(range: &str) -> bool {
    let r = range.trim();
    if r.is_empty() || r == "*" || r.eq_ignore_ascii_case("x") || r.eq_ignore_ascii_case("latest") {
        return true;
    }
    !r.chars().any(|c| c.is_ascii_digit())
}

/// Render a minimal `package.json` carrying the adapter-declared dependencies.
fn render_package_json(deps: &std::collections::BTreeMap<String, String>) -> String {
    let mut out = String::from("{\n  \"dependencies\": {\n");
    let entries: Vec<String> = deps
        .iter()
        .map(|(pkg, range)| format!("    {}: {}", json_string(pkg), json_string(range)))
        .collect();
    out.push_str(&entries.join(",\n"));
    out.push_str("\n  }\n}\n");
    out
}

/// Minimal JSON string escaping for package names and version ranges.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Normalise a relative path by resolving `.` and `..` components, so a binding
/// clause like `./tokens.binding.ts` beside `src/tokens.karn` yields the output
/// path `tokens.binding.ts`.
fn normalize_rel(p: &Path) -> PathBuf {
    let mut out: Vec<std::ffi::OsString> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s.to_os_string()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.iter().collect()
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

/// v0.9.1: per-project source-tree layout, read from `karn.toml`'s `[paths]`
/// section.
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    /// Source-unit root, relative to the project root.
    pub src: PathBuf,
    /// Test-unit root, relative to the project root.
    pub tests: PathBuf,
}

impl ProjectPaths {
    /// The conventional layout used when `karn.toml` is absent: sources under
    /// `src/`, tests under `tests/`.
    pub fn conventional() -> Self {
        ProjectPaths {
            src: PathBuf::from("src"),
            tests: PathBuf::from("tests"),
        }
    }
}

/// v0.9.1: read `karn.toml` from `project_root`. Returns the conventional
/// layout if the file is missing or doesn't declare `[paths]`. Only `src` and
/// `tests` keys under `[paths]` are honoured; anything else is ignored. A
/// minimal hand-rolled TOML reader — we only need string-valued keys here.
pub fn read_project_paths(project_root: &Path) -> ProjectPaths {
    let toml_path = project_root.join("karn.toml");
    let mut paths = ProjectPaths::conventional();
    let Ok(content) = fs::read_to_string(&toml_path) else {
        return paths;
    };
    let mut in_paths_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_paths_section = section.trim() == "paths";
            continue;
        }
        if !in_paths_section {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        match key {
            "src" => paths.src = PathBuf::from(unquoted),
            "tests" => paths.tests = PathBuf::from(unquoted),
            _ => {}
        }
    }
    paths
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
    let src_root = project_root.join(&paths.src);
    let tests_root = project_root.join(&paths.tests);
    compile_project_inner(&src_root, &tests_root, target, Platform::default())
}

/// Internal: do the work, given a source root (for commons/contexts) and a
/// test root (for test units). When both roots are the same path the
/// behaviour is identical to the v0.4+ single-tree layout. When they differ
/// — v0.9.1's split-paths mode — sources and tests are discovered separately
/// and the new `inconsistent_test_path` check fires.
fn compile_project_inner(
    src_root: &Path,
    tests_root: &Path,
    target: BuildTarget,
    platform: Platform,
) -> Result<ProjectOutput, Vec<CompileError>> {
    let mut errors = Vec::new();
    let split_mode = src_root != tests_root;

    // -- 1. Discovery. --
    let src_files = match discover_karn_files(src_root) {
        Ok(f) => f,
        Err(e) => return Err(vec![e]),
    };
    let tests_files = if split_mode {
        // Tests directory is optional in split mode — a project may have no
        // tests yet. Missing directory is not an error.
        if tests_root.exists() {
            match discover_karn_files(tests_root) {
                Ok(f) => f,
                Err(e) => return Err(vec![e]),
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if src_files.is_empty() && tests_files.is_empty() {
        return Err(vec![CompileError::new(
            "karn.project.no_sources",
            Span::default(),
            format!("no `.karn` source files found under {}", src_root.display()),
        )]);
    }
    if let Err(e) = check_file_directory_conflicts(src_root, &src_files) {
        errors.extend(e);
    }
    if split_mode && let Err(e) = check_file_directory_conflicts(tests_root, &tests_files) {
        errors.extend(e);
    }

    // -- 2. Parse every file. --
    let mut parsed: Vec<ParsedFile> = Vec::new();
    for path in &src_files {
        match parse_file(src_root, path) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend(errs),
        }
    }
    if split_mode {
        for path in &tests_files {
            match parse_file(tests_root, path) {
                Ok(pf) => parsed.push(pf),
                Err(errs) => errors.extend(errs),
            }
        }
    }
    if !errors.is_empty() && parsed.is_empty() {
        return Err(errors);
    }

    // v0.17: if any user unit consumes the first-party `karn` surface, inject it
    // as a synthetic adapter so it flows through the normal pipeline (tables,
    // exports, emission, compose). Its binding is supplied by the toolchain for
    // the selected platform (§4.2). Injected only when consumed, so adapter-free
    // projects are unchanged.
    let consumes_karn = parsed
        .iter()
        .any(|pf| pf.consumes().iter().any(|c| c.target.joined() == "karn"));
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
            Err(errs) => errors.extend(errs),
        }
    }

    // -- 3. Group by (name, kind) and validate per-directory consistency. --
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
    if let Err(e) = check_directory_name_consistency(&parsed) {
        errors.extend(e);
    }
    if let Err(e) = check_directory_kind_consistency(&parsed) {
        errors.extend(e);
    }
    // A group must agree on kind across all its files (different name but
    // same kind is fine; same name but different kind is an error).
    if let Err(e) = check_group_kind_consistency(&parsed, &groups) {
        errors.extend(e);
    }
    // Each file's path must match its declared qualified name.
    if let Err(e) = check_path_name_alignment(&parsed) {
        errors.extend(e);
    }
    // v0.9.1: in split-paths mode, also align test-file paths against the
    // target qualified name. In single-tree mode tests live wherever the
    // user puts them, so the check doesn't apply.
    if split_mode && let Err(e) = check_test_path_alignment(&parsed) {
        errors.extend(e);
    }

    // v0.17: the `karn` root namespace is reserved for the toolchain. No user
    // unit of any kind may be named `karn` or `karn.*` (§3.4).
    for pf in &parsed {
        if pf.synthetic {
            continue;
        }
        let qn = pf.unit.name();
        if qn.parts.first().is_some_and(|p| p.name == "karn") {
            errors.push(
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
    for pf in &parsed {
        if pf.synthetic {
            continue;
        }
        if let Some(a) = pf.adapter() {
            let has_external = a
                .items
                .iter()
                .any(|it| matches!(it, CommonsItem::Provider(p) if p.external));
            if has_external && a.binding.is_none() {
                errors.push(
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
            "karn".to_string(),
            AdapterBinding {
                output_path: PathBuf::from(platform.karn_binding_filename()),
                content: platform.karn_binding_source().to_string(),
            },
        );
    }
    for pf in &parsed {
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
                errors.push(
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
    for pf in &parsed {
        let Some(a) = pf.adapter() else { continue };
        let Some(b) = &a.binding else { continue };
        for dep in &b.requires {
            if is_unpinned_range(&dep.range) {
                errors.push(
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

    // -- 4. Build per-unit combined symbol tables. --
    let mut unit_tables: HashMap<String, UnitTable> = HashMap::new();
    for (name, indices) in &groups {
        let kind = *kinds.get(name).expect("every group has a kind");
        let table = build_unit_table(name, kind, indices, &parsed, &mut errors);
        unit_tables.insert(name.clone(), table);
    }

    // -- 5. Resolve `uses` clauses (target must exist + be a commons). --
    let mut unit_uses: HashMap<String, Vec<String>> = HashMap::new();
    for (name, indices) in &groups {
        let mut uses_targets: Vec<String> = Vec::new();
        for &i in indices {
            for u in parsed[i].uses() {
                let target = u.target.joined();
                if !unit_tables.contains_key(&target) {
                    errors.push(
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
                    errors.push(
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
                    errors.push(CompileError::new(
                        "karn.uses.self_reference",
                        u.span,
                        format!("`{name}` cannot `uses` itself"),
                    ));
                    continue;
                }
                if !uses_targets.contains(&target) {
                    uses_targets.push(target);
                }
            }
        }
        unit_uses.insert(name.clone(), uses_targets);
    }

    // -- 5b. Resolve `consumes` clauses (target must exist + be a context). --
    let mut unit_consumes: HashMap<String, Vec<String>> = HashMap::new();
    // v0.17: `consumes U { Cap, … }` flattens selected caps into the consumer's
    // local namespace. unit → bare-cap → consumed unit providing it.
    let mut unit_flattened: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in &groups {
        let kind = *kinds.get(name).unwrap();
        let mut consumes_targets: Vec<String> = Vec::new();
        let mut flattened: HashMap<String, String> = HashMap::new();
        let local_caps: HashSet<String> = unit_tables
            .get(name)
            .map(|t| t.capabilities.keys().cloned().collect())
            .unwrap_or_default();
        for &i in indices {
            for c in parsed[i].consumes() {
                let target = c.target.joined();
                if kind != UnitKind::Context {
                    errors.push(
                        CompileError::new(
                            "karn.consumes.in_commons",
                            c.span,
                            format!(
                                "`consumes` is only valid inside a context, not a commons `{name}`",
                            ),
                        )
                        .with_note(
                            "commons declare vocabulary; only contexts can declare behavioural dependencies",
                        ),
                    );
                    continue;
                }
                if !unit_tables.contains_key(&target) {
                    errors.push(
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
                    errors.push(
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
                if target == *name {
                    errors.push(CompileError::new(
                        "karn.consumes.self_reference",
                        c.span,
                        format!("context `{name}` cannot `consumes` itself"),
                    ));
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
                            errors.push(CompileError::new(
                                "karn.given.cross_context_unknown_capability",
                                cap.span,
                                format!(
                                    "`{target}` does not export a capability named `{}`",
                                    cap.name
                                ),
                            ));
                            continue;
                        }
                        if local_caps.contains(&cap.name) {
                            errors.push(CompileError::new(
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
                            errors.push(CompileError::new(
                                "karn.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "capability `{}` is flattened from both `{prev}` and `{target}` — qualify one with `given U.{}`",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
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

    // -- 5b'. Collect `consumes` aliases (v0.6 §3.1). Each consuming context
    //         has an alias map: alias → consumed-context qualified name.
    //         Detect alias-alias conflicts here; alias-vs-local-decl conflicts
    //         are checked once the local symbol tables are built (step 6+).
    let mut unit_consumes_aliases: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in &groups {
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
                    errors.push(
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
            let alias_span = parsed_alias_span(&parsed, &groups[name], alias).unwrap_or_default();
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
                errors.push(
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

    // -- 5c. Detect `consumes` cycles. --
    detect_consumes_cycles(&unit_consumes, &mut errors);

    // -- 6. Name-conflict detection for uses imports (commons-only check). --
    for (name, targets) in &unit_uses {
        let local = unit_tables.get(name).expect("unit table present");
        let mut imported: HashMap<String, String> = HashMap::new();
        for t in targets {
            let used = unit_tables.get(t).expect("used unit table present");
            for type_name in used.types.keys() {
                if local.types.contains_key(type_name) || local.fns.contains_key(type_name) {
                    continue;
                }
                if let Some(prev) = imported.get(type_name) {
                    let span = uses_span_of(&parsed, &groups[name], t).unwrap_or_default();
                    errors.push(
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
                    let span = uses_span_of(&parsed, &groups[name], t).unwrap_or_default();
                    errors.push(
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

    // -- 6b. Validate exports clauses (each name is a locally-declared type;
    //         no duplicates within or across opaque/transparent). --
    let mut exports_visibility: HashMap<String, HashMap<String, Visibility>> = HashMap::new();
    for (name, indices) in &groups {
        let kind = *kinds.get(name).unwrap();
        if kind != UnitKind::Context && kind != UnitKind::Adapter {
            // Commons may not have exports clauses (parsed grammar prevents it
            // at the parser level), but in case any sneak in, skip.
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, (Visibility, Span)> = HashMap::new();
        for &i in indices {
            for clause in parsed[i].exports() {
                // v0.15: `exports capability { ... }` clauses are validated
                // separately (§4.1); 6b handles only type exports.
                let ExportKind::Type(clause_vis) = clause.kind else {
                    continue;
                };
                let mut within: HashMap<String, Span> = HashMap::new();
                for n in &clause.names {
                    if let Some(prev) = within.get(&n.name) {
                        errors.push(
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
                        errors.push(
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

                    if let Some((prev_vis, prev_span)) = seen.get(&n.name) {
                        if *prev_vis == clause_vis {
                            errors.push(
                                CompileError::new(
                                    "karn.exports.duplicate_export",
                                    n.span,
                                    format!("type `{}` is exported more than once", n.name),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        } else {
                            errors.push(
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

    // -- 6b'. Validate `exports capability { … }` clauses (v0.15 §4.1): each
    //          name must be a capability the context declares *and* provides. --
    for (name, indices) in &groups {
        if kinds.get(name) != Some(&UnitKind::Context)
            && kinds.get(name) != Some(&UnitKind::Adapter)
        {
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            for clause in parsed[i].exports() {
                if !matches!(clause.kind, ExportKind::Capability) {
                    continue;
                }
                for n in &clause.names {
                    if let Some(prev) = seen.get(&n.name) {
                        errors.push(
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
                    if !local.capabilities.contains_key(&n.name) {
                        errors.push(
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
                        errors.push(
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

    // -- 6c. Validate that providers match their capabilities exactly. --
    for (name, table) in &unit_tables {
        let _ = name;
        for (cap_name, provider) in &table.providers {
            // v0.17: an external provider has no Karn body to match against the
            // capability — its implementation is the binding, checked by `tsc`.
            if provider.external {
                continue;
            }
            let Some(cap) = table.capabilities.get(cap_name) else {
                errors.push(
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
                    errors.push(CompileError::new(
                        "karn.provider.missing_operation",
                        provider.span,
                        format!(
                            "provider `{}` for capability `{}` is missing operation `{}`",
                            provider.provider_name.name, cap_name, cap_op.name.name
                        ),
                    ));
                }
            }
            // 2) Every provider op corresponds to a capability op with the
            //    same signature (param types and return type).
            for prov_op in &provider.ops {
                let Some(cap_op) = cap.ops.iter().find(|o| o.name.name == prov_op.name.name) else {
                    errors.push(CompileError::new(
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
                    errors.push(CompileError::new(
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
                        errors.push(CompileError::new(
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
                    errors.push(CompileError::new(
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

    if !errors.is_empty() {
        return Err(errors);
    }

    // -- 7. Build per-unit file index (which file declares which name). --
    let mut unit_file_index: HashMap<String, FileDeclIndex> = HashMap::new();
    for (name, indices) in &groups {
        unit_file_index.insert(name.clone(), build_file_decl_index(indices, &parsed));
    }

    // -- 8. For each unit, build the combined symbol space and run
    //       resolve+check per source file. --
    let mut compiled: Vec<CompiledFile> = Vec::new();
    let empty_exports = HashMap::new();

    for (name, indices) in &groups {
        let kind = *kinds.get(name).unwrap();
        let local_table = unit_tables.get(name).unwrap();

        // Compose: local + transitive (one level) uses. For commons, mixin
        // preserves type identity; for contexts, mixin produces per-context
        // nominal types. The resolver doesn't distinguish (the rebranding is
        // observable in emission); the symbol table union is the same.
        let mut combined_types = local_table.types.clone();
        let mut combined_fns = local_table.fns.clone();
        let mut combined_methods = local_table.methods.clone();
        let mut imported_from: HashMap<String, String> = HashMap::new();
        let mut imported_from_kind: HashMap<String, UnitKind> = HashMap::new();
        // Names visible from `consumes` (read-only types from consumed contexts).
        // For each name we track:
        // - the type decl, with the consumed context's identity
        // - the visibility (opaque/transparent)
        // - the owning context's qualified name (for external-construction errors)
        let mut consumed_types: HashMap<String, ConsumedType> = HashMap::new();

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

        // Now process `consumes` for contexts: add exported types into the
        // symbol table with visibility metadata so the checker can enforce
        // construction / inspection rules.
        for t in unit_consumes.get(name).into_iter().flatten() {
            let used = unit_tables.get(t).expect("consumed unit table present");
            let used_exports = exports_visibility.get(t).unwrap_or(&empty_exports);
            for (type_name, vis) in used_exports {
                let Some(decl) = used.types.get(type_name) else {
                    continue;
                };
                if combined_types.contains_key(type_name) {
                    // Name conflict between local/uses and consumed export.
                    let consumes_span =
                        consumes_span_of(&parsed, &groups[name], t).unwrap_or_default();
                    errors.push(
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

        if !errors.is_empty() {
            continue;
        }

        let local_names: HashSet<String> = local_table.types.keys().cloned().collect();

        // Collect methods authored anywhere in this unit, keyed by their
        // attached type's name. Used to surface a type's methods in the
        // file that declares the type even if the method is in a sibling file.
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

        // Per-context view information for the emitter and checker.
        let owning_context_for_emit = if kind == UnitKind::Context {
            Some(name.clone())
        } else {
            None
        };

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
            // for the resolver, checker, and emitter.
            let cross_context_for_file = if kind == UnitKind::Context {
                let mut cci = build_cross_context_info(
                    name,
                    &unit_consumes,
                    &unit_consumes_aliases,
                    &unit_uses,
                    &unit_tables,
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
            if let Err(errs) = resolver::resolve_file(&resolved) {
                errors.extend(errs);
                continue;
            }
            let typed = match checker::check(resolved) {
                Ok(t) => t,
                Err(errs) => {
                    errors.extend(errs);
                    continue;
                }
            };

            // Run the context-specific checks: forbidden construction,
            // private-type references.
            if kind == UnitKind::Context {
                let context_check_errs =
                    check_context_constraints(&typed, &consumed_types, &local_names);
                if !context_check_errs.is_empty() {
                    errors.extend(context_check_errs);
                    continue;
                }
            }

            // v0.5: check capability/provider/service/agent declarations.
            let mut typed = typed;
            let unit_table_owned = unit_tables.get(name).cloned();
            if kind == UnitKind::Context
                && let Some(table) = unit_table_owned.as_ref()
            {
                let v0_5_errs = check_v0_5_declarations(&mut typed, table, &cross_context_for_file);
                if !v0_5_errs.is_empty() {
                    errors.extend(v0_5_errs);
                    continue;
                }
            }

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
                    let exports_for_target = exports_visibility.get(t).unwrap_or(&empty_exports);
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
                compute_boundary_type_owners(
                    name,
                    &unit_consumes,
                    &unit_tables,
                    &parsed,
                    &unit_file_index,
                )
            } else {
                HashMap::new()
            };

            let emit_ctx = EmitProjectCtx {
                source_path: emit_source_path,
                commons_name: name.clone(),
                local_files: emit_local_files,
                file_decl_index: unit_file_index.get(name).cloned().unwrap_or_else(|| {
                    FileDeclIndex {
                        types: HashMap::new(),
                        fns: HashMap::new(),
                        methods: HashMap::new(),
                    }
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
            let ts = emitter::emit_project(&typed, &emit_ctx);
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
    }

    // v0.7: process test declarations. Each `test commerce.X` group resolves
    // its target, validates mocks against the target's capability/consumed-
    // context shapes, type-checks bodies with the target's privileged view,
    // and emits a per-target TypeScript test module under `tests/`.
    let (test_outputs, mut runnable_tests) = process_tests(
        &test_groups,
        &parsed,
        &kinds,
        &unit_tables,
        &exports_visibility,
        &unit_consumes,
        &unit_consumes_aliases,
        &unit_uses,
        &mut errors,
    );

    compiled.extend(test_outputs);

    // v0.16: process integration tests. Each `test integration "name"` suite
    // validates its `wires` participants, type-checks each case body as a
    // cross-context call from a synthetic harness root that consumes every
    // participant, and emits a TypeScript module that stands the participants
    // up as in-process Workers and exercises the flow across the real wire.
    let (integration_outputs, integration_runnables) = process_integration_tests(
        &integration_groups,
        &parsed,
        &kinds,
        &unit_tables,
        &unit_consumes,
        &unit_consumes_aliases,
        &unit_uses,
        &mut errors,
    );

    if !errors.is_empty() {
        return Err(errors);
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
                            b.output_path
                                .with_extension("js")
                                .to_string_lossy()
                                .to_string(),
                        )
                    })
                    .collect();
                let flattened = unit_flattened.get(ctx_name).cloned().unwrap_or_default();
                let compose_ts = emitter::emit_worker_compose(
                    ctx_name,
                    table,
                    &consumes_targets,
                    &aliases,
                    &unit_tables,
                    &binding_modules,
                    &flattened,
                );
                // Adapters are not Workers, so they get no Service Binding in
                // the consumer's wrangler config — drop them from the list.
                let service_consumes: Vec<String> = consumes_targets
                    .iter()
                    .filter(|t| !binding_modules.contains_key(*t))
                    .cloned()
                    .collect();
                let wrangler = emitter::emit_wrangler_toml(ctx_name, table, &service_consumes);
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
    Ok(ProjectOutput { files: compiled })
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

/// v0.15: build the TypeScript expression instantiating the provider of
/// capability `cap` declared in `provider_ctx`, recursively wiring its `given`
/// dependencies — local sibling providers and cross-context capability
/// providers alike. Stateless providers, so fresh instances per use are fine.
fn instantiate_provider_expr(
    provider_ctx: &str,
    cap: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let ns = provider_ctx.replace('.', "_");
    let Some(provider) = unit_tables
        .get(provider_ctx)
        .and_then(|t| t.providers.get(cap))
    else {
        return format!("new {ns}.{cap}()");
    };
    // v0.17: an external (adapter) provider's class lives in the binding module,
    // not the adapter's interface module — instantiate it from the binding
    // namespace (`<adapter>__binding`, imported by the composition root).
    if provider.external {
        return format!("new {ns}__binding.{}()", provider.provider_name.name);
    }
    if provider.given.is_empty() {
        return format!("new {ns}.{}()", provider.provider_name.name);
    }
    let consumed = unit_consumes.get(provider_ctx).cloned().unwrap_or_default();
    let aliases = unit_consumes_aliases
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
                None => provider_ctx.to_string(),
            };
            let expr = instantiate_provider_expr(
                &target_ctx,
                g.key(),
                unit_tables,
                unit_consumes,
                unit_consumes_aliases,
            );
            format!("{}: {}", g.key(), expr)
        })
        .collect();
    format!(
        "new {ns}.{}({{ {} }})",
        provider.provider_name.name,
        deps.join(", ")
    )
}

fn emit_composition_root(
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_tables: &HashMap<String, UnitTable>,
    adapter_bindings: &HashMap<String, AdapterBinding>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
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
                || table
                    .providers
                    .values()
                    .any(|p| p.given.iter().any(|g| g.is_cross_context()))
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

    let mut out = String::new();
    out.push_str("// Generated by karnc — do not edit by hand.\n");
    out.push_str("// composition root\n\n");

    // Import every context as a namespace.
    for ctx_name in &contexts {
        let dir = commons_dir_for(ctx_name).to_string_lossy().to_string();
        let ns = ctx_name.replace('.', "_");
        out.push_str(&format!("import * as {ns} from \"./{dir}.js\";\n"));
    }
    // v0.17: import each consumed adapter's binding module — the external
    // provider classes live there, not in the adapter's interface module.
    let mut consumed_adapters: Vec<&String> = unit_consumes
        .iter()
        .filter(|(name, _)| kinds.get(*name) == Some(&UnitKind::Context))
        .flat_map(|(_, targets)| targets.iter())
        .filter(|t| adapter_bindings.contains_key(*t))
        .collect();
    consumed_adapters.sort();
    consumed_adapters.dedup();
    for adapter in &consumed_adapters {
        let ns = adapter.replace('.', "_");
        let module = adapter_bindings[*adapter]
            .output_path
            .with_extension("js")
            .to_string_lossy()
            .to_string();
        out.push_str(&format!("import * as {ns}__binding from \"./{module}\";\n"));
    }
    out.push('\n');

    out.push_str("export function composeApp() {\n");

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

    Some(out)
}

// -- internals --

/// A parsed `.karn` file: its source, AST, and project-relative path.
struct ParsedFile {
    source_path: PathBuf,
    #[allow(dead_code)]
    source: String,
    unit: SourceUnit,
    kind: UnitKind,
    /// v0.17: true for toolchain-injected units (the `karn` surface) — exempt
    /// from the reserved-namespace and missing-binding checks.
    synthetic: bool,
}

impl ParsedFile {
    fn items(&self) -> &Vec<CommonsItem> {
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

    fn uses(&self) -> &Vec<UsesDecl> {
        match &self.unit {
            SourceUnit::Commons(c) => &c.uses,
            SourceUnit::Context(c) => &c.uses,
            SourceUnit::Adapter(a) => &a.uses,
            SourceUnit::Test(t) => &t.uses,
            SourceUnit::Integration(i) => &i.uses,
        }
    }

    fn consumes(&self) -> &[ConsumesDecl] {
        match &self.unit {
            SourceUnit::Commons(_) => &[],
            SourceUnit::Context(c) => &c.consumes,
            // Adapters don't `consumes` in this increment.
            SourceUnit::Adapter(_) => &[],
            // An integration test's participant edges are resolved separately
            // (the harness root consumes every participant); it has no
            // `consumes` of its own.
            SourceUnit::Test(_) | SourceUnit::Integration(_) => &[],
        }
    }

    /// `exports` clauses, for the unit kinds that have them (contexts and
    /// adapters). Empty for commons/tests.
    fn exports(&self) -> &[ExportsDecl] {
        match &self.unit {
            SourceUnit::Context(c) => &c.exports,
            SourceUnit::Adapter(a) => &a.exports,
            _ => &[],
        }
    }

    fn adapter(&self) -> Option<&AdapterDecl> {
        match &self.unit {
            SourceUnit::Adapter(a) => Some(a),
            _ => None,
        }
    }

    fn test(&self) -> Option<&TestDecl> {
        match &self.unit {
            SourceUnit::Test(t) => Some(t),
            _ => None,
        }
    }

    fn integration(&self) -> Option<&IntegrationDecl> {
        match &self.unit {
            SourceUnit::Integration(i) => Some(i),
            _ => None,
        }
    }

    /// Build a synthetic Commons AST node carrying the given items, so the
    /// existing resolver/checker pipeline can be driven uniformly.
    fn as_synthetic_commons(&self, items: Vec<CommonsItem>) -> Commons {
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

fn parse_file(root: &Path, path: &Path) -> Result<ParsedFile, Vec<CompileError>> {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Err(vec![CompileError::new(
                "karn.project.read_failed",
                Span::default(),
                format!("could not read `{}`: {e}", path.display()),
            )]);
        }
    };
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
        source_path: rel,
        source,
        unit,
        kind,
        synthetic: false,
    })
}

fn discover_karn_files(root: &Path) -> Result<Vec<PathBuf>, CompileError> {
    if !root.exists() {
        return Err(CompileError::new(
            "karn.project.no_root",
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
                    "karn.project.read_failed",
                    Span::default(),
                    format!("could not read directory `{}`: {e}", dir.display()),
                ));
            }
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("karn") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn commons_dir_for(name: &str) -> PathBuf {
    let parts: Vec<&str> = name.split('.').collect();
    let mut p = PathBuf::new();
    for part in parts {
        p.push(part);
    }
    p
}

fn ts_output_path(source: &Path) -> PathBuf {
    let mut out = source.to_path_buf();
    out.set_extension("ts");
    out
}

/// v0.8: directory name of a Worker for a given context, with dots replaced
/// by dashes (`commerce.payment` → `commerce-payment`).
pub fn worker_dir_name(context: &str) -> String {
    context.replace('.', "-")
}

/// v0.8: project-relative synthetic source path of the workers-mode
/// handlers file for a given context. Used so the emitter's relative-import
/// machinery resolves correctly against the workers layout.
pub fn worker_handlers_source_path(context: &str) -> PathBuf {
    PathBuf::from(format!(
        "workers/{}/handlers.karn",
        worker_dir_name(context)
    ))
}

/// v0.8: project-relative output path of the workers-mode handlers file.
pub fn worker_handlers_output_path(context: &str) -> PathBuf {
    PathBuf::from(format!("workers/{}/handlers.ts", worker_dir_name(context)))
}

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

/// Within a multi-file unit (i.e., 2+ files in the same directory that share
/// a qualified name), every file must declare exactly the same name.
///
/// In v0.4 the same directory may contain multiple *single-file* units (one
/// commons and one context, say), provided each file's path matches the
/// last segment of its declared qualified name. Mixed-name files in one
/// directory are only flagged when they collide on the same name (handled by
/// [`check_group_kind_consistency`]) or when path/name alignment fails.
fn check_directory_name_consistency(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    // For each unit (group of files sharing the same name), verify they all
    // live in the same directory. Tests are excluded — their files are
    // grouped by target, not by their own physical layout.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        if matches!(pf.kind, UnitKind::Test | UnitKind::Integration) {
            continue;
        }
        by_name.entry(pf.unit.name().joined()).or_default().push(i);
    }
    for indices in by_name.values() {
        if indices.len() < 2 {
            continue;
        }
        let first_dir = parsed[indices[0]]
            .source_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        for &idx in indices.iter().skip(1) {
            let dir = parsed[idx]
                .source_path
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();
            if dir != first_dir {
                errors.push(
                    CompileError::new(
                        "karn.project.inconsistent_commons_name",
                        parsed[idx].unit.span(),
                        format!(
                            "files declaring `{}` are spread across different directories: `{}` vs `{}`",
                            parsed[idx].unit.name().joined(),
                            first_dir.display(),
                            dir.display(),
                        ),
                    )
                    .with_label(parsed[indices[0]].unit.span(), "first file is here")
                    .with_note(
                        "all files of a multi-file commons or context must live in the same directory",
                    ),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Within a multi-file unit (files sharing a qualified name), every file must
/// agree on kind. Handled by [`check_group_kind_consistency`]; this check is
/// the v0.4-style directory-level guard which now defers to it.
fn check_directory_kind_consistency(_parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    Ok(())
}

/// Does a file's relative path match a qualified name? Two arrangements are
/// valid:
/// - **Single-file**: `a/b/c.karn` declaring `a.b.c`.
/// - **Multi-file**: `a/b/c/<any>.karn` declaring `a.b.c`.
///
/// v0.9.1: shared between source-unit and test-unit path validation. The
/// caller decides which root to strip from the file path before calling.
fn unit_path_matches(rel_path: &Path, qualified_name: &str) -> bool {
    let name_parts: Vec<&str> = qualified_name.split('.').collect();
    let stem = rel_path.with_extension("");
    let stem_parts: Vec<String> = stem
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    let parent_parts: Vec<String> = if stem_parts.is_empty() {
        Vec::new()
    } else {
        stem_parts[..stem_parts.len() - 1].to_vec()
    };
    let single_file_match = stem_parts.len() == name_parts.len()
        && stem_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b);
    let multi_file_match = parent_parts.len() == name_parts.len()
        && parent_parts
            .iter()
            .zip(name_parts.iter())
            .all(|(a, b)| a == b);
    single_file_match || multi_file_match
}

/// Each file's relative path must match its declared qualified name. Two
/// arrangements are valid:
/// - **Single-file**: `a/b/c.karn` declaring `a.b.c`.
/// - **Multi-file**: `a/b/c/<any>.karn` declaring `a.b.c`.
fn check_path_name_alignment(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for pf in parsed {
        if matches!(pf.kind, UnitKind::Test | UnitKind::Integration) {
            // Test files are not required to match their target's path.
            continue;
        }
        let name = pf.unit.name().joined();
        let name_parts: Vec<&str> = name.split('.').collect();
        let rel = &pf.source_path;
        if !unit_path_matches(rel, &name) {
            errors.push(
                CompileError::new(
                    "karn.project.inconsistent_commons_name",
                    pf.unit.span(),
                    format!(
                        "file `{}` declares `{name}`, but its path doesn't match — expected either `{}.karn` (single-file) or `{}/...karn` (multi-file)",
                        rel.display(),
                        name_parts.join("/"),
                        name_parts.join("/"),
                    ),
                )
                .with_note(
                    "the source-tree layout determines a unit's identity: each commons or context's qualified name must match its path",
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

/// v0.9.1: in split-paths mode, a test file's path (relative to the
/// configured `tests` root) must match the test's declared **target**
/// qualified name. Same path-alignment logic as `check_path_name_alignment`,
/// but applied to test units.
fn check_test_path_alignment(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for pf in parsed {
        if pf.kind != UnitKind::Test {
            continue;
        }
        let Some(test_decl) = pf.test() else { continue };
        let target_name = test_decl.target.joined();
        let target_parts: Vec<&str> = target_name.split('.').collect();
        let rel = &pf.source_path;
        if !unit_path_matches(rel, &target_name) {
            errors.push(
                CompileError::new(
                    "karn.project.inconsistent_test_path",
                    pf.unit.span(),
                    format!(
                        "test file `{}` targets `{target_name}`, but its path doesn't match — expected either `{}.karn` (single-file) or `{}/...karn` (multi-file)",
                        rel.display(),
                        target_parts.join("/"),
                        target_parts.join("/"),
                    ),
                )
                .with_note(
                    "in split-paths mode (configured via `karn.toml`'s `[paths]`), each test file's path under the `tests` directory must match its target's qualified name",
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

/// Files grouped by qualified name must agree on kind (even across directories).
fn check_group_kind_consistency(
    parsed: &[ParsedFile],
    groups: &HashMap<String, Vec<usize>>,
) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for (name, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let first_kind = parsed[indices[0]].kind;
        for &idx in indices.iter().skip(1) {
            if parsed[idx].kind != first_kind {
                errors.push(
                    CompileError::new(
                        "karn.project.kind_conflict",
                        parsed[idx].unit.span(),
                        format!(
                            "name `{name}` is declared as both a {} and a {}",
                            first_kind.display(),
                            parsed[idx].kind.display(),
                        ),
                    )
                    .with_label(
                        parsed[indices[0]].unit.span(),
                        format!("first declared as a {} here", first_kind.display()),
                    ),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_file_directory_conflicts(root: &Path, files: &[PathBuf]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    let mut karn_files: HashSet<PathBuf> = HashSet::new();
    let mut dirs_with_karn: HashSet<PathBuf> = HashSet::new();
    for p in files {
        let rel = p.strip_prefix(root).unwrap_or(p);
        karn_files.insert(rel.to_path_buf());
        if let Some(parent) = rel.parent() {
            dirs_with_karn.insert(parent.to_path_buf());
        }
    }
    for f in &karn_files {
        let stem = f.with_extension("");
        if dirs_with_karn.contains(&stem) {
            errors.push(
                CompileError::new(
                    "karn.project.file_and_directory",
                    Span::default(),
                    format!(
                        "commons at `{}` is ambiguous: both `{}` and `{}/` exist with `.karn` content",
                        f.with_extension("").display(),
                        f.display(),
                        stem.display()
                    ),
                )
                .with_note(
                    "a commons can be a single `.karn` file OR a directory of `.karn` files, not both",
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

/// Combined symbol tables for a single logical commons or context.
#[derive(Clone, Default)]
pub struct UnitTable {
    #[allow(dead_code)]
    pub kind: Option<UnitKind>,
    pub types: HashMap<String, TypeDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub methods: HashMap<String, ResolverMethodTable>,
    /// Per-context capabilities (v0.5). Empty for commons.
    pub capabilities: HashMap<String, CapabilityDecl>,
    /// Per-context providers (v0.5). One provider per capability in v0.5.
    /// Key: capability name. Value: provider declaration.
    pub providers: HashMap<String, ProviderDecl>,
    /// Per-context services (v0.5). Empty for commons.
    pub services: HashMap<String, ServiceDecl>,
    /// Per-context agents (v0.5). Empty for commons.
    pub agents: HashMap<String, AgentDecl>,
    /// v0.15: capability names this context offers to consumers via
    /// `exports capability { … }`. Empty for commons.
    pub exported_capabilities: std::collections::HashSet<String>,
}

fn build_unit_table(
    _name: &str,
    kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    errors: &mut Vec<CompileError>,
) -> UnitTable {
    let mut table = UnitTable {
        kind: Some(kind),
        ..UnitTable::default()
    };
    for &i in indices {
        for item in parsed[i].items() {
            if let CommonsItem::Type(t) = item {
                if let Some(prev) = table.types.get(&t.name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.resolve.duplicate_type",
                            t.name.span,
                            format!("type `{}` is already declared", t.name.name),
                        )
                        .with_label(prev.name.span, "previously declared here"),
                    );
                } else {
                    table.types.insert(t.name.name.clone(), t.clone());
                    table.methods.entry(t.name.name.clone()).or_default();
                }
            }
        }
    }
    // v0.15: collect the names a context exports as capabilities.
    // v0.17: adapters export capabilities too.
    for &i in indices {
        {
            for clause in parsed[i].exports() {
                if matches!(clause.kind, ExportKind::Capability) {
                    for n in &clause.names {
                        table.exported_capabilities.insert(n.name.clone());
                    }
                }
            }
        }
    }
    // v0.5: collect capabilities, providers, services, agents.
    for &i in indices {
        for item in parsed[i].items() {
            match item {
                CommonsItem::Capability(c) => {
                    if kind != UnitKind::Context && kind != UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.capability.outside_context",
                            c.span,
                            "`capability` declarations are only allowed inside a context or adapter",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.capabilities.get(&c.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_capability",
                                c.name.span,
                                format!("capability `{}` is already declared", c.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.capabilities.insert(c.name.name.clone(), c.clone());
                    }
                }
                CommonsItem::Provider(p) => {
                    match kind {
                        UnitKind::Context => {
                            // v0.17: a bodiless (external) provider is only legal
                            // inside an adapter.
                            if p.external {
                                errors.push(CompileError::new(
                                    "karn.context.external_provider",
                                    p.span,
                                    "an external (bodiless) provider is only allowed inside an `adapter` — a context provider must have a Karn body",
                                ));
                                continue;
                            }
                        }
                        UnitKind::Adapter => {
                            // v0.17: an adapter provider must be external — its
                            // implementation comes from the binding.
                            if !p.external {
                                errors.push(CompileError::new(
                                    "karn.adapter.provider_has_body",
                                    p.span,
                                    "a provider inside an `adapter` must be external (no body) — its implementation is supplied by the binding",
                                ));
                                continue;
                            }
                        }
                        _ => {
                            errors.push(CompileError::new(
                                "karn.provider.outside_context",
                                p.span,
                                "`provides` declarations are only allowed inside a context or adapter",
                            ));
                            continue;
                        }
                    }
                    if let Some(prev) = table.providers.get(&p.capability.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_provider",
                                p.span,
                                format!(
                                    "capability `{}` already has a provider in this context",
                                    p.capability.name
                                ),
                            )
                            .with_label(prev.span, "previously provided here"),
                        );
                    } else {
                        table.providers.insert(p.capability.name.clone(), p.clone());
                    }
                }
                CommonsItem::Service(s) => {
                    if kind == UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.adapter.disallowed_item",
                            s.span,
                            "an `adapter` may not declare a `service` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.service.outside_context",
                            s.span,
                            "`service` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.services.get(&s.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_service",
                                s.name.span,
                                format!("service `{}` is already declared", s.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.services.insert(s.name.name.clone(), s.clone());
                    }
                }
                CommonsItem::Agent(a) => {
                    if kind == UnitKind::Adapter {
                        errors.push(CompileError::new(
                            "karn.adapter.disallowed_item",
                            a.span,
                            "an `adapter` may not declare an `agent` — adapters contain only capabilities, boundary types, external providers, and helpers",
                        ));
                        continue;
                    }
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.agent.outside_context",
                            a.span,
                            "`agent` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
                    }
                    if let Some(prev) = table.agents.get(&a.name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_agent",
                                a.name.span,
                                format!("agent `{}` is already declared", a.name.name),
                            )
                            .with_label(prev.name.span, "previously declared here"),
                        );
                    } else {
                        table.agents.insert(a.name.name.clone(), a.clone());
                    }
                }
                _ => {}
            }
        }
    }
    for &i in indices {
        for item in parsed[i].items() {
            let CommonsItem::Fn(f) = item else { continue };
            match &f.name {
                FnName::Free(id) => {
                    if let Some(prev) = table.fns.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_fn",
                                id.span,
                                format!("function `{}` is already declared", id.name),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else if let Some(prev) = table.types.get(&id.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.name_conflict",
                                id.span,
                                format!(
                                    "function `{}` conflicts with a type of the same name",
                                    id.name
                                ),
                            )
                            .with_label(prev.name.span, "type declared here"),
                        );
                    } else {
                        table.fns.insert(id.name.clone(), f.clone());
                    }
                }
                FnName::Method {
                    type_name,
                    method_name,
                } => {
                    if !table.types.contains_key(&type_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.method_unknown_type",
                                type_name.span,
                                format!(
                                    "method `{}.{}` attached to an unknown type `{}`",
                                    type_name.name, method_name.name, type_name.name
                                ),
                            )
                            .with_note(
                                "methods can only be declared on types defined in the same commons or context (across all of its files)",
                            ),
                        );
                        continue;
                    }
                    let mt = table.methods.entry(type_name.name.clone()).or_default();
                    let bucket = if f.has_self {
                        &mut mt.instance
                    } else {
                        &mut mt.statics
                    };
                    if let Some(prev) = bucket.get(&method_name.name) {
                        errors.push(
                            CompileError::new(
                                "karn.resolve.duplicate_method",
                                method_name.span,
                                format!(
                                    "method `{}.{}` is already declared",
                                    type_name.name, method_name.name
                                ),
                            )
                            .with_label(prev.name.ident().span, "previously declared here"),
                        );
                    } else {
                        bucket.insert(method_name.name.clone(), f.clone());
                    }
                }
            }
        }
    }
    table
}

/// For each name declared in the unit (type, fn, method), record which
/// source file declared it. Used by the emitter to render relative imports.
#[derive(Clone)]
pub struct FileDeclIndex {
    pub types: HashMap<String, PathBuf>,
    pub fns: HashMap<String, PathBuf>,
    pub methods: HashMap<String, HashMap<String, PathBuf>>,
}

fn build_file_decl_index(indices: &[usize], parsed: &[ParsedFile]) -> FileDeclIndex {
    let mut idx = FileDeclIndex {
        types: HashMap::new(),
        fns: HashMap::new(),
        methods: HashMap::new(),
    };
    for &i in indices {
        let path = parsed[i].source_path.clone();
        for item in parsed[i].items() {
            match item {
                CommonsItem::Type(t) => {
                    idx.types
                        .entry(t.name.name.clone())
                        .or_insert_with(|| path.clone());
                }
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(id) => {
                        idx.fns
                            .entry(id.name.clone())
                            .or_insert_with(|| path.clone());
                    }
                    FnName::Method {
                        type_name,
                        method_name,
                    } => {
                        idx.methods
                            .entry(type_name.name.clone())
                            .or_default()
                            .entry(method_name.name.clone())
                            .or_insert_with(|| path.clone());
                    }
                },
                CommonsItem::Capability(_)
                | CommonsItem::Provider(_)
                | CommonsItem::Service(_)
                | CommonsItem::Agent(_) => {}
            }
        }
    }
    idx
}

fn uses_span_of(parsed: &[ParsedFile], indices: &[usize], target: &str) -> Option<Span> {
    for &i in indices {
        for u in parsed[i].uses() {
            if u.target.joined() == target {
                return Some(u.span);
            }
        }
    }
    None
}

/// Build the [`resolver::CrossContextInfo`] for a given consuming context.
/// Used by both the resolver/checker (per-file processing) and the emitter
/// (composition root + boundary casts).
fn build_cross_context_info(
    name: &str,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> resolver::CrossContextInfo {
    let consumed_contexts: Vec<String> = unit_consumes.get(name).cloned().unwrap_or_default();
    let aliases: HashMap<String, String> =
        unit_consumes_aliases.get(name).cloned().unwrap_or_default();
    let mut consumed_services: HashMap<String, HashMap<String, resolver::CrossContextService>> =
        HashMap::new();
    let mut consumed_types: HashMap<String, HashMap<String, TypeDecl>> = HashMap::new();
    let mut consumed_capabilities: HashMap<
        String,
        HashMap<String, resolver::CrossContextCapability>,
    > = HashMap::new();
    for t in &consumed_contexts {
        let other_types_combined = combined_types_for(t, unit_tables, unit_uses);
        consumed_types.insert(t.clone(), other_types_combined.clone());
        let Some(other_table) = unit_tables.get(t) else {
            continue;
        };
        let mut svcs: HashMap<String, resolver::CrossContextService> = HashMap::new();
        for (sname, sdecl) in &other_table.services {
            let Some(handler) = sdecl
                .handlers
                .iter()
                .find(|h| matches!(h.kind, HandlerKind::Call))
            else {
                continue;
            };
            let params: Vec<(String, TypeRef)> = handler
                .params
                .iter()
                .map(|p| (p.name.name.clone(), p.type_ref.clone()))
                .collect();
            svcs.insert(
                sname.clone(),
                resolver::CrossContextService {
                    name: sname.clone(),
                    params,
                    return_type: handler.return_type.clone(),
                    span: sdecl.span,
                },
            );
        }
        consumed_services.insert(t.clone(), svcs);

        // v0.15: gather the consumed context's exported capabilities, each
        // paired with the provider that implements it.
        let mut caps: HashMap<String, resolver::CrossContextCapability> = HashMap::new();
        for cap_name in &other_table.exported_capabilities {
            let Some(decl) = other_table.capabilities.get(cap_name) else {
                continue;
            };
            let Some(provider) = other_table.providers.get(cap_name) else {
                continue;
            };
            let ops = decl
                .ops
                .iter()
                .map(|op| resolver::CrossContextCapabilityOp {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| (p.name.name.clone(), p.type_ref.clone()))
                        .collect(),
                    return_type: op.return_type.clone(),
                })
                .collect();
            caps.insert(
                cap_name.clone(),
                resolver::CrossContextCapability {
                    name: cap_name.clone(),
                    ops,
                    provider_name: provider.provider_name.name.clone(),
                    provider_given: provider
                        .given
                        .iter()
                        .filter(|c| !c.is_cross_context())
                        .map(|c| c.key().to_string())
                        .collect(),
                    span: decl.span,
                },
            );
        }
        consumed_capabilities.insert(t.clone(), caps);
    }
    resolver::CrossContextInfo {
        self_context: Some(name.to_string()),
        consumed_contexts,
        aliases,
        consumed_services,
        consumed_types,
        consumed_capabilities,
        // Set by the caller from the unit's `consumes U { … }` clauses.
        flattened_caps: HashMap::new(),
    }
}

/// v0.15: validate one `given` capability reference. A bare reference must name
/// a capability declared in this context; a cross-context reference (`given
/// B.Cap`) must name a capability the consumed context exports. Returns the
/// local [`CapabilityInfo`] to add to the in-scope map for bare references;
/// cross-context references return `None` (their calls are type-checked via
/// `consumed_capabilities` at the call site) but are still validated here.
fn resolve_given_cap_ref(
    cap_ref: &CapRef,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    cross_context: &resolver::CrossContextInfo,
    errors: &mut Vec<CompileError>,
) -> Option<CapabilityInfo> {
    let Some(prefix) = cap_ref.prefix() else {
        // Local capability.
        match capability_info_map.get(cap_ref.key()) {
            Some(info) => return Some(info.clone()),
            None => {
                errors.push(CompileError::new(
                    "karn.given.unknown_capability",
                    cap_ref.span,
                    format!(
                        "capability `{}` is not declared in this context",
                        cap_ref.key()
                    ),
                ));
                return None;
            }
        }
    };
    // Cross-context capability (`given B.Cap` / `given Alias.Cap`).
    let Some(ctx_name) = cross_context.resolve_prefix(&prefix) else {
        errors.push(
            CompileError::new(
                "karn.resolve.unconsumed_context",
                cap_ref.span,
                format!(
                    "`given {}.{}` refers to a context that this context does not `consumes`",
                    prefix,
                    cap_ref.key()
                ),
            )
            .with_note(
                "add a `consumes` clause for the providing context (optionally with an alias) at the top of this context",
            ),
        );
        return None;
    };
    let exports_it = cross_context
        .consumed_capabilities
        .get(&ctx_name)
        .is_some_and(|m| m.contains_key(cap_ref.key()));
    if !exports_it {
        errors.push(
            CompileError::new(
                "karn.given.cross_context_unknown_capability",
                cap_ref.span,
                format!(
                    "context `{}` does not export a capability named `{}`",
                    ctx_name,
                    cap_ref.key()
                ),
            )
            .with_note(
                "the providing context must list the capability in an `exports capability { … }` clause",
            ),
        );
    }
    None
}

/// Build the combined type table for `unit`: its own types merged with the
/// types of every commons it `uses`. Used by cross-context resolution so we
/// can resolve a consumed context's service signatures against that context's
/// own view of types (v0.6 §4.5).
fn combined_types_for(
    unit: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> HashMap<String, TypeDecl> {
    let mut out: HashMap<String, TypeDecl> = HashMap::new();
    if let Some(table) = unit_tables.get(unit) {
        for (n, d) in &table.types {
            out.insert(n.clone(), d.clone());
        }
    }
    if let Some(targets) = unit_uses.get(unit) {
        for t in targets {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    out.entry(n.clone()).or_insert_with(|| d.clone());
                }
            }
        }
    }
    out
}

fn consumes_span_of(parsed: &[ParsedFile], indices: &[usize], target: &str) -> Option<Span> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if c.target.joined() == target {
                return Some(c.span);
            }
        }
    }
    None
}

fn parsed_alias_span(parsed: &[ParsedFile], indices: &[usize], alias: &str) -> Option<Span> {
    for &i in indices {
        for c in parsed[i].consumes() {
            if let Some(a) = &c.alias
                && a.name == alias
            {
                return Some(a.span);
            }
        }
    }
    None
}

fn detect_consumes_cycles(consumes: &HashMap<String, Vec<String>>, errors: &mut Vec<CompileError>) {
    // Tarjan / Kosaraju overkill — a simple DFS with a path stack catches
    // cycles and yields the cycle path for the diagnostic.
    let mut visited: HashSet<String> = HashSet::new();
    let mut reported: HashSet<Vec<String>> = HashSet::new();
    for start in consumes.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut stack: Vec<String> = Vec::new();
        let mut on_stack: HashSet<String> = HashSet::new();
        dfs_consumes(
            start,
            consumes,
            &mut visited,
            &mut stack,
            &mut on_stack,
            &mut reported,
            errors,
        );
    }
}

fn dfs_consumes(
    node: &str,
    consumes: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    on_stack: &mut HashSet<String>,
    reported: &mut HashSet<Vec<String>>,
    errors: &mut Vec<CompileError>,
) {
    if on_stack.contains(node) {
        // Found a cycle: extract the path from `node`'s position in stack.
        let start = stack.iter().position(|s| s == node).unwrap_or(0);
        let mut cycle: Vec<String> = stack[start..].to_vec();
        cycle.push(node.to_string());
        // Canonicalise the cycle for de-dup.
        let canon = canonicalise_cycle(&cycle);
        if reported.insert(canon.clone()) {
            errors.push(CompileError::new(
                "karn.context.consumes_cycle",
                Span::default(),
                format!(
                    "`consumes` cycle detected: {}",
                    cycle.join(" → ")
                ),
            )
            .with_note(
                "contexts must form an acyclic dependency graph; remove one of the `consumes` clauses or restructure",
            ));
        }
        return;
    }
    if visited.contains(node) {
        return;
    }
    visited.insert(node.to_string());
    on_stack.insert(node.to_string());
    stack.push(node.to_string());
    if let Some(targets) = consumes.get(node) {
        for t in targets {
            dfs_consumes(t, consumes, visited, stack, on_stack, reported, errors);
        }
    }
    stack.pop();
    on_stack.remove(node);
}

fn canonicalise_cycle(cycle: &[String]) -> Vec<String> {
    if cycle.is_empty() {
        return Vec::new();
    }
    // Drop the duplicated last element (cycle vector ends with the start node).
    let body = &cycle[..cycle.len() - 1];
    if body.is_empty() {
        return Vec::new();
    }
    let mut min_idx = 0;
    for (i, s) in body.iter().enumerate() {
        if s < &body[min_idx] {
            min_idx = i;
        }
    }
    let mut rotated: Vec<String> = body[min_idx..].to_vec();
    rotated.extend(body[..min_idx].iter().cloned());
    rotated
}

/// A type imported into a context via `consumes`. Carries enough metadata for
/// the checker and emitter to enforce / express visibility.
#[derive(Debug, Clone)]
pub struct ConsumedType {
    pub owning_context: String,
    pub visibility: Visibility,
}

/// Enforce v0.4 construction rules: types owned by a consumed context can be
/// referenced (held, passed, read for transparent exports) but cannot be
/// constructed. This catches `OtherType { ... }`, `OtherType.of(...)`,
/// `OtherType.unsafe(...)`, and `OtherType.Variant(...)` expressions where
/// `OtherType` is from a consumed context.
fn check_context_constraints(
    typed: &checker::TypedCommons,
    consumed_types: &HashMap<String, ConsumedType>,
    local_type_names: &HashSet<String>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    for item in &typed.commons.items {
        if let CommonsItem::Fn(f) = item {
            walk_block_for_constraints(
                &f.body,
                typed,
                consumed_types,
                local_type_names,
                &mut errors,
            );
        }
    }
    errors
}

fn walk_block_for_constraints(
    block: &Block,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                walk_expr_for_constraints(&l.value, typed, consumed, local, errors);
            }
            Statement::Commit(c) => {
                walk_expr_for_constraints(&c.value, typed, consumed, local, errors);
            }
            Statement::Assert(a) => {
                walk_expr_for_constraints(&a.value, typed, consumed, local, errors);
            }
        }
    }
    walk_expr_for_constraints(&block.tail, typed, consumed, local, errors);
}

fn walk_expr_for_constraints(
    e: &Expr,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    match &e.kind {
        ExprKind::RecordConstruction { type_name, fields } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                errors.push(
                    CompileError::new(
                        "karn.context.external_construction",
                        type_name.span,
                        format!(
                            "cannot construct `{}` here — it is owned by context `{}`",
                            type_name.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "values of an externally-owned type can only be created inside the owning context",
                    ),
                );
            }
            for f in fields {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                let is_construct = method.name == "of"
                    || method.name == "unsafe"
                    || matches!(
                        typed.types.get(&type_name.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "karn.context.external_construction",
                            type_name.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                type_name.name, method.name, type_name.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            // `T.method(...)` written as MethodCall with receiver Ident(T).
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
            {
                let is_construct = method.name == "of"
                    || method.name == "unsafe"
                    || matches!(
                        typed.types.get(&id.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "karn.context.external_construction",
                            id.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                id.name, method.name, id.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // For opaque-exported types from consumed contexts, field
            // access is forbidden — but record types have field access
            // anyway, so the visibility check applies only when the
            // receiver's type is a consumed type. To do this rigorously,
            // we'd consult the expr_types map. Easy path: peek at the
            // receiver if it's an Ident referring to a binding whose
            // declared type points to a consumed type.
            // For v0.4 we use a simpler conservative rule: if the
            // receiver is `T.X` syntax (FieldAccess from an Ident that's
            // a type name) and `T` is consumed and opaque, reject it.
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
                && ct.visibility == Visibility::Opaque
                && typed
                    .types
                    .get(&id.name)
                    .map(|d| matches!(d.body, TypeBody::Sum(_)))
                    .unwrap_or(false)
            {
                errors.push(
                    CompileError::new(
                        "karn.context.opaque_inspection",
                        id.span.merge(field.span),
                        format!(
                            "cannot inspect opaquely-exported type `{}` from outside context `{}`",
                            id.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "opaque exports hide the type's shape; the owning context did not expose variants or fields",
                    ),
                );
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
        }
        ExprKind::Match { discriminant, arms } => {
            // If the discriminant is typed as an opaquely-exported consumed
            // type, the match is forbidden because we can't reveal the
            // variants.
            if let Some(ty) = typed.expr_types.get(&discriminant.span) {
                let display = ty.display();
                if let Some(ct) = consumed.get(&display)
                    && ct.visibility == Visibility::Opaque
                {
                    errors.push(
                        CompileError::new(
                            "karn.context.opaque_inspection",
                            discriminant.span,
                            format!(
                                "cannot `match` on opaquely-exported type `{}` from outside context `{}`",
                                display, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "opaque exports hide the type's shape; the owning context did not expose variants",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(discriminant, typed, consumed, local, errors);
            for arm in arms {
                match &arm.body {
                    MatchBody::Expr(ex) => {
                        walk_expr_for_constraints(ex, typed, consumed, local, errors);
                    }
                    MatchBody::Block(b) => {
                        walk_block_for_constraints(b, typed, consumed, local, errors);
                    }
                }
            }
        }
        ExprKind::Is { value, pattern: _ } => {
            walk_expr_for_constraints(value, typed, consumed, local, errors);
        }
        ExprKind::Call(_, args) => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::BinOp(_, l, r) => {
            walk_expr_for_constraints(l, typed, consumed, local, errors);
            walk_expr_for_constraints(r, typed, consumed, local, errors);
        }
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i) => {
            walk_expr_for_constraints(i, typed, consumed, local, errors);
        }
        ExprKind::Block(b) => walk_block_for_constraints(b, typed, consumed, local, errors),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr_for_constraints(cond, typed, consumed, local, errors);
            walk_block_for_constraints(then_block, typed, consumed, local, errors);
            walk_block_for_constraints(else_block, typed, consumed, local, errors);
        }
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        ExprKind::EffectPure(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Assert(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Mock { args, .. } => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_expr_for_constraints(base, typed, consumed, local, errors);
            for f in overrides {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
    }
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

/// Check v0.5 capability/provider/service/agent bodies. Mutates `typed` to
/// extend the expr_types map with bindings observed in the new bodies.
fn check_v0_5_declarations(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
) -> Vec<CompileError> {
    let mut errors = Vec::new();

    // Build a resolved-commons snapshot for the per-handler checker.
    // We synthesise a ResolvedCommons by reusing typed.types / typed.fns /
    // typed.methods; the resolver wouldn't add anything new.
    let local_type_names: std::collections::HashSet<String> = typed.types.keys().cloned().collect();
    let resolved = ResolvedCommons {
        commons: typed.commons.clone(),
        types: typed.types.clone(),
        fns: typed.fns.clone(),
        methods: typed.methods.clone(),
        local_type_names,
        cross_context: cross_context.clone(),
        agents: table.agents.clone(),
    };

    // Capability info from the table.
    let mut capability_info_map: HashMap<String, CapabilityInfo> = table
        .capabilities
        .iter()
        .map(|(name, decl)| {
            let ops = decl
                .ops
                .iter()
                .map(|op| CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| checker::resolve_type_ref(&p.type_ref, &typed.types))
                        .map(|t| t.unwrap_or(Ty::Unit))
                        .collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                        .unwrap_or(Ty::Unit),
                })
                .collect();
            (
                name.clone(),
                CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            )
        })
        .collect();
    // v0.17: flattened capabilities (`consumes U { Cap }`) enter the local map
    // under their bare names, resolved from the consumed unit's exported
    // capability so bare `given Cap` / `Cap.op(…)` type-check as if local.
    for (cap, unit) in &cross_context.flattened_caps {
        let Some(xcap) = cross_context
            .consumed_capabilities
            .get(unit)
            .and_then(|m| m.get(cap))
        else {
            continue;
        };
        let ops = xcap
            .ops
            .iter()
            .map(|op| CapabilityOpInfo {
                name: op.name.clone(),
                params: op
                    .params
                    .iter()
                    .map(|(_, tr)| checker::resolve_type_ref(tr, &typed.types).unwrap_or(Ty::Unit))
                    .collect(),
                return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                    .unwrap_or(Ty::Unit),
            })
            .collect();
        capability_info_map.insert(
            cap.clone(),
            CapabilityInfo {
                name: cap.clone(),
                ops,
            },
        );
    }

    // Check provider bodies. v0.12: a provider may declare `given` and use
    // those capabilities in its bodies (provider composition). Bodies are
    // effectful if the operation returns Effect[T]; no `self`.
    for provider in table.providers.values() {
        // Build the provider's capability scope from its `given`, validating
        // each name is a declared capability.
        let mut provider_caps: HashMap<String, CapabilityInfo> = HashMap::new();
        for cap_ref in &provider.given {
            if let Some(info) =
                resolve_given_cap_ref(cap_ref, &capability_info_map, cross_context, &mut errors)
            {
                provider_caps.insert(cap_ref.key().to_string(), info);
            }
        }
        for op in &provider.ops {
            checker::check_handler_body(
                &op.body,
                &op.return_type,
                op.return_type.span(),
                &op.params,
                &resolved,
                &mut typed.expr_types,
                &mut errors,
                provider_caps.clone(),
                capability_info_map.clone(),
                None,
                None,
                // The provider's `given` keys are in scope (so cross-context
                // capability calls resolve), but unused-`given` is not reported
                // per-op: a capability may be used in one op but not another.
                provider.given.iter().map(|c| c.key().to_string()).collect(),
                false,
            );
        }
    }

    // v0.12: providers form a dependency graph over capabilities (a provider's
    // `given` are the capabilities its provided capability depends on). Reject
    // a cycle — the composition root cannot instantiate one in dependency
    // order. Self-provision (`provides X = … given X`) is the trivial cycle.
    detect_provider_dependency_cycles(&table.providers, &mut errors);

    // v0.9: validate HTTP handler shape and check for duplicate routes
    // across all services in this context.
    let mut route_first_span: HashMap<(HttpMethod, String), Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Http { method, path } = &handler.kind else {
                continue;
            };
            validate_http_handler(handler, *method, path, &typed.types, &mut errors);
            let key = (*method, path.clone());
            if let Some(prev) = route_first_span.get(&key).copied() {
                errors.push(
                    CompileError::new(
                        "karn.http.duplicate_route",
                        handler.span,
                        format!(
                            "duplicate HTTP route: another handler already declares `{} {}`",
                            method.as_str(),
                            path,
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                route_first_span.insert(key, handler.span);
            }
        }
    }

    // v0.10a: validate `on cron` handler shape and check for duplicate
    // schedules across all services in this context (the generated
    // `scheduled` dispatcher routes on `event.cron`, so duplicates are
    // ambiguous).
    let mut schedule_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Cron { expr } = &handler.kind else {
                continue;
            };
            validate_cron_handler(handler, expr, &mut errors);
            if let Some(prev) = schedule_first_span.get(expr).copied() {
                errors.push(
                    CompileError::new(
                        "karn.cron.duplicate_schedule",
                        handler.span,
                        format!(
                            "duplicate cron schedule: another handler already declares `{expr}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                schedule_first_span.insert(expr.clone(), handler.span);
            }
        }
    }

    // v0.10b: validate `on queue` handler shape and check for duplicate
    // consumers across all services in this context (the generated `queue`
    // dispatcher routes on `batch.queue`, so two consumers of the same queue
    // are ambiguous).
    let mut consumer_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Queue { name } = &handler.kind else {
                continue;
            };
            validate_queue_handler(handler, name, &mut errors);
            if let Some(prev) = consumer_first_span.get(name).copied() {
                errors.push(
                    CompileError::new(
                        "karn.queue.duplicate_consumer",
                        handler.span,
                        format!(
                            "duplicate queue consumer: another handler already consumes `{name}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                consumer_first_span.insert(name.clone(), handler.span);
            }
        }
    }

    // Check service handlers.
    for service in table.services.values() {
        for handler in &service.handlers {
            // The given clause must reference only declared (local) or
            // exported (cross-context) capabilities.
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, &capability_info_map, cross_context, &mut errors)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "karn.service.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "service handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            let given_declared: Vec<String> =
                handler.given.iter().map(|c| c.key().to_string()).collect();
            checker::check_handler_body(
                &handler.body,
                &handler.return_type,
                handler.return_type.span(),
                &handler.params,
                &resolved,
                &mut typed.expr_types,
                &mut errors,
                handler_caps,
                capability_info_map.clone(),
                None,
                None,
                given_declared,
                true,
            );
        }
    }

    // Check agent handlers.
    for agent in table.agents.values() {
        // Build the agent's state type as a synthetic record. We expose it
        // under the name `<AgentName>State` in the type table so the body
        // can reference it.
        let agent_state_name = format!("{}State", agent.name.name);
        // Build a synthetic Record TypeDecl and stuff it into a *clone* of
        // the resolved types so handler bodies see it.
        let synthetic_state = TypeDecl {
            name: Ident {
                name: agent_state_name.clone(),
                span: agent.state_span,
            },
            body: TypeBody::Record(RecordBody {
                fields: agent.state_fields.clone(),
                span: agent.state_span,
            }),
            documentation: None,
            span: agent.state_span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = typed.types.clone();
        types_for_handler.insert(agent_state_name.clone(), synthetic_state.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
        };
        // v0.11: every state field must have a defined initial value for a
        // fresh key — an explicit static initialiser, or (v0.9.2) an implicit
        // zero. A field with neither is rejected.
        for field in &agent.state_fields {
            if let Some(init) = &field.init {
                checker::check_state_initialiser(
                    init,
                    &field.type_ref,
                    &resolved_for_handler,
                    &mut typed.expr_types,
                    &mut errors,
                );
            } else if checker::zero_value_ts(
                &field.type_ref,
                field.refinement.as_ref(),
                &typed.types,
            )
            .is_none()
            {
                errors.push(
                    CompileError::new(
                        "karn.agents.non_zeroable_state_field",
                        field.span,
                        format!(
                            "agent `{}` state field `{}` has no defined zero value, so a \
                             fresh key cannot be initialised",
                            agent.name.name, field.name.name
                        ),
                    )
                    .with_note(
                        "add an initialiser (`field: T = value`) to give a fresh key its \
                         starting value, or wrap the field in `Option[…]` (None means \
                         \"never set\")",
                    ),
                );
            }
        }
        let state_ty = Ty::Named {
            name: agent_state_name.clone(),
            kind: checker::NamedKind::Record,
        };
        let key_ty = checker::resolve_type_ref(&agent.key_type, &typed.types).unwrap_or(Ty::Unit);
        let mut self_scope: HashMap<String, Ty> = HashMap::new();
        // `self` is a synthetic record with two fields: the key and `state`.
        // But the parser treats `self.x` as FieldAccess on Ident("self"), so
        // we need to give `self` a record type with both. Easiest: a one-off
        // synthetic record type.
        let agent_self_name = format!("__{}Self", agent.name.name);
        let self_decl = TypeDecl {
            name: Ident {
                name: agent_self_name.clone(),
                span: agent.span,
            },
            body: TypeBody::Record(RecordBody {
                fields: vec![
                    RecordField {
                        name: Ident {
                            name: agent.key_name.name.clone(),
                            span: agent.key_name.span,
                        },
                        type_ref: agent.key_type.clone(),
                        refinement: None,
                        init: None,
                        span: agent.key_name.span,
                    },
                    RecordField {
                        name: Ident {
                            name: "state".to_string(),
                            span: agent.state_span,
                        },
                        type_ref: TypeRef::Named(Ident {
                            name: agent_state_name.clone(),
                            span: agent.state_span,
                        }),
                        refinement: None,
                        init: None,
                        span: agent.state_span,
                    },
                ],
                span: agent.span,
            }),
            documentation: None,
            span: agent.span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = resolved_for_handler.types.clone();
        types_for_handler.insert(agent_self_name.clone(), self_decl.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
        };
        self_scope.insert(
            "self".to_string(),
            Ty::Named {
                name: agent_self_name.clone(),
                kind: checker::NamedKind::Record,
            },
        );
        let _ = key_ty;

        for handler in &agent.handlers {
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, &capability_info_map, cross_context, &mut errors)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "karn.agent.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "agent handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            let given_declared: Vec<String> =
                handler.given.iter().map(|c| c.key().to_string()).collect();
            checker::check_handler_body(
                &handler.body,
                &handler.return_type,
                handler.return_type.span(),
                &handler.params,
                &resolved_for_handler,
                &mut typed.expr_types,
                &mut errors,
                handler_caps,
                capability_info_map.clone(),
                Some(state_ty.clone()),
                Some(self_scope.clone()),
                given_declared,
                true,
            );
        }
    }

    errors
}

/// Structural equality for TypeRef, used by v0.5 capability/provider signature
/// matching. Doesn't resolve names — it compares the surface syntax. Named
/// types match by their literal identifier; built-ins match by variant.
fn type_refs_match(a: &TypeRef, b: &TypeRef) -> bool {
    match (a, b) {
        (TypeRef::Base(x, _), TypeRef::Base(y, _)) => x == y,
        (TypeRef::Named(x), TypeRef::Named(y)) => x.name == y.name,
        (TypeRef::Result(t1, e1, _), TypeRef::Result(t2, e2, _)) => {
            type_refs_match(t1, t2) && type_refs_match(e1, e2)
        }
        (TypeRef::Option(t1, _), TypeRef::Option(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::Effect(t1, _), TypeRef::Effect(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::HttpResult(t1, _), TypeRef::HttpResult(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::ValidationError(_), TypeRef::ValidationError(_)) => true,
        (TypeRef::Unit(_), TypeRef::Unit(_)) => true,
        _ => false,
    }
}

/// Validate an `on http METHOD "path"` handler (v0.9 §4.1):
///
/// - Path must start with `/`, must not be `/_karn/...` (reserved).
/// - Every `:name` segment binds to a handler parameter of the same name.
/// - Every parameter is either a path parameter or named `body`.
/// - Path parameter types are constructible from `String` (`String`, refined
///   `String`, or opaque `String`).
/// - GET / DELETE handlers may not have a `body` parameter.
/// - The handler return type must be `Effect[HttpResult[T]]`.
fn validate_http_handler(
    handler: &Handler,
    method: HttpMethod,
    path: &str,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    if !path.starts_with('/') {
        errors.push(CompileError::new(
            "karn.http.invalid_path",
            handler.span,
            format!("HTTP path `{path}` must start with `/`"),
        ));
    }
    if path.starts_with("/_karn/") || path == "/_karn" {
        errors.push(
            CompileError::new(
                "karn.http.reserved_prefix",
                handler.span,
                format!("HTTP path `{path}` uses the reserved `/_karn/` prefix",),
            )
            .with_note("paths under `/_karn/` are reserved for internal Karn dispatch"),
        );
    }
    // Parse segments and collect path-parameter names.
    let mut path_param_names: Vec<&str> = Vec::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if let Some(rest) = seg.strip_prefix(':') {
            if rest.is_empty() {
                errors.push(CompileError::new(
                    "karn.http.invalid_path",
                    handler.span,
                    format!("HTTP path `{path}` has an empty parameter segment `:`"),
                ));
            } else {
                path_param_names.push(rest);
            }
        }
    }
    // Every :name must have a matching handler parameter.
    for name in &path_param_names {
        if !handler.params.iter().any(|p| p.name.name == *name) {
            errors.push(CompileError::new(
                "karn.http.unbound_path_param",
                handler.span,
                format!("path parameter `:{name}` has no matching handler parameter `{name}`",),
            ));
        }
    }
    // Every handler parameter must be either a path param or `body`.
    for p in &handler.params {
        let is_path = path_param_names.iter().any(|n| n == &p.name.name.as_str());
        let is_body = p.name.name == "body";
        if !is_path && !is_body {
            errors.push(
                CompileError::new(
                    "karn.http.extra_param",
                    p.span,
                    format!(
                        "handler parameter `{}` is not a path parameter and is not named `body`",
                        p.name.name
                    ),
                )
                .with_note(
                    "HTTP handler parameters must either match a `:name` path segment or be named `body`",
                ),
            );
        }
        // Path params must be constructible from String.
        if is_path && !is_string_constructible(&p.type_ref, types) {
            errors.push(
                CompileError::new(
                    "karn.http.path_param_not_stringy",
                    p.type_ref.span(),
                    format!(
                        "path parameter `{}` must have a type constructible from `String` (got `{}`)",
                        p.name.name,
                        ts_type_ref_display(&p.type_ref),
                    ),
                )
                .with_note(
                    "use `String`, a refined `String`, or an opaque type whose base is `String`",
                ),
            );
        }
        if is_body && method.forbids_body() {
            errors.push(
                CompileError::new(
                    "karn.http.body_on_get_or_delete",
                    p.span,
                    format!(
                        "`on http {}` handlers may not declare a `body` parameter",
                        method.as_str()
                    ),
                )
                .with_note("GET and DELETE requests conventionally carry no body in Karn v0.9"),
            );
        }
    }
    // Validate return type shape.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => matches!(inner.as_ref(), TypeRef::HttpResult(_, _)),
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.http.return_not_effect_http_result",
            handler.return_type.span(),
            format!(
                "`on http` handler must return `Effect[HttpResult[T]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// Validate an `on cron "expr" (at: Int?) -> Effect[Result[(), E]]` handler
/// (v0.10a §4.1): at most one `Int` parameter (the scheduled time, Unix epoch
/// milliseconds), a structurally well-formed schedule, and the unit-Result
/// return shape. The service-only rule is enforced earlier, in the parser
/// (`karn.parse.cron_in_agent`).
fn validate_cron_handler(handler: &Handler, expr: &str, errors: &mut Vec<CompileError>) {
    // A cron handler takes at most one parameter — the scheduled time, typed
    // `Int` (epoch milliseconds). A scheduled trigger has no other payload.
    if handler.params.len() > 1 {
        errors.push(
            CompileError::new(
                "karn.cron.bad_params",
                handler.params[1].span,
                "`on cron` handlers take at most one parameter (the scheduled time)",
            )
            .with_note("a scheduled trigger's only input is the time it fired"),
        );
    } else if let Some(p) = handler.params.first()
        && !matches!(p.type_ref, TypeRef::Base(BaseType::Int, _))
    {
        errors.push(
            CompileError::new(
                "karn.cron.bad_params",
                p.type_ref.span(),
                format!(
                    "an `on cron` parameter must be `Int` (the scheduled time in epoch milliseconds), got `{}`",
                    ts_type_ref_display(&p.type_ref),
                ),
            )
            .with_note("wrap it in your own time type inside the body if you want stronger typing"),
        );
    }
    // The schedule must be five whitespace-separated fields (light structural
    // check; per-field validation is deferred — v0.10 §4.1, [DECISION 4]).
    let fields = expr.split_whitespace().count();
    if fields != 5 {
        errors.push(
            CompileError::new(
                "karn.cron.invalid_schedule",
                handler.span,
                format!(
                    "cron expression `{expr}` must have exactly five whitespace-separated fields (got {fields})",
                ),
            )
            .with_note("the fields are: minute hour day-of-month month day-of-week"),
        );
    }
    // The return type must be `Effect[Result[(), E]]`.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => match inner.as_ref() {
            TypeRef::Result(ok, _err, _) => matches!(ok.as_ref(), TypeRef::Unit(_)),
            _ => false,
        },
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.cron.return_not_effect_result",
            handler.return_type.span(),
            format!(
                "`on cron` handler must return `Effect[Result[(), E]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// v0.12: detect cycles in the provider dependency graph. Each provided
/// capability depends (via its provider's `given`) on other capabilities; a
/// cycle means the composition root cannot order instantiation. Emits
/// `karn.provider.dependency_cycle` on every provider that participates in a
/// cycle. `providers` is keyed by capability name.
fn detect_provider_dependency_cycles(
    providers: &HashMap<String, ProviderDecl>,
    errors: &mut Vec<CompileError>,
) {
    fn visit(
        node: &str,
        providers: &HashMap<String, ProviderDecl>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
        in_stack: &mut HashSet<String>,
        cyclic: &mut HashSet<String>,
    ) {
        if visited.contains(node) {
            return;
        }
        in_stack.insert(node.to_string());
        stack.push(node.to_string());
        if let Some(p) = providers.get(node) {
            for dep in &p.given {
                // Cross-context dependencies follow the (acyclic) `consumes`
                // graph; only intra-context provider edges can form a cycle here.
                if dep.is_cross_context() {
                    continue;
                }
                // Only follow dependencies that have a provider in this context.
                if !providers.contains_key(dep.key()) {
                    continue;
                }
                if in_stack.contains(dep.key()) {
                    // A back-edge: everything from `dep` down the current stack
                    // is on the cycle.
                    let start = stack.iter().position(|n| n == dep.key()).unwrap_or(0);
                    for n in &stack[start..] {
                        cyclic.insert(n.clone());
                    }
                } else if !visited.contains(dep.key()) {
                    visit(dep.key(), providers, visited, stack, in_stack, cyclic);
                }
            }
        }
        stack.pop();
        in_stack.remove(node);
        visited.insert(node.to_string());
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut cyclic: HashSet<String> = HashSet::new();
    let mut keys: Vec<&String> = providers.keys().collect();
    keys.sort();
    for k in keys {
        let mut stack: Vec<String> = Vec::new();
        let mut in_stack: HashSet<String> = HashSet::new();
        visit(
            k,
            providers,
            &mut visited,
            &mut stack,
            &mut in_stack,
            &mut cyclic,
        );
    }

    let mut cyclic_sorted: Vec<&String> = cyclic.iter().collect();
    cyclic_sorted.sort();
    for cap in cyclic_sorted {
        if let Some(p) = providers.get(cap) {
            errors.push(
                CompileError::new(
                    "karn.provider.dependency_cycle",
                    p.span,
                    format!(
                        "provider `{}` for capability `{}` is part of a capability dependency cycle",
                        p.provider_name.name, cap,
                    ),
                )
                .with_note(
                    "a capability cannot depend on itself, directly or transitively, through \
                     provider `given`",
                ),
            );
        }
    }
}

/// Validate an `on queue "name" (message: T) -> Effect[Result[(), E]]` handler
/// (v0.10b §4.2): a non-empty queue name, exactly one parameter (the message,
/// any wire-deserialisable type), and the unit-Result return shape. `Ok(())`
/// acknowledges the message at emission; `Err` retries it. The service-only
/// rule is enforced earlier, in the parser (`karn.parse.queue_in_agent`).
fn validate_queue_handler(handler: &Handler, name: &str, errors: &mut Vec<CompileError>) {
    if name.is_empty() {
        errors.push(CompileError::new(
            "karn.queue.invalid_name",
            handler.span,
            "`on queue` requires a non-empty queue name",
        ));
    }
    // Exactly one parameter — the message. (Conventionally named `message`.)
    if handler.params.len() != 1 {
        errors.push(
            CompileError::new(
                "karn.queue.bad_params",
                handler.span,
                format!(
                    "`on queue` handlers take exactly one parameter (the message), got {}",
                    handler.params.len(),
                ),
            )
            .with_note("a queue consumer processes one message per invocation"),
        );
    }
    // The return type must be `Effect[Result[(), E]]`.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => match inner.as_ref() {
            TypeRef::Result(ok, _err, _) => matches!(ok.as_ref(), TypeRef::Unit(_)),
            _ => false,
        },
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "karn.queue.return_not_effect_result",
            handler.return_type.span(),
            format!(
                "`on queue` handler must return `Effect[Result[(), E]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// True when `r` resolves to `String`, a refined-base `String`, or an
/// opaque-base `String`. v0.9 path parameter requirement.
fn is_string_constructible(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> bool {
    match r {
        TypeRef::Base(BaseType::String, _) => true,
        TypeRef::Named(id) => match types.get(&id.name).map(|t| &t.body) {
            Some(TypeBody::Refined { base, .. }) => *base == BaseType::String,
            Some(TypeBody::Opaque { base, .. }) => *base == BaseType::String,
            _ => false,
        },
        _ => false,
    }
}

/// Render a type-ref in the same form the user wrote it, for diagnostics.
fn ts_type_ref_display(r: &TypeRef) -> String {
    match r {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(t, e, _) => format!(
            "Result[{}, {}]",
            ts_type_ref_display(t),
            ts_type_ref_display(e)
        ),
        TypeRef::Option(t, _) => format!("Option[{}]", ts_type_ref_display(t)),
        TypeRef::Effect(t, _) => format!("Effect[{}]", ts_type_ref_display(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult[{}]", ts_type_ref_display(t)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
    }
}

// -- v0.7: test declaration processing --

/// Classification of a mock target inside a test declaration.
#[derive(Debug, Clone)]
enum MockTarget {
    /// Provider mock — replaces a capability the target context declares.
    Capability(String),
    /// Consumed-context mock — replaces the consumed context with the given
    /// qualified name (resolved through the target context's `consumes`
    /// table, including aliases).
    ConsumedContext { qualified: String, alias: String },
}

#[allow(clippy::too_many_arguments)]
fn process_tests(
    test_groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut Vec<CompileError>,
) -> (Vec<CompiledFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<CompiledFile> = Vec::new();
    let mut runnable_tests: Vec<RunnableTest> = Vec::new();

    let mut sorted_targets: Vec<&String> = test_groups.keys().collect();
    sorted_targets.sort();

    for target_name in sorted_targets {
        let indices = test_groups.get(target_name).unwrap();
        // -- Phase 2: target resolution --
        let target_kind = match kinds.get(target_name) {
            Some(k) => *k,
            None => {
                let span = first_test_target_span(indices, parsed);
                errors.push(
                    CompileError::new(
                        "karn.test.unknown_target",
                        span,
                        format!(
                            "test target `{target_name}` is not a declared commons or context in this project",
                        ),
                    )
                    .with_note(
                        "the target of a `test` declaration must be a commons or context declared elsewhere in the project",
                    ),
                );
                continue;
            }
        };

        // -- Phase 2: duplicate test case names --
        let mut seen_cases: HashMap<String, Span> = HashMap::new();
        let mut had_dup = false;
        for &i in indices {
            if let Some(t) = parsed[i].test() {
                for case in &t.cases {
                    if let Some(prev) = seen_cases.get(&case.name) {
                        had_dup = true;
                        errors.push(
                            CompileError::new(
                                "karn.test.duplicate_case_name",
                                case.name_span,
                                format!(
                                    "test case `\"{}\"` is declared more than once in tests targeting `{target_name}`",
                                    case.name
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        seen_cases.insert(case.name.clone(), case.name_span);
                    }
                }
            }
        }

        // -- Phase 3: validate mocks --
        let mut target_mocks: HashMap<String, ResolvedMock> = HashMap::new();
        // The target's per-context info we'll use during mock resolution.
        let target_table = unit_tables.get(target_name);
        let target_aliases_map = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let target_consumed = unit_consumes.get(target_name).cloned().unwrap_or_default();

        for &i in indices {
            let Some(t) = parsed[i].test() else { continue };
            for mock in &t.mocks {
                // Tests targeting a commons have no providers and no
                // consumed contexts to mock.
                if target_kind == UnitKind::Commons {
                    errors.push(
                        CompileError::new(
                            "karn.mock.in_commons_test",
                            mock.span,
                            format!(
                                "`mocks` declarations are not allowed in a test of commons `{target_name}` — commons have no providers or consumes to replace",
                            ),
                        )
                        .with_note(
                            "remove the mock, or move the test to target a context",
                        ),
                    );
                    continue;
                }
                if let Some(prev) = target_mocks.get(&mock.target_name.name) {
                    errors.push(
                        CompileError::new(
                            "karn.mock.duplicate_target",
                            mock.target_name.span,
                            format!(
                                "name `{}` is mocked more than once in tests of `{target_name}`",
                                mock.target_name.name
                            ),
                        )
                        .with_label(prev.decl.span, "previously mocked here"),
                    );
                    continue;
                }
                // Disambiguate capability vs consumed-context.
                let cap_match =
                    target_table.and_then(|tbl| tbl.capabilities.get(&mock.target_name.name));
                let alias_match = target_aliases_map.get(&mock.target_name.name).cloned();
                let qualified_match = target_consumed
                    .iter()
                    .find(|q| {
                        q.as_str() == mock.target_name.name
                            || q.rsplit('.').next() == Some(mock.target_name.name.as_str())
                    })
                    .cloned();
                let resolution: Option<MockTarget> = if cap_match.is_some() {
                    Some(MockTarget::Capability(mock.target_name.name.clone()))
                } else if let Some(qual) = alias_match {
                    Some(MockTarget::ConsumedContext {
                        qualified: qual,
                        alias: mock.target_name.name.clone(),
                    })
                } else {
                    qualified_match.map(|qual| MockTarget::ConsumedContext {
                        qualified: qual,
                        alias: mock.target_name.name.clone(),
                    })
                };
                let resolved_target = match resolution {
                    Some(r) => r,
                    None => {
                        errors.push(
                            CompileError::new(
                                "karn.mock.unknown_target",
                                mock.target_name.span,
                                format!(
                                    "`{}` is not a capability of context `{target_name}` and not a consumed-context alias",
                                    mock.target_name.name
                                ),
                            )
                            .with_note(
                                "mocks must target either a capability declared in the test's target context, or the alias / qualified name of a consumed context",
                            ),
                        );
                        continue;
                    }
                };

                // -- Phase 3: validate signatures --
                let signature_errs =
                    check_mock_signatures(mock, &resolved_target, target_name, unit_tables);
                let had_sig_err = !signature_errs.is_empty();
                errors.extend(signature_errs);

                target_mocks.insert(
                    mock.target_name.name.clone(),
                    ResolvedMock {
                        decl: mock.clone(),
                        target: resolved_target,
                        had_sig_err,
                    },
                );
            }
        }

        if had_dup {
            // Skip body/type-checking for this target; we have name conflicts.
            continue;
        }

        // -- Phase 4: type-check bodies. --
        // (We build a resolved view targeting either commons or context;
        // mock bodies are type-checked with the mocked entity's privileges.)
        let bodies_errs = check_test_bodies(
            target_name,
            target_kind,
            indices,
            parsed,
            &target_mocks,
            unit_tables,
            exports_visibility,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
        );
        let bodies_failed = !bodies_errs.is_empty();
        errors.extend(bodies_errs);

        if bodies_failed {
            continue;
        }

        // -- Phase 5: emit TypeScript test module. --
        let emit_out = emit_test_module(
            target_name,
            target_kind,
            indices,
            parsed,
            &target_mocks,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_uses,
            exports_visibility,
        );
        if let Some((path, source, runnable)) = emit_out {
            outputs.push(CompiledFile {
                source_path: path.clone(),
                output_path: path,
                typescript: source,
            });
            runnable_tests.push(runnable);
        }
    }

    // v0.16: the top-level `tests/main.ts` runner is emitted once by the caller
    // after both unit- and integration-test passes, so it can aggregate both.
    (outputs, runnable_tests)
}

/// v0.16: process every `test integration "name"` suite. Validates the `wires`
/// participant set (existence, ≥ 2, no duplicates, full `consumes` closure),
/// type-checks each case body as a cross-context call from a synthetic harness
/// root that consumes every participant, and emits a TypeScript module that
/// stands the participants up as in-process Workers wired by simulated Service
/// Bindings and runs the cases across the real serialise/deserialise wire.
#[allow(clippy::too_many_arguments)]
fn process_integration_tests(
    integration_groups: &HashMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    kinds: &HashMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut Vec<CompileError>,
) -> (Vec<CompiledFile>, Vec<RunnableTest>) {
    let mut outputs: Vec<CompiledFile> = Vec::new();
    let mut runnables: Vec<RunnableTest> = Vec::new();

    let mut sorted: Vec<&String> = integration_groups.keys().collect();
    sorted.sort();

    let mut seen_suites: HashMap<String, Span> = HashMap::new();

    for group_name in sorted {
        let indices = integration_groups.get(group_name).unwrap();
        // Each suite is a single declaration. Two declarations sharing a name
        // collide into one group → a duplicate suite.
        let first = indices[0];
        let Some(decl) = parsed[first].integration() else {
            continue;
        };
        let mut duplicate = false;
        if let Some(prev) = seen_suites.get(&decl.suite) {
            duplicate = true;
            errors.push(
                CompileError::new(
                    "karn.integration.duplicate_suite",
                    decl.suite_span,
                    format!(
                        "integration test `\"{}\"` is declared more than once",
                        decl.suite
                    ),
                )
                .with_label(*prev, "previously declared here"),
            );
        } else {
            seen_suites.insert(decl.suite.clone(), decl.suite_span);
        }
        for &i in &indices[1..] {
            if let Some(other) = parsed[i].integration() {
                errors.push(
                    CompileError::new(
                        "karn.integration.duplicate_suite",
                        other.suite_span,
                        format!(
                            "integration test `\"{}\"` is declared more than once",
                            other.suite
                        ),
                    )
                    .with_label(decl.suite_span, "previously declared here"),
                );
                duplicate = true;
            }
        }

        // -- Validate participants. --
        let mut participants: Vec<String> = Vec::new();
        let mut participant_set: HashSet<String> = HashSet::new();
        let mut bad_participant = false;
        for p in &decl.participants {
            let q = p.joined();
            match kinds.get(&q) {
                Some(UnitKind::Context) => {}
                _ => {
                    errors.push(
                        CompileError::new(
                            "karn.integration.unknown_participant",
                            p.span,
                            format!("`{q}` is not a declared context in this project"),
                        )
                        .with_note(
                            "every name in a `wires` clause must be a context the project declares",
                        ),
                    );
                    bad_participant = true;
                    continue;
                }
            }
            if !participant_set.insert(q.clone()) {
                errors.push(CompileError::new(
                    "karn.integration.duplicate_participant",
                    p.span,
                    format!("context `{q}` is listed more than once in `wires`"),
                ));
                continue;
            }
            participants.push(q);
        }

        if participant_set.len() < 2 {
            errors.push(
                CompileError::new(
                    "karn.integration.too_few_participants",
                    decl.suite_span,
                    "an integration test must wire at least two contexts",
                )
                .with_note(
                    "to test a single context in isolation, use a unit test (`test <context> { … }`)",
                ),
            );
            bad_participant = true;
        }

        // -- Closure: every transitively-consumed context must be a participant.
        for p in &participants {
            if let Some(deps) = unit_consumes.get(p) {
                for d in deps {
                    if !participant_set.contains(d) {
                        errors.push(
                            CompileError::new(
                                "karn.integration.unwired_dependency",
                                decl.suite_span,
                                format!(
                                    "participant `{p}` consumes `{d}`, which is not wired into this integration test",
                                ),
                            )
                            .with_note(format!(
                                "add `{d}` to the `wires` clause — an integration test runs each participant as a real Worker, so every consumed context needs one",
                            )),
                        );
                        bad_participant = true;
                    }
                }
            }
        }

        // -- Duplicate case names within the suite. --
        let mut seen_cases: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            if let Some(d) = parsed[i].integration() {
                for case in &d.cases {
                    if let Some(prev) = seen_cases.get(&case.name) {
                        errors.push(
                            CompileError::new(
                                "karn.test.duplicate_case_name",
                                case.name_span,
                                format!(
                                    "test case `\"{}\"` is declared more than once in integration test `\"{}\"`",
                                    case.name, decl.suite
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                        bad_participant = true;
                    } else {
                        seen_cases.insert(case.name.clone(), case.name_span);
                    }
                }
            }
        }

        if duplicate || bad_participant {
            continue;
        }

        // -- Build the harness-root cross-context view (consumes all). --
        let harness_name = group_name.clone();
        let uses_targets: Vec<String> = decl.uses.iter().map(|u| u.target.joined()).collect();
        let mut harness_consumes = unit_consumes.clone();
        harness_consumes.insert(harness_name.clone(), participants.clone());
        let mut harness_uses = unit_uses.clone();
        harness_uses.insert(harness_name.clone(), uses_targets.clone());
        let cross_context = build_cross_context_info(
            &harness_name,
            &harness_consumes,
            unit_consumes_aliases,
            &harness_uses,
            unit_tables,
        );

        // -- Type-check each case body. --
        let mut body_errs: Vec<CompileError> = Vec::new();
        for &i in indices {
            let Some(d) = parsed[i].integration() else {
                continue;
            };
            for case in &d.cases {
                check_integration_case_body(
                    &participants,
                    &uses_targets,
                    case,
                    &cross_context,
                    unit_tables,
                    &mut body_errs,
                );
            }
        }
        let bodies_failed = !body_errs.is_empty();
        errors.extend(body_errs);
        if bodies_failed {
            continue;
        }

        // -- Emit the integration module. --
        if let Some((path, source, runnable)) = emit_integration_module(
            decl,
            &participants,
            &uses_targets,
            &cross_context,
            unit_consumes,
            unit_tables,
        ) {
            outputs.push(CompiledFile {
                source_path: path.clone(),
                output_path: path,
                typescript: source,
            });
            runnables.push(runnable);
        }
    }

    (outputs, runnables)
}

/// Type-check one integration test case body. The body lives in a synthetic
/// harness root that consumes every participant; entry calls
/// (`ctx.service(args)`) are therefore ordinary cross-context calls. The body
/// has type `Effect[Result[(), AssertionError]]` (modelled as
/// `Effect[Result[(), ValidationError]]`, as in unit tests).
fn check_integration_case_body(
    participants: &[String],
    uses_targets: &[String],
    case: &TestCase,
    cross_context: &resolver::CrossContextInfo,
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut Vec<CompileError>,
) {
    // Names in scope: types/fns/methods from `uses` commons (for constructing
    // arguments) plus each participant's types/methods (so return types rebrand
    // and variant patterns resolve).
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
    let mut methods: HashMap<String, ResolverMethodTable> = HashMap::new();
    let mut merge = |src: Option<&UnitTable>, with_fns: bool| {
        let Some(t) = src else { return };
        for (n, d) in &t.types {
            types.entry(n.clone()).or_insert_with(|| d.clone());
        }
        if with_fns {
            for (n, f) in &t.fns {
                fns.entry(n.clone()).or_insert_with(|| f.clone());
            }
        }
        for (n, mt) in &t.methods {
            let entry = methods.entry(n.clone()).or_default();
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
    };
    for u in uses_targets {
        merge(unit_tables.get(u), true);
    }
    for p in participants {
        merge(unit_tables.get(p), false);
    }

    let synthetic_commons = Commons {
        name: QualifiedName {
            parts: vec![Ident {
                name: "integration".to_string(),
                span: Span::default(),
            }],
            span: Span::default(),
        },
        items: Vec::new(),
        uses: Vec::new(),
        documentation: None,
        form: CommonsForm::Brace,
        span: Span::default(),
        trivia: Trivia::default(),
        trailing_comments: Vec::new(),
    };
    let resolved = crate::resolver::ResolvedCommons {
        commons: synthetic_commons,
        types,
        fns,
        methods,
        local_type_names: HashSet::new(),
        cross_context: cross_context.clone(),
        agents: HashMap::new(),
    };

    let unit_span = case.span;
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );
    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types).unwrap();
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        scopes: vec![HashMap::new()],
        return_ty: return_ty.clone(),
        return_ty_span: case.span,
        effectful: true,
        agent_state_ty: None,
        commit_seen: false,
        capabilities: HashMap::new(),
        declared_capabilities: HashMap::new(),
        given_remaining: HashSet::new(),
        given_used: HashSet::new(),
        in_test_body: true,
    };
    let _ = checker::type_of_block(&case.body, Some(&return_ty), &mut ctx);
}

/// Emit a single integration-test module plus its [`RunnableTest`] pointer.
/// The module imports each participant's workers-mode handler namespace (for
/// serialise/deserialise) and Worker entry (for dispatch), builds an in-process
/// env graph wiring the Service Bindings, and runs each case across the wire.
fn emit_integration_module(
    decl: &IntegrationDecl,
    participants: &[String],
    uses_targets: &[String],
    cross_context: &resolver::CrossContextInfo,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> Option<(PathBuf, String, RunnableTest)> {
    let sanitized = sanitise_suite(&decl.suite);
    let module_path = PathBuf::from(format!("tests/integration_{sanitized}.test.ts"));
    let mut out = String::new();
    out.push_str("// Generated by karnc — do not edit by hand.\n");
    out.push_str(&format!("// integration test: {}\n\n", decl.suite));

    // Runtime imports. When a participant owns agents, also pull in the
    // Durable-Object namespace helper + types for the in-memory DO stubs.
    let has_agents = participants
        .iter()
        .any(|p| unit_tables.get(p).is_some_and(|t| !t.agents.is_empty()));
    let runtime_import = emitter::runtime_import_for(&module_path);
    let agent_imports = if has_agents {
        ", makeIntegrationDoNamespace, type DurableObjectState, type DurableObjectNamespace"
    } else {
        ""
    };
    out.push_str(&format!(
        "import {{ Ok, Err, Some, None, callService, type Result, type Option, type ValidationError, type JsonValue, type ServiceBinding{agent_imports} }} from \"{runtime_import}\";\n"
    ));

    // Per-participant: workers handler namespace + Worker entry default export.
    for p in participants {
        let ns = p.replace('.', "_");
        let dir = worker_dir_name(p);
        out.push_str(&format!(
            "import * as {ns} from \"../workers/{dir}/handlers.js\";\n"
        ));
        out.push_str(&format!(
            "import worker_{ns} from \"../workers/{dir}/index.js\";\n"
        ));
    }

    // `uses` commons (for constructing arguments).
    let mut uses_imports: Vec<(String, String)> = Vec::new();
    for u in uses_targets {
        let ns = u.replace('.', "_");
        let path = relative_import_for_test(&commons_dir_for(u));
        uses_imports.push((ns, path));
    }
    uses_imports.sort();
    uses_imports.dedup();
    for (ns, path) in &uses_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.js\";\n"));
    }
    out.push('\n');

    out.push_str(&assertion_runtime_helpers());

    // The env-graph harness: stand each participant up as an in-process Worker
    // and wire its Service Bindings to its siblings; the root env binds to all.
    out.push_str(&emit_integration_harness(
        participants,
        unit_consumes,
        unit_tables,
    ));
    out.push('\n');

    // One async function per case.
    let mut typed = integration_typed_commons(uses_targets, participants, unit_tables);
    let mut case_runners: Vec<String> = Vec::new();
    for case in &decl.cases {
        let runner_name = sanitise_case_name(&case.name, &mut case_runners.len());
        case_runners.push(runner_name.clone());
        out.push_str(&format!("async function {runner_name}() {{\n"));
        out.push_str("  try {\n");
        out.push_str("    const deps = makeHarness();\n");
        // Bring `uses` commons names into scope for argument construction.
        for u in uses_targets {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                let mut names: Vec<&String> = table.types.keys().chain(table.fns.keys()).collect();
                names.sort();
                names.dedup();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
        }
        let body_src = emitter::lower_integration_case_body(&case.body, &mut typed, cross_context);
        for line in body_src.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    return { pass: true };\n");
        out.push_str("  } catch (e) {\n");
        out.push_str("    if (e instanceof AssertionError) {\n");
        out.push_str(
            "      return { pass: false, error: { message: e.message, location: e.location } };\n",
        );
        out.push_str("    }\n");
        out.push_str(
            "    return { pass: false, error: { message: String(e), location: \"unknown\" } };\n",
        );
        out.push_str("  }\n");
        out.push_str("}\n\n");
    }

    // Module runner.
    out.push_str("export async function run() {\n");
    out.push_str("  const results = [];\n");
    for (idx, case) in decl.cases.iter().enumerate() {
        let runner_name = &case_runners[idx];
        let escaped = escape_ts_string(&case.name);
        out.push_str(&format!(
            "  results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
        ));
    }
    out.push_str("  return results;\n");
    out.push_str("}\n");

    Some((
        module_path.clone(),
        out,
        RunnableTest {
            target_name: format!("integration · {}", decl.suite),
            module_path,
        },
    ))
}

/// Emit the `makeHarness()` factory: an in-process env per participant whose
/// Service Bindings call the sibling participants' real Worker `fetch` and whose
/// Durable-Object namespaces back the participant's own agents in memory, plus a
/// root env binding every participant (the test cases call in through it). A
/// fresh harness per case gives each case clean agent state.
fn emit_integration_harness(
    participants: &[String],
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
) -> String {
    let mut out = String::new();
    out.push_str("function makeHarness() {\n");
    // Declare every participant env first so sibling references resolve.
    for p in participants {
        let ns = p.replace('.', "_");
        out.push_str(&format!("  const env_{ns}: any = {{}};\n"));
    }
    // Wire each participant's consumed Service Bindings to its sibling Workers,
    // and back its own agents with in-memory Durable Object namespaces.
    for p in participants {
        let ns = p.replace('.', "_");
        if let Some(deps) = unit_consumes.get(p) {
            let mut deps_sorted = deps.clone();
            deps_sorted.sort();
            for d in &deps_sorted {
                let dns = d.replace('.', "_");
                let binding = crate::emitter::wrangler::consumed_binding_name(d);
                out.push_str(&format!(
                    "  env_{ns}.{binding} = {{ fetch: (req: Request) => worker_{dns}.fetch(req, env_{dns}) }} as ServiceBinding;\n"
                ));
            }
        }
        if let Some(table) = unit_tables.get(p) {
            let mut agents: Vec<&String> = table.agents.keys().collect();
            agents.sort();
            for agent in agents {
                let binding = crate::emitter::wrangler::agent_binding_name(agent);
                out.push_str(&format!(
                    "  env_{ns}.{binding} = makeIntegrationDoNamespace((state) => new {ns}.{agent}(state));\n"
                ));
            }
        }
    }
    // Root env binds to every participant.
    out.push_str("  const rootEnv: any = {};\n");
    for p in participants {
        let ns = p.replace('.', "_");
        let binding = crate::emitter::wrangler::consumed_binding_name(p);
        out.push_str(&format!(
            "  rootEnv.{binding} = {{ fetch: (req: Request) => worker_{ns}.fetch(req, env_{ns}) }} as ServiceBinding;\n"
        ));
    }
    out.push_str("  return { env: rootEnv };\n");
    out.push_str("}\n");
    out
}

/// Build the [`checker::TypedCommons`] used to lower integration case bodies —
/// `uses` commons plus participant types/fns/methods, so static calls and
/// constructors resolve.
fn integration_typed_commons(
    uses_targets: &[String],
    participants: &[String],
    unit_tables: &HashMap<String, UnitTable>,
) -> checker::TypedCommons {
    let mut types: HashMap<String, TypeDecl> = HashMap::new();
    let mut fns: HashMap<String, FnDecl> = HashMap::new();
    let mut methods: HashMap<String, ResolverMethodTable> = HashMap::new();
    let mut add = |t: Option<&UnitTable>, with_fns: bool| {
        let Some(t) = t else { return };
        for (n, d) in &t.types {
            types.entry(n.clone()).or_insert_with(|| d.clone());
        }
        if with_fns {
            for (n, f) in &t.fns {
                fns.entry(n.clone()).or_insert_with(|| f.clone());
            }
        }
        for (n, mt) in &t.methods {
            let entry = methods.entry(n.clone()).or_default();
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
    };
    for u in uses_targets {
        add(unit_tables.get(u), true);
    }
    for p in participants {
        add(unit_tables.get(p), false);
    }
    checker::TypedCommons {
        commons: Commons {
            name: QualifiedName {
                parts: vec![Ident {
                    name: "integration".to_string(),
                    span: Span::default(),
                }],
                span: Span::default(),
            },
            items: Vec::new(),
            uses: Vec::new(),
            documentation: None,
            form: CommonsForm::Brace,
            span: Span::default(),
            trivia: Trivia::default(),
            trailing_comments: Vec::new(),
        },
        types,
        fns,
        methods,
        expr_types: HashMap::new(),
    }
}

fn sanitise_suite(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "suite".to_string()
    } else {
        trimmed
    }
}

#[derive(Debug, Clone)]
struct ResolvedMock {
    decl: MockDecl,
    target: MockTarget,
    had_sig_err: bool,
}

/// Discovered, named test ready to be invoked from the top-level runner.
struct RunnableTest {
    /// Joined target name (e.g., `commerce.payment`).
    target_name: String,
    /// The module's output path relative to the project root.
    module_path: PathBuf,
}

fn first_test_target_span(indices: &[usize], parsed: &[ParsedFile]) -> Span {
    indices
        .first()
        .and_then(|&i| parsed[i].test().map(|t| t.target.span))
        .unwrap_or_default()
}

fn check_mock_signatures(
    mock: &MockDecl,
    target: &MockTarget,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    match target {
        MockTarget::Capability(cap_name) => {
            let Some(table) = unit_tables.get(target_name) else {
                return errors;
            };
            let Some(cap) = table.capabilities.get(cap_name) else {
                return errors;
            };
            for cap_op in &cap.ops {
                if !mock.ops.iter().any(|o| o.name.name == cap_op.name.name) {
                    errors.push(CompileError::new(
                        "karn.mock.signature_mismatch",
                        mock.span,
                        format!(
                            "mock `{}` for capability `{}` is missing operation `{}`",
                            mock.impl_name.name, cap_name, cap_op.name.name
                        ),
                    ));
                }
            }
            for op in &mock.ops {
                let Some(cap_op) = cap.ops.iter().find(|o| o.name.name == op.name.name) else {
                    errors.push(CompileError::new(
                        "karn.mock.signature_mismatch",
                        op.span,
                        format!(
                            "mock operation `{}.{}` does not match any operation in capability `{}`",
                            mock.impl_name.name, op.name.name, cap_name
                        ),
                    ));
                    continue;
                };
                check_mock_op_signature(op, &cap_op.params, &cap_op.return_type, &mut errors);
            }
        }
        MockTarget::ConsumedContext { qualified, .. } => {
            let Some(table) = unit_tables.get(qualified) else {
                return errors;
            };
            // Each mock op must match a service in the consumed context.
            for op in &mock.ops {
                let Some(service) = table.services.get(&op.name.name) else {
                    errors.push(CompileError::new(
                        "karn.mock.signature_mismatch",
                        op.span,
                        format!(
                            "mock operation `{}.{}` does not match any service in consumed context `{qualified}`",
                            mock.impl_name.name, op.name.name
                        ),
                    ));
                    continue;
                };
                // Find an `on call` handler and compare signatures.
                let Some(handler) = service
                    .handlers
                    .iter()
                    .find(|h| matches!(h.kind, HandlerKind::Call))
                else {
                    errors.push(CompileError::new(
                        "karn.mock.signature_mismatch",
                        op.span,
                        format!(
                            "service `{}` in consumed context `{qualified}` has no `on call` handler to mock",
                            op.name.name
                        ),
                    ));
                    continue;
                };
                check_mock_op_signature(op, &handler.params, &handler.return_type, &mut errors);
            }
        }
    }
    errors
}

fn check_mock_op_signature(
    op: &MockOp,
    target_params: &[Param],
    target_return: &TypeRef,
    errors: &mut Vec<CompileError>,
) {
    if op.params.len() != target_params.len() {
        errors.push(CompileError::new(
            "karn.mock.signature_mismatch",
            op.span,
            format!(
                "mock operation `{}` has {} parameter(s), but the target declares {}",
                op.name.name,
                op.params.len(),
                target_params.len()
            ),
        ));
        return;
    }
    for (i, (target_p, mock_p)) in target_params.iter().zip(op.params.iter()).enumerate() {
        if !type_refs_match(&target_p.type_ref, &mock_p.type_ref) {
            errors.push(CompileError::new(
                "karn.mock.signature_mismatch",
                mock_p.span,
                format!(
                    "mock operation `{}` parameter {} has type `{}`, but the target declares `{}`",
                    op.name.name,
                    i + 1,
                    ts_type_ref_display(&mock_p.type_ref),
                    ts_type_ref_display(&target_p.type_ref),
                ),
            ));
        }
    }
    if !type_refs_match(target_return, &op.return_type) {
        errors.push(CompileError::new(
            "karn.mock.signature_mismatch",
            op.return_type.span(),
            format!(
                "mock operation `{}` returns `{}`, but the target declares `{}`",
                op.name.name,
                ts_type_ref_display(&op.return_type),
                ts_type_ref_display(target_return),
            ),
        ));
    }
}

/// Type-check all mocks and test bodies for a target. Bodies use the target's
/// privileged view; consumed-context mock bodies use the consumed context's
/// privileged view.
#[allow(clippy::too_many_arguments)]
fn check_test_bodies(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    let _ = exports_visibility;

    // Type-check mock bodies. Provider mock bodies share the target context's
    // privileges; consumed-context mock bodies use the consumed context's
    // privileged view (so they can construct opaque types from there).
    for mock_entry in mocks.values() {
        if mock_entry.had_sig_err {
            continue;
        }
        let owning_unit = match &mock_entry.target {
            MockTarget::Capability(_) => target_name.to_string(),
            MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
        };
        for op in &mock_entry.decl.ops {
            check_op_body_with_privileged_view(
                &owning_unit,
                op,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &mut errors,
                /* in_test_body */ false,
            );
        }
    }

    // Type-check test case bodies — they live in the target's privileged
    // view, with mocked surfaces replacing the target's normal providers /
    // consumed contexts.
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            check_test_case_body(
                target_name,
                target_kind,
                case,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
                &mut errors,
            );
        }
    }

    errors
}

#[allow(clippy::too_many_arguments)]
fn check_op_body_with_privileged_view(
    owning_unit: &str,
    op: &MockOp,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
    in_test_body: bool,
) {
    let Some((resolved, _)) = build_privileged_resolved(
        owning_unit,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    checker::check_handler_body(
        &op.body,
        &op.return_type,
        op.return_type.span(),
        &op.params,
        &resolved,
        &mut expr_types,
        errors,
        HashMap::new(),
        HashMap::new(),
        None,
        None,
        Vec::new(),
        false,
    );
    let _ = in_test_body; // Mock op bodies are not test bodies; assert is not valid here.
}

#[allow(clippy::too_many_arguments)]
fn check_test_case_body(
    target_name: &str,
    target_kind: UnitKind,
    case: &TestCase,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
) {
    let Some((resolved, _)) = build_privileged_resolved(
        target_name,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) else {
        return;
    };
    let _ = target_kind;
    let mut expr_types: HashMap<Span, checker::Ty> = HashMap::new();
    // Synthesise an Effect[Result[(), ValidationError]] return type as a
    // stand-in for Effect[Result[(), AssertionError]]. v0.7 doesn't model an
    // explicit AssertionError type — the runtime catches it instead.
    let unit_span = case.span;
    let synthetic_return = TypeRef::Effect(
        Box::new(TypeRef::Result(
            Box::new(TypeRef::Unit(unit_span)),
            Box::new(TypeRef::ValidationError(unit_span)),
            unit_span,
        )),
        unit_span,
    );

    // Capabilities of the target context, if any (so the test body can
    // call capabilities directly when targeting a context).
    let mut capability_info_map: HashMap<String, checker::CapabilityInfo> = HashMap::new();
    if let Some(table) = unit_tables.get(target_name) {
        for (name, decl) in &table.capabilities {
            let ops = decl
                .ops
                .iter()
                .map(|op| checker::CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| {
                            checker::resolve_type_ref(&p.type_ref, &resolved.types)
                                .unwrap_or(checker::Ty::Unit)
                        })
                        .collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &resolved.types)
                        .unwrap_or(checker::Ty::Unit),
                })
                .collect();
            capability_info_map.insert(
                name.clone(),
                checker::CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            );
        }
    }

    // All declared capabilities are implicitly "given" inside a test body;
    // the test runner wires them via the mocked deps. We feed the same map
    // to both `capabilities` (in-scope) and `declared_capabilities`.
    let given_declared: Vec<String> = capability_info_map.keys().cloned().collect();

    let return_ty = checker::resolve_type_ref(&synthetic_return, &resolved.types).unwrap();
    let return_ty_span = case.span;
    let effectful = matches!(return_ty, checker::Ty::Effect(_));
    let mut ctx = checker::Ctx {
        input: &resolved,
        expr_types: &mut expr_types,
        errors,
        scopes: vec![HashMap::new()],
        return_ty: return_ty.clone(),
        return_ty_span,
        effectful,
        agent_state_ty: None,
        commit_seen: false,
        capabilities: capability_info_map.clone(),
        declared_capabilities: capability_info_map,
        given_remaining: given_declared.iter().cloned().collect(),
        given_used: HashSet::new(),
        in_test_body: true,
    };
    let _ = checker::type_of_block(&case.body, Some(&return_ty), &mut ctx);
    // Don't enforce return-type equality; the test runner discards the
    // tail expression and recovers success/failure from assertion outcome.
    // Don't enforce "every given used" — capabilities are implicitly
    // available in a test body.
}

/// Build a [`resolver::ResolvedCommons`] backed by `owning_unit`'s privileged
/// view: its types, fns, methods, plus types/fns from every commons it
/// `uses`, plus exported types from every consumed context. The same
/// shape used by the production pipeline. Returns the [`ResolvedCommons`]
/// plus a synthetic commons span for the test.
fn build_privileged_resolved(
    owning_unit: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> Option<(crate::resolver::ResolvedCommons, ())> {
    let local = unit_tables.get(owning_unit)?;
    let mut types = local.types.clone();
    let mut fns = local.fns.clone();
    let mut methods = local.methods.clone();
    if let Some(targets) = unit_uses.get(owning_unit) {
        for t in targets {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, d) in &used.fns {
                    fns.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, mt) in &used.methods {
                    let entry = methods.entry(n.clone()).or_default();
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
        }
    }
    // Consumed-context types come in too (only the exported ones).
    if let Some(consumed) = unit_consumes.get(owning_unit) {
        for t in consumed {
            if let Some(used) = unit_tables.get(t) {
                for (n, d) in &used.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, mt) in &used.methods {
                    let entry = methods.entry(n.clone()).or_default();
                    for (m, decl) in &mt.instance {
                        entry
                            .instance
                            .entry(m.clone())
                            .or_insert_with(|| decl.clone());
                    }
                }
            }
        }
    }
    let local_type_names: HashSet<String> = local.types.keys().cloned().collect();
    let cross_context = build_cross_context_info(
        owning_unit,
        unit_consumes,
        unit_consumes_aliases,
        unit_uses,
        unit_tables,
    );
    let synthetic_commons = Commons {
        name: QualifiedName {
            parts: owning_unit
                .split('.')
                .map(|part| Ident {
                    name: part.to_string(),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        },
        items: Vec::new(),
        uses: Vec::new(),
        documentation: None,
        form: CommonsForm::Brace,
        span: Span::default(),
        trivia: Trivia::default(),
        trailing_comments: Vec::new(),
    };
    let agents_for_resolved = unit_tables
        .get(owning_unit)
        .map(|t| t.agents.clone())
        .unwrap_or_default();
    let resolved = crate::resolver::ResolvedCommons {
        commons: synthetic_commons,
        types,
        fns,
        methods,
        local_type_names,
        cross_context,
        agents: agents_for_resolved,
    };
    Some((resolved, ()))
}

/// Emit a single test module TypeScript file plus the [`RunnableTest`]
/// pointer used by the top-level runner.
#[allow(clippy::too_many_arguments)]
fn emit_test_module(
    target_name: &str,
    target_kind: UnitKind,
    indices: &[usize],
    parsed: &[ParsedFile],
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
) -> Option<(PathBuf, String, RunnableTest)> {
    let _ = exports_visibility;
    let mut out = String::new();
    let target_ns = target_name.replace('.', "_");
    let target_dir = commons_dir_for(target_name);
    // Output file: tests/<sanitised-target>.test.ts
    let module_path = PathBuf::from(format!("tests/{}.test.ts", target_name.replace('.', "_")));

    out.push_str("// Generated by karnc — do not edit by hand.\n");
    out.push_str(&format!("// test target: {target_name}\n\n"));

    // Result/Option helpers — same shape as the production runtime imports.
    // The test module lives at `tests/<file>.test.ts`, so the runtime is one
    // directory up. Compute through the same depth machinery used by the
    // per-context emitter. If the target context declares agents, also pull
    // in `makeTestState` so agent invocations can synthesise DO state.
    let has_agents = unit_tables
        .get(target_name)
        .map(|t| !t.agents.is_empty())
        .unwrap_or(false);
    let runtime_import = emitter::runtime_import_for(&module_path);
    let extra = if has_agents { ", makeTestState" } else { "" };
    out.push_str(&format!(
        "import {{ Ok, Err, Some, None{extra}, type Result, type Option, type ValidationError }} from \"{runtime_import}\";\n"
    ));

    // Compute relative import path from tests/ to the target's output dir.
    let import_target = relative_import_for_test(&target_dir);
    out.push_str(&format!(
        "import * as {target_ns} from \"./{import_target}.js\";\n"
    ));

    // Consumed contexts (for the target context, if any).
    let mut consumed_imports: Vec<(String, String)> = Vec::new();
    if let Some(consumed) = unit_consumes.get(target_name) {
        for q in consumed {
            let ns = q.replace('.', "_");
            let dir = commons_dir_for(q);
            let import_path = relative_import_for_test(&dir);
            consumed_imports.push((ns, import_path));
        }
    }
    consumed_imports.sort();
    for (ns, path) in &consumed_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.js\";\n"));
    }

    // `uses` commons reachable from the test fragments — needed for `Money`,
    // etc., used inside test bodies. We pull from the target context's uses.
    let mut uses_imports: Vec<(String, String)> = Vec::new();
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            let dir = commons_dir_for(u);
            let import_path = relative_import_for_test(&dir);
            uses_imports.push((ns, import_path));
        }
    }
    uses_imports.sort();
    for (ns, path) in &uses_imports {
        out.push_str(&format!("import * as {ns} from \"./{path}.js\";\n"));
    }
    out.push('\n');

    // Assertion helper used by lowered `assert` statements.
    out.push_str(&assertion_runtime_helpers());

    // Emit mock implementations. Sort by target name so emission is
    // deterministic regardless of the mock map's hash iteration order (a test
    // with more than one mock would otherwise flake).
    let mut sorted_mocks: Vec<(&String, &ResolvedMock)> = mocks.iter().collect();
    sorted_mocks.sort_by(|a, b| a.0.cmp(b.0));
    for (_, mock) in sorted_mocks {
        out.push_str(&emit_mock_class(
            mock,
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        ));
        out.push('\n');
    }

    // Emit the deps factory.
    out.push_str(&emit_test_deps(
        target_name,
        target_kind,
        mocks,
        unit_tables,
        unit_consumes,
        unit_consumes_aliases,
    ));
    out.push('\n');

    // Emit one async function per test case.
    let mut case_runners: Vec<String> = Vec::new();
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            let runner_name = sanitise_case_name(&case.name, &mut case_runners.len());
            case_runners.push(runner_name.clone());
            out.push_str(&emit_test_case_function(
                &runner_name,
                case,
                target_name,
                target_kind,
                mocks,
                unit_tables,
                unit_uses,
                unit_consumes,
                unit_consumes_aliases,
            ));
            out.push('\n');
        }
    }

    // Module-level runner.
    out.push_str("export async function run() {\n");
    out.push_str("  const results = [];\n");
    let mut case_index = 0;
    for &i in indices {
        let Some(test_decl) = parsed[i].test() else {
            continue;
        };
        for case in &test_decl.cases {
            let runner_name = &case_runners[case_index];
            let escaped = escape_ts_string(&case.name);
            out.push_str(&format!(
                "  results.push({{ name: \"{escaped}\", ...(await {runner_name}()) }});\n"
            ));
            case_index += 1;
        }
    }
    out.push_str("  return results;\n");
    out.push_str("}\n");

    Some((
        module_path.clone(),
        out,
        RunnableTest {
            target_name: target_name.to_string(),
            module_path,
        },
    ))
}

/// Render the relative import path from the `tests/` output directory to the
/// directory holding a target unit's TypeScript output.
fn relative_import_for_test(target_dir: &Path) -> String {
    let parts: Vec<String> = target_dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        "../index".to_string()
    } else {
        format!("../{}", parts.join("/"))
    }
}

fn assertion_runtime_helpers() -> String {
    let mut out = String::new();
    out.push_str("class AssertionError extends Error {\n");
    out.push_str(
        "  constructor(public location: string, public start: number, public end: number) {\n",
    );
    out.push_str("    super(`assertion failed at ${location}`);\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(
        "function __karnAssertionFailure(location: string, start: number, end: number) {\n",
    );
    out.push_str("  return new AssertionError(location, start, end);\n");
    out.push_str("}\n");
    out.push_str(
        "function __karnAssert(cond: boolean, location: string, start: number, end: number): void {\n",
    );
    out.push_str("  if (!cond) { throw __karnAssertionFailure(location, start, end); }\n");
    out.push_str("}\n\n");
    out
}

fn emit_mock_class(
    mock: &ResolvedMock,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let mut out = String::new();
    let impl_name = &mock.decl.impl_name.name;
    out.push_str(&format!("class {impl_name} {{\n"));
    // Bring the mocked entity's privileged namespace into local scope so the
    // body can reference its types and variants unqualified.
    let owning_unit = match &mock.target {
        MockTarget::Capability(_) => target_name.to_string(),
        MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
    };
    let scope_ns = owning_unit.replace('.', "_");
    let mut scope_type_names: HashSet<String> = unit_tables
        .get(&owning_unit)
        .map(|t| t.types.keys().cloned().collect())
        .unwrap_or_default();
    // v0.9.2: the owning context re-exports the commons types it `uses` under
    // its own namespace (branded), so a mock signature that names one — e.g.
    // `track(code: ShortCode)` — must qualify it to `<ns>.ShortCode` too.
    if let Some(used) = unit_uses.get(&owning_unit) {
        for u in used {
            if let Some(table) = unit_tables.get(u) {
                scope_type_names.extend(table.types.keys().cloned());
            }
        }
    }
    let scope_names: Vec<String> = if let Some(table) = unit_tables.get(&owning_unit) {
        let mut v: Vec<String> = table
            .types
            .keys()
            .chain(table.fns.keys())
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    } else {
        Vec::new()
    };
    for op in &mock.decl.ops {
        let params = op
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}: {}",
                    p.name.name,
                    ts_type_ref_emit_qualified(&p.type_ref, &scope_type_names, &scope_ns)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_ty = ts_type_ref_emit_qualified(&op.return_type, &scope_type_names, &scope_ns);
        out.push_str(&format!(
            "  async {}({params}): {return_ty} {{\n",
            op.name.name
        ));
        if !scope_names.is_empty() {
            out.push_str(&format!(
                "    const {{ {} }} = {scope_ns} as any;\n",
                scope_names.join(", ")
            ));
        }
        let body_src = emit_mock_op_body(
            op,
            mock,
            target_name,
            unit_tables,
            unit_uses,
            unit_consumes,
            unit_consumes_aliases,
        );
        for line in body_src.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("  }\n");
    }
    out.push_str("}\n");
    out
}

/// Render a mock operation body using the same lowering the production
/// emitter applies to provider operations. We don't have direct access to
/// the typed-commons machinery here, so we hand-roll a small lowerer.
fn emit_mock_op_body(
    op: &MockOp,
    mock: &ResolvedMock,
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    // For consumed-context mocks the body has the consumed context's
    // privileges; for provider mocks the body shares the target context.
    let owning_unit = match &mock.target {
        MockTarget::Capability(_) => target_name.to_string(),
        MockTarget::ConsumedContext { qualified, .. } => qualified.clone(),
    };
    // Run the type checker first so the lowering knows the type of each
    // expression (notably: variant constructor references).
    let mut typed = synthetic_typed_commons_for_target(&owning_unit, unit_tables, unit_uses);
    if let Some((resolved, _)) = build_privileged_resolved(
        &owning_unit,
        unit_tables,
        unit_uses,
        unit_consumes,
        unit_consumes_aliases,
    ) {
        let mut errs: Vec<CompileError> = Vec::new();
        checker::check_handler_body(
            &op.body,
            &op.return_type,
            op.return_type.span(),
            &op.params,
            &resolved,
            &mut typed.expr_types,
            &mut errs,
            HashMap::new(),
            HashMap::new(),
            None,
            None,
            Vec::new(),
            false,
        );
    }
    let cross = crate::resolver::CrossContextInfo::default();
    emitter::lower_block_to_async_body(&op.body, &op.return_type, &mut typed, &cross)
}

fn synthetic_typed_commons_for_target(
    target_name: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
) -> checker::TypedCommons {
    let table = unit_tables.get(target_name).cloned().unwrap_or_default();
    let mut types = table.types;
    let mut fns = table.fns;
    let mut methods = table.methods;
    // Pull in names that come into scope via the target's `uses` clauses, so
    // the test-body lowering's static-call check (`<Type>.of(...)` etc.)
    // resolves against the same set of names the source can mention.
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            if let Some(t) = unit_tables.get(u) {
                for (n, d) in &t.types {
                    types.entry(n.clone()).or_insert_with(|| d.clone());
                }
                for (n, f) in &t.fns {
                    fns.entry(n.clone()).or_insert_with(|| f.clone());
                }
                for (n, mt) in &t.methods {
                    let entry = methods.entry(n.clone()).or_default();
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
        }
    }
    checker::TypedCommons {
        commons: Commons {
            name: QualifiedName {
                parts: target_name
                    .split('.')
                    .map(|p| Ident {
                        name: p.to_string(),
                        span: Span::default(),
                    })
                    .collect(),
                span: Span::default(),
            },
            items: Vec::new(),
            uses: Vec::new(),
            documentation: None,
            form: CommonsForm::Brace,
            span: Span::default(),
            trivia: Trivia::default(),
            trailing_comments: Vec::new(),
        },
        types,
        fns,
        methods,
        expr_types: HashMap::new(),
    }
}

fn emit_test_deps(
    target_name: &str,
    target_kind: UnitKind,
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let mut out = String::new();
    out.push_str("function makeTestDeps() {\n");
    let mut entries: Vec<String> = Vec::new();
    if target_kind == UnitKind::Context
        && let Some(table) = unit_tables.get(target_name)
    {
        let ns = target_name.replace('.', "_");
        // Sorted so `makeTestDeps` field order is deterministic across the
        // capability map's hash iteration order.
        let mut caps: Vec<&String> = table.capabilities.keys().collect();
        caps.sort();
        for cap in caps {
            // Find a mock for this capability, otherwise fall back to the
            // declared provider.
            let entry = match mocks.get(cap) {
                Some(m) if matches!(m.target, MockTarget::Capability(_)) => {
                    format!("{cap}: new {}()", m.decl.impl_name.name)
                }
                _ => {
                    if let Some(provider) = table.providers.get(cap) {
                        format!("{cap}: new {ns}.{}()", provider.provider_name.name)
                    } else {
                        format!("{cap}: undefined as unknown as {ns}.{cap}")
                    }
                }
            };
            entries.push(entry);
        }
        // Cross-context surface: substitute mocks when present.
        let consumed = unit_consumes.get(target_name).cloned().unwrap_or_default();
        let aliases = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let mut alias_for_target: HashMap<String, String> = HashMap::new();
        for (alias, q) in &aliases {
            alias_for_target.insert(q.clone(), alias.clone());
        }
        let mut surface_entries: Vec<String> = Vec::new();
        for q in &consumed {
            let key = alias_for_target
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            let mock_for_key = mocks.values().find(|m| match &m.target {
                MockTarget::ConsumedContext { qualified, alias } => {
                    qualified == q && (alias == &key || alias == q)
                }
                _ => false,
            });
            if let Some(m) = mock_for_key {
                surface_entries.push(format!("{key}: new {}()", m.decl.impl_name.name));
            } else {
                let other_ns = q.replace('.', "_");
                surface_entries.push(format!(
                    "{key}: undefined as unknown as ReturnType<typeof {other_ns}.makeSurface>"
                ));
            }
        }
        if !surface_entries.is_empty() {
            entries.push(format!("surface: {{ {} }}", surface_entries.join(", ")));
        }
    }
    out.push_str(&format!("  return {{ {} }};\n", entries.join(", ")));
    out.push_str("}\n");
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_test_case_function(
    runner_name: &str,
    case: &TestCase,
    target_name: &str,
    target_kind: UnitKind,
    mocks: &HashMap<String, ResolvedMock>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
) -> String {
    let _ = mocks;
    let mut out = String::new();
    let target_ns = target_name.replace('.', "_");
    out.push_str(&format!("async function {runner_name}() {{\n"));
    out.push_str("  try {\n");
    // v0.9.2: reset the target context's agent registries so each test sees a
    // fresh per-key state (finding #10's "fresh per test" half).
    let target_has_agents = unit_tables
        .get(target_name)
        .is_some_and(|t| !t.agents.is_empty());
    if target_has_agents {
        out.push_str(&format!("    {target_ns}.__resetAgents();\n"));
    }
    if target_kind == UnitKind::Context {
        out.push_str("    const deps = makeTestDeps();\n");
    } else {
        out.push_str("    const deps = {};\n");
    }
    // Bring the target's top-level names into local scope so the lowered
    // body can reference them unqualified. The target's types and fns are
    // exported from its namespace by the production emitter.
    if let Some(table) = unit_tables.get(target_name) {
        let mut names: Vec<String> = table
            .types
            .keys()
            .chain(table.fns.keys())
            .cloned()
            .collect();
        // For contexts, also bring services and providers into scope.
        let extras: Vec<String> = table
            .services
            .keys()
            .chain(table.agents.keys())
            .cloned()
            .collect();
        names.extend(extras);
        // v0.9.2: bring each agent's construction factory into scope so a test
        // body's `AgentName(key)` lowers to `__makeAgentName(key)`.
        for agent in table.agents.keys() {
            names.push(crate::emitter::agent_factory_name(agent));
        }
        names.sort();
        names.dedup();
        if !names.is_empty() {
            let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
            out.push_str(&format!(
                "    const {{ {} }} = {target_ns} as any;\n",
                joined.join(", ")
            ));
        }
    }
    // Bring in `uses` commons names too — the target's body can use them.
    if let Some(used) = unit_uses.get(target_name) {
        for u in used {
            let ns = u.replace('.', "_");
            if let Some(table) = unit_tables.get(u) {
                let mut names: Vec<&String> = table.types.keys().chain(table.fns.keys()).collect();
                names.sort();
                names.dedup();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
        }
    }
    // Bring consumed-context exported names into scope, plus a `Payment`
    // alias for the consumed surface (so `Payment.authorise.call(...)` works).
    if let Some(consumed) = unit_consumes.get(target_name) {
        let aliases = unit_consumes_aliases
            .get(target_name)
            .cloned()
            .unwrap_or_default();
        let mut alias_for: HashMap<String, String> = HashMap::new();
        for (alias, q) in &aliases {
            alias_for.insert(q.clone(), alias.clone());
        }
        for q in consumed {
            let ns = q.replace('.', "_");
            if let Some(table) = unit_tables.get(q) {
                let mut names: Vec<&String> = table.types.keys().collect();
                names.sort();
                if !names.is_empty() {
                    let joined: Vec<String> = names.iter().map(|n| (*n).clone()).collect();
                    out.push_str(&format!(
                        "    const {{ {} }} = {ns} as any;\n",
                        joined.join(", ")
                    ));
                }
            }
            let key = alias_for
                .get(q)
                .cloned()
                .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
            out.push_str(&format!(
                "    const {key} = (deps as any).surface?.{key};\n"
            ));
        }
    }
    let mut typed = synthetic_typed_commons_for_target(target_name, unit_tables, unit_uses);
    let cross = crate::resolver::CrossContextInfo::default();
    let test_services: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.services.keys().cloned().collect())
        .unwrap_or_default();
    let test_agents: HashSet<String> = unit_tables
        .get(target_name)
        .map(|t| t.agents.keys().cloned().collect())
        .unwrap_or_default();
    let body_src =
        emitter::lower_test_case_body(&case.body, &mut typed, &cross, test_services, test_agents);
    for line in body_src.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    return { pass: true };\n");
    out.push_str("  } catch (e) {\n");
    out.push_str("    if (e instanceof AssertionError) {\n");
    out.push_str(
        "      return { pass: false, error: { message: e.message, location: e.location } };\n",
    );
    out.push_str("    }\n");
    out.push_str(
        "    return { pass: false, error: { message: String(e), location: \"unknown\" } };\n",
    );
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn emit_test_main(tests: &[RunnableTest]) -> String {
    let mut out = String::new();
    out.push_str("// Generated by karnc — do not edit by hand.\n");
    out.push_str("// top-level test runner\n\n");
    // Node's `process` global isn't declared without @types/node. The
    // runner only uses `process.exit`, so we narrow the global with a
    // minimal ambient declaration rather than pulling in a dependency.
    out.push_str("declare const process: { exit(code: number): never };\n\n");
    let mut sorted: Vec<&RunnableTest> = tests.iter().collect();
    sorted.sort_by(|a, b| a.target_name.cmp(&b.target_name));
    for (i, t) in sorted.iter().enumerate() {
        let module_stem = t
            .module_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("test");
        out.push_str(&format!(
            "import * as test_{i} from \"./{module_stem}.js\";\n"
        ));
    }
    out.push('\n');
    out.push_str("async function main() {\n");
    out.push_str("  const modules = [\n");
    for (i, t) in sorted.iter().enumerate() {
        out.push_str(&format!(
            "    {{ name: \"{}\", run: test_{i}.run }},\n",
            t.target_name
        ));
    }
    out.push_str("  ];\n");
    out.push_str("  let passed = 0;\n");
    out.push_str("  let failed = 0;\n");
    out.push_str("  console.log(\"Running tests...\\n\");\n");
    out.push_str("  for (const m of modules) {\n");
    out.push_str("    console.log(`${m.name}:`);\n");
    out.push_str("    const results = await m.run();\n");
    out.push_str("    for (const r of results) {\n");
    out.push_str(
        "      if (r.pass) { passed++; console.log(`  \\u2713 ${r.name}`); } else { failed++; console.log(`  \\u2717 ${r.name}`); if (r.error) console.log(`    ${r.error.message}`); }\n",
    );
    out.push_str("    }\n");
    out.push_str("    console.log(\"\");\n");
    out.push_str("  }\n");
    out.push_str("  console.log(`${passed} passed, ${failed} failed.`);\n");
    out.push_str("  if (failed > 0) process.exit(1);\n");
    out.push_str("}\n\n");
    out.push_str("main();\n");
    out
}

fn sanitise_case_name(name: &str, index: &mut usize) -> String {
    let mut s = String::from("test_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
        } else {
            s.push('_');
        }
    }
    if s == "test_" {
        s.push_str(&index.to_string());
    }
    *index += 1;
    s
}

fn escape_ts_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

#[allow(dead_code)]
fn ts_type_ref_emit(r: &TypeRef) -> String {
    // For mocks we lean on the same TS rendering used by the production
    // emitter; for now, render via a simple display that matches what's
    // emitted by the production path for capability/service signatures.
    match r {
        TypeRef::Base(b, _) => match b {
            BaseType::Int => "number".to_string(),
            BaseType::String => "string".to_string(),
            BaseType::Bool => "boolean".to_string(),
        },
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(t, e, _) => {
            format!("Result<{}, {}>", ts_type_ref_emit(t), ts_type_ref_emit(e))
        }
        TypeRef::Option(t, _) => format!("Option<{}>", ts_type_ref_emit(t)),
        TypeRef::Effect(t, _) => format!("Promise<{}>", ts_type_ref_emit(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult<{}>", ts_type_ref_emit(t)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}

/// Like `ts_type_ref_emit`, but qualifies named types that live in the
/// privileged scope of a mocked entity with the owning context's namespace.
/// Mock method signatures sit outside the destructuring statement that brings
/// the namespace's value-side names into local scope, so the types need to
/// be referenced fully qualified.
fn ts_type_ref_emit_qualified(
    r: &TypeRef,
    scope_type_names: &HashSet<String>,
    scope_ns: &str,
) -> String {
    match r {
        TypeRef::Base(b, _) => match b {
            BaseType::Int => "number".to_string(),
            BaseType::String => "string".to_string(),
            BaseType::Bool => "boolean".to_string(),
        },
        TypeRef::Named(id) => {
            if scope_type_names.contains(&id.name) {
                format!("{scope_ns}.{}", id.name)
            } else {
                id.name.clone()
            }
        }
        TypeRef::Result(t, e, _) => format!(
            "Result<{}, {}>",
            ts_type_ref_emit_qualified(t, scope_type_names, scope_ns),
            ts_type_ref_emit_qualified(e, scope_type_names, scope_ns)
        ),
        TypeRef::Option(t, _) => format!(
            "Option<{}>",
            ts_type_ref_emit_qualified(t, scope_type_names, scope_ns)
        ),
        TypeRef::Effect(t, _) => format!(
            "Promise<{}>",
            ts_type_ref_emit_qualified(t, scope_type_names, scope_ns)
        ),
        TypeRef::HttpResult(t, _) => format!(
            "HttpResult<{}>",
            ts_type_ref_emit_qualified(t, scope_type_names, scope_ns)
        ),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}
