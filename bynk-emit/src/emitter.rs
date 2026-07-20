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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use self::source_map::SourceMapBuilder;

use crate::project::{BuildTarget, EmitProjectCtx, ImportExt, UnitKind};
use bynk_check::builtin_names::map_query;
use bynk_check::builtin_names::methods::{
    FOLD_EFF, FOR_EACH, PAR_TRAVERSE, PAR_TRAVERSE_ALL, PAR_TRAVERSE_TRY, RAW, TRAVERSE_ALL,
    TRAVERSE_TRY,
};
use bynk_check::builtin_names::types::*;
use bynk_check::checker::{NamedKind, Ty, TypedCommons};
use bynk_syntax::ast::*;

pub mod contracts;
pub mod secrets;
pub mod serialisation;
pub mod workers;
pub mod workers_entry;
pub mod wrangler;

pub use secrets::emit_secrets_manifest;
pub use workers::emit_worker_compose;
pub use workers_entry::emit_worker_entry;
pub use wrangler::emit_wrangler_toml;

mod lower;
pub(crate) mod source_map;
pub(crate) use lower::*;
mod emit;
pub(crate) use emit::*;
pub(crate) mod websocket;

const INDENT_STEP: usize = 2;

/// Emit the contents of `out/runtime.ts`. This module ships with every
/// project so the per-context / per-test emissions can `import { Ok, Err,
/// Some, None, ... }` from a single source. It includes:
///
/// - `Result`/`Option` discriminated unions (using `tag` for the
///   discriminant — same shape user sum types lower to).
/// - `ValidationError` (the record shape refined-value constructors return).
/// - The `DurableObjectState`/`DurableObjectStorage` interfaces that agent
///   classes consume, plus an `InMemoryStorage` implementation and a
///   `makeTestState(name)` factory for use in test execution.
///
/// The content is identical across projects — there is no per-project
/// tailoring. Dead code is harmless; tsc handles it.
pub fn emit_runtime_module() -> String {
    RUNTIME_TS.to_string()
}

/// The embedded runtime. This is a BUILD OUTPUT, not a hand-edited file: it is
/// bundled from the focused TypeScript modules in `bynk-emit/runtime/src` by
/// that package's `scripts/bundle.mjs`. Edit the modules there and run
/// `npm run bundle` (CI's `runtime` job guards against drift); never edit this
/// file by hand. Keeping it a committed artifact means `cargo build` stays
/// Node-free and the emitter stays lockstep with the runtime it embeds.
const RUNTIME_TS: &str = include_str!("emitter/runtime.ts");

/// Emit the contents of `out/tsconfig.json`. The CLI uses `tsc -p` against
/// this when running `bynkc test`; users can also drive `tsc` against it
/// directly to produce JS for deployment.
pub fn emit_tsconfig() -> String {
    TSCONFIG_JSON.to_string()
}

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noImplicitAny": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": false,
    "outDir": "../out-js",
    "rootDir": "."
  },
  "include": ["**/*.ts"]
}
"#;

/// Compute the runtime import specifier for a module at `from_source`. For a
/// file at `commerce/payment.ts` the runtime sits two levels up, so this
/// returns `../runtime.js`; for a top-level file it returns `./runtime.js`.
pub fn runtime_import_for(from_source: &Path, ext: ImportExt) -> String {
    let depth = from_source
        .parent()
        .map(|p| {
            p.components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .count()
        })
        .unwrap_or(0);
    let ext = ext.as_str();
    if depth == 0 {
        format!("./runtime.{ext}")
    } else {
        let prefix: String = "../".repeat(depth);
        format!("{prefix}runtime.{ext}")
    }
}

/// Emit TypeScript source for the typed commons (single-file mode).
pub fn emit(commons: &TypedCommons) -> String {
    // Emit the body first so the header can decide which runtime helpers to
    // import from what the body actually references (v0.110: the `__bynkBytes*`
    // helpers are imported only when a `Bytes` value is constructed/compared).
    let mut body = String::new();
    write_commons_doc(&mut body, commons);
    let dummy_ctx = single_file_ctx();
    // Types come first (they define interfaces and namespaces).
    for item in &commons.commons.items {
        if let CommonsItem::Type(t) = item {
            emit_type(&mut body, t, commons, &dummy_ctx);
        }
    }
    // Free functions afterward.
    for item in &commons.commons.items {
        if let CommonsItem::Fn(f) = item
            && let FnName::Free(_) = &f.name
        {
            emit_free_fn(&mut body, f, commons, None, false);
        }
    }
    // v0.22b: module-local codec helpers for Json.encode/decode targets.
    emit_json_codec_helpers(
        &mut body,
        commons,
        &dummy_ctx,
        &HashSet::new(),
        &HashSet::new(),
    );
    let mut out = String::new();
    // v0.153 (ADR 0177): a commons that names `HttpResult` in any signature —
    // e.g. a free `fn -> HttpResult[T]` using the `?`-Option lift — imports it.
    // Structural (over the AST), not a body-string scan, so a comment or string
    // literal mentioning `HttpResult` never triggers a spurious import.
    let uses_http = file_mentions_http_result(commons);
    write_header_single(&mut out, commons, body.contains("__bynkBytes"), uses_http);
    out.push_str(&body);
    out
}

/// A no-op project context for single-file emission. Single-file mode never
/// involves contexts or cross-unit imports, so most fields default to empty.
fn single_file_ctx() -> EmitProjectCtx {
    EmitProjectCtx {
        import_ext: crate::project::ImportExt::Js,
        contracts: false,
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
        cross_context: bynk_check::resolver::CrossContextInfo::default(),
        is_consumed_by_others: false,
        target: BuildTarget::Bundle,
        boundary_type_owners: HashMap::new(),
        local_agents: HashSet::new(),
        agent_given_deps: HashMap::new(),
        extra_import_lines: Vec::new(),
        agent_method_givens: HashMap::new(),
        actors: HashMap::new(),
        consumed_adapters: HashSet::new(),
        history_target_agents: HashSet::new(),
        imported_methods: HashMap::new(),
    }
}

/// Emit TypeScript source for a single file inside a multi-file project,
/// including cross-file and cross-commons imports computed from
/// [`EmitProjectCtx`].
/// Emit one unit's TypeScript, plus its source map (slice 1, ADR 0103).
///
/// `source_text` is the originating `.bynk` file's text and `source_name` its
/// project-root-relative path; together they let the source-map builder resolve
/// each recorded span to a `(line, col)` and embed `sourcesContent`. Returns the
/// generated TS and the serialised source-map v3 JSON (`None` when nothing
/// mapped — e.g. a unit whose items all came from sibling files).
pub fn emit_project(
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_text: &str,
    source_name: &str,
) -> (String, Option<String>) {
    let mut out = String::new();
    // The file's source-map builder. The free-function bodies record statement /
    // match-arm checkpoints through their `LowerCtx`; the declaration loops below
    // record one checkpoint per top-level item so signatures (and the bodies of
    // services/agents, which lower via spliced local buffers) anchor to their
    // declaration (ADR 0103 D2, nearest-enclosing).
    let smb = RefCell::new(SourceMapBuilder::new());
    // The file's `.bynk` source is the primary map source (id 0); `record` targets
    // it and spliced handler bodies in the same file merge against it (v0.70).
    smb.borrow_mut().add_source(source_name, source_text);
    write_header(&mut out, commons, ctx);
    // Compute which names this file actually references that live elsewhere
    // (sibling file in the same commons/context, or a used commons / consumed
    // context).
    let references = collect_external_references(commons, ctx);
    emit_project_imports(&mut out, commons, ctx, &references);
    if !references.is_empty() {
        writeln!(out).unwrap();
    }
    // v0.6: namespace imports for each consumed context that exposes services.
    // v0.15: also for consumed contexts whose capabilities this context uses.
    emit_cross_context_namespace_imports(&mut out, commons, ctx);
    // For contexts: emit per-context nominal rebrand aliases for each type
    // imported via `uses` that this file references. The structural shape is
    // inherited from the original commons type; the brand makes the
    // rebranded type nominally distinct (v0.4 §6.2).
    if ctx.unit_kind == UnitKind::Context {
        emit_context_rebrands(&mut out, &references, commons, ctx);
    }
    write_commons_doc(&mut out, commons);
    for item in &commons.commons.items {
        if let CommonsItem::Type(t) = item {
            smb.borrow_mut().record(out.len(), t.span);
            emit_type(&mut out, t, commons, ctx);
        }
    }
    for item in &commons.commons.items {
        if let CommonsItem::Fn(f) = item
            && let FnName::Free(_) = &f.name
        {
            smb.borrow_mut().record(out.len(), f.span);
            emit_free_fn(&mut out, f, commons, Some(&smb), ctx.contracts);
        }
    }
    // v0.5: behavioural items follow the type/fn declarations.
    for item in &commons.commons.items {
        match item {
            CommonsItem::Capability(c) => {
                smb.borrow_mut().record(out.len(), c.span);
                emit_capability(&mut out, c);
            }
            CommonsItem::Provider(p) => {
                smb.borrow_mut().record(out.len(), p.span);
                emit_provider(&mut out, p, commons, ctx, Some(&smb));
            }
            CommonsItem::Service(s) => {
                smb.borrow_mut().record(out.len(), s.span);
                emit_service(&mut out, s, commons, ctx, Some(&smb));
            }
            CommonsItem::Agent(a) => {
                smb.borrow_mut().record(out.len(), a.span);
                emit_agent(&mut out, a, commons, ctx, Some(&smb));
            }
            _ => {}
        }
    }
    // v0.9.2: per-test registry reset. The test runner calls this before each
    // test so a fresh test sees clean agent state (finding #10's "fresh per
    // test" half).
    let agent_names: Vec<&str> = commons
        .commons
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Agent(a) => Some(a.name.name.as_str()),
            _ => None,
        })
        .collect();
    if !agent_names.is_empty() {
        writeln!(out, "export function __resetAgents(): void {{").unwrap();
        for name in &agent_names {
            writeln!(out, "  {}.reset();", agent_registry_name(name)).unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
    // v0.6: cross-context surface assembly. Emit `makeSurface` for any
    // context that declares services — the composition root references it
    // for every such context, not just those consumed by others. Skipped
    // in workers mode where each Worker has its own `compose(env)` root.
    if ctx.unit_kind == UnitKind::Context && matches!(ctx.target, BuildTarget::Bundle) {
        let has_services = commons
            .commons
            .items
            .iter()
            .any(|i| matches!(i, CommonsItem::Service(_)));
        if has_services {
            emit_make_surface(&mut out, commons, ctx);
        }
    }
    // v0.8: in workers mode, the context module also exports per-type
    // serialise/deserialise helpers for every type that crosses a
    // boundary. The commons modules likewise carry helpers for their
    // own commons-declared boundary types.
    // v0.96 (ADR 0124): runs on both targets — workers emits service-call +
    // agent-rehydration boundary helpers; bundle emits only the agent-rehydration
    // ones (the gate's deserialisers), since in-process calls need no wire codec.
    let (boundary_names, boundary_insts) = emit_boundary_helpers(&mut out, commons, ctx);
    // v0.22b: module-local codec helpers for this file's Json.encode/decode
    // targets, deduped against the workers boundary helpers above.
    emit_json_codec_helpers(&mut out, commons, ctx, &boundary_names, &boundary_insts);
    // The generated `file` name: the source basename with `.bynk` → `.ts`.
    let generated_file = Path::new(source_name)
        .file_stem()
        .map(|s| format!("{}.ts", s.to_string_lossy()))
        .unwrap_or_else(|| "module.ts".to_string());
    let source_map = smb.borrow().to_v3(&out, &generated_file);
    // v0.110 (ADR 0142): import the `Bytes` runtime helpers iff the emitted body
    // actually references them. Injected into the existing runtime import line
    // (no new line, no body-column shift), so the source map computed above from
    // the pre-injection text stays valid.
    if out.contains("__bynkBytes") {
        out = inject_bytes_runtime_imports(
            out,
            &runtime_import_for(&ctx.source_path, ctx.import_ext),
        );
    }
    (out, source_map)
}

/// v0.110 (ADR 0142): append the `Bytes` runtime helpers to a module's existing
/// runtime import. Done as a post-pass so the decision keys on what the body
/// references, without a second emission or a source-map-shifting reorder.
///
/// v0.176 (#642): anchored on the runtime import's **exact specifier** rather
/// than on the `type ValidationError` binding it happens to carry. With `Bytes`
/// now able to cross a workers boundary (ADR 0142 D8's guard retired), the
/// *Worker entry* references `__bynkBytesFromBase64` too — and its import line
/// names no `ValidationError`, so the old anchor silently failed to inject and
/// `tsc` reported an unresolved name.
///
/// The specifier is matched exactly (`from "<specifier>"`), not by substring: a
/// `contains("runtime.js")` would also match a *user* module that happens to be
/// named `runtime` — or anything like `"./my-runtime.js"` — and appending the
/// `__bynkBytes*` bindings to that import would produce an unresolved export.
/// The caller already knows the exact path it emitted, so there is no reason to
/// guess.
pub(crate) fn inject_bytes_runtime_imports(out: String, runtime_specifier: &str) -> String {
    let mut result = String::with_capacity(out.len() + BYTES_RUNTIME_IMPORTS.len());
    let mut injected = false;
    let from_runtime = format!(" }} from \"{runtime_specifier}\"");
    for line in out.split_inclusive('\n') {
        if !injected
            && line.starts_with("import {")
            && line.contains(&from_runtime)
            && let Some(pos) = line.rfind(&from_runtime)
        {
            result.push_str(&line[..pos]);
            result.push_str(BYTES_RUNTIME_IMPORTS);
            result.push_str(&line[pos..]);
            injected = true;
            continue;
        }
        result.push_str(line);
    }
    result
}

/// v0.22b: pre-order expression visitor — visits `e`, then every
/// sub-expression, including statements and tails of nested blocks.
pub(crate) fn walk_exprs(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match &e.kind {
        ExprKind::IntLit { .. }
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::Ident(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        // v0.43: visit each interpolation hole's expression.
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(hole) = part {
                    walk_exprs(hole, f);
                }
            }
        }
        ExprKind::Lambda(l) => walk_exprs(&l.body, f),
        ExprKind::EffectPure(i)
        | ExprKind::Expect(i)
        | ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Wire(i)
        | ExprKind::Question(i) => walk_exprs(i, f),
        ExprKind::Val { args, .. }
        | ExprKind::Call { args, .. }
        | ExprKind::ConstructorCall { args, .. } => {
            for a in args {
                walk_exprs(a, f);
            }
        }
        ExprKind::ListLit(elems) => {
            for el in elems {
                walk_exprs(el, f);
            }
        }
        // v0.117: observations visit their optional count/with predicate; a
        // `trace(Cap.op)` has no sub-expressions.
        ExprKind::Observation(o) => {
            if let ObservationMatcher::Called { count, with_pred } = &o.matcher {
                if let Some(c) = count {
                    walk_exprs(c, f);
                }
                if let Some(p) = with_pred {
                    walk_exprs(p, f);
                }
            }
        }
        ExprKind::Trace { .. } => {}
        ExprKind::RecordConstruction { fields, .. } => {
            for fld in fields {
                if let Some(v) = &fld.value {
                    walk_exprs(v, f);
                }
            }
        }
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_exprs(base, f);
            for fld in overrides {
                if let Some(v) = &fld.value {
                    walk_exprs(v, f);
                }
            }
        }
        ExprKind::BinOp(_, l, r) => {
            walk_exprs(l, f);
            walk_exprs(r, f);
        }
        ExprKind::Block(b) => walk_block_exprs(b, f),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_exprs(cond, f);
            walk_block_exprs(then_block, f);
            walk_block_exprs(else_block, f);
        }
        ExprKind::FieldAccess { receiver, .. } => walk_exprs(receiver, f),
        ExprKind::MethodCall { receiver, args, .. } => {
            walk_exprs(receiver, f);
            for a in args {
                walk_exprs(a, f);
            }
        }
        ExprKind::Match { discriminant, arms } => {
            walk_exprs(discriminant, f);
            for arm in arms {
                match &arm.body {
                    MatchBody::Expr(e) => walk_exprs(e, f),
                    MatchBody::Block(b) => walk_block_exprs(b, f),
                }
            }
        }
        ExprKind::Is { value, .. } => walk_exprs(value, f),
    }
}

/// v0.79: does this block contain a `~>` send anywhere — including nested
/// branches, match arms, and lambdas? Gates execution-context threading
/// (`deps.__exec`) so a context that never sends keeps byte-identical output.
pub(crate) fn block_uses_send(b: &Block) -> bool {
    fn stmt(s: &Statement) -> bool {
        match s {
            Statement::Send(_) => true,
            Statement::Let(l) | Statement::EffectLet(l) => expr(&l.value),
            Statement::Expect(a) => expr(&a.value),
            Statement::Do(d) => expr(&d.value),
            Statement::Assign(a) => expr(&a.value),
        }
    }
    fn expr(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Block(b) => block_uses_send(b),
            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => expr(cond) || block_uses_send(then_block) || block_uses_send(else_block),
            ExprKind::Match { discriminant, arms } => {
                expr(discriminant)
                    || arms.iter().any(|a| match &a.body {
                        MatchBody::Expr(e) => expr(e),
                        MatchBody::Block(b) => block_uses_send(b),
                    })
            }
            ExprKind::Lambda(l) => expr(&l.body),
            _ => false,
        }
    }
    b.statements.iter().any(stmt) || expr(&b.tail)
}

/// v0.81–v0.87: does this block write durable state — a `:=` `Cell` write, a
/// mutating storage-`Map`/`Cache` op (`put`/`remove`/`update`/`upsert`), or a
/// The names of an agent's `store` fields grouped by kind:
/// `(maps, sets, caches, logs, cells)`. Threaded through the write-detection
/// walk so each kind's mutating ops can be recognised.
type StoreKinds<'a> = (
    &'a HashSet<String>,
    &'a HashSet<String>,
    &'a HashSet<String>,
    &'a HashSet<String>,
    &'a HashSet<String>,
);

/// mutating `Set` op (`add`/`remove`) on a `store` field — anywhere, including
/// nested `if`/`match`/block expressions? Drives whether a store-agent handler
/// needs the implicit-commit wrapper (read-only handlers skip it). The kinds are
/// `(maps, sets, caches, logs, cells)`; all empty for a read-only agent.
pub(crate) fn block_writes_state(b: &Block, m: StoreKinds<'_>) -> bool {
    fn mutating_op(e: &Expr, (maps, sets, caches, logs, cells): StoreKinds<'_>) -> bool {
        if let ExprKind::MethodCall {
            receiver, method, ..
        } = &e.kind
            && let ExprKind::Ident(id) = &receiver.kind
        {
            if (maps.contains(&id.name) || caches.contains(&id.name))
                && matches!(method.name.as_str(), "put" | "remove" | "update" | "upsert")
            {
                return true;
            }
            if sets.contains(&id.name) && matches!(method.name.as_str(), "add" | "remove") {
                return true;
            }
            // v0.95: `Log.append` mutates the durable array.
            if logs.contains(&id.name) && method.name == "append" {
                return true;
            }
            // v0.98 (ADR 0125): `Cell.update` is a read-modify-write of the
            // working state, so a handler whose only mutation is `cell.update`
            // still needs the end-of-handler commit flush.
            if cells.contains(&id.name) && method.name == "update" {
                return true;
            }
        }
        false
    }
    fn stmt(s: &Statement, m: StoreKinds<'_>) -> bool {
        match s {
            Statement::Assign(_) => true,
            Statement::Let(l) | Statement::EffectLet(l) => expr(&l.value, m),
            Statement::Expect(a) => expr(&a.value, m),
            Statement::Send(s) => expr(&s.value, m),
            Statement::Do(d) => expr(&d.value, m),
        }
    }
    fn expr(e: &Expr, m: StoreKinds<'_>) -> bool {
        if mutating_op(e, m) {
            return true;
        }
        match &e.kind {
            ExprKind::Block(b) => block_writes_state(b, m),
            ExprKind::If {
                cond,
                then_block,
                else_block,
            } => {
                expr(cond, m)
                    || block_writes_state(then_block, m)
                    || block_writes_state(else_block, m)
            }
            ExprKind::Match { discriminant, arms } => {
                expr(discriminant, m)
                    || arms.iter().any(|a| match &a.body {
                        MatchBody::Expr(e) => expr(e, m),
                        MatchBody::Block(b) => block_writes_state(b, m),
                    })
            }
            ExprKind::Paren(inner) => expr(inner, m),
            ExprKind::MethodCall { receiver, args, .. } => {
                expr(receiver, m) || args.iter().any(|x| expr(x, m))
            }
            ExprKind::Call { args, .. } => args.iter().any(|x| expr(x, m)),
            _ => false,
        }
    }
    b.statements.iter().any(|s| stmt(s, m)) || expr(&b.tail, m)
}

pub(crate) fn walk_block_exprs(b: &Block, f: &mut impl FnMut(&Expr)) {
    for s in &b.statements {
        match s {
            Statement::Let(l) | Statement::EffectLet(l) => walk_exprs(&l.value, f),
            Statement::Expect(a) => walk_exprs(&a.value, f),
            Statement::Send(s) => walk_exprs(&s.value, f),
            Statement::Do(d) => walk_exprs(&d.value, f),
            Statement::Assign(a) => walk_exprs(&a.value, f),
        }
    }
    walk_exprs(&b.tail, f);
}

/// v0.22b: whether any signature or type declaration in this file names
/// `JsonError` — drives the conditional `type JsonError` runtime import.
fn file_mentions_json_error(commons: &TypedCommons) -> bool {
    fn in_type_ref(t: &TypeRef) -> bool {
        match t {
            TypeRef::JsonError(_) => true,
            TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => in_type_ref(a) || in_type_ref(b),
            TypeRef::Option(a, _)
            | TypeRef::Effect(a, _)
            | TypeRef::HttpResult(a, _)
            | TypeRef::Query(a, _)
            | TypeRef::Stream(a, _)
            | TypeRef::Connection(a, _)
            | TypeRef::History(a, _)
            | TypeRef::List(a, _) => in_type_ref(a),
            TypeRef::Fn(params, ret, _) => params.iter().any(in_type_ref) || in_type_ref(ret),
            // v0.157 (ADR 0183): recurse into a generic application's arguments.
            TypeRef::App { args, .. } => args.iter().any(in_type_ref),
            TypeRef::Base(..)
            | TypeRef::Named(_)
            | TypeRef::QueueResult(_)
            | TypeRef::ValidationError(_)
            | TypeRef::Unit(_) => false,
        }
    }
    let sig = |params: &[Param], ret: &TypeRef| {
        params.iter().any(|p| in_type_ref(&p.type_ref)) || in_type_ref(ret)
    };
    commons.commons.items.iter().any(|item| match item {
        CommonsItem::Fn(f) => sig(&f.params, &f.return_type),
        CommonsItem::Service(s) => s.handlers.iter().any(|h| sig(&h.params, &h.return_type)),
        CommonsItem::Agent(a) => a.handlers.iter().any(|h| sig(&h.params, &h.return_type)),
        CommonsItem::Provider(p) => p.ops.iter().any(|op| sig(&op.params, &op.return_type)),
        CommonsItem::Type(t) => match &t.body {
            TypeBody::Record(r) => r.fields.iter().any(|f| in_type_ref(&f.type_ref)),
            TypeBody::Sum(s) => s
                .variants
                .iter()
                .any(|v| v.payload.iter().any(|p| in_type_ref(&p.type_ref))),
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => false,
        },
        _ => false,
    })
}

/// v0.153 (ADR 0177): true if any signature or type declaration in this file
/// names `HttpResult` — a service HTTP handler, or a free `fn` / provider /
/// capability whose parameter or return type mentions it (the `?`-Option lift
/// makes a bare `fn -> HttpResult[T]` emit `HttpResult.NotFound`). Drives the
/// conditional `HttpResult` runtime import in both single-file and project
/// headers, so the import can never be missing nor spuriously added.
fn file_mentions_http_result(commons: &TypedCommons) -> bool {
    fn in_type_ref(t: &TypeRef) -> bool {
        match t {
            TypeRef::HttpResult(..) => true,
            TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => in_type_ref(a) || in_type_ref(b),
            TypeRef::Option(a, _)
            | TypeRef::Effect(a, _)
            | TypeRef::Query(a, _)
            | TypeRef::Stream(a, _)
            | TypeRef::Connection(a, _)
            | TypeRef::History(a, _)
            | TypeRef::List(a, _) => in_type_ref(a),
            TypeRef::Fn(params, ret, _) => params.iter().any(in_type_ref) || in_type_ref(ret),
            _ => false,
        }
    }
    let sig = |params: &[Param], ret: &TypeRef| {
        params.iter().any(|p| in_type_ref(&p.type_ref)) || in_type_ref(ret)
    };
    commons.commons.items.iter().any(|item| match item {
        CommonsItem::Fn(f) => sig(&f.params, &f.return_type),
        CommonsItem::Service(s) => s.handlers.iter().any(|h| sig(&h.params, &h.return_type)),
        CommonsItem::Agent(a) => a.handlers.iter().any(|h| sig(&h.params, &h.return_type)),
        CommonsItem::Capability(c) => c.ops.iter().any(|op| sig(&op.params, &op.return_type)),
        CommonsItem::Provider(p) => p.ops.iter().any(|op| sig(&op.params, &op.return_type)),
        CommonsItem::Type(t) => match &t.body {
            TypeBody::Record(r) => r.fields.iter().any(|f| in_type_ref(&f.type_ref)),
            TypeBody::Sum(s) => s
                .variants
                .iter()
                .any(|v| v.payload.iter().any(|p| in_type_ref(&p.type_ref))),
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => false,
        },
        _ => false,
    })
}

/// v0.102: true if a file's signatures or store fields mention `Connection[F]`,
/// so the header imports the runtime `Connection` interface. Covers the held
/// sites: capability-operation returns, service/agent handler parameters, and
/// `store` field value types (`Map[K, Connection]` / `Cell[Option[Connection]]`).
fn file_mentions_connection(commons: &TypedCommons) -> bool {
    fn in_type_ref(t: &TypeRef) -> bool {
        match t {
            TypeRef::Connection(..) => true,
            TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => in_type_ref(a) || in_type_ref(b),
            TypeRef::Option(a, _)
            | TypeRef::Effect(a, _)
            | TypeRef::HttpResult(a, _)
            | TypeRef::Query(a, _)
            | TypeRef::Stream(a, _)
            | TypeRef::List(a, _) => in_type_ref(a),
            TypeRef::Fn(params, ret, _) => params.iter().any(in_type_ref) || in_type_ref(ret),
            _ => false,
        }
    }
    let sig = |params: &[Param], ret: &TypeRef| {
        params.iter().any(|p| in_type_ref(&p.type_ref)) || in_type_ref(ret)
    };
    commons.commons.items.iter().any(|item| match item {
        CommonsItem::Fn(f) => sig(&f.params, &f.return_type),
        CommonsItem::Service(s) => s.handlers.iter().any(|h| sig(&h.params, &h.return_type)),
        CommonsItem::Agent(a) => {
            a.handlers.iter().any(|h| sig(&h.params, &h.return_type))
                || a.store_fields
                    .iter()
                    .any(|f| f.kind.args.iter().any(in_type_ref))
        }
        CommonsItem::Capability(c) => c.ops.iter().any(|op| sig(&op.params, &op.return_type)),
        CommonsItem::Provider(p) => p.ops.iter().any(|op| sig(&op.params, &op.return_type)),
        _ => false,
    })
}

/// v0.22b: a checker `Ty` rendered back to a `TypeRef` for the codec
/// machinery (which is `TypeRef`-driven). `None` for types the codec
/// rejects anyway (functions, effects, type variables).
fn ty_to_type_ref(t: &Ty) -> Option<TypeRef> {
    let sp = bynk_syntax::span::Span::new(0, 0);
    Some(match t {
        Ty::Base(b) => TypeRef::Base(*b, sp),
        // v0.174 (#592): a generic-record instantiation (`Paginated[User]`,
        // `args` non-empty) round-trips as a `TypeRef::App` so the codec closure
        // reaches its monomorphised helper; a non-generic named type stays a
        // bare `Named`.
        Ty::Named { name, args, .. } if !args.is_empty() => TypeRef::App {
            name: Ident {
                name: name.clone(),
                span: sp,
            },
            args: args
                .iter()
                .map(ty_to_type_ref)
                .collect::<Option<Vec<_>>>()?,
            span: sp,
        },
        Ty::Named { name, .. } => TypeRef::Named(Ident {
            name: name.clone(),
            span: sp,
        }),
        Ty::Result(a, b) => TypeRef::Result(
            Box::new(ty_to_type_ref(a)?),
            Box::new(ty_to_type_ref(b)?),
            sp,
        ),
        Ty::Option(a) => TypeRef::Option(Box::new(ty_to_type_ref(a)?), sp),
        Ty::List(a) => TypeRef::List(Box::new(ty_to_type_ref(a)?), sp),
        Ty::Map(k, v) => TypeRef::Map(
            Box::new(ty_to_type_ref(k)?),
            Box::new(ty_to_type_ref(v)?),
            sp,
        ),
        Ty::Unit => TypeRef::Unit(sp),
        Ty::ValidationError => TypeRef::ValidationError(sp),
        Ty::JsonError => TypeRef::JsonError(sp),
        Ty::Effect(_)
        | Ty::Query(_)
        | Ty::Stream(_)
        | Ty::Connection(_)
        | Ty::HttpResult(_)
        | Ty::QueueResult
        | Ty::Fn { .. }
        | Ty::Var(_)
        | Ty::Actor(_)
        | Ty::ActorSum(_) => {
            return None;
        }
    })
}

/// v0.22b: collect the `Json.encode`/`Json.decode[T]` target type-refs in
/// this file's bodies — the roots of the module-local codec-helper closure.
fn collect_json_codec_roots(commons: &TypedCommons) -> Vec<TypeRef> {
    let mut roots: Vec<TypeRef> = Vec::new();
    {
        let mut visit = |e: &Expr| {
            let ExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } = &e.kind
            else {
                return;
            };
            let ExprKind::Ident(id) = &receiver.kind else {
                return;
            };
            if id.name != JSON {
                return;
            }
            match method.name.as_str() {
                "decode" => {
                    if let Some(Ty::Result(t, _)) = commons.expr_types.get(&e.span)
                        && let Some(tr) = ty_to_type_ref(t)
                    {
                        roots.push(tr);
                    }
                }
                "encode" => {
                    if let Some(a) = args.first()
                        && let Some(t) = commons.expr_types.get(&a.span)
                        && let Some(tr) = ty_to_type_ref(t)
                    {
                        roots.push(tr);
                    }
                }
                _ => {}
            }
        };
        for item in &commons.commons.items {
            match item {
                CommonsItem::Fn(f) => walk_block_exprs(&f.body, &mut visit),
                CommonsItem::Service(s) => {
                    for h in &s.handlers {
                        walk_block_exprs(&h.body, &mut visit);
                    }
                }
                CommonsItem::Agent(a) => {
                    for h in &a.handlers {
                        walk_block_exprs(&h.body, &mut visit);
                    }
                }
                CommonsItem::Provider(p) => {
                    for op in &p.ops {
                        walk_block_exprs(&op.body, &mut visit);
                    }
                }
                _ => {}
            }
        }
    }
    roots
}

/// v0.22b: module-local serialise/deserialise helpers for the types this
/// file's `Json.encode`/`Json.decode[T]` calls reference (ADR 0045). The
/// closure machinery is shared with the workers boundary path; `skip_names`
/// / `skip_insts` dedupe against helpers that path already emitted into
/// this module.
fn emit_json_codec_helpers(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    skip_names: &HashSet<String>,
    skip_insts: &HashSet<String>,
) {
    use serialisation::{collect_codec_closure, emit_generic_helpers, emit_helpers_for_owner};
    let roots = collect_json_codec_roots(commons);
    if roots.is_empty() {
        return;
    }
    let (names, insts) = collect_codec_closure(&roots, &commons.types);
    let names: Vec<String> = names
        .into_iter()
        .filter(|n| !skip_names.contains(n))
        .collect();
    emit_helpers_for_owner(out, &names, &commons.types, &ctx.commons_name);
    let insts: Vec<serialisation::GenericInst> = insts
        .into_iter()
        .filter(|i| !skip_insts.contains(&i.ts_name()))
        .collect();
    if !insts.is_empty() {
        emit_generic_helpers(out, &insts, &commons.types);
    }
}

/// Emit boundary serialise/deserialise helpers (v0.8 §3.4 / §5.2) for
/// every named type declared in this file that flows through a
/// cross-context call, plus the specialised generic helpers for any
/// Result/Option instantiation used at the boundary. Returns the emitted
/// (or locally-bound) helper type names and generic-instantiation names so
/// the v0.22b codec emission can dedupe against them.
fn emit_boundary_helpers(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) -> (HashSet<String>, HashSet<String>) {
    use serialisation::{
        collect_boundary_types, collect_generic_instantiations, emit_generic_helpers,
        emit_helpers_for_owner,
    };

    // For contexts: walk the local services to discover boundary types.
    // For commons: walk every consumer's services that reference us
    // (approximated as: emit for every type declared in this file).
    //
    // Service handler types cross the *cross-Worker call* boundary, which only
    // exists on the `workers` target; on `bundle` calls are in-process, so their
    // serialise/deserialise helpers are not emitted. The agent **rehydration**
    // boundary (ADR 0124), in contrast, exists on both targets, so agent
    // store-field types are always collected (below).
    let workers = matches!(ctx.target, BuildTarget::Workers);
    let services: HashMap<String, ServiceDecl> = if workers {
        commons
            .commons
            .items
            .iter()
            .filter_map(|i| match i {
                CommonsItem::Service(s) => Some((s.name.name.clone(), s.clone())),
                _ => None,
            })
            .collect()
    } else {
        HashMap::new()
    };

    // v0.96 (ADR 0124): an agent's `store`-field types are rehydration-boundary
    // types — their deserialisers drive the load-time validation gate.
    let agents: HashMap<String, AgentDecl> = commons
        .commons
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Agent(a) => Some((a.name.name.clone(), a.clone())),
            _ => None,
        })
        .collect();

    let locally_declared: HashSet<String> = ctx.file_decl_index.types.keys().cloned().collect();
    if ctx.unit_kind == UnitKind::Context {
        let boundary_types_all = collect_boundary_types(&commons.types, &services, &agents);
        // Locally-declared boundary types get full helpers in this module. On
        // `bundle` (v0.96, ADR 0124) the commons modules emit no boundary helpers,
        // so a cross-commons *agent-state* type's deserialiser — needed by the
        // rehydration gate — is emitted here in the context instead of re-exported.
        let local_boundary: Vec<String> = boundary_types_all
            .iter()
            .filter(|n| !workers || locally_declared.contains(*n))
            .cloned()
            .collect();
        emit_helpers_for_owner(
            out,
            &local_boundary,
            &commons.types,
            ctx.commons_name.as_str(),
        );

        // Re-export helpers for commons-owned boundary types so consumers
        // can address them through this context's handlers.ts namespace
        // (matching the namespace import they already use for cross-
        // context types). Grouped by source commons. Workers only — on `bundle`
        // the commons emit no helpers, so cross-commons types are emitted
        // locally above (v0.96) rather than imported.
        let mut by_commons: HashMap<String, Vec<String>> = HashMap::new();
        for n in &boundary_types_all {
            if !workers || locally_declared.contains(n) {
                continue;
            }
            if matches!(ctx.imported_from_kind.get(n), Some(UnitKind::Commons))
                && let Some(commons_name) = ctx.imported_from.get(n)
            {
                by_commons
                    .entry(commons_name.clone())
                    .or_default()
                    .push(n.clone());
            }
        }
        let mut commons_keys: Vec<&String> = by_commons.keys().collect();
        commons_keys.sort();
        for commons_name in commons_keys {
            let names = by_commons.get(commons_name).unwrap();
            let mut sorted_names: Vec<String> = names.clone();
            sorted_names.sort();
            sorted_names.dedup();
            let target_path = ctx
                .imported_decl_paths
                .get(commons_name)
                .and_then(|m| sorted_names.iter().find_map(|n| m.get(n).cloned()))
                .unwrap_or_else(|| EmitProjectCtx::commons_path(commons_name));
            let import_spec = cross_commons_import_specifier_for_path(
                &ctx.source_path,
                &target_path,
                ctx.import_ext,
            );
            let mut parts: Vec<String> = Vec::new();
            for n in &sorted_names {
                parts.push(format!("serialise_{n}"));
                parts.push(format!("deserialise_{n}"));
            }
            // v0.9.1: emit both a regular import (so the names are bound
            // locally for use inside this file's serialisation helpers) and a
            // re-export (so downstream consumers can still reach them
            // through this module). A bare `export { ... } from "..."`
            // re-export does not create a local binding, which `tsc --strict`
            // catches when the body calls one of the helpers directly.
            writeln!(
                out,
                "import {{ {} }} from \"{import_spec}\";",
                parts.join(", ")
            )
            .unwrap();
            writeln!(out, "export {{ {} }};", parts.join(", ")).unwrap();
        }
        if !by_commons.is_empty() {
            writeln!(out).unwrap();
        }

        // Specialised Result_/Option_ helpers for the instantiations used —
        // in handler signatures or in boundary-type fields (v0.18).
        let insts =
            collect_generic_instantiations(&services, &agents, &boundary_types_all, &commons.types);
        emit_generic_helpers(out, &insts, &commons.types);

        // #661 (ADR 0199 Decision G discharged): the caller's own view of each
        // consumed context's boundary codecs, so a cross-context call reaches
        // `deserialise_Result_AuthId_PaymentError` **locally** instead of through
        // the callee's module. Workers only — on `bundle` the call is in-process
        // and needs no wire codec. Everything the caller already emits (its own
        // boundary types, the commons re-exports above, its own generic
        // instantiations) is skipped, so only the callee-*owned* types the caller
        // lacks a local view of are generated.
        let (consumed_names, consumed_insts) = if workers {
            let mut emitted_names: HashSet<String> = local_boundary.iter().cloned().collect();
            for names in by_commons.values() {
                emitted_names.extend(names.iter().cloned());
            }
            let mut emitted_insts: HashSet<String> = insts.iter().map(|i| i.ts_name()).collect();
            emit_consumed_context_helpers(out, commons, ctx, &mut emitted_names, &mut emitted_insts)
        } else {
            (Vec::new(), Vec::new())
        };

        let mut ret_names: HashSet<String> = boundary_types_all.into_iter().collect();
        ret_names.extend(consumed_names);
        let mut ret_insts: HashSet<String> = insts.iter().map(|i| i.ts_name()).collect();
        ret_insts.extend(consumed_insts);
        (ret_names, ret_insts)
    } else if !workers {
        // Commons/adapters have no agents (no rehydration boundary), and on
        // `bundle` there is no cross-Worker call boundary either — so emit no
        // boundary helpers, matching pre-v0.96 bundle output (the rehydration
        // pass that now always runs is for context-declared agents only).
        (HashSet::new(), HashSet::new())
    } else {
        // Commons/adapters (workers): emit helpers for every type declared in
        // this file, plus (v0.18) the generic instantiations their fields use —
        // a record like the bynk surface's `Request` carries
        // `Option[String]` fields whose serialisers delegate to the
        // specialised helpers.
        //
        // v0.132 (#479): scope to types declared in *this* file, not the whole
        // unit. `file_decl_index` is unit-wide (name -> declaring-file path), so a
        // multi-file commons must filter by the current file — otherwise a
        // non-declaring sibling (e.g. the file holding `fn T.make`, not `type T`)
        // emits an orphan `serialise_T`/`deserialise_T` with `T` out of scope, and
        // the codec is duplicated across files. Unlike a workers *context* (which
        // collapses to one `handlers.ts` under a synthetic source_path, so its
        // unit-wide `locally_declared` above is correct), a commons emits per file.
        let mut locally: Vec<String> = ctx
            .file_decl_index
            .types
            .iter()
            .filter(|(_, path)| path.as_path() == ctx.source_path.as_path())
            .map(|(name, _)| name.clone())
            .collect();
        locally.sort();
        emit_helpers_for_owner(out, &locally, &commons.types, ctx.commons_name.as_str());
        let insts = collect_generic_instantiations(
            &HashMap::new(),
            &HashMap::new(),
            &locally,
            &commons.types,
        );
        emit_generic_helpers(out, &insts, &commons.types);
        (
            locally.into_iter().collect(),
            insts.iter().map(|i| i.ts_name()).collect(),
        )
    }
}

/// #661: emit the caller's own `serialise_*`/`deserialise_*` for every
/// callee-owned boundary type reachable from the services this context
/// **calls**, so a `workers` cross-context call resolves its codecs locally
/// rather than importing the callee's module as a value.
///
/// The codec function names stay bare and local; only the TS *type* positions
/// reach through the callee's `import type * as <ns>` alias (via the `Qual`
/// map built here). Refinement validation follows the export visibility: an
/// opaque type casts structurally (Decision C), a transparent refined type
/// inlines its predicates (Decision D) — both decided inside the codec emitter
/// from the type's own body.
///
/// Only the callee-*owned* types (its `exports`) are generated. Commons types
/// reachable through the boundary (`Money`) are already emitted or re-exported
/// by the caller's own path, so they are left out here and deduped against
/// `emitted_names` / `emitted_insts`, which the caller seeds with everything it
/// has already emitted. Returns the names and generic-instantiation names newly
/// emitted, so the Json-codec pass dedupes against them too.
fn emit_consumed_context_helpers(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    emitted_names: &mut HashSet<String>,
    emitted_insts: &mut HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    use serialisation::{
        collect_codec_closure, emit_generic_helpers_qualified, emit_helpers_for_owner_qualified,
    };
    let info = &ctx.cross_context;
    let mut consumed_names_out: Vec<String> = Vec::new();
    let mut consumed_insts_out: Vec<String> = Vec::new();

    // Only the services this context actually **calls** — not the callee's whole
    // provided surface. `consumed_services` carries every service the dependency
    // provides; generating a codec for one this context never reaches would bloat
    // the bundle with a contract it does not participate in (and pull in the
    // uncalled service's own boundary types). Mirrors the `called` narrowing the
    // contract manifest applies to `expects` (ADR 0200 Decision E, one layer up).
    let called = called_consumed_services(commons, info);

    let mut consumed_keys: Vec<&String> = info.consumed_services.keys().collect();
    consumed_keys.sort();
    for c in consumed_keys {
        let svcs = &info.consumed_services[c];
        if svcs.is_empty() {
            continue;
        }
        let Some(called_here) = called.get(c) else {
            continue;
        };
        let Some(types_table) = info.consumed_types.get(c) else {
            continue;
        };
        // The callee's exports — the set of types it *owns* and a consumer may
        // name. A closure type outside this set (a commons type the callee only
        // `uses`, e.g. `Money`) is the caller's own already, not the callee's to
        // hand out, so the caller never regenerates it under the callee's ns.
        let exports = ctx.exports_for_consumed.get(c);
        let owned = |n: &str| exports.is_some_and(|e| e.contains_key(n));

        // Roots: every called service's parameter and return types, in a stable
        // order.
        let mut svc_names: Vec<&String> =
            svcs.keys().filter(|s| called_here.contains(*s)).collect();
        svc_names.sort();
        let mut roots: Vec<bynk_syntax::ast::TypeRef> = Vec::new();
        for sn in svc_names {
            let svc = &svcs[sn];
            for (_, t) in &svc.params {
                roots.push(t.clone());
            }
            roots.push(svc.return_type.clone());
        }
        let (names, cinsts) = collect_codec_closure(&roots, types_table);

        let ns = format!("{}.", qualified_to_ns(c));
        let mut qual: HashMap<String, String> = HashMap::new();
        for n in &names {
            if owned(n) {
                qual.insert(n.clone(), ns.clone());
            }
        }

        let mut to_emit: Vec<String> = names
            .iter()
            .filter(|n| owned(n) && emitted_names.insert((*n).clone()))
            .cloned()
            .collect();
        to_emit.sort();
        emit_helpers_for_owner_qualified(
            out,
            &to_emit,
            types_table,
            ctx.commons_name.as_str(),
            &qual,
        );
        consumed_names_out.extend(to_emit);

        let to_emit_insts: Vec<serialisation::GenericInst> = cinsts
            .into_iter()
            .filter(|i| emitted_insts.insert(i.ts_name()))
            .collect();
        for i in &to_emit_insts {
            consumed_insts_out.push(i.ts_name());
        }
        emit_generic_helpers_qualified(out, &to_emit_insts, types_table, &qual);
    }

    (consumed_names_out, consumed_insts_out)
}

/// #661: the cross-context services this unit actually **calls**, as `consumed
/// context → service names`. A copy of `project::called_cross_context_services`
/// over the emitter's AST view (`commons`) — the caller-side codec set follows
/// the *called* subset, not the callee's full provided surface, so it stays in
/// step with what the contract manifest records under `expects`.
fn called_consumed_services(
    commons: &TypedCommons,
    info: &bynk_check::resolver::CrossContextInfo,
) -> HashMap<String, HashSet<String>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    if info.consumed_contexts.is_empty() && info.aliases.is_empty() {
        return out;
    }
    let mut visit = |e: &Expr| {
        if let ExprKind::MethodCall {
            receiver, method, ..
        } = &e.kind
            && let Some(chain) = flatten_emit_ident_chain(receiver)
            && let Some(target) = info.resolve_prefix(&chain)
        {
            out.entry(target).or_default().insert(method.name.clone());
        }
    };
    for item in &commons.commons.items {
        match item {
            CommonsItem::Service(s) => {
                for h in &s.handlers {
                    walk_block_exprs(&h.body, &mut visit);
                }
            }
            CommonsItem::Agent(a) => {
                for h in &a.handlers {
                    walk_block_exprs(&h.body, &mut visit);
                }
            }
            CommonsItem::Provider(p) => {
                for op in &p.ops {
                    walk_block_exprs(&op.body, &mut visit);
                }
            }
            _ => {}
        }
    }
    out
}

/// For each type imported via `uses` that's referenced in this file, emit:
/// 1. (Done in imports) an aliased import: `import { Money as __CommonsMoney } from ...`
/// 2. A rebranded type alias: `export type Money = __CommonsMoney & { readonly __ctxBrand: "..." }`
///
/// The brand makes two contexts that both `uses` the same commons see distinct
/// nominal `Money` types in their TypeScript output (v0.4 §3.4 / §6.2).
fn emit_context_rebrands(
    out: &mut String,
    refs: &ExternalReferences,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) {
    let Some(owning) = &ctx.owning_context else {
        return;
    };
    // Collect names imported via `uses` (kind == Commons in imported_from_kind).
    let mut names: Vec<String> = Vec::new();
    for set in refs.by_commons.values() {
        for n in set {
            // v0.20b: only *types* get the context rebrand — a
            // `uses`-imported function is a value and imports plainly.
            if matches!(ctx.imported_from_kind.get(n), Some(UnitKind::Commons))
                && commons.types.contains_key(n)
            {
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
        // v0.174 (#592): a generic commons type keeps its parameters across the
        // rebrand — `Paginated[T]` aliases as `Paginated<T> =
        // __CommonsPaginated<T> & { … }`, not a bare `Paginated`, which would
        // both drop the parameter and make every `Paginated<User>` reference in
        // the context a "type is not generic" error.
        let params: Vec<&str> = commons
            .types
            .get(name)
            .map(|d| d.type_params.iter().map(|p| p.name.name.as_str()).collect())
            .unwrap_or_default();
        let generics = if params.is_empty() {
            String::new()
        } else {
            format!("<{}>", params.join(", "))
        };
        writeln!(
            out,
            "export type {name}{generics} = __Commons{name}{generics} & {{ readonly __ctxBrand: \"{owning}\" }};",
        )
        .unwrap();
        // v0.9.2: a commons refined/opaque type carries a value-side
        // constructor (`.of`, and `.unsafe` for opaque). Re-export it under the
        // rebranded name so a context calling `ShortCode.of(...)` resolves to a
        // value — delegating to the imported commons constructor but reporting
        // the context-branded type. (Without this, `ShortCode` is type-only in
        // the context and `.of` fails to resolve.)
        if let Some(base) = commons.types.get(name).and_then(refined_or_opaque_base) {
            let ts_base = ts_base(base);
            let is_opaque = matches!(
                commons.types.get(name).map(|d| &d.body),
                Some(TypeBody::Opaque { .. })
            );
            writeln!(out, "export const {name} = {{").unwrap();
            writeln!(
                out,
                "  of(value: {ts_base}): Result<{name}, ValidationError> {{ return __Commons{name}.of(value) as unknown as Result<{name}, ValidationError>; }},",
            )
            .unwrap();
            // ADR 0182: only opaque types have a public `.unsafe` to forward.
            // A refined/alias type has none — a consuming context brands an
            // admitted literal with an inline `as` cast, not a forwarder call.
            if is_opaque {
                writeln!(
                    out,
                    "  unsafe(value: {ts_base}): {name} {{ return __Commons{name}.unsafe(value) as unknown as {name}; }},",
                )
                .unwrap();
            }
            // v0.132.1 (#481): forward the commons' user-defined attached methods
            // (`Cents.fromInt`, …) so the rebranded const carries more than the
            // built-in `of`/`unsafe`. Without this a consumer's `Cents.fromInt(n)`
            // — which `bynkc check` accepts — fails `tsc`. The methods aren't in
            // this context's own `commons` (only imported *types* are merged);
            // they arrive via `ctx.imported_methods`, keyed by type name.
            if let Some(methods) = ctx.imported_methods.get(name) {
                emit_forwarded_methods(out, name, methods);
            }
            writeln!(out, "}};").unwrap();
        }
    }
    writeln!(out).unwrap();
}

/// If a type declaration is a refined or opaque base type, return its base
/// (both lower to a branded base with a `.of` / `.unsafe` constructor object).
fn refined_or_opaque_base(decl: &TypeDecl) -> Option<BaseType> {
    match &decl.body {
        TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. } => Some(*base),
        _ => None,
    }
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
                collect_refs_in_fn(f, &local_to_file, commons, ctx, &mut refs);
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
                    collect_refs_in_block(&op.body, &local_to_file, commons, ctx, &mut refs);
                }
            }
            CommonsItem::Service(s) => {
                for h in &s.handlers {
                    for p in &h.params {
                        collect_refs_in_typeref(&p.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&h.return_type, &local_to_file, ctx, &mut refs);
                    collect_refs_in_block(&h.body, &local_to_file, commons, ctx, &mut refs);
                }
            }
            CommonsItem::Agent(a) => {
                collect_refs_in_typeref(&a.key_type, &local_to_file, ctx, &mut refs);
                for f in &a.store_fields {
                    for arg in &f.kind.args {
                        collect_refs_in_typeref(arg, &local_to_file, ctx, &mut refs);
                    }
                }
                for h in &a.handlers {
                    for p in &h.params {
                        collect_refs_in_typeref(&p.type_ref, &local_to_file, ctx, &mut refs);
                    }
                    collect_refs_in_typeref(&h.return_type, &local_to_file, ctx, &mut refs);
                    collect_refs_in_block(&h.body, &local_to_file, commons, ctx, &mut refs);
                }
            }
            CommonsItem::Actor(a) => {
                if let Some(id) = &a.identity {
                    collect_refs_in_typeref(id, &local_to_file, ctx, &mut refs);
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
    commons: &TypedCommons,
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
    collect_refs_in_block(&f.body, local_to_file, commons, ctx, out);
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
        // Exhaustive over the compound constructors (#527, the #507 disease):
        // the old `_ => {}` catch-all dropped `List[KindCount]` and friends,
        // so a name referenced only inside such a position was never
        // imported and the emitted module failed `tsc`.
        TypeRef::Option(t, _)
        | TypeRef::Effect(t, _)
        | TypeRef::HttpResult(t, _)
        | TypeRef::List(t, _)
        | TypeRef::Query(t, _)
        | TypeRef::Stream(t, _)
        | TypeRef::Connection(t, _)
        | TypeRef::History(t, _) => collect_refs_in_typeref(t, local_to_file, ctx, out),
        TypeRef::Map(k, v, _) => {
            collect_refs_in_typeref(k, local_to_file, ctx, out);
            collect_refs_in_typeref(v, local_to_file, ctx, out);
        }
        TypeRef::Fn(params, ret, _) => {
            for t in params {
                collect_refs_in_typeref(t, local_to_file, ctx, out);
            }
            collect_refs_in_typeref(ret, local_to_file, ctx, out);
        }
        // v0.157 (ADR 0183): a `Name[Arg, …]` application references the
        // generic type plus every argument — all must be imported.
        TypeRef::App { name, args, .. } => {
            record_name_ref(&name.name, local_to_file, ctx, out);
            for t in args {
                collect_refs_in_typeref(t, local_to_file, ctx, out);
            }
        }
        TypeRef::Base(..)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
    }
}

fn collect_refs_in_block(
    b: &Block,
    local_to_file: &HashSet<String>,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    for stmt in &b.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                if let Some(t) = &l.type_annot {
                    collect_refs_in_typeref(t, local_to_file, ctx, out);
                }
                collect_refs_in_expr(&l.value, local_to_file, commons, ctx, out);
            }
            Statement::Expect(a) => {
                collect_refs_in_expr(&a.value, local_to_file, commons, ctx, out);
            }
            Statement::Send(s) => {
                collect_refs_in_expr(&s.value, local_to_file, commons, ctx, out);
            }
            Statement::Do(d) => {
                collect_refs_in_expr(&d.value, local_to_file, commons, ctx, out);
            }
            Statement::Assign(a) => {
                collect_refs_in_expr(&a.value, local_to_file, commons, ctx, out);
            }
        }
    }
    collect_refs_in_expr(&b.tail, local_to_file, commons, ctx, out);
}

fn collect_refs_in_expr(
    e: &Expr,
    local_to_file: &HashSet<String>,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    out: &mut ExternalReferences,
) {
    match &e.kind {
        // A bare ident the checker typed as a sum is a nullary variant
        // constructor — the lowering qualifies it to `Type.Variant`, so the
        // owning type must be imported (v0.18: first hit by `Get` from the
        // consumed bynk surface's `Method`).
        ExprKind::Ident(id) => {
            if let Some(type_name) = sum_owner_of_variant(&id.name, e.span, commons) {
                record_name_ref(&type_name, local_to_file, ctx, out);
            }
        }
        ExprKind::IntLit { .. }
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        // v0.43: a hole's expression may reference imported names.
        ExprKind::Wire(inner) => collect_refs_in_expr(inner, local_to_file, commons, ctx, out),
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(hole) = part {
                    collect_refs_in_expr(hole, local_to_file, commons, ctx, out);
                }
            }
        }
        // v0.20a: a lambda — its annotated param types may reference
        // imported types; the body walks like any expression.
        ExprKind::Lambda(lambda) => {
            for p in &lambda.params {
                if let Some(tr) = &p.type_ref {
                    collect_refs_in_typeref(tr, local_to_file, ctx, out);
                }
            }
            collect_refs_in_expr(&lambda.body, local_to_file, commons, ctx, out);
        }
        ExprKind::EffectPure(inner) => {
            collect_refs_in_expr(inner, local_to_file, commons, ctx, out);
        }
        ExprKind::Expect(inner) => {
            collect_refs_in_expr(inner, local_to_file, commons, ctx, out);
        }
        ExprKind::Val { args, .. } => {
            for a in args {
                collect_refs_in_expr(a, local_to_file, commons, ctx, out);
            }
        }
        ExprKind::ListLit(elems) => {
            for el in elems {
                collect_refs_in_expr(el, local_to_file, commons, ctx, out);
            }
        }
        // v0.117: observation predicates may reference types/fns; `trace` does not.
        ExprKind::Observation(o) => {
            if let ObservationMatcher::Called { count, with_pred } = &o.matcher {
                if let Some(c) = count {
                    collect_refs_in_expr(c, local_to_file, commons, ctx, out);
                }
                if let Some(p) = with_pred {
                    collect_refs_in_expr(p, local_to_file, commons, ctx, out);
                }
            }
        }
        ExprKind::Trace { .. } => {}
        ExprKind::RecordSpread {
            type_name,
            base,
            overrides,
        } => {
            if let Some(tn) = type_name {
                record_name_ref(&tn.name, local_to_file, ctx, out);
            }
            collect_refs_in_expr(base, local_to_file, commons, ctx, out);
            for f in overrides {
                if let Some(v) = &f.value {
                    collect_refs_in_expr(v, local_to_file, commons, ctx, out);
                }
            }
        }
        ExprKind::Call { name, args, .. } => {
            record_name_ref(&name.name, local_to_file, ctx, out);
            // A payload-carrying bare variant call (`Won(prize)`) lowers to
            // `Type.Variant(…)` — import the owning sum type too.
            if let Some(type_name) = sum_owner_of_variant(&name.name, e.span, commons) {
                record_name_ref(&type_name, local_to_file, ctx, out);
            }
            // #527: a call to a commons-imported fn may lower with a rebrand
            // assertion naming its return type (`(decide(…) as Decision)`),
            // so the return type's names must be imported (and rebranded)
            // in step with the cast.
            if ctx.unit_kind == UnitKind::Context
                && ctx.imported_from_kind.get(&name.name) == Some(&UnitKind::Commons)
                && let Some(f) = commons.fns.get(&name.name)
            {
                collect_refs_in_typeref(&f.return_type, local_to_file, ctx, out);
            }
            for a in args {
                collect_refs_in_expr(a, local_to_file, commons, ctx, out);
            }
        }
        ExprKind::BinOp(_, l, r) => {
            collect_refs_in_expr(l, local_to_file, commons, ctx, out);
            collect_refs_in_expr(r, local_to_file, commons, ctx, out);
        }
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i) => collect_refs_in_expr(i, local_to_file, commons, ctx, out),
        ExprKind::Block(b) => collect_refs_in_block(b, local_to_file, commons, ctx, out),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_refs_in_expr(cond, local_to_file, commons, ctx, out);
            collect_refs_in_block(then_block, local_to_file, commons, ctx, out);
            collect_refs_in_block(else_block, local_to_file, commons, ctx, out);
        }
        ExprKind::ConstructorCall {
            type_name,
            method: _,
            args,
        } => {
            record_name_ref(&type_name.name, local_to_file, ctx, out);
            for a in args {
                collect_refs_in_expr(a, local_to_file, commons, ctx, out);
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            record_name_ref(&type_name.name, local_to_file, ctx, out);
            for f in fields {
                if let Some(v) = &f.value {
                    collect_refs_in_expr(v, local_to_file, commons, ctx, out);
                }
            }
        }
        ExprKind::FieldAccess { receiver, field: _ } => {
            // The bare-ident-as-type case (`TypeName.Variant`) — record the
            // name so we import the type.
            if let ExprKind::Ident(id) = &receiver.kind {
                record_name_ref(&id.name, local_to_file, ctx, out);
            } else {
                collect_refs_in_expr(receiver, local_to_file, commons, ctx, out);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method: _,
            args,
            ..
        } => {
            if let ExprKind::Ident(id) = &receiver.kind {
                record_name_ref(&id.name, local_to_file, ctx, out);
            } else {
                collect_refs_in_expr(receiver, local_to_file, commons, ctx, out);
            }
            for a in args {
                collect_refs_in_expr(a, local_to_file, commons, ctx, out);
            }
        }
        ExprKind::Match { discriminant, arms } => {
            collect_refs_in_expr(discriminant, local_to_file, commons, ctx, out);
            for arm in arms {
                if let Pattern::Variant {
                    type_name: Some(tn),
                    ..
                } = &arm.pattern
                {
                    record_name_ref(&tn.name, local_to_file, ctx, out);
                }
                match &arm.body {
                    MatchBody::Expr(e) => collect_refs_in_expr(e, local_to_file, commons, ctx, out),
                    MatchBody::Block(b) => {
                        collect_refs_in_block(b, local_to_file, commons, ctx, out)
                    }
                }
            }
        }
        ExprKind::Is { value, pattern } => {
            collect_refs_in_expr(value, local_to_file, commons, ctx, out);
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

/// If `name` at `span` is a bare reference to a variant of a sum type (per
/// the checker's expression type), return the owning sum's name — the same
/// test the lowering uses to qualify it as `Type.Variant` (see the
/// `ExprKind::Ident` arm of `lower_expr`).
fn sum_owner_of_variant(
    name: &str,
    span: bynk_syntax::span::Span,
    commons: &TypedCommons,
) -> Option<String> {
    if let Some(Ty::Named {
        kind: NamedKind::Sum,
        name: type_name,
        ..
    }) = commons.expr_types.get(&span)
        && let Some(decl) = commons.types.get(type_name)
        && let TypeBody::Sum(s) = &decl.body
        && s.variants.iter().any(|v| v.name.name == name)
    {
        return Some(type_name.clone());
    }
    None
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
fn emit_cross_context_namespace_imports(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) {
    let info = &ctx.cross_context;
    // Consumed contexts that expose services (v0.6) plus, v0.15, those whose
    // capabilities this context references via `given B.Cap`.
    let mut needed: std::collections::BTreeSet<String> = info
        .consumed_services
        .iter()
        .filter(|(_, svcs)| !svcs.is_empty())
        .map(|(q, _)| q.clone())
        .collect();
    needed.extend(cross_context_cap_namespaces(commons, info));
    if needed.is_empty() {
        return;
    }
    let consumed_with_services: Vec<&String> = needed.iter().collect();
    for q in &consumed_with_services {
        // Pick the first known file path for the consumed context as the
        // import target. (The composition root lives in the consumed
        // context's directory; any of its files would work as an import
        // target since they're all in the same module namespace, but we
        // currently emit one file per .bynk source so a single import per
        // consumed name suffices for the surface contract.)
        let target_paths = ctx.imported_decl_paths.get(q.as_str());
        let target = target_paths
            .and_then(|m| m.values().next().cloned())
            .unwrap_or_else(|| {
                // No imported declaration pins the path (e.g. a capability-only
                // consumed context, v0.15). Fall back to the unit's own module:
                // its per-Worker handlers in workers mode, or its <segment>.bynk
                // source in bundle mode. v0.17: a consumed *adapter* is not a
                // Worker — its capability types live in its root module
                // (`<adapter>.ts`) in both targets.
                if ctx.consumed_adapters.contains(q.as_str()) {
                    let mut p = EmitProjectCtx::commons_path(q);
                    p.set_extension("bynk");
                    p
                } else {
                    match ctx.target {
                        BuildTarget::Workers => crate::project::worker_handlers_source_path(q),
                        BuildTarget::Bundle => {
                            let mut p = EmitProjectCtx::commons_path(q);
                            p.set_extension("bynk");
                            p
                        }
                    }
                }
            });
        let import =
            cross_commons_import_specifier_for_path(&ctx.source_path, &target, ctx.import_ext);
        let ns = qualified_to_ns(q);
        // #661: under `workers`, a consumed *context*'s module is imported for
        // its **types only** — the caller now generates its own codecs
        // (`emit_boundary_helpers`) and reaches the callee's types through this
        // alias in type position (`deps: { Clock: platform_time.Clock }`,
        // `Result<commerce_payment.AuthId, …>`). An `import type` is erased
        // outright, so the callee's *module* — and its provider implementation —
        // never enters the caller's Worker bundle. This does **not** apply to a
        // consumed *adapter* (its binding namespace, e.g. `tokens`, is a real
        // value import used by `compose.ts`) nor on `bundle` (contexts compile
        // together, and the value uses in `compose.ts` are legitimate).
        let type_only = matches!(ctx.target, BuildTarget::Workers)
            && !ctx.consumed_adapters.contains(q.as_str());
        let kw = if type_only { "import type" } else { "import" };
        writeln!(out, "{kw} * as {ns} from \"{import}\";").unwrap();
    }
    writeln!(out).unwrap();
}

fn emit_project_imports(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    refs: &ExternalReferences,
) {
    // Sibling imports: relative path within the same commons/context directory.
    let mut sibling_paths: Vec<(&PathBuf, &HashSet<String>)> = refs.by_sibling.iter().collect();
    sibling_paths.sort_by(|a, b| a.0.cmp(b.0));
    for (path, names) in sibling_paths {
        let import = sibling_import_specifier(&ctx.source_path, path, ctx.import_ext);
        let mut sorted: Vec<&String> = names.iter().collect();
        sorted.sort();
        let joined = sorted
            .iter()
            .map(|s| ts_ident(s))
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
            let import =
                cross_commons_import_specifier_for_path(&ctx.source_path, &target, ctx.import_ext);
            // For context units, aliase commons-source imports so we can emit
            // rebrand aliases of the same short name. Imports from consumed
            // contexts keep their original name. v0.20b: the rebrand applies
            // to *types* only — a `uses`-imported function (bynk.list's
            // `traverse`) is a value, imports plainly, and is never branded.
            let mut parts: Vec<String> = Vec::new();
            for n in &name_list {
                let from_kind = ctx.imported_from_kind.get(n.as_str()).copied();
                if ctx.unit_kind == UnitKind::Context
                    && from_kind == Some(UnitKind::Commons)
                    && commons.types.contains_key(n.as_str())
                {
                    parts.push(format!("{n} as __Commons{n}"));
                } else {
                    parts.push(ts_ident(n));
                }
            }
            let joined = parts.join(", ");
            writeln!(out, "import {{ {joined} }} from \"{import}\";").unwrap();
        }
    }
    // #527: imports the DO-side agent-deps expressions need (binding modules,
    // other Workers' handlers). Precomputed by the project driver.
    for line in &ctx.extra_import_lines {
        writeln!(out, "{line}").unwrap();
    }
}

/// Compute a relative import specifier from `from_source` (a `.bynk` path)
/// to `to_source` (another `.bynk` path), with `.bynk` rewritten to `.js`
/// for compatibility with NodeNext/strict TS resolution.
fn sibling_import_specifier(from_source: &Path, to_source: &Path, ext: ImportExt) -> String {
    let from_dir = from_source.parent().unwrap_or(Path::new(""));
    let target = to_source.with_extension(ext.as_str());
    let rel = relative_to(from_dir, &target);
    format!("./{}", ts_specifier(&rel))
}

/// Render a path as a TypeScript module specifier: **always forward
/// slashes**. `Path::display()` uses the platform separator, and on Windows
/// that emitted `import ... from "./commerce\orders.js"` — broken ESM
/// output, caught by the first CI matrix run on windows-latest.
pub(crate) fn ts_specifier(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Compute a relative import specifier from this file's location to a
/// specific source file in another commons. `target_source` is the project-
/// relative path of the target `.bynk` file. The result is suitable for
/// `import { ... } from "..."` in NodeNext/strict TypeScript.
pub(crate) fn cross_commons_import_specifier_for_path(
    from_source: &Path,
    target_source: &Path,
    ext: ImportExt,
) -> String {
    let from_dir = from_source.parent().unwrap_or(Path::new(""));
    let target = target_source.with_extension(ext.as_str());
    let rel = relative_to(from_dir, &target);
    let display = ts_specifier(&rel);
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
    writeln!(out, "// Generated by bynkc — do not edit by hand.").unwrap();
    let kind = match ctx.unit_kind {
        UnitKind::Commons => "commons",
        UnitKind::Context => "context",
        UnitKind::Test => "test",
        UnitKind::Integration => "integration test",
        UnitKind::Adapter => "adapter",
    };
    writeln!(out, "// {kind} {}", commons.commons.name.joined()).unwrap();
    writeln!(out).unwrap();
    if !commons.commons.items.is_empty() {
        let runtime_import = runtime_import_for(&ctx.source_path, ctx.import_ext);
        let has_agent = commons
            .commons
            .items
            .iter()
            .any(|i| matches!(i, CommonsItem::Agent(_)));
        // v0.80: a file with any agent invariant imports the `invariantViolation`
        // fault helper used by the generated `commitState` gate. v0.116: a step
        // invariant (`transition`) uses the same fault helper, so a transition-only
        // agent must import it too.
        let has_agent_invariants = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Agent(a) => !a.invariants.is_empty() || !a.transitions.is_empty(),
            _ => false,
        });
        // v0.153 (ADR 0177): a service HTTP handler imports `HttpResult`, and so
        // does any *free* `fn` / provider / capability whose signature names it
        // (the `?`-Option lift makes a bare `fn -> HttpResult[T]` emit
        // `HttpResult.NotFound`) — the structural scan covers both, closing the
        // free-fn gap the single-file path already handles.
        let has_http = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Service(s) => s
                .handlers
                .iter()
                .any(|h| matches!(h.kind, HandlerKind::Http { .. })),
            _ => false,
        }) || file_mentions_http_result(commons);
        // A `from queue` `on message` is the queue consumer (imports `QueueResult`);
        // a `from websocket` `on message` (slice 3b-iii) is the inbound handler and
        // is not a queue concern.
        let has_queue = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Service(s) => {
                !matches!(s.protocol, ServiceProtocol::WebSocket { .. })
                    && s.handlers
                        .iter()
                        .any(|h| matches!(h.kind, HandlerKind::Message))
            }
            _ => false,
        });
        let workers = matches!(ctx.target, BuildTarget::Workers);
        let mut parts: Vec<&str> = vec![
            "Ok",
            "Err",
            "Some",
            "None",
            "type Result",
            "type Option",
            "type ValidationError",
        ];
        // v0.22b: the codec types are imported only when the file uses the
        // `Json` codec (or names `JsonError` in a signature) — keeping every
        // non-codec module's header byte-identical to v0.22a.
        let uses_codec = !collect_json_codec_roots(commons).is_empty();
        let mentions_json_error = file_mentions_json_error(commons);
        if uses_codec || mentions_json_error {
            parts.push("type JsonError");
        }
        // v0.102: a file naming `Connection[F]` imports the runtime interface.
        if file_mentions_connection(commons) {
            parts.push("type Connection");
        }
        if has_agent {
            // v0.9.2: agent-declaring files lower instantiation through the
            // `makeAgent` helper and a per-agent `StateRegistry`, and the
            // generated factory's signature names `DurableObjectNamespace`.
            parts.push("type DurableObjectState");
            parts.push("type DurableObjectNamespace");
            parts.push("StateRegistry");
            parts.push("makeAgent");
        }
        if has_agent_invariants {
            parts.push("invariantViolation");
        }
        // v0.96 (ADR 0124): an agent whose load-time validation gate fires imports
        // the `rehydrationViolation` fault helper.
        let has_rehydration_gate = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Agent(a) => emit::agent_needs_rehydrate(a, &commons.types),
            _ => false,
        });
        if has_rehydration_gate {
            parts.push("rehydrationViolation");
        }
        // v0.104/v0.105 (real-time track slice 3b): on Workers a `store Map[K,
        // Connection]` persists the connection id; its entry ops re-resolve the live
        // socket via `resolveConnection` and read a connection's id via `connIdOf`.
        if workers
            && commons.commons.items.iter().any(|i| match i {
                CommonsItem::Agent(a) => emit::agent_has_held_storage(a),
                _ => false,
            })
        {
            parts.push("resolveConnection");
            parts.push("connIdOf");
        }
        // v0.104/v0.105 (real-time track slice 3b): on Workers a context hosting a
        // `from websocket` `on open` accepts the socket inside its Durable Object via
        // the hibernatable API — `acceptHibernatableConnection` (accept + tag + wrap),
        // a `WebSocketPair`, and the `101` upgrade response. (The service and its
        // hosting agent share the one Worker module, so these land in one
        // `handlers.ts`.)
        let hosts_ws_open = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Service(s) => s
                .handlers
                .iter()
                .any(|h| matches!(h.kind, HandlerKind::Open)),
            _ => false,
        });
        if workers && hosts_ws_open {
            parts.push("acceptHibernatableConnection");
            parts.push("newWebSocketPair");
            parts.push("webSocketUpgradeResponse");
        }
        // v0.106 (slice 3b-iii): a context with an inbound/close handler re-wraps
        // the firing socket as a `WorkersConnection` in `webSocketMessage`/
        // `webSocketClose`.
        let hosts_ws_inbound = commons.commons.items.iter().any(|i| match i {
            CommonsItem::Service(s) => {
                matches!(s.protocol, ServiceProtocol::WebSocket { .. })
                    && s.handlers
                        .iter()
                        .any(|h| matches!(h.kind, HandlerKind::Message | HandlerKind::Close))
            }
            _ => false,
        });
        if workers && hosts_ws_inbound {
            parts.push("WorkersConnection");
        }
        if has_http {
            // `HttpResult` is both a value (the constructor namespace) and a
            // type (the discriminated union). A bare named import brings both
            // in — `type HttpResult` would duplicate the identifier.
            parts.push(HTTP_RESULT);
        }
        if has_queue {
            // v0.44: `QueueResult` is both a value (the verdict namespace) and a
            // type; a bare named import brings both in.
            parts.push(QUEUE_RESULT);
        }
        if workers {
            parts.push("type JsonValue");
            parts.push("type BoundaryError");
            parts.push("type ServiceBinding");
            parts.push("callService");
            parts.push("boundaryError");
        } else if uses_codec || has_agent {
            // v0.22b: the bundle-mode codec helpers reference JsonValue and
            // BoundaryError. v0.96 (ADR 0124): so do an agent's emitted
            // rehydration deserialisers and the gate's inline base checks — the
            // boundary helpers now emit on bundle too (for the rehydration gate).
            parts.push("type JsonValue");
            parts.push("type BoundaryError");
        }
        writeln!(
            out,
            "import {{ {} }} from \"{runtime_import}\";",
            parts.join(", ")
        )
        .unwrap();
        writeln!(out).unwrap();
    }
}

/// Variant of write_header for single-file (no project context) emission.
fn write_header_single(
    out: &mut String,
    commons: &TypedCommons,
    uses_bytes: bool,
    uses_http: bool,
) {
    writeln!(out, "// Generated by bynkc — do not edit by hand.").unwrap();
    writeln!(out, "// commons {}", commons.commons.name.joined()).unwrap();
    writeln!(out).unwrap();
    if !commons.commons.items.is_empty() {
        // v0.22b: codec imports only when the file uses the `Json` codec.
        let uses_codec = !collect_json_codec_roots(commons).is_empty();
        let codec_imports = if uses_codec {
            ", type JsonError, type JsonValue, type BoundaryError"
        } else if file_mentions_json_error(commons) {
            ", type JsonError"
        } else {
            ""
        };
        // v0.110 (ADR 0142): the `Bytes` runtime helpers, imported only when a
        // `Bytes` value is constructed or compared in the body.
        let bytes_imports = if uses_bytes {
            BYTES_RUNTIME_IMPORTS
        } else {
            ""
        };
        // v0.153 (ADR 0177): `HttpResult` is a value (its variant namespace) and
        // a type, so it imports without a `type` prefix — one binding serves
        // both `HttpResult.NotFound` and the `HttpResult<T>` annotation.
        let http_imports = if uses_http { ", HttpResult" } else { "" };
        writeln!(
            out,
            "import {{ Ok, Err, Some, None, type Result, type Option, type ValidationError{codec_imports}{bytes_imports}{http_imports} }} from \"./runtime.js\";",
        )
        .unwrap();
        writeln!(out).unwrap();
    }
}

/// v0.110 (ADR 0142): the `Bytes` runtime helpers, appended to a module's
/// import list when the emitted body references them. `bytesEqual` backs `==`;
/// the base64/UTF-8 helpers back the kernel and codec.
const BYTES_RUNTIME_IMPORTS: &str =
    ", __bynkBytesEqual, __bynkBytesToBase64, __bynkBytesFromBase64, __bynkBytesDecodeUtf8";

/// Emit the commons-level doc block (if any) at the current position.
fn write_commons_doc(out: &mut String, commons: &TypedCommons) {
    if let Some(doc) = &commons.commons.documentation {
        emit_doc_block(out, Some(doc), 0);
        writeln!(out).unwrap();
    }
}

/// The module-level state-registry constant name for an agent class.
fn agent_registry_name(agent: &str) -> String {
    format!("__{agent}Registry")
}

/// The exported agent-construction factory name for an agent class.
pub fn agent_factory_name(agent: &str) -> String {
    format!("__make{agent}")
}

/// Per-function lowering context: fresh-temp counter + typed-commons handle
/// (used to look up receiver types for method-call UFCS lowering).
pub(crate) struct LowerCtx<'a> {
    next_tmp: u32,
    commons: &'a TypedCommons,
    /// v0.154 (ADR 0178): the enclosing function/handler's resolved return type,
    /// set at each body-emission site. The `?` lowering reads it to decide
    /// whether a declared error embedding (`embeds E as V`) converts the
    /// propagated `Err` — via the same `embedding_for` rule the checker used.
    return_ty: Option<bynk_check::checker::Ty>,
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
    /// v0.80: when lowering an agent invariant predicate, the name of the
    /// proposed-state variable (the `commitState` parameter) and the set of
    /// state field names. A bare ident matching a state field lowers to
    /// `<var>.<field>` — invariants read state fields directly (§14).
    invariant_state: Option<(String, HashSet<String>)>,
    /// v0.117 (testing track slice 5): when lowering a test `case` body, the name
    /// of the recorded-call trace object (`__obs`), over which an observation
    /// (`Cap.op called …`) and `trace(Cap.op)` are lowered.
    observation_trace: Option<String>,
    /// v0.116 (testing track slice 4): when lowering a `transition` predicate, the
    /// JS names bound to the contextual `old` and `new` state records. The Bynk
    /// identifiers `old`/`new` lower to these (`new` is a JS reserved word, so both
    /// are renamed), and field access `old.<field>` reads off the `old` record.
    transition_states: Option<(String, String)>,
    /// v0.81 (storage track): when lowering a `store`-agent handler body, the
    /// name of the mutable working-state variable (`__state`) and the set of
    /// `Cell` field names. A bare `Cell` read lowers to `<var>.<cell>`, and a
    /// `cell := v` write lowers to `<var>.<cell> = <v>` — read-your-writes via the
    /// in-memory record, flushed once at handler end (ADR 0109).
    agent_store_state: Option<(String, HashSet<String>)>,
    /// v0.82 (ADR 0110): the agent's `store` `Map` field names. A method call
    /// whose receiver is one lowers to an entry operation over `__state.<map>`
    /// (a JSON-serialisable `Record<string, V>`), staged in the working record and
    /// flushed at commit like any other state field.
    agent_store_maps: HashSet<String>,
    /// v0.83: the agent's `store` `Set` field names. A method call whose receiver
    /// is one lowers to an entry operation over `__state.<set>` (a
    /// `Record<string, boolean>`), staged in the working record.
    agent_store_sets: HashSet<String>,
    /// v0.87 (ADR 0113): the agent's `store` `Cache` fields (name → ttl millis).
    /// A method call whose receiver is one lowers to an entry op over
    /// `__state.<cache>` (a `Record<string, { v, exp }>`), applying TTL expiry
    /// against the injected `Clock`.
    agent_store_caches: HashMap<String, i64>,
    /// v0.95 (ADR 0121): the agent's `store` `Log` fields (name → optional
    /// `@retain` millis). `<log>.append` pushes `{ t: now(), v }` to
    /// `__state.<log>` (an array) and prunes past the retain horizon; the
    /// time-window roots / builders lower to a query pipeline over the array.
    agent_store_logs: HashMap<String, Option<i64>>,
    /// v0.93 (ADR 0118): the agent's `@indexed` secondary indexes (map name →
    /// the value-record fields indexed on). A mutating op on the map maintains a
    /// sibling posting-list `Record<string, string[]>` per field (`<map>__idx_<f>`);
    /// an equality `filter` on an indexed field routes to a posting lookup.
    agent_store_indexes: HashMap<String, Vec<String>>,
    /// v0.104 (real-time track slice 3b): when lowering a `from websocket`
    /// `on open` body **into its hosting Durable Object** (the agent the upgrade
    /// transfers the connection to), the name of that agent. A transfer call
    /// `<Agent>(<key>).method(args)` whose `<Agent>` is this self-agent lowers to a
    /// direct `this.method(args, deps)` self-call rather than the cross-instance
    /// `__make<Agent>(key)` factory — the connection is already in this DO, so it
    /// never crosses an RPC boundary (DECISION A). `None` everywhere else.
    pub ws_self_agent: Option<String>,
    /// v0.104/v0.105 (real-time track slice 3b): the agent's held `store Map[K,
    /// Connection]` fields (name → the connection's **frame type** `F`, e.g.
    /// `ServerFrame`). On Workers these persist `K → connId` in the durable state
    /// record; a method call whose receiver is one lowers to an entry op over
    /// `__state.<map>` (the connId record) with `connIdOf`/`resolveConnection<F>` —
    /// not the plain `Record<string, V>` ops (held maps are excluded from
    /// `agent_store_maps`).
    pub agent_held_maps: HashMap<String, String>,
    /// Cross-context info for v0.6 cross-context call lowering.
    cross_context: &'a bynk_check::resolver::CrossContextInfo,
    /// True if the current handler made at least one cross-context call
    /// (drives whether `deps` gets a `surface` field type).
    cross_context_used: bool,
    /// v0.7: when lowering a test case body, the target context's local
    /// service names. A `service.call(args)` or `service(args)` invocation
    /// where `service` is in this set lowers to `<service>.call(args, deps)`
    /// so the test wires its `deps` through.
    pub test_services: HashSet<String>,
    /// v0.182 (#664): while lowering an `EffectLet` whose value addresses a
    /// service handler, the call-site principal's identity expression (already
    /// lowered), if the statement carries `by <Actor>(<identity>)`. The
    /// address-call lowering reads it to build the handler's `deps.identity`.
    /// `None` for a unit-identity actor or a non-principal statement.
    pub call_site_identity: Option<String>,
    /// #706: the call-site principal is `by Nobody` — drive the route with no
    /// `Authorization` header so the real auth seam rejects it (`401` →
    /// `Rejected(Unauthorized)`). Routes a `system` http address to the no-auth
    /// driver. `false` for any other (or no) principal.
    pub call_site_no_credential: bool,
    /// v0.182 (#664): the ordered handler kinds of each test service, so a cron
    /// (`svc.schedule("…")`) or queue (`svc.message(m)`) address can recover the
    /// position index the emitted key encodes (`cron_<svc>_<i>` / `queue_…`).
    /// http keys are a pure function of verb + path and need no lookup here.
    pub test_service_handlers: HashMap<String, Vec<bynk_syntax::ast::HandlerKind>>,
    /// v0.182 (Slice B, #667): the target's http service names when lowering a
    /// `system` case. An http address on one of these lowers to a driver call
    /// (`__sysdrive_<svc>_<key>(args, sub)`) that drives a real `worker.fetch`
    /// with a signed credential, instead of the unit-tier direct handler call.
    /// Empty at the unit tier.
    pub system_http_services: std::collections::HashSet<String>,
    /// #707: the declared `(service, method, path)` http routes of the system
    /// target. A `(method, path)` call whose method is absent here but whose path
    /// is present is a **wrong-method** call — it drives the `405` fall-through
    /// through the generic `__sysdrive_wrongmethod_<svc>` driver.
    pub system_http_routes: std::collections::HashSet<(String, String, String)>,
    /// v0.7: when lowering a test case body, the target context's local
    /// agent names. `<Agent>(<key>).method(args)` lowers to
    /// `new Agent(makeTestState(...)).method(args, {})`.
    pub test_agents: HashSet<String>,
    /// Agent names declared in the surrounding context. Drives lowering of
    /// `Agent(key)` (to `new Agent(makeTestState(String(key)))`) and of
    /// `agent_instance.method(args)` (to `instance.method(args, deps)`) in
    /// service and agent-handler bodies. Populated by the caller for non-test
    /// emission and from `test_agents` in test emission.
    pub local_agents: HashSet<String>,
    /// #527: agent → method → the method's `given` caps (mirrors
    /// [`crate::project::EmitProjectCtx::agent_method_givens`]). Consulted by
    /// the agent-call lowering to record capability requirements.
    pub agent_method_givens: HashMap<String, HashMap<String, Vec<bynk_syntax::ast::CapRef>>>,
    /// #527: capabilities required by agent methods this body calls, keyed by
    /// deps key. After body lowering these widen the handler's deps *type* to
    /// match the runtime value compose builds (which always carried them).
    pub agent_given_caps_used: std::collections::BTreeMap<String, bynk_syntax::ast::CapRef>,
    /// #527: type names this context *rebrands* (`uses`-imported commons
    /// types re-exported as `T & { __ctxBrand }`). Drives brand-assertion
    /// casts where unbranded commons values meet branded local positions.
    pub rebranded_types: HashSet<String>,
    /// #527: fn names imported from a commons. Such a fn's signature uses the
    /// *unbranded* commons types, so calls whose return mentions a rebranded
    /// type are asserted back into the local (branded) namespace.
    pub commons_imported_fns: HashSet<String>,
    /// Variable bindings that point at agent instances. Updated by the
    /// statement emitter when it sees `let x = AgentName(key)`. Used by
    /// the method-call lowering so `x.method(args)` resolves through
    /// the agent's class rather than via the receiver-namespace lookup.
    pub local_agent_vars: HashMap<String, String>,
    /// v0.8 build target. In workers mode cross-context calls lower to
    /// `callService(...)` instead of `deps.surface.<key>.<method>(...)`.
    pub target: BuildTarget,
    /// v0.9.2: set when the body instantiates a local agent. In workers mode
    /// this drives `env` (carrying the DO namespaces) into the handler's deps
    /// type so the agent factory can reach its Durable Object binding.
    pub agents_instantiated: bool,
    /// When an `is` receiver is not a simple, repeatable lvalue (e.g. a call
    /// like `parse(x) is Ok(n)`), it is evaluated once into a temp; the temp
    /// name is cached here keyed by the receiver expression's span so the
    /// `.tag` check and every pattern binding reference the *same* single
    /// evaluation. Simple receivers (idents / field chains) are never cached
    /// and continue to be rendered inline as before.
    is_receiver_temps: HashMap<bynk_syntax::span::Span, String>,
    /// v0.12: the receiver expression a capability call resolves against —
    /// `deps` in a handler body, `this.deps` in a composed provider body.
    cap_deps_expr: String,
    /// v0.47: when lowering a Bearer handler body, the `by` binder whose
    /// `.identity` is threaded through `deps` (so `<binder>.identity` lowers to
    /// `deps.identity` rather than the unit-value `undefined`).
    pub deps_identity_binder: Option<String>,
    /// v0.52: when lowering a multi-actor sum handler body, the `by` binder that
    /// names the resolved-actor value (threaded through `deps`, so the binder
    /// ident lowers to `deps.who` — the tagged union the body `match`es).
    pub actor_sum_binder: Option<String>,
    /// v0.59: when lowering a **test case body**, the source text and
    /// project-relative path of the file the body came from, so an `assert`
    /// can emit a real `path:line:col` location (for `--format json`
    /// click-through) rather than a bare byte offset. `None` for non-test
    /// emission, where `assert` doesn't appear.
    pub assert_loc: Option<AssertLoc>,
    /// Slice 1 (ADR 0103): the source-map builder for the file being emitted, if
    /// any. The deep lowering chain records `(generated offset → source span)`
    /// checkpoints here; `emit_project` owns the `RefCell` and threads a shared
    /// borrow in. `None` for the single-file `emit()` path and any body emitted
    /// outside a project, where no map is produced.
    pub source_map: Option<&'a RefCell<SourceMapBuilder>>,
}

/// v0.59: the source context an `assert` lowering needs to turn its span into a
/// `path:line:col` location. Owned (cloned once per test-case body) to keep the
/// lowering free of extra lifetime threading; test-file sources are small and
/// this is compile-time only.
#[derive(Clone)]
pub(crate) struct AssertLoc {
    pub source: String,
    pub rel_path: String,
}

impl<'a> LowerCtx<'a> {
    /// True when lowering a **generated test-case body** (`tests/*.test.ts`), where
    /// branded types are destructured into `any`-typed value bindings rather than
    /// referenced as types. `assert_loc` is set only by the two test-body lowering
    /// entry points and is `None` for all production emission, so it doubles as the
    /// test-scaffold discriminator. Callers that emit a branded `as`-cast consult
    /// this to pick `unchecked_construct_test` (→ `(v as any)`) over the production
    /// `(v as T)` form, which cannot resolve `T` in the test module's scope.
    pub(crate) fn in_test_scaffold(&self) -> bool {
        self.assert_loc.is_some()
    }

    fn new(
        commons: &'a TypedCommons,
        cross_context: &'a bynk_check::resolver::CrossContextInfo,
    ) -> Self {
        Self {
            next_tmp: 0,
            commons,
            return_ty: None,
            capabilities: HashSet::new(),
            in_agent_handler: false,
            agent_state_var: None,
            agent_key_field: None,
            invariant_state: None,
            observation_trace: None,
            transition_states: None,
            agent_store_state: None,
            agent_store_maps: HashSet::new(),
            agent_store_sets: HashSet::new(),
            agent_store_caches: HashMap::new(),
            agent_store_logs: HashMap::new(),
            agent_store_indexes: HashMap::new(),
            ws_self_agent: None,
            agent_held_maps: HashMap::new(),
            cross_context,
            cross_context_used: false,
            test_services: HashSet::new(),
            call_site_identity: None,
            call_site_no_credential: false,
            test_service_handlers: HashMap::new(),
            system_http_services: std::collections::HashSet::new(),
            system_http_routes: std::collections::HashSet::new(),
            test_agents: HashSet::new(),
            local_agents: HashSet::new(),
            agent_method_givens: HashMap::new(),
            agent_given_caps_used: std::collections::BTreeMap::new(),
            rebranded_types: HashSet::new(),
            commons_imported_fns: HashSet::new(),
            local_agent_vars: HashMap::new(),
            target: BuildTarget::Bundle,
            agents_instantiated: false,
            is_receiver_temps: HashMap::new(),
            cap_deps_expr: "deps".to_string(),
            deps_identity_binder: None,
            actor_sum_binder: None,
            assert_loc: None,
            source_map: None,
        }
    }

    /// Attach the file's source-map builder (slice 1, ADR 0103). Builder-style so
    /// the existing `LowerCtx::new(commons, cross)` call sites stay untouched —
    /// only the project-emission path that has a builder calls this.
    fn with_source_map(mut self, map: Option<&'a RefCell<SourceMapBuilder>>) -> Self {
        self.source_map = map;
        self
    }

    /// Record a checkpoint: generated text from `out_len` onward originates at
    /// `span`, until the next checkpoint (ADR 0103 D2, nearest-enclosing). A
    /// no-op when no builder is attached. `out_len` is the buffer length *before*
    /// the statement's text is appended.
    fn record_span(&self, out_len: usize, span: bynk_syntax::span::Span) {
        if let Some(map) = self.source_map {
            map.borrow_mut().record(out_len, span);
        }
    }
    /// v0.9.2: lower an agent instantiation `AgentName(key)` to its factory
    /// call. Bundle/test mode passes only the key; workers mode also threads
    /// `deps.env` so the factory can reach the agent's DO namespace.
    fn agent_construct(&mut self, agent: &str, key_expr: &str) -> String {
        self.agents_instantiated = true;
        let factory = agent_factory_name(agent);
        if matches!(self.target, BuildTarget::Workers) {
            format!("{factory}({key_expr}, deps.env)")
        } else {
            format!("{factory}({key_expr})")
        }
    }

    /// #527: derive which imported names this context rebrands and which fns
    /// come from a commons (and so speak the unbranded types). Mirrors the
    /// alias predicate in `emit_project_imports`.
    pub(crate) fn set_rebrand_info(
        &mut self,
        commons: &TypedCommons,
        ctx: &crate::project::EmitProjectCtx,
    ) {
        if ctx.unit_kind != UnitKind::Context {
            return;
        }
        for (name, kind) in &ctx.imported_from_kind {
            if *kind != UnitKind::Commons {
                continue;
            }
            if commons.types.contains_key(name) {
                self.rebranded_types.insert(name.clone());
            } else if commons.fns.contains_key(name) {
                self.commons_imported_fns.insert(name.clone());
            }
        }
    }

    /// #527: note that the body calls `agent.method`, folding the method's
    /// `given` capabilities into this handler's requirement set.
    pub(crate) fn record_agent_call(&mut self, agent: &str, method: &str) {
        let givens = self
            .agent_method_givens
            .get(agent)
            .and_then(|m| m.get(method))
            .cloned()
            .unwrap_or_default();
        for c in givens {
            self.agent_given_caps_used
                .entry(c.key().to_string())
                .or_insert(c);
        }
    }
    fn fresh(&mut self) -> String {
        let n = self.next_tmp;
        self.next_tmp += 1;
        format!("__r{n}")
    }
    /// Return a stable textual reference to an `is` receiver, used by the
    /// `.tag` check in `lower_is`. A simple, repeatable lvalue is lowered
    /// inline exactly as before (preserving rewrites such as `self.state` or
    /// capability access). A complex receiver (anything `value_text_for_is`
    /// could not render — e.g. a call) is evaluated once into a fresh temp
    /// emitted into `stmts` and cached by span, so the bindings gathered later
    /// reference the same evaluation rather than re-running the expression.
    fn is_receiver_ref(&mut self, value: &Expr, stmts: &mut Vec<String>) -> String {
        if let Some(t) = self.is_receiver_temps.get(&value.span) {
            return t.clone();
        }
        let lowered = lower_expr(value, stmts, self);
        if is_simple_is_receiver(value) {
            return lowered;
        }
        let tmp = self.fresh();
        stmts.push(format!("const {tmp} = {lowered};"));
        self.is_receiver_temps.insert(value.span, tmp.clone());
        tmp
    }

    /// v0.13: like `is_receiver_ref` but always lifts to a temp, even for a
    /// simple ident. A refined `is`-narrowing re-binds the value's name to the
    /// branded refined type (`const n = <temp> as Quantity`); that shadowing
    /// const cannot reference the same name (TDZ), so the value is captured in a
    /// temp first and both the check and the binding read the temp.
    fn is_receiver_ref_forced(&mut self, value: &Expr, stmts: &mut Vec<String>) -> String {
        if let Some(t) = self.is_receiver_temps.get(&value.span) {
            return t.clone();
        }
        let lowered = lower_expr(value, stmts, self);
        let tmp = self.fresh();
        stmts.push(format!("const {tmp} = {lowered};"));
        self.is_receiver_temps.insert(value.span, tmp.clone());
        tmp
    }

    /// v0.13: true when `value is Name` is a *refinement* check — the value is a
    /// base/refined value and `Name` is a refined type — rather than a sum
    /// variant test. Mirrors the checker's disambiguation.
    fn is_refined_is_check(&self, value: &Expr, name: &str) -> bool {
        let value_baseish = matches!(
            self.commons.expr_types.get(&value.span),
            Some(Ty::Base(_))
                | Some(Ty::Named {
                    kind: NamedKind::Refined(_),
                    ..
                })
        );
        let name_refined = matches!(
            self.commons.types.get(name).map(|d| &d.body),
            Some(TypeBody::Refined { .. })
        );
        value_baseish && name_refined
    }
    /// Read-only counterpart for the binding gatherer (which has no `stmts`
    /// and cannot lift). If the receiver was already lifted to a temp during
    /// condition lowering, reuse that temp; otherwise it must be a simple
    /// repeatable lvalue, rendered inline. The "lower the condition before
    /// gathering its bindings" ordering in `emit_if_tail` / `lower_and_with_is`
    /// guarantees the temp exists before this is called for complex receivers.
    fn is_receiver_text(&self, value: &Expr) -> String {
        if let Some(t) = self.is_receiver_temps.get(&value.span) {
            return t.clone();
        }
        value_text_for_is(value)
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
        // v0.52: a multi-actor sum arm binds the resolved actor's identity,
        // carried in the `identity` field of the tagged object.
        if let Some(Ty::ActorSum(_)) = discriminant_ty {
            return "identity".to_string();
        }
        if let Some(Ty::Named {
            kind: NamedKind::Sum,
            name,
            ..
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

    /// The type of a variant's `idx`-th payload field, when resolvable — used to
    /// recurse field-name resolution through nested payload patterns (ADR 0169).
    /// Precise for `Result`/`Option`/`HttpResult` and user sums; `None` otherwise
    /// (callers fall back to the single-field `"value"` name).
    fn payload_field_ty(&self, ty: Option<&Ty>, variant: &str, idx: usize) -> Option<Ty> {
        match ty {
            Some(Ty::Result(t, e)) => match (variant, idx) {
                ("Ok", 0) => Some((**t).clone()),
                ("Err", 0) => Some((**e).clone()),
                _ => None,
            },
            Some(Ty::HttpResult(t)) if variant == "Ok" && idx == 0 => Some((**t).clone()),
            Some(Ty::Option(t)) if variant == "Some" && idx == 0 => Some((**t).clone()),
            Some(Ty::Named {
                kind: NamedKind::Sum,
                name,
                ..
            }) => {
                let decl = self.commons.types.get(name)?;
                let TypeBody::Sum(s) = &decl.body else {
                    return None;
                };
                let v = s.variants.iter().find(|v| v.name.name == variant)?;
                let f = v.payload.get(idx)?;
                bynk_check::checker::resolve_type_ref(&f.type_ref, &self.commons.types)
            }
            _ => None,
        }
    }
}

/// Unchecked construction of a branded value in emitted TypeScript.
///
/// ADR 0182: an **opaque** type exposes a runtime `.unsafe(value)` constructor
/// (source-callable within its defining commons, and the target of its internal
/// uses), so opaque construction stays `T.unsafe(value)`. A **refined** or
/// **alias** type has **no** public `.unsafe`: exposing one let hand-written host
/// or adapter code bypass the refinement predicate, the credibility hole #545
/// closed. Its admitted / generated values are branded with an inline `as` cast
/// — byte-for-byte the old `.unsafe` body (`return value as T`) at the call site,
/// but not a callable API surface a consumer can reach.
pub(crate) fn unchecked_construct(name: &str, value: &str, is_opaque: bool) -> String {
    if is_opaque {
        format!("{name}.unsafe({value})")
    } else {
        format!("({value} as {name})")
    }
}

/// Unchecked construction inside GENERATED TEST scaffolding (`tests/*.test.ts`).
///
/// There a branded type is in scope only as an `any`-typed value binding
/// (`const {{ T }} = ns as any`) — never as a type — so the production
/// `(value as T)` form fails to resolve `T`. Opaque still constructs through its
/// `.unsafe` value method (kept, ADR 0182); a refined/alias value brands to `any`,
/// which is exactly the type the pre-0182 `T.unsafe(value)` already produced here
/// (`T` being `any`) and erases to the raw value at runtime — without
/// reintroducing a callable refined `.unsafe`.
pub(crate) fn unchecked_construct_test(name: &str, value: &str, is_opaque: bool) -> String {
    if is_opaque {
        format!("{name}.unsafe({value})")
    } else {
        format!("({value} as any)")
    }
}

fn ts_base(b: BaseType) -> &'static str {
    match b {
        BaseType::Int => "number",
        BaseType::String => "string",
        BaseType::Bool => "boolean",
        BaseType::Float => "number",
        BaseType::Duration | BaseType::Instant => "number",
        // v0.110 (ADR 0142): `Bytes` is the one base type that does NOT erase
        // to `number` — it lowers to an immutable octet sequence, `Uint8Array`.
        BaseType::Bytes => "Uint8Array",
    }
}

pub(crate) fn ts_type_ref(r: &TypeRef) -> String {
    ts_type_ref_with(r, None)
}

/// Like `ts_type_ref`, but qualifies named types that live in `scope` with the
/// namespace `ns` (`Order` → `Ns.Order`). Used by the test-emission harness for
/// mock method signatures that sit outside the destructuring that brings a
/// namespace's value-side names into local scope, so the types must be
/// referenced fully qualified. Qualification recurses through generic
/// arguments; base/unit types are unaffected.
pub(crate) fn ts_type_ref_qualified(r: &TypeRef, scope: &HashSet<String>, ns: &str) -> String {
    ts_type_ref_with(r, Some((scope, ns)))
}

/// Shared renderer behind `ts_type_ref` (`qualify = None`) and
/// `ts_type_ref_qualified` (`qualify = Some((scope, ns))`). With `None` it is
/// output-identical to the historic `ts_type_ref`; the only divergence is the
/// `Named` arm, which qualifies in-scope names when `qualify` is set.
fn ts_type_ref_with(r: &TypeRef, qualify: Option<(&HashSet<String>, &str)>) -> String {
    match r {
        TypeRef::Base(b, _) => ts_base(*b).to_string(),
        TypeRef::Named(id) => {
            if let Some((scope, ns)) = qualify
                && scope.contains(&id.name)
            {
                format!("{ns}.{}", id.name)
            } else {
                id.name.clone()
            }
        }
        TypeRef::Result(t, e, _) => format!(
            "Result<{}, {}>",
            ts_type_ref_with(t, qualify),
            ts_type_ref_with(e, qualify)
        ),
        TypeRef::Option(t, _) => format!("Option<{}>", ts_type_ref_with(t, qualify)),
        TypeRef::Effect(t, _) => {
            let inner = ts_type_ref_with(t, qualify);
            if inner == "()" || inner == "void" {
                "Promise<void>".to_string()
            } else {
                format!("Promise<{inner}>")
            }
        }
        TypeRef::HttpResult(t, _) => format!("HttpResult<{}>", ts_type_ref_with(t, qualify)),
        // v0.20b: collections lower to immutable TS shapes.
        TypeRef::List(t, _) => format!("readonly {}[]", ts_type_ref_with(t, qualify)),
        TypeRef::Query(t, _) => {
            format!("(() => readonly {}[])", ts_type_ref_with(t, qualify))
        }
        // v0.100: `Stream[T]` lowers to a host async iterable.
        TypeRef::Stream(t, _) => format!("AsyncIterable<{}>", ts_type_ref_with(t, qualify)),
        // v0.102: a `Connection[F]` lowers to the runtime `Connection<F>`
        // interface (the concrete implementation arrives with the protocol).
        TypeRef::Connection(t, _) => format!("Connection<{}>", ts_type_ref_with(t, qualify)),
        // v0.119: `History[Agent]` is a test-only generator with no emitted TS
        // type — it never reaches a signature/field position (the property runner
        // binds the driven history as an ordinary array). Rendered defensively.
        TypeRef::History(_, _) => "never".to_string(),
        TypeRef::Map(k, v, _) => {
            format!(
                "ReadonlyMap<{}, {}>",
                ts_type_ref_with(k, qualify),
                ts_type_ref_with(v, qualify)
            )
        }
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
        // v0.157 (ADR 0183): `Name[Arg, …]` lowers to the erased TS generic
        // `Name<Arg, …>` — the generic record's interface is emitted with the
        // same type parameters (like a generic function's erased `<A, B>`).
        TypeRef::App { name, args, .. } => {
            let head = if let Some((scope, ns)) = qualify
                && scope.contains(&name.name)
            {
                format!("{ns}.{}", name.name)
            } else {
                name.name.clone()
            };
            let rendered: Vec<String> = args.iter().map(|a| ts_type_ref_with(a, qualify)).collect();
            format!("{head}<{}>", rendered.join(", "))
        }
        // v0.20a: a function type lowers to a TS function type. Positional
        // parameter names (`a0`, `a1`, …) — TS requires names in function
        // type syntax; an Effect return is already Promise via recursion.
        TypeRef::Fn(params, ret, _) => {
            let params: Vec<String> = params
                .iter()
                .enumerate()
                .map(|(i, p)| format!("a{i}: {}", ts_type_ref_with(p, qualify)))
                .collect();
            let ret = match ts_type_ref_with(ret, qualify).as_str() {
                "()" => "void".to_string(),
                other => other.to_string(),
            };
            format!("({}) => {ret}", params.join(", "))
        }
    }
}

/// v0.20b: render a checker `Ty` as a TypeScript type. Used by the inline
/// kernel-method lowerings, whose IIFE parameters must be annotated
/// (`noImplicitAny`). Rigid type variables render as themselves — inside an
/// emitted generic function they are in scope as TS type parameters.
fn ts_ty(t: &Ty) -> String {
    match t {
        Ty::Base(BaseType::Int) => "number".to_string(),
        Ty::Base(BaseType::String) => "string".to_string(),
        Ty::Base(BaseType::Bool) => "boolean".to_string(),
        Ty::Base(BaseType::Float) => "number".to_string(),
        Ty::Base(BaseType::Duration | BaseType::Instant) => "number".to_string(),
        // v0.110 (ADR 0142): `Bytes` erases to `Uint8Array`, not `number`.
        Ty::Base(BaseType::Bytes) => "Uint8Array".to_string(),
        // v0.157 (ADR 0183): a generic record instantiation renders as the
        // erased TS generic `Name<Arg, …>`; a non-generic named type is bare.
        Ty::Named { name, args, .. } if args.is_empty() => name.clone(),
        Ty::Named { name, args, .. } => format!(
            "{name}<{}>",
            args.iter().map(ts_ty).collect::<Vec<_>>().join(", ")
        ),
        Ty::Result(t, e) => format!("Result<{}, {}>", ts_ty(t), ts_ty(e)),
        Ty::Option(t) => format!("Option<{}>", ts_ty(t)),
        Ty::Effect(t) => match &**t {
            Ty::Unit => "Promise<void>".to_string(),
            other => format!("Promise<{}>", ts_ty(other)),
        },
        Ty::HttpResult(t) => format!("HttpResult<{}>", ts_ty(t)),
        Ty::List(t) => format!("readonly {}[]", ts_ty(t)),
        // v0.91 (ADR 0119): a `Query[T]` lowers to a deferred producer of its
        // elements — a thunk run by the terminal.
        Ty::Query(t) => format!("(() => readonly {}[])", ts_ty(t)),
        // v0.100: a `Stream[T]` lowers to a host async iterable.
        Ty::Stream(t) => format!("AsyncIterable<{}>", ts_ty(t)),
        // v0.102: a `Connection[F]` lowers to the runtime `Connection<F>` interface.
        Ty::Connection(t) => format!("Connection<{}>", ts_ty(t)),
        Ty::Map(k, v) => format!("ReadonlyMap<{}, {}>", ts_ty(k), ts_ty(v)),
        Ty::QueueResult => "QueueResult".to_string(),
        Ty::ValidationError => "ValidationError".to_string(),
        Ty::JsonError => "JsonError".to_string(),
        Ty::Unit => "void".to_string(),
        Ty::Fn { params, ret } => {
            let params: Vec<String> = params
                .iter()
                .enumerate()
                .map(|(i, p)| format!("a{i}: {}", ts_ty(p)))
                .collect();
            format!("({}) => {}", params.join(", "), ts_ty(ret))
        }
        Ty::Var(n) => n.clone(),
        // The identity type the actor binding yields (`name.identity`).
        Ty::Actor(id) => ts_ty(id),
        // v0.52: a resolved multi-actor sum lowers to a discriminated union
        // tagged by actor name; non-unit members carry their identity.
        Ty::ActorSum(members) => members
            .iter()
            .map(|(name, id)| match id {
                Ty::Unit => format!("{{ tag: \"{name}\" }}"),
                _ => format!("{{ tag: \"{name}\", identity: {} }}", ts_ty(id)),
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn ts_binop(op: BinOp) -> &'static str {
    match op {
        // `implies` has no single TS operator — `lower_bin_op` rewrites it to
        // `(!(P) || Q)` before reaching here, so this arm is never used.
        BinOp::Implies => "||",
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

/// The TypeScript spelling of a user identifier in a *binding or reference*
/// position (params, locals, function names, import names). Bynk identifiers
/// that are illegal as TS binding names — the JS reserved words plus the
/// strict-mode/module sets (emitted modules are always strict ESM) — and
/// names the emitter itself introduces alongside user bindings (`deps`) are
/// renamed into the generated-name namespace (`__id_<name>`), which the
/// parser keeps free of user identifiers. Property/field names never pass
/// through here: reserved words are legal there, and record field names are
/// wire format.
pub(crate) fn ts_ident(name: &str) -> String {
    const RESERVED: &[&str] = &[
        // ES reserved words.
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        // Strict-mode reserved (emitted modules are always strict).
        "implements",
        "interface",
        "let",
        "package",
        "private",
        "protected",
        "public",
        "static",
        "yield",
        // Module-code reserved.
        "await",
        // Illegal binding targets in strict mode.
        "arguments",
        "eval",
        // Generated identifiers a user binding may sit next to: handler
        // signatures append a `deps` parameter, so a user param named `deps`
        // would otherwise duplicate it.
        "deps",
    ];
    if RESERVED.contains(&name) {
        format!("__id_{name}")
    } else {
        name.to_string()
    }
}

pub(crate) fn escape_ts_string(s: &str) -> String {
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

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[test]
    fn runtime_emits_all_required_exports() {
        let s = emit_runtime_module();
        // Core types and constructors used by every emitted module.
        assert!(s.contains("export type Result<T, E>"));
        assert!(s.contains("export const Ok"));
        assert!(s.contains("export const Err"));
        assert!(s.contains("export type Option<T>"));
        assert!(s.contains("export const Some"));
        assert!(s.contains("export const None"));
        assert!(s.contains("export interface ValidationError"));
        // Durable Object surface used by agent classes.
        assert!(s.contains("export interface DurableObjectStorage"));
        assert!(s.contains("export interface DurableObjectState"));
        assert!(s.contains("export class InMemoryStorage"));
        assert!(s.contains("export function makeTestState"));
        // Discriminator must be `tag` to match emitted code.
        assert!(s.contains("tag: \"Ok\""));
        assert!(s.contains("tag: \"Err\""));
        assert!(s.contains("tag: \"Some\""));
        assert!(s.contains("tag: \"None\""));
    }

    #[test]
    fn tsconfig_is_well_formed_json() {
        let s = emit_tsconfig();
        // Spot-check the key fields; we don't reach for a JSON parser.
        assert!(s.contains("\"target\": \"ES2022\""));
        assert!(s.contains("\"strict\": true"));
        assert!(s.contains("\"include\""));
    }

    #[test]
    fn workers_dir_name_replaces_dots_with_dashes() {
        assert_eq!(
            crate::project::worker_dir_name("commerce.payment"),
            "commerce-payment"
        );
        assert_eq!(crate::project::worker_dir_name("a.b.c"), "a-b-c");
    }

    // Refactor track: characterisation pin for the canonical `escape_ts_string`.
    // It escapes backslash/quote/newline/tab and carriage return (`\r` → `\r`).
    #[test]
    fn escape_ts_string_escapes_cr() {
        assert_eq!(escape_ts_string("a\\b"), "a\\\\b");
        assert_eq!(escape_ts_string("a\"b"), "a\\\"b");
        assert_eq!(escape_ts_string("a\nb"), "a\\nb");
        assert_eq!(escape_ts_string("a\tb"), "a\\tb");
        assert_eq!(escape_ts_string("a\rb"), "a\\rb"); // CR escaped here; raw in project copy
    }

    #[test]
    fn runtime_import_depth_resolves_correctly() {
        assert_eq!(
            runtime_import_for(Path::new("compose.ts"), ImportExt::Js),
            "./runtime.js"
        );
        assert_eq!(
            runtime_import_for(Path::new("commerce/payment.ts"), ImportExt::Js),
            "../runtime.js"
        );
        assert_eq!(
            runtime_import_for(Path::new("commerce/orders/types.ts"), ImportExt::Js),
            "../../runtime.js"
        );
        assert_eq!(
            runtime_import_for(Path::new("tests/commerce_payment.test.ts"), ImportExt::Js),
            "../runtime.js"
        );
    }
}
