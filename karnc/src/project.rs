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

/// Distinguishes a commons from a context in the project graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Commons,
    Context,
}

impl UnitKind {
    pub fn display(self) -> &'static str {
        match self {
            UnitKind::Commons => "commons",
            UnitKind::Context => "context",
        }
    }
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

    // -- 3. Group by (name, kind) and validate per-directory consistency. --
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut kinds: HashMap<String, UnitKind> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let name = pf.unit.name().joined();
        groups.entry(name.clone()).or_default().push(i);
        kinds.entry(name).or_insert(pf.kind);
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
    for (name, indices) in &groups {
        let kind = *kinds.get(name).unwrap();
        let mut consumes_targets: Vec<String> = Vec::new();
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
                if target_kind != UnitKind::Context {
                    errors.push(
                        CompileError::new(
                            "karn.consumes.target_is_commons",
                            c.span,
                            format!(
                                "`consumes {target}` targets a commons — `consumes` may only target a context"
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
                if !consumes_targets.contains(&target) {
                    consumes_targets.push(target);
                }
            }
        }
        unit_consumes.insert(name.clone(), consumes_targets);
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
        if kind != UnitKind::Context {
            // Commons may not have exports clauses (parsed grammar prevents it
            // at the parser level), but in case any sneak in, skip.
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, (Visibility, Span)> = HashMap::new();
        for &i in indices {
            let Some(ctx) = parsed[i].context() else {
                continue;
            };
            for clause in &ctx.exports {
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
                        if *prev_vis == clause.visibility {
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
                    seen.insert(n.name.clone(), (clause.visibility, n.span));
                }
            }
        }
        let mut visibility_map: HashMap<String, Visibility> = HashMap::new();
        for (n, (v, _)) in seen {
            visibility_map.insert(n, v);
        }
        exports_visibility.insert(name.clone(), visibility_map);
    }

    // -- 6c. Validate that providers match their capabilities exactly. --
    for (name, table) in &unit_tables {
        let _ = name;
        for (cap_name, provider) in &table.providers {
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

            let resolved = ResolvedCommons {
                commons: synthetic_commons,
                types: combined_types.clone(),
                fns: combined_fns.clone(),
                methods: combined_methods.clone(),
                local_type_names: local_names.clone(),
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
                let v0_5_errs = check_v0_5_declarations(&mut typed, table);
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
                file_decl_index: unit_file_index.get(name).cloned().unwrap_or_else(|| {
                    FileDeclIndex {
                        types: HashMap::new(),
                        fns: HashMap::new(),
                        methods: HashMap::new(),
                    }
                }),
                imported_from: imported_from.clone(),
                imported_from_kind: imported_from_kind.clone(),
                imported_decl_paths,
                commons_dir: commons_dir_for(name),
                unit_kind: kind,
                owning_context: owning_context_for_emit.clone(),
                exports_local,
                exports_for_consumed,
                consumed_types: consumed_types.clone(),
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
    compiled.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    Ok(ProjectOutput { files: compiled })
}

// -- internals --

/// A parsed `.karn` file: its source, AST, and project-relative path.
struct ParsedFile {
    source_path: PathBuf,
    #[allow(dead_code)]
    source: String,
    unit: SourceUnit,
    kind: UnitKind,
}

impl ParsedFile {
    fn items(&self) -> &Vec<CommonsItem> {
        match &self.unit {
            SourceUnit::Commons(c) => &c.items,
            SourceUnit::Context(c) => &c.items,
        }
    }

    fn uses(&self) -> &Vec<UsesDecl> {
        match &self.unit {
            SourceUnit::Commons(c) => &c.uses,
            SourceUnit::Context(c) => &c.uses,
        }
    }

    fn consumes(&self) -> &[ConsumesDecl] {
        match &self.unit {
            SourceUnit::Commons(_) => &[],
            SourceUnit::Context(c) => &c.consumes,
        }
    }

    fn context(&self) -> Option<&Context> {
        match &self.unit {
            SourceUnit::Context(c) => Some(c),
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
    };
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    Ok(ParsedFile {
        source_path: rel,
        source,
        unit,
        kind,
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
    // live in the same directory.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
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

/// Each file's relative path must match its declared qualified name. Two
/// arrangements are valid:
/// - **Single-file**: `a/b/c.karn` declaring `a.b.c`.
/// - **Multi-file**: `a/b/c/<any>.karn` declaring `a.b.c`.
fn check_path_name_alignment(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for pf in parsed {
        let name = pf.unit.name().joined();
        let name_parts: Vec<&str> = name.split('.').collect();
        let rel = &pf.source_path;
        let stem = rel.with_extension("");
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
        if !single_file_match && !multi_file_match {
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
struct UnitTable {
    #[allow(dead_code)]
    kind: Option<UnitKind>,
    types: HashMap<String, TypeDecl>,
    fns: HashMap<String, FnDecl>,
    methods: HashMap<String, ResolverMethodTable>,
    /// Per-context capabilities (v0.5). Empty for commons.
    capabilities: HashMap<String, CapabilityDecl>,
    /// Per-context providers (v0.5). One provider per capability in v0.5.
    /// Key: capability name. Value: provider declaration.
    providers: HashMap<String, ProviderDecl>,
    /// Per-context services (v0.5). Empty for commons.
    services: HashMap<String, ServiceDecl>,
    /// Per-context agents (v0.5). Empty for commons.
    agents: HashMap<String, AgentDecl>,
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
    // v0.5: collect capabilities, providers, services, agents.
    for &i in indices {
        for item in parsed[i].items() {
            match item {
                CommonsItem::Capability(c) => {
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.capability.outside_context",
                            c.span,
                            "`capability` declarations are only allowed inside a context, not a commons",
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
                    if kind != UnitKind::Context {
                        errors.push(CompileError::new(
                            "karn.provider.outside_context",
                            p.span,
                            "`provides` declarations are only allowed inside a context, not a commons",
                        ));
                        continue;
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
    };

    // Capability info from the table.
    let capability_info_map: HashMap<String, CapabilityInfo> = table
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

    // Check provider bodies. Providers have no `given` clause, no `self`.
    // Their bodies are effectful if the operation returns Effect[T].
    for provider in table.providers.values() {
        for op in &provider.ops {
            checker::check_handler_body(
                &op.body,
                &op.return_type,
                op.return_type.span(),
                &op.params,
                &resolved,
                &mut typed.expr_types,
                &mut errors,
                HashMap::new(), // no capabilities in provider bodies in v0.5
                capability_info_map.clone(),
                None,
                None,
                Vec::new(),
            );
        }
    }

    // Check service handlers.
    for service in table.services.values() {
        for handler in &service.handlers {
            // The given clause must reference only declared capabilities.
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_name in &handler.given {
                let Some(info) = capability_info_map.get(&cap_name.name) else {
                    errors.push(CompileError::new(
                        "karn.given.unknown_capability",
                        cap_name.span,
                        format!(
                            "capability `{}` is not declared in this context",
                            cap_name.name
                        ),
                    ));
                    continue;
                };
                handler_caps.insert(cap_name.name.clone(), info.clone());
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
                handler.given.iter().map(|c| c.name.clone()).collect();
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
        };
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
            for cap_name in &handler.given {
                let Some(info) = capability_info_map.get(&cap_name.name) else {
                    errors.push(CompileError::new(
                        "karn.given.unknown_capability",
                        cap_name.span,
                        format!(
                            "capability `{}` is not declared in this context",
                            cap_name.name
                        ),
                    ));
                    continue;
                };
                handler_caps.insert(cap_name.name.clone(), info.clone());
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
                handler.given.iter().map(|c| c.name.clone()).collect();
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
        (TypeRef::ValidationError(_), TypeRef::ValidationError(_)) => true,
        (TypeRef::Unit(_), TypeRef::Unit(_)) => true,
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
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
    }
}
