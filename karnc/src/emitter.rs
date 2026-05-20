//! TypeScript emission (spec §7, v0.1 §6, v0.2 §6).
//!
//! Walks the typed AST and writes a single TypeScript module.
//!
//! v0.2 lowering rules:
//! - Refined-base types: branded type alias + constructor object with
//!   `of`/`unsafe` (+ any user-declared methods).
//! - Record types: TypeScript `interface` + namespace object with methods.
//! - Sum types: discriminated-union type alias + namespace object with
//!   variant constructors and methods.
//! - Field access lowers to property access.
//! - Method calls lower to `Type.method(receiver, args)` (UFCS).
//! - `match` lowers to a switch on `.tag`; in tail position it inlines,
//!   otherwise it becomes an IIFE.
//! - `is` lowers to a tag check; bindings become `const` declarations
//!   on the truthy side of `if`/`&&`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::checker::{NamedKind, Ty, TypedCommons};
use crate::project::{EmitProjectCtx, UnitKind};

const RUNTIME_IMPORT: &str = "./runtime.js";
const INDENT_STEP: usize = 2;

/// Emit TypeScript source for the typed commons (single-file mode).
pub fn emit(commons: &TypedCommons) -> String {
    let mut out = String::new();
    write_header_single(&mut out, commons);
    write_commons_doc(&mut out, commons);
    let dummy_ctx = single_file_ctx();
    // Types come first (they define interfaces and namespaces).
    for item in &commons.commons.items {
        if let CommonsItem::Type(t) = item {
            emit_type(&mut out, t, commons, &dummy_ctx);
        }
    }
    // Free functions afterward.
    for item in &commons.commons.items {
        if let CommonsItem::Fn(f) = item
            && let FnName::Free(_) = &f.name
        {
            emit_free_fn(&mut out, f, commons);
        }
    }
    out
}

/// A no-op project context for single-file emission. Single-file mode never
/// involves contexts or cross-unit imports, so most fields default to empty.
fn single_file_ctx() -> EmitProjectCtx {
    EmitProjectCtx {
        source_path: PathBuf::new(),
        commons_name: String::new(),
        local_files: Vec::new(),
        file_decl_index: crate::project::FileDeclIndex {
            types: HashMap::new(),
            fns: HashMap::new(),
            methods: HashMap::new(),
        },
        imported_from: HashMap::new(),
        imported_from_kind: HashMap::new(),
        imported_decl_paths: HashMap::new(),
        commons_dir: PathBuf::new(),
        unit_kind: UnitKind::Commons,
        owning_context: None,
        exports_local: HashMap::new(),
        exports_for_consumed: HashMap::new(),
        consumed_types: HashMap::new(),
        cross_context: crate::resolver::CrossContextInfo::default(),
        is_consumed_by_others: false,
    }
}

/// Emit TypeScript source for a single file inside a multi-file project,
/// including cross-file and cross-commons imports computed from
/// [`EmitProjectCtx`].
pub fn emit_project(commons: &TypedCommons, ctx: &EmitProjectCtx) -> String {
    let mut out = String::new();
    write_header(&mut out, commons, ctx);
    // Compute which names this file actually references that live elsewhere
    // (sibling file in the same commons/context, or a used commons / consumed
    // context).
    let references = collect_external_references(commons, ctx);
    emit_project_imports(&mut out, ctx, &references);
    if !references.is_empty() {
        writeln!(out).unwrap();
    }
    // v0.6: namespace imports for each consumed context that exposes services.
    emit_cross_context_namespace_imports(&mut out, ctx);
    // For contexts: emit per-context nominal rebrand aliases for each type
    // imported via `uses` that this file references. The structural shape is
    // inherited from the original commons type; the brand makes the
    // rebranded type nominally distinct (v0.4 §6.2).
    if ctx.unit_kind == UnitKind::Context {
        emit_context_rebrands(&mut out, &references, ctx);
    }
    write_commons_doc(&mut out, commons);
    for item in &commons.commons.items {
        if let CommonsItem::Type(t) = item {
            emit_type(&mut out, t, commons, ctx);
        }
    }
    for item in &commons.commons.items {
        if let CommonsItem::Fn(f) = item
            && let FnName::Free(_) = &f.name
        {
            emit_free_fn(&mut out, f, commons);
        }
    }
    // v0.5: behavioural items follow the type/fn declarations.
    for item in &commons.commons.items {
        match item {
            CommonsItem::Capability(c) => emit_capability(&mut out, c),
            CommonsItem::Provider(p) => emit_provider(&mut out, p, commons, ctx),
            CommonsItem::Service(s) => emit_service(&mut out, s, commons, ctx),
            CommonsItem::Agent(a) => emit_agent(&mut out, a, commons, ctx),
            _ => {}
        }
    }
    // v0.6: cross-context surface assembly. Only emit `makeSurface` when
    // another context consumes this one.
    if ctx.unit_kind == UnitKind::Context && ctx.is_consumed_by_others {
        emit_make_surface(&mut out, commons);
    }
    out
}

/// For each type imported via `uses` that's referenced in this file, emit:
/// 1. (Done in imports) an aliased import: `import { Money as __CommonsMoney } from ...`
/// 2. A rebranded type alias: `export type Money = __CommonsMoney & { readonly __ctxBrand: "..." }`
///
/// The brand makes two contexts that both `uses` the same commons see distinct
/// nominal `Money` types in their TypeScript output (v0.4 §3.4 / §6.2).
fn emit_context_rebrands(out: &mut String, refs: &ExternalReferences, ctx: &EmitProjectCtx) {
    let Some(owning) = &ctx.owning_context else {
        return;
    };
    // Collect names imported via `uses` (kind == Commons in imported_from_kind).
    let mut names: Vec<String> = Vec::new();
    for set in refs.by_commons.values() {
        for n in set {
            if matches!(ctx.imported_from_kind.get(n), Some(UnitKind::Commons)) {
                names.push(n.clone());
            }
        }
    }
    names.sort();
    names.dedup();
    if names.is_empty() {
        return;
    }
    for name in &names {
        writeln!(
            out,
            "export type {name} = __Commons{name} & {{ readonly __ctxBrand: \"{owning}\" }};",
        )
        .unwrap();
    }
    writeln!(out).unwrap();
}

/// Names that this file needs to import from elsewhere (sibling files of
/// the same commons, or other commons via `uses`).
#[derive(Default)]
struct ExternalReferences {
    /// `commons name` → set of names to import.
    by_commons: HashMap<String, HashSet<String>>,
    /// `sibling source path` → set of names to import (same-commons).
    by_sibling: HashMap<PathBuf, HashSet<String>>,
}

impl ExternalReferences {
    fn is_empty(&self) -> bool {
        self.by_commons.is_empty() && self.by_sibling.is_empty()
    }
}

fn collect_external_references(commons: &TypedCommons, ctx: &EmitProjectCtx) -> ExternalReferences {
    // Names declared in this file (so we know what's local-to-file).
    let local_to_file: HashSet<String> = commons
        .commons
        .items
        .iter()
        .map(|i| i.name().name.clone())
        .collect();

    let mut refs = ExternalReferences::default();

    // Walk every expression and TypeRef in this file's items, recording
    // any reference that resolves to a name declared in a sibling file or
    // an imported commons.
    for item in &commons.commons.items {
        match item {
            CommonsItem::Type(t) => {
                collect_refs_in_type_decl(t, &local_to_file, ctx, &mut refs);
            }
            CommonsItem::Fn(f) => {
                collect_refs_in_fn(f, &local_to_file, ctx, &mut refs);
            }
            CommonsItem::Capability(c) => {
                for op in &c.ops {
                    for p in &op.params {
                        collect_refs_in_typeref(&p.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&op.return_type, &local_to_file, ctx, &mut refs);
                }
            }
            CommonsItem::Provider(p) => {
                // Reference to the capability so we can import it (locally
                // declared, so usually no extra work).
                let _ = &p.capability;
                for op in &p.ops {
                    for param in &op.params {
                        collect_refs_in_typeref(&param.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&op.return_type, &local_to_file, ctx, &mut refs);
                    collect_refs_in_block(&op.body, &local_to_file, ctx, &mut refs);
                }
            }
            CommonsItem::Service(s) => {
                for h in &s.handlers {
                    for p in &h.params {
                        collect_refs_in_typeref(&p.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&h.return_type, &local_to_file, ctx, &mut refs);
                    collect_refs_in_block(&h.body, &local_to_file, ctx, &mut refs);
                }
            }
            CommonsItem::Agent(a) => {
                collect_refs_in_typeref(&a.key_type, &local_to_file, ctx, &mut refs);
                for f in &a.state_fields {
                    collect_refs_in_typeref(&f.type_ref, &local_to_file, ctx, &mut refs);
                }
                for h in &a.handlers {
                    for p in &h.params {
                        collect_refs_in_typeref(&p.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&h.return_type, &local_to_file, ctx, &mut refs);
                    collect_refs_in_block(&h.body, &local_to_file, ctx, &mut refs);
                }
            }
        }
    }
    refs
}

fn collect_refs_in_type_decl(
    t: &TypeDecl,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    match &t.body {
        TypeBody::Record(r) => {
            for f in &r.fields {
                collect_refs_in_typeref(&f.type_ref, local_to_file, ctx, out);
            }
        }
        TypeBody::Sum(s) => {
            for v in &s.variants {
                for p in &v.payload {
                    collect_refs_in_typeref(&p.type_ref, local_to_file, ctx, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_refs_in_fn(
    f: &FnDecl,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    for p in &f.params {
        collect_refs_in_typeref(&p.type_ref, local_to_file, ctx, out);
    }
    collect_refs_in_typeref(&f.return_type, local_to_file, ctx, out);
    // For methods: the attached type may also be elsewhere.
    if let FnName::Method { type_name, .. } = &f.name {
        record_name_ref(&type_name.name, local_to_file, ctx, out);
    }
    collect_refs_in_block(&f.body, local_to_file, ctx, out);
}

fn collect_refs_in_typeref(
    r: &TypeRef,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    match r {
        TypeRef::Named(id) => record_name_ref(&id.name, local_to_file, ctx, out),
        TypeRef::Result(t, e, _) => {
            collect_refs_in_typeref(t, local_to_file, ctx, out);
            collect_refs_in_typeref(e, local_to_file, ctx, out);
        }
        TypeRef::Option(t, _) => collect_refs_in_typeref(t, local_to_file, ctx, out),
        TypeRef::Effect(t, _) => collect_refs_in_typeref(t, local_to_file, ctx, out),
        _ => {}
    }
}

fn collect_refs_in_block(
    b: &Block,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    for stmt in &b.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                if let Some(t) = &l.type_annot {
                    collect_refs_in_typeref(t, local_to_file, ctx, out);
                }
                collect_refs_in_expr(&l.value, local_to_file, ctx, out);
            }
            Statement::Commit(c) => {
                collect_refs_in_expr(&c.value, local_to_file, ctx, out);
            }
            Statement::Assert(a) => {
                collect_refs_in_expr(&a.value, local_to_file, ctx, out);
            }
        }
    }
    collect_refs_in_expr(&b.tail, local_to_file, ctx, out);
}

fn collect_refs_in_expr(
    e: &Expr,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    match &e.kind {
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        ExprKind::EffectPure(inner) => {
            collect_refs_in_expr(inner, local_to_file, ctx, out);
        }
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => {
            if let Some(tn) = type_name {
                record_name_ref(&tn.name, local_to_file, ctx, out);
            }
            collect_refs_in_expr(base, local_to_file, ctx, out);
            for f in overrides {
                if let Some(v) = &f.value {
                    collect_refs_in_expr(v, local_to_file, ctx, out);
                }
            }
        }
        ExprKind::Call(name, args) => {
            record_name_ref(&name.name, local_to_file, ctx, out);
            for a in args {
                collect_refs_in_expr(a, local_to_file, ctx, out);
            }
        }
        ExprKind::BinOp(_, l, r) => {
            collect_refs_in_expr(l, local_to_file, ctx, out);
            collect_refs_in_expr(r, local_to_file, ctx, out);
        }
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i) => collect_refs_in_expr(i, local_to_file, ctx, out),
        ExprKind::Block(b) => collect_refs_in_block(b, local_to_file, ctx, out),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_refs_in_expr(cond, local_to_file, ctx, out);
            collect_refs_in_block(then_block, local_to_file, ctx, out);
            collect_refs_in_block(else_block, local_to_file, ctx, out);
        }
        ExprKind::ConstructorCall {
            type_name,
            method: _,
            args,
        } => {
            record_name_ref(&type_name.name, local_to_file, ctx, out);
            for a in args {
                collect_refs_in_expr(a, local_to_file, ctx, out);
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            record_name_ref(&type_name.name, local_to_file, ctx, out);
            for f in fields {
                if let Some(v) = &f.value {
                    collect_refs_in_expr(v, local_to_file, ctx, out);
                }
            }
        }
        ExprKind::FieldAccess { receiver, field: _ } => {
            // The bare-ident-as-type case (`TypeName.Variant`) — record the
            // name so we import the type.
            if let ExprKind::Ident(id) = &receiver.kind {
                record_name_ref(&id.name, local_to_file, ctx, out);
            } else {
                collect_refs_in_expr(receiver, local_to_file, ctx, out);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            if let ExprKind::Ident(id) = &receiver.kind {
                record_name_ref(&id.name, local_to_file, ctx, out);
            } else {
                collect_refs_in_expr(receiver, local_to_file, ctx, out);
            }
            for a in args {
                collect_refs_in_expr(a, local_to_file, ctx, out);
            }
        }
        ExprKind::Match { discriminant, arms } => {
            collect_refs_in_expr(discriminant, local_to_file, ctx, out);
            for arm in arms {
                if let Pattern::Variant {
                    type_name: Some(tn),
                    ..
                } = &arm.pattern
                {
                    record_name_ref(&tn.name, local_to_file, ctx, out);
                }
                match &arm.body {
                    MatchBody::Expr(e) => collect_refs_in_expr(e, local_to_file, ctx, out),
                    MatchBody::Block(b) => collect_refs_in_block(b, local_to_file, ctx, out),
                }
            }
        }
        ExprKind::Is { value, pattern } => {
            collect_refs_in_expr(value, local_to_file, ctx, out);
            if let Pattern::Variant {
                type_name: Some(tn),
                ..
            } = pattern
            {
                record_name_ref(&tn.name, local_to_file, ctx, out);
            }
        }
    }
}

fn record_name_ref(
    name: &str,
    local_to_file: &HashSet<String>,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    if local_to_file.contains(name) {
        return;
    }
    // Imported from another commons?
    if let Some(commons_name) = ctx.imported_from.get(name) {
        out.by_commons
            .entry(commons_name.clone())
            .or_default()
            .insert(name.to_string());
        return;
    }
    // Sibling file in the same commons?
    if let Some(path) = ctx.file_decl_index.types.get(name)
        && path != &ctx.source_path
    {
        out.by_sibling
            .entry(path.clone())
            .or_default()
            .insert(name.to_string());
        return;
    }
    if let Some(path) = ctx.file_decl_index.fns.get(name)
        && path != &ctx.source_path
    {
        out.by_sibling
            .entry(path.clone())
            .or_default()
            .insert(name.to_string());
    }
}

/// Emit `import * as <ns> from "..."` for each consumed context that
/// exposes services (so the consuming file can reference its `makeSurface`
/// return type and brand the cross-context call arguments).
fn emit_cross_context_namespace_imports(out: &mut String, ctx: &EmitProjectCtx) {
    let info = &ctx.cross_context;
    if info.consumed_services.is_empty() {
        return;
    }
    let mut consumed_with_services: Vec<&String> = info
        .consumed_services
        .iter()
        .filter(|(_, svcs)| !svcs.is_empty())
        .map(|(q, _)| q)
        .collect();
    consumed_with_services.sort();
    if consumed_with_services.is_empty() {
        return;
    }
    for q in &consumed_with_services {
        // Pick the first known file path for the consumed context as the
        // import target. (The composition root lives in the consumed
        // context's directory; any of its files would work as an import
        // target since they're all in the same module namespace, but we
        // currently emit one file per .karn source so a single import per
        // consumed name suffices for the surface contract.)
        let target_paths = ctx.imported_decl_paths.get(q.as_str());
        let target = target_paths
            .and_then(|m| m.values().next().cloned())
            .unwrap_or_else(|| {
                // Fall back to <last-segment>.karn if no specific path is known.
                let mut p = EmitProjectCtx::commons_path(q);
                p.set_extension("karn");
                p
            });
        let import = cross_commons_import_specifier_for_path(&ctx.source_path, &target);
        let ns = qualified_to_ns(q);
        writeln!(out, "import * as {ns} from \"{import}\";").unwrap();
    }
    writeln!(out).unwrap();
}

fn emit_project_imports(out: &mut String, ctx: &EmitProjectCtx, refs: &ExternalReferences) {
    // Sibling imports: relative path within the same commons/context directory.
    let mut sibling_paths: Vec<(&PathBuf, &HashSet<String>)> = refs.by_sibling.iter().collect();
    sibling_paths.sort_by(|a, b| a.0.cmp(b.0));
    for (path, names) in sibling_paths {
        let import = sibling_import_specifier(&ctx.source_path, path);
        let mut sorted: Vec<&String> = names.iter().collect();
        sorted.sort();
        let joined = sorted
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "import {{ {joined} }} from \"{import}\";").unwrap();
    }
    // Cross-unit imports: group by *target file path*.
    let mut unit_names: Vec<(&String, &HashSet<String>)> = refs.by_commons.iter().collect();
    unit_names.sort_by(|a, b| a.0.cmp(b.0));
    for (unit_name, names) in unit_names {
        let target_paths = ctx.imported_decl_paths.get(unit_name.as_str());
        let mut by_target: std::collections::BTreeMap<PathBuf, Vec<&String>> =
            std::collections::BTreeMap::new();
        for n in names {
            let path = target_paths
                .and_then(|p| p.get(n))
                .cloned()
                .unwrap_or_else(|| EmitProjectCtx::commons_path(unit_name));
            by_target.entry(path).or_default().push(n);
        }
        for (target, mut name_list) in by_target {
            name_list.sort();
            let import = cross_commons_import_specifier_for_path(&ctx.source_path, &target);
            // For context units, aliase commons-source imports so we can emit
            // rebrand aliases of the same short name. Imports from consumed
            // contexts keep their original name.
            let mut parts: Vec<String> = Vec::new();
            for n in &name_list {
                let from_kind = ctx.imported_from_kind.get(n.as_str()).copied();
                if ctx.unit_kind == UnitKind::Context && from_kind == Some(UnitKind::Commons) {
                    parts.push(format!("{n} as __Commons{n}"));
                } else {
                    parts.push((*n).clone());
                }
            }
            let joined = parts.join(", ");
            writeln!(out, "import {{ {joined} }} from \"{import}\";").unwrap();
        }
    }
}

/// Compute a relative import specifier from `from_source` (a `.karn` path)
/// to `to_source` (another `.karn` path), with `.karn` rewritten to `.js`
/// for compatibility with NodeNext/strict TS resolution.
fn sibling_import_specifier(from_source: &Path, to_source: &Path) -> String {
    let from_dir = from_source.parent().unwrap_or(Path::new(""));
    let target = to_source.with_extension("js");
    let rel = relative_to(from_dir, &target);
    format!("./{}", rel.display())
}

/// Compute a relative import specifier from this file's location to a
/// specific source file in another commons. `target_source` is the project-
/// relative path of the target `.karn` file. The result is suitable for
/// `import { ... } from "..."` in NodeNext/strict TypeScript.
fn cross_commons_import_specifier_for_path(from_source: &Path, target_source: &Path) -> String {
    let from_dir = from_source.parent().unwrap_or(Path::new(""));
    let target = target_source.with_extension("js");
    let rel = relative_to(from_dir, &target);
    let display = rel.display().to_string();
    if display.starts_with("../") || display.starts_with("./") {
        display
    } else {
        format!("./{display}")
    }
}

/// Compute `target` as a path relative to `from`. Handles parent traversal
/// (`..`) for cases where `target` lives in a sibling directory.
fn relative_to(from: &Path, target: &Path) -> PathBuf {
    use std::path::Component as C;
    let f_comps: Vec<C> = from.components().collect();
    let t_comps: Vec<C> = target.components().collect();
    let mut shared = 0;
    while shared < f_comps.len() && shared < t_comps.len() && f_comps[shared] == t_comps[shared] {
        shared += 1;
    }
    let mut out = PathBuf::new();
    for _ in shared..f_comps.len() {
        out.push("..");
    }
    for c in &t_comps[shared..] {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

fn write_header(out: &mut String, commons: &TypedCommons, ctx: &EmitProjectCtx) {
    writeln!(out, "// Generated by karnc — do not edit by hand.").unwrap();
    let kind = match ctx.unit_kind {
        UnitKind::Commons => "commons",
        UnitKind::Context => "context",
        UnitKind::Test => "test",
    };
    writeln!(out, "// {kind} {}", commons.commons.name.joined()).unwrap();
    writeln!(out).unwrap();
    if !commons.commons.items.is_empty() {
        let has_agent = commons
            .commons
            .items
            .iter()
            .any(|i| matches!(i, CommonsItem::Agent(_)));
        if has_agent {
            writeln!(
                out,
                "import {{ Ok, Err, Some, None, type Result, type Option, type ValidationError, type DurableObjectState }} from \"{RUNTIME_IMPORT}\";",
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "import {{ Ok, Err, Some, None, type Result, type Option, type ValidationError }} from \"{RUNTIME_IMPORT}\";",
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }
}

/// Variant of write_header for single-file (no project context) emission.
fn write_header_single(out: &mut String, commons: &TypedCommons) {
    writeln!(out, "// Generated by karnc — do not edit by hand.").unwrap();
    writeln!(out, "// commons {}", commons.commons.name.joined()).unwrap();
    writeln!(out).unwrap();
    if !commons.commons.items.is_empty() {
        writeln!(
            out,
            "import {{ Ok, Err, Some, None, type Result, type Option, type ValidationError }} from \"{RUNTIME_IMPORT}\";",
        )
        .unwrap();
        writeln!(out).unwrap();
    }
}

/// Emit the commons-level doc block (if any) at the current position.
fn write_commons_doc(out: &mut String, commons: &TypedCommons) {
    if let Some(doc) = &commons.commons.documentation {
        emit_doc_block(out, Some(doc), 0);
        writeln!(out).unwrap();
    }
}

fn emit_type(out: &mut String, t: &TypeDecl, commons: &TypedCommons, ctx: &EmitProjectCtx) {
    emit_doc_block(out, t.documentation.as_deref(), 0);
    // For contexts, the per-type brand string is qualified by the context's
    // name (so two contexts' locally-declared `Order` types have distinct
    // brands at the TS level).
    let brand_prefix = ctx
        .owning_context
        .as_deref()
        .map(|c| format!("{c}."))
        .unwrap_or_default();
    match &t.body {
        TypeBody::Refined {
            base, refinement, ..
        } => emit_refined_type(out, t, *base, refinement.as_ref(), commons, &brand_prefix),
        TypeBody::Opaque {
            base, refinement, ..
        } => {
            // Opaque types lower identically to refined types: a branded base
            // type alias plus an `of`/`unsafe` constructor object.
            emit_refined_type(out, t, *base, refinement.as_ref(), commons, &brand_prefix);
        }
        TypeBody::Record(r) => emit_record_type(out, t, r, commons),
        TypeBody::Sum(s) => emit_sum_type(out, t, s, commons),
    }
}

/// Emit a doc block as a JSDoc-style comment at the given indent. Each line
/// of the doc body is prefixed with ` * `; empty lines become ` *`.
fn emit_doc_block(out: &mut String, doc: Option<&str>, indent: usize) {
    let Some(doc) = doc else { return };
    let indent_str: String = " ".repeat(indent);
    writeln!(out, "{indent_str}/**").unwrap();
    for line in doc.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            writeln!(out, "{indent_str} *").unwrap();
        } else {
            writeln!(out, "{indent_str} * {trimmed}").unwrap();
        }
    }
    writeln!(out, "{indent_str} */").unwrap();
}

fn emit_refined_type(
    out: &mut String,
    t: &TypeDecl,
    base: BaseType,
    refinement: Option<&Refinement>,
    commons: &TypedCommons,
    brand_prefix: &str,
) {
    let ts_base = ts_base(base);
    writeln!(
        out,
        "export type {name} = {base} & {{ readonly __brand: \"{prefix}{name}\" }};",
        name = t.name.name,
        base = ts_base,
        prefix = brand_prefix,
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "export const {name} = {{", name = t.name.name).unwrap();
    writeln!(
        out,
        "  of(value: {base}): Result<{name}, ValidationError> {{",
        name = t.name.name,
        base = ts_base,
    )
    .unwrap();
    emit_refined_checks(out, t, base, refinement);
    writeln!(out, "    return Ok(value as {name});", name = t.name.name).unwrap();
    writeln!(out, "  }},").unwrap();
    writeln!(
        out,
        "  unsafe(value: {base}): {name} {{",
        name = t.name.name,
        base = ts_base,
    )
    .unwrap();
    writeln!(out, "    return value as {name};", name = t.name.name).unwrap();
    writeln!(out, "  }},").unwrap();
    emit_attached_methods(out, &t.name.name, commons);
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

fn emit_refined_checks(
    out: &mut String,
    t: &TypeDecl,
    base: BaseType,
    refinement: Option<&Refinement>,
) {
    let name = &t.name.name;
    if base == BaseType::Int {
        writeln!(out, "    if (!Number.isInteger(value)) {{").unwrap();
        writeln!(
            out,
            "      return Err({{ field: \"{name}\", message: \"must be an integer\", value }});"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
    }
    if let Some(r) = refinement {
        for pred in &r.predicates {
            emit_pred_check(out, name, &pred.kind);
        }
    }
}

fn emit_pred_check(out: &mut String, type_name: &str, pred: &PredKind) {
    match pred {
        PredKind::NonNegative => {
            writeln!(out, "    if (!(value >= 0)) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must be non-negative\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::Positive => {
            writeln!(out, "    if (!(value > 0)) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must be positive\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::InRange(a, b) => {
            writeln!(out, "    if (!(value >= {a} && value <= {b})) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must be in range [{a}, {b}]\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::NonEmpty => {
            writeln!(out, "    if (!(value.length > 0)) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must be non-empty\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::MinLength(n) => {
            writeln!(out, "    if (!(value.length >= {n})) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"length must be at least {n}\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::MaxLength(n) => {
            writeln!(out, "    if (!(value.length <= {n})) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"length must be at most {n}\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::Length(n) => {
            writeln!(out, "    if (!(value.length === {n})) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"length must be exactly {n}\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::Matches(pat) => {
            let escaped = escape_ts_string(pat);
            writeln!(
                out,
                "    if (!new RegExp(\"^\" + \"{escaped}\" + \"$\").test(value)) {{"
            )
            .unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must match /{}/\", value }});",
                escape_ts_string(pat),
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
}

fn emit_record_type(out: &mut String, t: &TypeDecl, r: &RecordBody, commons: &TypedCommons) {
    writeln!(out, "export interface {name} {{", name = t.name.name).unwrap();
    for f in &r.fields {
        writeln!(
            out,
            "  readonly {name}: {ty};",
            name = f.name.name,
            ty = ts_type_ref(&f.type_ref),
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "export const {name} = {{", name = t.name.name).unwrap();
    emit_attached_methods(out, &t.name.name, commons);
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

fn emit_sum_type(out: &mut String, t: &TypeDecl, s: &SumBody, commons: &TypedCommons) {
    writeln!(out, "export type {name} =", name = t.name.name).unwrap();
    for (i, v) in s.variants.iter().enumerate() {
        let pipe = if i == 0 { " " } else { "|" };
        if v.payload.is_empty() {
            let term = if i == s.variants.len() - 1 { ";" } else { "" };
            writeln!(
                out,
                "  {pipe} {{ readonly tag: \"{tag}\" }}{term}",
                tag = v.name.name
            )
            .unwrap();
        } else {
            let fields: Vec<String> = v
                .payload
                .iter()
                .map(|f| {
                    format!(
                        "readonly {name}: {ty}",
                        name = f.name.name,
                        ty = ts_type_ref(&f.type_ref)
                    )
                })
                .collect();
            let term = if i == s.variants.len() - 1 { ";" } else { "" };
            writeln!(
                out,
                "  {pipe} {{ readonly tag: \"{tag}\"; {fields} }}{term}",
                tag = v.name.name,
                fields = fields.join("; "),
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();
    writeln!(out, "export const {name} = {{", name = t.name.name).unwrap();
    for v in &s.variants {
        if v.payload.is_empty() {
            writeln!(
                out,
                "  {tag}: {{ tag: \"{tag}\" }} as {name},",
                tag = v.name.name,
                name = t.name.name,
            )
            .unwrap();
        } else {
            let params: Vec<String> = v
                .payload
                .iter()
                .map(|f| {
                    format!(
                        "{name}: {ty}",
                        name = f.name.name,
                        ty = ts_type_ref(&f.type_ref)
                    )
                })
                .collect();
            let obj_fields: Vec<String> = v.payload.iter().map(|f| f.name.name.clone()).collect();
            writeln!(
                out,
                "  {tag}: ({params}): {name} => ({{ tag: \"{tag}\", {fields} }}),",
                tag = v.name.name,
                params = params.join(", "),
                name = t.name.name,
                fields = obj_fields.join(", "),
            )
            .unwrap();
        }
    }
    emit_attached_methods(out, &t.name.name, commons);
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

fn emit_attached_methods(out: &mut String, type_name: &str, commons: &TypedCommons) {
    for item in &commons.commons.items {
        let CommonsItem::Fn(f) = item else { continue };
        let FnName::Method {
            type_name: t,
            method_name,
        } = &f.name
        else {
            continue;
        };
        if t.name != type_name {
            continue;
        }
        emit_method(out, f, type_name, method_name, commons);
    }
}

fn emit_method(
    out: &mut String,
    f: &FnDecl,
    type_name: &str,
    method_name: &Ident,
    commons: &TypedCommons,
) {
    emit_doc_block(out, f.documentation.as_deref(), INDENT_STEP);
    let mut params: Vec<String> = Vec::new();
    if f.has_self {
        params.push(format!("self: {type_name}"));
    }
    for p in &f.params {
        params.push(format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)));
    }
    writeln!(
        out,
        "  {method}({params}): {ret} {{",
        method = method_name.name,
        params = params.join(", "),
        ret = ts_type_ref(&f.return_type),
    )
    .unwrap();
    let empty = crate::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty);
    emit_block_as_function_body(out, &f.body, &mut cx, INDENT_STEP * 2);
    writeln!(out, "  }},").unwrap();
}

fn emit_free_fn(out: &mut String, f: &FnDecl, commons: &TypedCommons) {
    let FnName::Free(name) = &f.name else {
        return;
    };
    emit_doc_block(out, f.documentation.as_deref(), 0);
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
        .collect();
    let async_kw = if is_effectful_return(&f.return_type) {
        "async "
    } else {
        ""
    };
    writeln!(
        out,
        "export {async_kw}function {name}({params}): {ret} {{",
        name = name.name,
        params = params.join(", "),
        ret = ts_type_ref(&f.return_type),
    )
    .unwrap();
    let empty = crate::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty);
    emit_block_as_function_body(out, &f.body, &mut cx, INDENT_STEP);
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn is_effectful_return(r: &TypeRef) -> bool {
    matches!(r, TypeRef::Effect(_, _))
}

// -- v0.5 emission --

fn emit_capability(out: &mut String, c: &CapabilityDecl) {
    emit_doc_block(out, c.documentation.as_deref(), 0);
    writeln!(out, "export interface {name} {{", name = c.name.name).unwrap();
    for op in &c.ops {
        emit_doc_block(out, op.documentation.as_deref(), INDENT_STEP);
        let params: Vec<String> = op
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        writeln!(
            out,
            "  {name}({params}): {ret};",
            name = op.name.name,
            params = params.join(", "),
            ret = ts_type_ref(&op.return_type),
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    // Injection token (symbol carrying the interface type).
    writeln!(
        out,
        "export const {name}Token: unique symbol = Symbol(\"{name}\");",
        name = c.name.name
    )
    .unwrap();
    writeln!(out).unwrap();
}

fn emit_provider(out: &mut String, p: &ProviderDecl, commons: &TypedCommons, ctx: &EmitProjectCtx) {
    emit_doc_block(out, p.documentation.as_deref(), 0);
    writeln!(
        out,
        "export class {prov} implements {cap} {{",
        prov = p.provider_name.name,
        cap = p.capability.name,
    )
    .unwrap();
    for op in &p.ops {
        let params: Vec<String> = op
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        let async_kw = if is_effectful_return(&op.return_type) {
            "async "
        } else {
            ""
        };
        writeln!(
            out,
            "  {async_kw}{name}({params}): {ret} {{",
            name = op.name.name,
            params = params.join(", "),
            ret = ts_type_ref(&op.return_type),
        )
        .unwrap();
        let mut cx = LowerCtx::new(commons, &ctx.cross_context);
        emit_block_as_function_body(out, &op.body, &mut cx, INDENT_STEP * 2);
        writeln!(out, "  }}").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "export const {prov}Provider = {{ token: {cap}Token, factory: () => new {prov}() }};",
        prov = p.provider_name.name,
        cap = p.capability.name,
    )
    .unwrap();
    writeln!(out).unwrap();
}

fn emit_service(out: &mut String, s: &ServiceDecl, commons: &TypedCommons, ctx: &EmitProjectCtx) {
    emit_doc_block(out, s.documentation.as_deref(), 0);
    writeln!(out, "export const {name} = {{", name = s.name.name).unwrap();
    for handler in &s.handlers {
        emit_doc_block(out, handler.documentation.as_deref(), INDENT_STEP);
        let kind_name = match handler.kind {
            HandlerKind::Call => "call",
        };
        // For service handlers the operation name is the handler kind
        // (e.g. `call`). v0.5 has only one handler kind, so the service is a
        // single-operation object literal.
        let mut params: Vec<String> = handler
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        // Lower the body first so we can detect cross-context usage and
        // adjust the deps shape accordingly.
        let mut body_out = String::new();
        let mut cx = LowerCtx::new(commons, &ctx.cross_context);
        cx.capabilities = handler
            .given
            .iter()
            .map(|c| c.name.clone())
            .collect::<HashSet<_>>();
        emit_block_as_function_body(&mut body_out, &handler.body, &mut cx, INDENT_STEP * 2);
        // Append the deps parameter (may include surface field if the body
        // made cross-context calls).
        let deps_ty = build_deps_object_ty_with_surface(&handler.given, &cx, &ctx.cross_context);
        params.push(format!("deps: {deps_ty}"));
        let ret = ts_type_ref(&handler.return_type);
        let async_kw = if is_effectful_return(&handler.return_type) {
            "async "
        } else {
            ""
        };
        writeln!(
            out,
            "  {async_kw}{op}({params}): {ret} {{",
            op = kind_name,
            params = params.join(", "),
        )
        .unwrap();
        out.push_str(&body_out);
        writeln!(out, "  }},").unwrap();
    }
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

fn build_deps_object_ty_with_surface(
    given: &[Ident],
    cx: &LowerCtx<'_>,
    cross_context: &crate::resolver::CrossContextInfo,
) -> String {
    let mut parts: Vec<String> = given
        .iter()
        .map(|c| format!("{}: {}", c.name, c.name))
        .collect();
    if cx.cross_context_used {
        parts.push(format!("surface: {}", surface_ty(cross_context)));
    }
    if parts.is_empty() {
        return "{}".to_string();
    }
    format!("{{ {} }}", parts.join("; "))
}

/// Build the TS type for the `surface` field in deps, naming each consumed
/// context by its surface key plus the consumed context's makeSurface type.
fn surface_ty(cross_context: &crate::resolver::CrossContextInfo) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();
    // Use alias if present, else the last segment of the qualified name.
    // Order: stable (sorted) so the diff is deterministic.
    let mut consumed_sorted = cross_context.consumed_contexts.clone();
    consumed_sorted.sort();
    // Reverse lookup: consumed-context qualified name → alias.
    let mut alias_for: HashMap<String, String> = HashMap::new();
    for (alias, target) in &cross_context.aliases {
        alias_for.insert(target.clone(), alias.clone());
    }
    for q in &consumed_sorted {
        let key = alias_for
            .get(q)
            .cloned()
            .unwrap_or_else(|| q.rsplit('.').next().unwrap_or(q.as_str()).to_string());
        let ns = qualified_to_ns(q);
        entries.push((key, format!("ReturnType<typeof {ns}.makeSurface>")));
    }
    if entries.is_empty() {
        return "{}".to_string();
    }
    let body = entries
        .into_iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!("{{ {body} }}")
}

/// Turn a qualified context name (e.g. `commerce.payment`) into the JS
/// namespace ident used in `import * as <ns>` (`commerce_payment`).
fn qualified_to_ns(q: &str) -> String {
    q.replace('.', "_")
}

/// Emit the `makeSurface(deps)` function for a context that exposes
/// services to other contexts (v0.6 §6.3 / §6.4).
fn emit_make_surface(out: &mut String, commons: &TypedCommons) {
    let services: Vec<&ServiceDecl> = commons
        .commons
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Service(s) => Some(s),
            _ => None,
        })
        .collect();
    if services.is_empty() {
        return;
    }
    writeln!(
        out,
        "export function makeSurface(deps: Parameters<typeof {svc}.call>[1]) {{",
        svc = services[0].name.name,
    )
    .unwrap();
    writeln!(out, "  return {{").unwrap();
    for s in &services {
        // For each handler kind currently only `call`. We bind it as a
        // method on the surface with the deps captured.
        let handler = s
            .handlers
            .iter()
            .find(|h| matches!(h.kind, HandlerKind::Call));
        let Some(h) = handler else { continue };
        let async_kw = if is_effectful_return(&h.return_type) {
            "async "
        } else {
            ""
        };
        let param_decls: Vec<String> = h
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        let param_args: Vec<String> = h.params.iter().map(|p| p.name.name.clone()).collect();
        let ret = ts_type_ref(&h.return_type);
        writeln!(
            out,
            "    {async_kw}{sname}({params}): {ret} {{",
            sname = s.name.name,
            params = param_decls.join(", "),
        )
        .unwrap();
        writeln!(
            out,
            "      return {svc}.call({args}{sep}deps);",
            svc = s.name.name,
            args = param_args.join(", "),
            sep = if param_args.is_empty() { "" } else { ", " },
        )
        .unwrap();
        writeln!(out, "    }},").unwrap();
    }
    writeln!(out, "  }};").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// If `receiver` is a dotted chain or single ident that matches one of the
/// current context's `consumes` clauses (by alias or qualified name), return
/// the consumed context's qualified name plus the surface key used to access
/// it through `deps.surface.<key>`.
fn cross_context_lowering_prefix(receiver: &Expr, cx: &LowerCtx<'_>) -> Option<(String, String)> {
    let chain = flatten_emit_ident_chain(receiver)?;
    let info = cx.cross_context;
    if info.consumed_contexts.is_empty() && info.aliases.is_empty() {
        return None;
    }
    let consumed = info.resolve_prefix(&chain)?;
    // Surface key: prefer the alias if there is one, else the last segment.
    let mut alias_for: HashMap<String, String> = HashMap::new();
    for (alias, target) in &info.aliases {
        alias_for.insert(target.clone(), alias.clone());
    }
    let key = alias_for.get(&consumed).cloned().unwrap_or_else(|| {
        consumed
            .rsplit('.')
            .next()
            .unwrap_or(consumed.as_str())
            .to_string()
    });
    Some((consumed, key))
}

fn flatten_emit_ident_chain(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Ident(id) => Some(id.name.clone()),
        ExprKind::FieldAccess { receiver, field } => {
            let head = flatten_emit_ident_chain(receiver)?;
            Some(format!("{head}.{}", field.name))
        }
        _ => None,
    }
}

/// Cast an argument crossing a context boundary to the consumed context's
/// type. For named types we emit `arg as <ns>.<TypeName>`. For other types
/// (base, ()), no cast is needed. The structural compatibility check at the
/// karn layer guarantees the cast is sound.
fn param_cast(
    consumed: &str,
    info: &crate::resolver::CrossContextInfo,
    method: &Ident,
    idx: usize,
    arg: String,
) -> String {
    let Some(svcs) = info.consumed_services.get(consumed) else {
        return arg;
    };
    let Some(service) = svcs.get(&method.name) else {
        return arg;
    };
    let Some((_, ptype_ref)) = service.params.get(idx) else {
        return arg;
    };
    if let Some(name) = type_ref_named_root(ptype_ref) {
        let ns = qualified_to_ns(consumed);
        return format!("({arg} as {ns}.{name})");
    }
    arg
}

/// If the type-ref names a single user type at its root, return that name.
/// (For generics like `Result[T, E]`, we don't emit a cast at the outer
/// layer — TypeScript handles the variance through the intersection.)
fn type_ref_named_root(r: &TypeRef) -> Option<&str> {
    match r {
        TypeRef::Named(id) => Some(id.name.as_str()),
        _ => None,
    }
}

#[allow(dead_code)]
fn build_deps_object_ty(given: &[Ident]) -> String {
    if given.is_empty() {
        return "{}".to_string();
    }
    let parts: Vec<String> = given
        .iter()
        .map(|c| format!("{}: {}", c.name, c.name))
        .collect();
    format!("{{ {} }}", parts.join("; "))
}

fn emit_agent(out: &mut String, a: &AgentDecl, commons: &TypedCommons, ctx: &EmitProjectCtx) {
    emit_doc_block(out, a.documentation.as_deref(), 0);
    let state_ty = format!("{}State", a.name.name);
    // 1) State record type.
    writeln!(out, "export interface {state_ty} {{").unwrap();
    for f in &a.state_fields {
        writeln!(
            out,
            "  readonly {name}: {ty};",
            name = f.name.name,
            ty = ts_type_ref(&f.type_ref),
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    // 2) Durable Object class.
    writeln!(out, "export class {name} {{", name = a.name.name).unwrap();
    writeln!(out, "  state: DurableObjectState;").unwrap();
    writeln!(out, "  constructor(state: DurableObjectState) {{").unwrap();
    writeln!(out, "    this.state = state;").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "  private async loadState(): Promise<{state_ty}> {{").unwrap();
    writeln!(
        out,
        "    return (await this.state.storage.get<{state_ty}>(\"state\"))!;"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "  private async commitState(s: {state_ty}): Promise<void> {{"
    )
    .unwrap();
    writeln!(out, "    await this.state.storage.put(\"state\", s);").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
    // 3) Handlers.
    for h in &a.handlers {
        emit_doc_block(out, h.documentation.as_deref(), INDENT_STEP);
        let mut params: Vec<String> = h
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        // Lower body into a buffer so we can detect cross-context usage and
        // shape the deps type accordingly.
        let mut body_out = String::new();
        let mut cx = LowerCtx::new(commons, &ctx.cross_context);
        cx.in_agent_handler = true;
        cx.agent_state_var = Some("currentState".to_string());
        cx.agent_key_field = Some(a.key_name.name.clone());
        cx.capabilities = h
            .given
            .iter()
            .map(|c| c.name.clone())
            .collect::<HashSet<_>>();
        emit_block_as_function_body(&mut body_out, &h.body, &mut cx, INDENT_STEP * 2);
        let deps_ty = build_deps_object_ty_with_surface(&h.given, &cx, &ctx.cross_context);
        params.push(format!("deps: {deps_ty}"));
        let ret = ts_type_ref(&h.return_type);
        let async_kw = if is_effectful_return(&h.return_type) {
            "async "
        } else {
            ""
        };
        let method = h
            .method_name
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| match h.kind {
                HandlerKind::Call => "call".to_string(),
            });
        writeln!(
            out,
            "  {async_kw}{method}({params}): {ret} {{",
            params = params.join(", "),
        )
        .unwrap();
        // Load state at entry so the body's references to `self.state` work.
        // (We bind a local `currentState` for the body and provide `self`
        // through ID substitution at lowering time.)
        writeln!(out, "    const currentState = await this.loadState();").unwrap();
        out.push_str(&body_out);
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// Per-function lowering context: fresh-temp counter + typed-commons handle
/// (used to look up receiver types for method-call UFCS lowering).
struct LowerCtx<'a> {
    next_tmp: u32,
    commons: &'a TypedCommons,
    /// Names of capabilities in scope as `given C1, C2, ...`. Used to lower
    /// `Capability.op(args)` calls to `deps.Capability.op(args)`.
    capabilities: HashSet<String>,
    /// True when lowering an agent handler body. Used to rewrite `self.state`
    /// and `self.<keyField>` access into the appropriate locals.
    in_agent_handler: bool,
    /// The local variable holding the loaded state inside an agent handler.
    agent_state_var: Option<String>,
    /// The name of the agent's `key id` field (so `self.<id>` resolves).
    agent_key_field: Option<String>,
    /// Cross-context info for v0.6 cross-context call lowering.
    cross_context: &'a crate::resolver::CrossContextInfo,
    /// True if the current handler made at least one cross-context call
    /// (drives whether `deps` gets a `surface` field type).
    cross_context_used: bool,
    /// v0.7: when lowering a test case body, the target context's local
    /// service names. A `service.call(args)` or `service(args)` invocation
    /// where `service` is in this set lowers to `<service>.call(args, deps)`
    /// so the test wires its `deps` through.
    pub test_services: HashSet<String>,
}

impl<'a> LowerCtx<'a> {
    fn new(
        commons: &'a TypedCommons,
        cross_context: &'a crate::resolver::CrossContextInfo,
    ) -> Self {
        Self {
            next_tmp: 0,
            commons,
            capabilities: HashSet::new(),
            in_agent_handler: false,
            agent_state_var: None,
            agent_key_field: None,
            cross_context,
            cross_context_used: false,
            test_services: HashSet::new(),
        }
    }
    fn fresh(&mut self) -> String {
        let n = self.next_tmp;
        self.next_tmp += 1;
        format!("__r{n}")
    }
    fn receiver_namespace(&self, e: &Expr) -> Option<String> {
        let ty = self.commons.expr_types.get(&e.span)?;
        if let Ty::Named { name, .. } = ty {
            Some(name.clone())
        } else {
            None
        }
    }
    /// Resolve the payload field name for the i-th positional binding of
    /// a variant. Built-ins are recognised by name; user variants are
    /// looked up via the type tables.
    fn positional_field_name(
        &self,
        discriminant_ty: Option<&Ty>,
        variant: &str,
        idx: usize,
    ) -> String {
        match (variant, idx) {
            ("Ok", 0) | ("Some", 0) => return "value".to_string(),
            ("Err", 0) => return "error".to_string(),
            _ => {}
        }
        if let Some(Ty::Named {
            kind: NamedKind::Sum,
            name,
        }) = discriminant_ty
            && let Some(decl) = self.commons.types.get(name)
            && let TypeBody::Sum(s) = &decl.body
            && let Some(v) = s.variants.iter().find(|v| v.name.name == variant)
            && let Some(f) = v.payload.get(idx)
        {
            return f.name.name.clone();
        }
        // Single-field fallback. The checker rejects mixed bindings already.
        "value".to_string()
    }
}

/// Lower a block to a sequence of TypeScript statements suitable for use as
/// an async function body. Used by v0.7 mock-operation emission.
pub fn lower_block_to_async_body(
    block: &Block,
    _return_type: &TypeRef,
    typed: &mut TypedCommons,
    cross_context: &crate::resolver::CrossContextInfo,
) -> String {
    let mut out = String::new();
    let mut cx = LowerCtx::new(typed, cross_context);
    emit_block_as_function_body(&mut out, block, &mut cx, 0);
    out
}

/// Lower a test-case body: statements followed by a discarded tail expression
/// (the runner records success via the assertion mechanism, not via a return
/// value). Used by v0.7 test emission.
pub fn lower_test_case_body(
    block: &Block,
    typed: &mut TypedCommons,
    cross_context: &crate::resolver::CrossContextInfo,
    test_services: HashSet<String>,
) -> String {
    let mut out = String::new();
    let mut cx = LowerCtx::new(typed, cross_context);
    cx.test_services = test_services;
    for stmt in &block.statements {
        emit_statement(&mut out, stmt, &mut cx, 0);
    }
    // Evaluate the tail expression but discard its value; assertions inside
    // it still take effect via thrown AssertionErrors.
    let mut stmts = Vec::new();
    let tail = lower_expr(&block.tail, &mut stmts, &mut cx);
    for s in &stmts {
        write_line(&mut out, 0, s);
    }
    if !tail.is_empty() && tail != "undefined" {
        write_line(&mut out, 0, &format!("void ({tail});"));
    }
    out
}

fn emit_block_as_function_body(out: &mut String, block: &Block, cx: &mut LowerCtx, indent: usize) {
    for stmt in &block.statements {
        emit_statement(out, stmt, cx, indent);
    }
    // Tail position: match → inline switch, if → inline if, otherwise return expr.
    match &block.tail.kind {
        ExprKind::Match { discriminant, arms } => {
            emit_match_tail(out, discriminant, arms, cx, indent);
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } if !both_simple(then_block, else_block) || cond_has_is_bindings(cond) => {
            emit_if_tail(out, cond, then_block, else_block, cx, indent);
        }
        _ => {
            let mut stmts = Vec::new();
            let tail = lower_expr(&block.tail, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("return {tail};"));
        }
    }
}

fn emit_statement(out: &mut String, stmt: &Statement, cx: &mut LowerCtx, indent: usize) {
    match stmt {
        Statement::Let(l) => {
            let mut stmts = Vec::new();
            let value = lower_expr(&l.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            let bind_name = if l.name.name == "_" {
                // Emit a unique throwaway local; TS allows `const _ = ...` only
                // once per scope, so use a fresh name to be safe.
                cx.fresh()
            } else {
                l.name.name.clone()
            };
            match &l.type_annot {
                Some(annot) => write_line(
                    out,
                    indent,
                    &format!(
                        "const {bind_name}: {ty} = {value};",
                        ty = ts_type_ref(annot),
                    ),
                ),
                None => write_line(out, indent, &format!("const {bind_name} = {value};")),
            }
        }
        Statement::EffectLet(l) => {
            // `let x <- expr` → `const x = await expr;`
            let mut stmts = Vec::new();
            let value = lower_expr(&l.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            let bind_name = if l.name.name == "_" {
                cx.fresh()
            } else {
                l.name.name.clone()
            };
            match &l.type_annot {
                Some(annot) => write_line(
                    out,
                    indent,
                    &format!(
                        "const {bind_name}: {ty} = await {value};",
                        ty = ts_type_ref(annot),
                    ),
                ),
                None => write_line(out, indent, &format!("const {bind_name} = await {value};")),
            }
        }
        Statement::Commit(c) => {
            // Inside an agent handler, `commit expr` → `await this.commitState(expr);`
            let mut stmts = Vec::new();
            let value = lower_expr(&c.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("await this.commitState({value});"));
        }
        Statement::Assert(a) => {
            // Inside a test case body, `assert expr` lowers to a runtime check
            // that throws an AssertionError so the surrounding test-case
            // runner catches it and records the failure.
            let mut stmts = Vec::new();
            let value = lower_expr(&a.value, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            let display = a.value.span.start.to_string();
            let span_start = a.value.span.start;
            let span_end = a.value.span.end;
            write_line(
                out,
                indent,
                &format!(
                    "if (!({value})) {{ throw __karnAssertionFailure(\"offset {display}\", {span_start}, {span_end}); }}",
                ),
            );
        }
    }
}

fn write_line(out: &mut String, indent: usize, line: &str) {
    for _ in 0..indent {
        out.push(' ');
    }
    out.push_str(line);
    out.push('\n');
}

fn lower_expr(e: &Expr, stmts: &mut Vec<String>, cx: &mut LowerCtx) -> String {
    match &e.kind {
        ExprKind::IntLit(n) => n.to_string(),
        ExprKind::StrLit(s) => format!("\"{}\"", escape_ts_string(s)),
        ExprKind::BoolLit(b) => b.to_string(),
        ExprKind::Ident(id) => {
            // A bare ident whose name matches a declared variant of a sum
            // type (and whose checker type is that sum) is a nullary
            // variant constructor reference. Qualify it as `Type.Variant`.
            // Otherwise (locals, params, `self`) emit the identifier as-is.
            if let Some(Ty::Named {
                kind: NamedKind::Sum,
                name: type_name,
            }) = cx.commons.expr_types.get(&e.span)
                && let Some(decl) = cx.commons.types.get(type_name)
                && let TypeBody::Sum(s) = &decl.body
                && s.variants.iter().any(|v| v.name.name == id.name)
            {
                return format!("{}.{}", type_name, id.name);
            }
            id.name.clone()
        }
        ExprKind::Call(name, args) => {
            // Bare variant constructor with payload → qualify.
            let args_lowered: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
            if let Some(Ty::Named {
                kind: NamedKind::Sum,
                name: type_name,
            }) = cx.commons.expr_types.get(&e.span)
                && type_name != &name.name
            {
                return format!("{}.{}({})", type_name, name.name, args_lowered.join(", "));
            }
            format!("{}({})", name.name, args_lowered.join(", "))
        }
        ExprKind::UnaryOp(op, inner) => {
            let inner = lower_expr(inner, stmts, cx);
            let sym = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{sym}{inner}")
        }
        ExprKind::BinOp(op, lhs, rhs) => {
            // For `&&` we need to lower `is` bindings into the rhs scope.
            // We handle that here by collecting bindings from lhs, emitting
            // them as `const` declarations before evaluating rhs — but
            // `&&` short-circuits, so simply emitting them inline is wrong.
            // We compile `lhs && (...is binding flow...) rhs` to a function
            // expression: `(lhs && ((bindings) => rhs)())`. Simpler: rely
            // on TypeScript's narrowing for the value-from-is part of
            // `is` patterns. For now, for the special pattern `x is Ok(n)`
            // we lower the rhs assuming the binding `n = x.value` was
            // captured. We use a parenthesised IIFE to scope the binding.
            if *op == BinOp::And
                && let Some((bindings, lhs_expr, rhs_expr)) = lower_and_with_is(lhs, rhs, stmts, cx)
            {
                if bindings.is_empty() {
                    return format!("{lhs_expr} && {rhs_expr}");
                }
                // Emit:  lhs && (() => { const n = ...; return rhs; })()
                let mut wrap = String::new();
                wrap.push_str(&lhs_expr);
                wrap.push_str(" && ((() => { ");
                for b in &bindings {
                    wrap.push_str(b);
                    wrap.push(' ');
                }
                wrap.push_str(&format!("return {rhs_expr}; }})())"));
                return wrap;
            }
            let l = lower_expr(lhs, stmts, cx);
            let r = lower_expr(rhs, stmts, cx);
            if *op == BinOp::Div {
                format!("Math.trunc({l} / {r})")
            } else {
                format!("{l} {} {r}", ts_binop(*op))
            }
        }
        ExprKind::Paren(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("({s})")
        }
        ExprKind::Ok(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("Ok({s})")
        }
        ExprKind::Err(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("Err({s})")
        }
        ExprKind::Some(inner) => {
            let s = lower_expr(inner, stmts, cx);
            format!("Some({s})")
        }
        ExprKind::None => "None".to_string(),
        ExprKind::Question(inner) => {
            let inner_expr = lower_expr(inner, stmts, cx);
            let tmp = cx.fresh();
            stmts.push(format!("const {tmp} = {inner_expr};"));
            stmts.push(format!("if ({tmp}.tag === \"Err\") return {tmp};"));
            format!("{tmp}.value")
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            let args: Vec<String> = args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
            // Nullary variant qualified construction: `T.V` (no parens) at the
            // source level wouldn't reach here, so `T.V()` always means call.
            format!("{}.{}({})", type_name.name, method.name, args.join(", "))
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            let mut parts = Vec::new();
            for f in fields {
                match &f.value {
                    Some(v) => {
                        let val = lower_expr(v, stmts, cx);
                        parts.push(format!("{}: {}", f.name.name, val));
                    }
                    None => parts.push(f.name.name.clone()),
                }
            }
            let _ = type_name;
            format!("{{ {} }}", parts.join(", "))
        }
        ExprKind::FieldAccess { receiver, field } => {
            // Agent-handler `self.state` and `self.<key>` rewrites.
            if cx.in_agent_handler
                && let ExprKind::Ident(id) = &receiver.kind
                && id.name == "self"
            {
                if field.name == "state"
                    && let Some(s) = &cx.agent_state_var
                {
                    return s.clone();
                }
                if let Some(k) = &cx.agent_key_field
                    && field.name == *k
                {
                    return format!("(this.state.id.toString() as {})", k);
                }
            }
            let r = lower_expr(receiver, stmts, cx);
            // `.raw` on an opaque value compiles to a TypeScript type
            // assertion back to the base type. The checker has already
            // verified that the receiver is opaque and the call site is
            // inside the defining commons.
            if field.name == "raw"
                && let Some(Ty::Named {
                    kind: NamedKind::Opaque(base),
                    ..
                }) = cx.commons.expr_types.get(&receiver.span)
            {
                return format!("({r} as {})", ts_base(*base));
            }
            format!("{r}.{}", field.name)
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            // v0.6 cross-context service call: receiver is an alias or the
            // dotted name of a consumed context. Lower to
            // `deps.surface.<key>.<method>(args as <consumed_ns>.<T>)`. The
            // call returns a Promise; the surrounding effect-let / await
            // unwraps it.
            if let Some((consumed, key)) = cross_context_lowering_prefix(receiver, cx) {
                cx.cross_context_used = true;
                let args_lowered: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let lowered = lower_expr(a, stmts, cx);
                        param_cast(&consumed, cx.cross_context, method, i, lowered)
                    })
                    .collect();
                return format!(
                    "deps.surface.{key}.{}({})",
                    method.name,
                    args_lowered.join(", ")
                );
            }
            // Capability call: receiver is a bare ident naming a declared
            // capability in `given`. Lower to `deps.Capability.op(args)`.
            if let ExprKind::Ident(id) = &receiver.kind
                && cx.capabilities.contains(&id.name)
            {
                let args_lowered: Vec<String> =
                    args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
                return format!(
                    "deps.{}.{}({})",
                    id.name,
                    method.name,
                    args_lowered.join(", ")
                );
            }
            // Static call: receiver is a bare ident naming a declared type.
            if let ExprKind::Ident(id) = &receiver.kind
                && cx.commons.types.contains_key(&id.name)
            {
                let args_lowered: Vec<String> =
                    args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
                return format!("{}.{}({})", id.name, method.name, args_lowered.join(", "));
            }
            // v0.7: local service call inside a test case body.
            // `serviceName.method(args)` → `serviceName.method(args, deps)`.
            if let ExprKind::Ident(id) = &receiver.kind
                && cx.test_services.contains(&id.name)
            {
                let args_lowered: Vec<String> =
                    args.iter().map(|a| lower_expr(a, stmts, cx)).collect();
                let mut all = args_lowered;
                all.push("deps".to_string());
                return format!("{}.{}({})", id.name, method.name, all.join(", "));
            }
            // Instance call: UFCS lowering with the receiver as first arg.
            let ns = cx
                .receiver_namespace(receiver)
                .unwrap_or_else(|| "/* unknown */".to_string());
            let recv = lower_expr(receiver, stmts, cx);
            let mut all = vec![recv];
            for a in args {
                all.push(lower_expr(a, stmts, cx));
            }
            format!("{ns}.{}({})", method.name, all.join(", "))
        }
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => lower_if(cond, then_block, else_block, stmts, cx),
        ExprKind::Block(b) => lower_block_as_expr(b, cx),
        ExprKind::Match { discriminant, arms } => lower_match_as_iife(discriminant, arms, cx),
        ExprKind::Is { value, pattern } => lower_is(value, pattern, stmts, cx),
        ExprKind::UnitLit => "undefined".to_string(),
        ExprKind::EffectPure(inner) => {
            let inner_expr = lower_expr(inner, stmts, cx);
            format!("Promise.resolve({inner_expr})")
        }
        ExprKind::RecordSpread {
            type_name: _,
            base,
            overrides,
        } => {
            let base_expr = lower_expr(base, stmts, cx);
            let mut parts = vec![format!("...{base_expr}")];
            for f in overrides {
                match &f.value {
                    Some(v) => {
                        let val = lower_expr(v, stmts, cx);
                        parts.push(format!("{}: {}", f.name.name, val));
                    }
                    None => parts.push(f.name.name.clone()),
                }
            }
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

/// When we encounter `lhs && rhs`, see if lhs is an `is` (possibly wrapped
/// in parens or nested `&&`) and if so collect the bindings to inject into
/// rhs. Returns `(binding_const_decls, lowered_lhs, lowered_rhs)` if
/// special handling is appropriate; otherwise returns None.
fn lower_and_with_is(
    lhs: &Expr,
    rhs: &Expr,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> Option<(Vec<String>, String, String)> {
    let mut bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(lhs, cx, &mut bindings, &mut found);
    if !found {
        return None;
    }
    let lhs_expr = lower_expr(lhs, stmts, cx);
    // We lower rhs ourselves (not into the outer stmts), so that any
    // statement-style prefix from rhs is folded into the IIFE properly.
    let mut rhs_stmts = Vec::new();
    let rhs_expr = lower_expr(rhs, &mut rhs_stmts, cx);
    let mut rhs_full = String::new();
    for s in &rhs_stmts {
        rhs_full.push_str(s);
        rhs_full.push(' ');
    }
    rhs_full.push_str(&rhs_expr);
    Some((bindings, lhs_expr, rhs_full))
}

/// Walk an expression collecting `const name = expr.field;` strings for
/// any `is`-pattern bindings on the truthy path. `found` indicates whether
/// at least one `is` was seen.
fn gather_is_bindings_for_emit(e: &Expr, cx: &LowerCtx, out: &mut Vec<String>, found: &mut bool) {
    match &e.kind {
        ExprKind::Is { value, pattern } => {
            *found = true;
            let value_text = value_text_for_is(value);
            let disc_ty = cx.commons.expr_types.get(&value.span).cloned();
            if let Pattern::Variant {
                variant, bindings, ..
            } = pattern
            {
                for (i, b) in bindings.iter().enumerate() {
                    if b.is_wildcard() {
                        continue;
                    }
                    match &b.kind {
                        PatternBindingKind::Named { field, name } => {
                            out.push(format!(
                                "const {name} = {value}.{field};",
                                name = name.name,
                                value = value_text,
                                field = field.name
                            ));
                        }
                        PatternBindingKind::Positional { name } => {
                            let field =
                                cx.positional_field_name(disc_ty.as_ref(), &variant.name, i);
                            out.push(format!(
                                "const {name} = {value}.{field};",
                                name = name.name,
                                value = value_text,
                                field = field
                            ));
                        }
                    }
                }
            }
        }
        ExprKind::BinOp(BinOp::And, l, r) => {
            gather_is_bindings_for_emit(l, cx, out, found);
            gather_is_bindings_for_emit(r, cx, out, found);
        }
        ExprKind::Paren(inner) => gather_is_bindings_for_emit(inner, cx, out, found),
        _ => {}
    }
}

/// Render the receiver of an `is` expression for use in binding lookups.
/// Only simple expressions are sound — for arbitrary call expressions we'd
/// need to lift to a temporary. The checker should reject anything that
/// makes this dangerous; for now we handle ident and member-access cases.
fn value_text_for_is(value: &Expr) -> String {
    match &value.kind {
        ExprKind::Ident(id) => id.name.clone(),
        ExprKind::FieldAccess { receiver, field } => {
            format!("{}.{}", value_text_for_is(receiver), field.name)
        }
        ExprKind::Paren(inner) => value_text_for_is(inner),
        _ => "(/* TODO: complex is-receiver */ )".to_string(),
    }
}

fn lower_if(
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx,
) -> String {
    let cond_expr = lower_expr(cond, stmts, cx);
    // If the cond contains `is`-bindings, the then-branch needs a place
    // for the `const name = receiver.field;` declarations — a ternary
    // has no such place. Force the IIFE form.
    if both_simple(then_block, else_block) && !cond_has_is_bindings(cond) {
        let mut tstmts = Vec::new();
        let testr = lower_expr(&then_block.tail, &mut tstmts, cx);
        debug_assert!(tstmts.is_empty());
        let mut estmts = Vec::new();
        let eestr = lower_expr(&else_block.tail, &mut estmts, cx);
        debug_assert!(estmts.is_empty());
        format!("({cond_expr} ? {testr} : {eestr})")
    } else {
        let mut iife = String::new();
        iife.push_str("(() => {\n");
        iife.push_str("    if (");
        iife.push_str(&cond_expr);
        iife.push_str(") {\n");
        // Inject is-binding declarations on the truthy side.
        let mut is_bindings = Vec::new();
        let mut found = false;
        gather_is_bindings_for_emit(cond, cx, &mut is_bindings, &mut found);
        for b in &is_bindings {
            for _ in 0..(INDENT_STEP * 3) {
                iife.push(' ');
            }
            iife.push_str(b);
            iife.push('\n');
        }
        emit_block_as_function_body(&mut iife, then_block, cx, INDENT_STEP * 3);
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("} else {\n");
        emit_block_as_function_body(&mut iife, else_block, cx, INDENT_STEP * 3);
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("}\n");
        for _ in 0..INDENT_STEP {
            iife.push(' ');
        }
        iife.push_str("})()");
        iife
    }
}

/// True if the expression contains an `is` test with at least one
/// non-wildcard binding. Walks through `&&`, `||`, and parens.
fn cond_has_is_bindings(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Is {
            pattern: Pattern::Variant { bindings, .. },
            ..
        } => bindings.iter().any(|b| !b.is_wildcard()),
        ExprKind::BinOp(BinOp::And, l, r) | ExprKind::BinOp(BinOp::Or, l, r) => {
            cond_has_is_bindings(l) || cond_has_is_bindings(r)
        }
        ExprKind::Paren(inner) => cond_has_is_bindings(inner),
        _ => false,
    }
}

fn emit_if_tail(
    out: &mut String,
    cond: &Expr,
    then_block: &Block,
    else_block: &Block,
    cx: &mut LowerCtx,
    indent: usize,
) {
    let mut pre = Vec::new();
    let cond_expr = lower_expr(cond, &mut pre, cx);
    for s in &pre {
        write_line(out, indent, s);
    }
    write_line(out, indent, &format!("if ({cond_expr}) {{"));
    // is-binding declarations on the truthy path.
    let mut is_bindings = Vec::new();
    let mut found = false;
    gather_is_bindings_for_emit(cond, cx, &mut is_bindings, &mut found);
    for b in &is_bindings {
        write_line(out, indent + INDENT_STEP, b);
    }
    emit_block_as_function_body(out, then_block, cx, indent + INDENT_STEP);
    write_line(out, indent, "} else {");
    emit_block_as_function_body(out, else_block, cx, indent + INDENT_STEP);
    write_line(out, indent, "}");
}

fn both_simple(a: &Block, b: &Block) -> bool {
    a.statements.is_empty()
        && b.statements.is_empty()
        && simple_expr(&a.tail)
        && simple_expr(&b.tail)
}

fn simple_expr(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Question(_) => false,
        ExprKind::Match { .. } => false,
        ExprKind::Block(b) => b.statements.is_empty() && simple_expr(&b.tail),
        ExprKind::If {
            then_block,
            else_block,
            cond,
        } => simple_expr(cond) && both_simple(then_block, else_block),
        ExprKind::Ok(i) | ExprKind::Err(i) | ExprKind::Some(i) => simple_expr(i),
        ExprKind::Paren(i) | ExprKind::UnaryOp(_, i) => simple_expr(i),
        ExprKind::BinOp(_, l, r) => simple_expr(l) && simple_expr(r),
        ExprKind::Call(_, args) | ExprKind::ConstructorCall { args, .. } => {
            args.iter().all(simple_expr)
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            simple_expr(receiver) && args.iter().all(simple_expr)
        }
        ExprKind::FieldAccess { receiver, .. } => simple_expr(receiver),
        ExprKind::RecordConstruction { fields, .. } => fields.iter().all(|f| match &f.value {
            Some(v) => simple_expr(v),
            None => true,
        }),
        ExprKind::Is { value, .. } => simple_expr(value),
        _ => true,
    }
}

fn lower_block_as_expr(b: &Block, cx: &mut LowerCtx) -> String {
    let mut iife = String::new();
    iife.push_str("(() => {\n");
    emit_block_as_function_body(&mut iife, b, cx, INDENT_STEP * 2);
    for _ in 0..INDENT_STEP {
        iife.push(' ');
    }
    iife.push_str("})()");
    iife
}

fn lower_match_as_iife(discriminant: &Expr, arms: &[MatchArm], cx: &mut LowerCtx) -> String {
    let disc_ty = cx.commons.expr_types.get(&discriminant.span).cloned();
    let mut stmts = Vec::new();
    let disc = lower_expr(discriminant, &mut stmts, cx);
    let mut iife = String::new();
    // Pre-statements need to be evaluated before the IIFE; lift them into
    // a sequence: `(prestmt1, prestmt2, iife)`. Since TS doesn't let us
    // evaluate statements inline, we wrap in another arrow.
    let inner_iife = build_match_iife(&disc, &disc_ty, arms, cx);
    if stmts.is_empty() {
        iife.push_str(&inner_iife);
    } else {
        iife.push_str("(() => {\n");
        for s in &stmts {
            for _ in 0..(INDENT_STEP * 2) {
                iife.push(' ');
            }
            iife.push_str(s);
            iife.push('\n');
        }
        for _ in 0..(INDENT_STEP * 2) {
            iife.push(' ');
        }
        iife.push_str("return ");
        iife.push_str(&inner_iife);
        iife.push_str(";\n");
        for _ in 0..INDENT_STEP {
            iife.push(' ');
        }
        iife.push_str("})()");
    }
    iife
}

fn build_match_iife(
    disc_expr: &str,
    disc_ty: &Option<Ty>,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
) -> String {
    let mut out = String::new();
    out.push_str("((__d) => {\n");
    for _ in 0..(INDENT_STEP * 2) {
        out.push(' ');
    }
    out.push_str("switch (__d.tag) {\n");
    for arm in arms {
        emit_match_case(&mut out, "__d", disc_ty, arm, cx, INDENT_STEP * 3);
    }
    for _ in 0..(INDENT_STEP * 2) {
        out.push(' ');
    }
    out.push_str("}\n");
    for _ in 0..(INDENT_STEP * 2) {
        out.push(' ');
    }
    out.push_str("throw new Error(\"non-exhaustive match\");\n");
    for _ in 0..INDENT_STEP {
        out.push(' ');
    }
    out.push_str(&format!("}})({disc_expr})"));
    out
}

fn emit_match_tail(
    out: &mut String,
    discriminant: &Expr,
    arms: &[MatchArm],
    cx: &mut LowerCtx,
    indent: usize,
) {
    let mut pre = Vec::new();
    let disc = lower_expr(discriminant, &mut pre, cx);
    let disc_ty = cx.commons.expr_types.get(&discriminant.span).cloned();
    for s in &pre {
        write_line(out, indent, s);
    }
    write_line(out, indent, &format!("switch ({disc}.tag) {{"));
    for arm in arms {
        emit_match_case(out, &disc, &disc_ty, arm, cx, indent + INDENT_STEP);
    }
    write_line(out, indent, "}");
    write_line(out, indent, "throw new Error(\"non-exhaustive match\");");
}

fn emit_match_case(
    out: &mut String,
    disc_var: &str,
    disc_ty: &Option<Ty>,
    arm: &MatchArm,
    cx: &mut LowerCtx,
    indent: usize,
) {
    match &arm.pattern {
        Pattern::Wildcard(_) => {
            write_line(out, indent, "default: {");
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP);
            write_line(out, indent, "}");
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            write_line(
                out,
                indent,
                &format!("case \"{tag}\": {{", tag = variant.name),
            );
            for (i, b) in bindings.iter().enumerate() {
                if b.is_wildcard() {
                    continue;
                }
                let field = match &b.kind {
                    PatternBindingKind::Named { field, .. } => field.name.clone(),
                    PatternBindingKind::Positional { .. } => {
                        cx.positional_field_name(disc_ty.as_ref(), &variant.name, i)
                    }
                };
                let local = b.local_name().name.clone();
                write_line(
                    out,
                    indent + INDENT_STEP,
                    &format!("const {local} = {disc_var}.{field};"),
                );
            }
            emit_match_body(out, &arm.body, cx, indent + INDENT_STEP);
            write_line(out, indent, "}");
        }
    }
}

fn emit_match_body(out: &mut String, body: &MatchBody, cx: &mut LowerCtx, indent: usize) {
    match body {
        MatchBody::Expr(e) => {
            let mut stmts = Vec::new();
            let v = lower_expr(e, &mut stmts, cx);
            for s in &stmts {
                write_line(out, indent, s);
            }
            write_line(out, indent, &format!("return {v};"));
        }
        MatchBody::Block(b) => emit_block_as_function_body(out, b, cx, indent),
    }
}

fn lower_is(value: &Expr, pattern: &Pattern, stmts: &mut Vec<String>, cx: &mut LowerCtx) -> String {
    let v = lower_expr(value, stmts, cx);
    match pattern {
        Pattern::Wildcard(_) => "true".to_string(),
        Pattern::Variant { variant, .. } => {
            format!("{v}.tag === \"{}\"", variant.name)
        }
    }
}

fn ts_base(b: BaseType) -> &'static str {
    match b {
        BaseType::Int => "number",
        BaseType::String => "string",
        BaseType::Bool => "boolean",
    }
}

pub(crate) fn ts_type_ref(r: &TypeRef) -> String {
    match r {
        TypeRef::Base(b, _) => ts_base(*b).to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(t, e, _) => format!("Result<{}, {}>", ts_type_ref(t), ts_type_ref(e)),
        TypeRef::Option(t, _) => format!("Option<{}>", ts_type_ref(t)),
        TypeRef::Effect(t, _) => {
            let inner = ts_type_ref(t);
            if inner == "()" || inner == "void" {
                "Promise<void>".to_string()
            } else {
                format!("Promise<{inner}>")
            }
        }
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}

fn ts_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "||",
        BinOp::And => "&&",
        BinOp::Eq => "===",
        BinOp::NotEq => "!==",
        BinOp::Lt => "<",
        BinOp::LtEq => "<=",
        BinOp::Gt => ">",
        BinOp::GtEq => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
    }
}

fn escape_ts_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

#[allow(dead_code)]
fn _unused_hashmap(_h: HashMap<String, ()>) {}
