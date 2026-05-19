//! Multi-file project compilation (v0.3 §3.2 and §3.3).
//!
//! A "project" is a directory tree of `.karn` source files. The dotted name
//! of a commons (e.g., `karn.time`) maps to a path under the project root —
//! either a single file (`karn/time.karn`) or a directory of files all
//! sharing the same `commons karn.time` header (`karn/time/*.karn`).
//!
//! Compilation proceeds in two passes:
//!   1. **Discover and parse** every `.karn` file. Group by commons name.
//!      Build a global symbol table where each commons contributes its
//!      declarations.
//!   2. **Resolve, type-check, and emit** each commons with full visibility
//!      of the commons it transitively `uses`. Two passes keep `uses` cycles
//!      trivial — there is no order-of-evaluation, only declarative mixin.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::ast::*;
use crate::checker;
use crate::emitter;
use crate::error::CompileError;
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

/// Compile a Karn project rooted at `root`. The root must be a directory.
pub fn compile_project(root: &Path) -> Result<ProjectOutput, Vec<CompileError>> {
    let mut errors = Vec::new();

    // -- 1. Discovery. --
    let karn_files = match discover_karn_files(root) {
        Ok(f) => f,
        Err(e) => return Err(vec![e]),
    };
    if karn_files.is_empty() {
        return Err(vec![CompileError::new(
            "karn.project.no_sources",
            Span::default(),
            format!("no `.karn` source files found under {}", root.display()),
        )]);
    }
    // Detect conflicts: a file `X.karn` and a directory `X/` (with .karn files)
    // both encoding the same logical commons.
    if let Err(e) = check_file_directory_conflicts(root, &karn_files) {
        errors.extend(e);
    }

    // -- 2. Parse every file. --
    let mut parsed: Vec<ParsedFile> = Vec::new();
    for path in &karn_files {
        match parse_file(root, path) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() && parsed.is_empty() {
        return Err(errors);
    }

    // -- 3. Group by commons name and validate per-directory consistency. --
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let name = pf.commons.name.joined();
        groups.entry(name).or_default().push(i);
    }
    // Per spec §4.1: every file in a single directory must declare the same
    // commons name. (A multi-file commons is one directory of files.)
    if let Err(e) = check_directory_name_consistency(&parsed) {
        errors.extend(e);
    }

    // -- 4. Build per-commons combined symbol tables. --
    let mut commons_tables: HashMap<String, CommonsTable> = HashMap::new();
    for (name, indices) in &groups {
        let table = build_commons_table(name, indices, &parsed, &mut errors);
        commons_tables.insert(name.clone(), table);
    }

    if !errors.is_empty() {
        // Don't proceed to checking if we already have hard structural errors.
        // Some of the errors below might be redundant but it's safer.
        // We still attempt checking only if there are no fatal errors yet.
    }

    // -- 5. Resolve `uses` clauses (existence + name-conflict detection). --
    let mut commons_uses: HashMap<String, Vec<String>> = HashMap::new();
    for (name, indices) in &groups {
        let mut uses_targets: Vec<String> = Vec::new();
        for &i in indices {
            for u in &parsed[i].commons.uses {
                let target = u.target.joined();
                if !commons_tables.contains_key(&target) {
                    errors.push(
                        CompileError::new(
                            "karn.uses.unknown_commons",
                            u.span,
                            format!("unknown commons `{target}`"),
                        )
                        .with_note(
                            "the target of a `uses` clause must be another commons in the project",
                        ),
                    );
                    continue;
                }
                if target == *name {
                    errors.push(CompileError::new(
                        "karn.uses.self_reference",
                        u.span,
                        format!("commons `{name}` cannot `uses` itself"),
                    ));
                    continue;
                }
                if !uses_targets.contains(&target) {
                    uses_targets.push(target);
                }
            }
        }
        commons_uses.insert(name.clone(), uses_targets);
    }

    // -- 6. Check for name conflicts among used commons. --
    for (name, targets) in &commons_uses {
        let local = commons_tables.get(name).expect("commons table present");
        let mut imported: HashMap<String, String> = HashMap::new();
        for t in targets {
            let used = commons_tables.get(t).expect("used commons table present");
            for type_name in used.types.keys() {
                // Local declarations always take precedence; only flag
                // conflicts between two used commons.
                if local.types.contains_key(type_name) || local.fns.contains_key(type_name) {
                    continue;
                }
                if let Some(prev) = imported.get(type_name) {
                    // Find the uses-span of either target to report on.
                    let span = uses_span_of(&parsed, &groups[name], t).unwrap_or_default();
                    errors.push(
                        CompileError::new(
                            "karn.uses.name_conflict",
                            span,
                            format!(
                                "commons `{name}` uses two commons that both declare `{type_name}`: `{prev}` and `{t}`",
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
                                "commons `{name}` uses two commons that both declare `{fn_name}`: `{prev}` and `{t}`",
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

    if !errors.is_empty() {
        return Err(errors);
    }

    // -- 6b. Build per-commons file index (which file declares which name).
    // Needed by the emitter to compute cross-commons import paths.
    let mut commons_file_index: HashMap<String, FileDeclIndex> = HashMap::new();
    for (name, indices) in &groups {
        commons_file_index.insert(name.clone(), build_file_decl_index(indices, &parsed));
    }

    // -- 7. For each commons, build the full (local + imported) symbol space
    //       and run resolve+check per source file. --
    let mut compiled: Vec<CompiledFile> = Vec::new();

    for (name, indices) in &groups {
        // Compose: local + transitive (one level) uses. The spec says
        // imported types share identity with the defining commons, so we
        // simply union the symbol tables, with local shadowing imports.
        let local_table = commons_tables.get(name).expect("commons table present");
        let mut combined_types = local_table.types.clone();
        let mut combined_fns = local_table.fns.clone();
        let mut combined_methods = local_table.methods.clone();
        // Track which type names came from which used commons (for emission
        // and for "is this type local?" decisions).
        let mut imported_from: HashMap<String, String> = HashMap::new();
        for t in commons_uses.get(name).into_iter().flatten() {
            let used = commons_tables.get(t).expect("used commons table present");
            for (type_name, decl) in &used.types {
                if !combined_types.contains_key(type_name) {
                    combined_types.insert(type_name.clone(), decl.clone());
                    imported_from.insert(type_name.clone(), t.clone());
                }
            }
            for (fn_name, decl) in &used.fns {
                if !combined_fns.contains_key(fn_name) {
                    combined_fns.insert(fn_name.clone(), decl.clone());
                    imported_from.insert(fn_name.clone(), t.clone());
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

        let local_names: HashSet<String> = local_table.types.keys().cloned().collect();

        // Collect every method declaration in this commons, keyed by
        // (type-name, method-name), so each file's emission can include the
        // methods attached to types it declares — even if the method itself
        // was authored in a sibling file.
        let mut local_methods_for_type: HashMap<String, Vec<FnDecl>> = HashMap::new();
        for &j in indices {
            for item in &parsed[j].commons.items {
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

        // For each file in this commons, type-check and emit.
        for &i in indices {
            let pf = &parsed[i];
            // Synthesize the items list for emission: this file's type and
            // free-fn declarations, plus methods on those types from any
            // file in the commons. Methods declared in this file but
            // attached to a sibling-file type are skipped (they'll surface
            // in the sibling's output instead).
            let mut emit_items: Vec<CommonsItem> = Vec::new();
            let types_in_this_file: HashSet<String> = pf
                .commons
                .items
                .iter()
                .filter_map(|it| match it {
                    CommonsItem::Type(t) => Some(t.name.name.clone()),
                    _ => None,
                })
                .collect();
            for item in &pf.commons.items {
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
                }
            }
            // Add methods authored elsewhere whose attached type is in this file.
            for type_name in &types_in_this_file {
                if let Some(methods) = local_methods_for_type.get(type_name) {
                    for m in methods {
                        // Skip methods already added (declared in this file).
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
            let mut commons_for_emit = pf.commons.clone();
            commons_for_emit.items = emit_items;
            let resolved = ResolvedCommons {
                commons: commons_for_emit,
                types: combined_types.clone(),
                fns: combined_fns.clone(),
                methods: combined_methods.clone(),
                local_type_names: local_names.clone(),
            };
            // Run resolution checks restricted to this file's items only
            // (cross-references to the rest of the commons resolve via the
            // combined tables we already plugged in).
            if let Err(errs) = resolver::resolve_file(&resolved) {
                errors.extend(errs);
                continue;
            }
            // Type-check this file's items against the combined symbol table.
            let typed = match checker::check(resolved) {
                Ok(t) => t,
                Err(errs) => {
                    errors.extend(errs);
                    continue;
                }
            };
            // Emit. We need to know inter-file dependencies so we can render
            // imports. The emitter computes them by walking expressions.
            // For cross-commons imports, plug in per-target file indexes so
            // the emitter can point each import at the exact source file
            // that declares each name.
            let mut imported_decl_paths: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
            for t in commons_uses.get(name).into_iter().flatten() {
                if let Some(target_index) = commons_file_index.get(t) {
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
            let emit_ctx = EmitProjectCtx {
                source_path: pf.source_path.clone(),
                commons_name: name.clone(),
                local_files: indices
                    .iter()
                    .filter_map(|&j| {
                        if j == i {
                            None
                        } else {
                            Some(parsed[j].source_path.clone())
                        }
                    })
                    .collect(),
                file_decl_index: commons_file_index.get(name).cloned().unwrap_or_else(|| {
                    FileDeclIndex {
                        types: HashMap::new(),
                        fns: HashMap::new(),
                        methods: HashMap::new(),
                    }
                }),
                imported_from: imported_from.clone(),
                imported_decl_paths,
                commons_dir: commons_dir_for(name),
            };
            let ts = emitter::emit_project(&typed, &emit_ctx);
            let output_path = ts_output_path(&pf.source_path);
            compiled.push(CompiledFile {
                source_path: pf.source_path.clone(),
                output_path,
                typescript: ts,
            });
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    // Sort outputs for deterministic ordering.
    compiled.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    Ok(ProjectOutput { files: compiled })
}

// -- internals --

/// A parsed `.karn` file: its source, AST, and project-relative path.
struct ParsedFile {
    source_path: PathBuf,
    #[allow(dead_code)]
    source: String,
    commons: Commons,
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
    let commons = parser::parse(&tokens, &source)?;
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    Ok(ParsedFile {
        source_path: rel,
        source,
        commons,
    })
}

/// Walk `root` and collect every `.karn` file (recursively).
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

/// Compute the commons-name dotted path for a single-file commons at path
/// `karn/time.karn` (`karn.time`) or for a multi-file commons directory
/// `karn/time/foo.karn` (`karn.time`). Returns None for paths that can't
/// be interpreted (e.g., empty).
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

/// Check that every file in a single directory declares the same commons
/// name. Per spec §4.1.
fn check_directory_name_consistency(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    let mut by_dir: HashMap<PathBuf, Vec<(usize, &str)>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let dir = pf
            .source_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        by_dir.entry(dir).or_default().push((
            i,
            pf.commons
                .name
                .parts
                .last()
                .map(|p| p.name.as_str())
                .unwrap_or(""),
        ));
    }
    for (dir, files) in &by_dir {
        // The implicit "commons directory" rule is: when more than one file
        // lives in the same directory, they must all share the exact same
        // commons header. Otherwise the directory can't be a multi-file
        // commons.
        if files.len() < 2 {
            continue;
        }
        // Compare full joined names, not just the leaf.
        let first = parsed[files[0].0].commons.name.joined();
        for &(idx, _) in files.iter().skip(1) {
            let other = parsed[idx].commons.name.joined();
            if other != first {
                errors.push(
                    CompileError::new(
                        "karn.project.inconsistent_commons_name",
                        parsed[idx].commons.span,
                        format!(
                            "files in directory `{}` declare different commons names: `{first}` vs `{other}`",
                            dir.display()
                        ),
                    )
                    .with_label(
                        parsed[files[0].0].commons.span,
                        format!("first file declares `{first}`"),
                    )
                    .with_note(
                        "every `.karn` file in a directory must share the same `commons` header",
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

/// Check that no commons is represented both by a single file `X.karn` and
/// a directory `X/` (with `.karn` files).
fn check_file_directory_conflicts(root: &Path, files: &[PathBuf]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    let mut karn_files: HashSet<PathBuf> = HashSet::new();
    let mut dirs_with_karn: HashSet<PathBuf> = HashSet::new();
    for p in files {
        let rel = p.strip_prefix(root).unwrap_or(p);
        karn_files.insert(rel.to_path_buf());
        // The dir containing this file (relative).
        if let Some(parent) = rel.parent() {
            dirs_with_karn.insert(parent.to_path_buf());
        }
    }
    for f in &karn_files {
        // If the file `X/Y.karn` exists, and the directory `X/Y/` also has
        // karn files, conflict.
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

/// Combined symbol tables for a single logical commons.
#[derive(Clone, Default)]
struct CommonsTable {
    types: HashMap<String, TypeDecl>,
    fns: HashMap<String, FnDecl>,
    methods: HashMap<String, ResolverMethodTable>,
}

fn build_commons_table(
    _name: &str,
    indices: &[usize],
    parsed: &[ParsedFile],
    errors: &mut Vec<CompileError>,
) -> CommonsTable {
    let mut table = CommonsTable::default();
    // Pass A: register all types across every file in the commons. Doing
    // types first lets methods attached to a type defined in *another* file
    // of the same commons resolve in pass B (file order is unspecified).
    for &i in indices {
        for item in &parsed[i].commons.items {
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
    // Pass B: register all functions and methods.
    for &i in indices {
        for item in &parsed[i].commons.items {
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
                                "methods can only be declared on types defined in the same commons (across all of its files)",
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

/// For each name declared in the commons (type, fn, method), record which
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
        for item in &parsed[i].commons.items {
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
            }
        }
    }
    idx
}

fn uses_span_of(parsed: &[ParsedFile], indices: &[usize], target: &str) -> Option<Span> {
    for &i in indices {
        for u in &parsed[i].commons.uses {
            if u.target.joined() == target {
                return Some(u.span);
            }
        }
    }
    None
}

/// Context passed to the emitter so it can resolve cross-file and
/// cross-commons references into TypeScript import statements.
pub struct EmitProjectCtx {
    /// Source path of the file being emitted (relative to project root).
    pub source_path: PathBuf,
    /// Joined name of the commons this file belongs to.
    pub commons_name: String,
    /// Sibling files in the same commons (project-relative paths).
    pub local_files: Vec<PathBuf>,
    /// Which file declares each name in the local commons.
    pub file_decl_index: FileDeclIndex,
    /// For each imported name, the joined name of the commons it came from.
    pub imported_from: HashMap<String, String>,
    /// For each imported commons, the file path that declares each name.
    /// `imported_decl_paths[commons_name][decl_name] = source path of the
    /// file in the *target* commons that declares `decl_name`.
    pub imported_decl_paths: HashMap<String, HashMap<String, PathBuf>>,
    /// The directory (project-relative) that holds this commons. Used for
    /// computing relative paths to other commons in the same project.
    pub commons_dir: PathBuf,
}

impl EmitProjectCtx {
    /// Where in the file tree is this commons rooted? Returns the relative
    /// path to the commons's directory or file stem.
    pub fn commons_path(name: &str) -> PathBuf {
        commons_dir_for(name)
    }
}

/// Avoid stripping a single-file commons of its enclosing path component;
/// the spec leaves the input format flexible.
#[allow(dead_code)]
fn _ensure_components_used(_p: &Path) {
    let _ = Component::CurDir;
}
