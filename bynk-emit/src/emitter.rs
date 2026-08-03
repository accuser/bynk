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
use std::sync::Arc;

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
pub(crate) mod events_fanout;
pub mod secrets;
pub(crate) mod serialisation;
pub(crate) mod workers;
pub(crate) mod workers_entry;
pub mod wrangler;

pub(crate) use events_fanout::emit_events_fanout_do;
pub(crate) use secrets::emit_secrets_manifest;
pub(crate) use workers::emit_worker_compose;
pub(crate) use workers_entry::emit_worker_entry;
pub(crate) use wrangler::emit_wrangler_toml;

mod lower;
pub(crate) mod runtime_use;
pub(crate) use runtime_use::RuntimeUse;
pub(crate) mod source_map;
pub(crate) use lower::*;
pub(crate) mod emit;
pub(crate) use bynk_check::icu::{self, *};
pub(crate) use bynk_check::websocket;
pub(crate) use emit::*;

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

/// Events track, slice 2 (spine #936): the TS type of one buffered/fanned-out
/// event — a handler's `__events` local, the `__eventsDispatch` deps field
/// (service, agent, provider, and the Bundle/Workers dispatch closures that
/// implement it), and the fan-out DO's `FanoutEvent` (`events_fanout.rs`) all
/// declare this same shape independently, with no shared type today. Routing
/// every Rust-side site through one constant means the envelope field can
/// never drift by being added at 8 of 9 of them. The two TypeScript runtime
/// sources that also declare it (`runtime/src/agent.ts`'s
/// `dispatchToEventsFanout`, `runtime/src/boundary.ts`'s `deliverEvent`) are
/// hand-edited files this constant cannot reach — keep them textually
/// identical to this shape by hand; `cargo test -p bynkc --test
/// events_workers_wiring` and `events_envelope_behaviour` both exercise every
/// hop and would fail on a real mismatch.
///
/// #973: `runtime/src/boundary.ts`'s `deserialiseEventEnvelope` is a related
/// but distinct hand-written piece — it validates the *inner* `envelope`
/// object's shape at the receiving `/_bynk/event/` route, not this outer
/// wire wrapper. Keep its field list in sync with the `envelope: { ... }`
/// portion of this shape by hand; nothing generates either from the other.
pub(crate) const EVENTS_WIRE_EVENT_TS_TYPE: &str = "{ type: string; payload: unknown; envelope: { eventId: string; publisherId: string; emittedAt: number; schemaVersion: number } }";

/// Emit the contents of `out/tsconfig.json`. The CLI uses `tsc -p` against
/// this when running `bynkc test`; users can also drive `tsc` against it
/// directly to produce JS for deployment.
pub fn emit_tsconfig() -> String {
    TSCONFIG_JSON.to_string()
}

/// The `bynkc test --coverage` variant (#854): the same config with `sourceMap`
/// enabled, so `tsc` emits the `.js.map`s the coverage remap consumes (hop 1,
/// `.js` → emitted `.ts`). Kept coverage-only rather than folded into the
/// default so a normal `bynkc test` / deployment `tsc` run ships no `.js.map`s.
/// The runner overwrites the default `out/tsconfig.json` with this before `tsc`.
pub fn emit_tsconfig_with_source_maps() -> String {
    TSCONFIG_JSON.replace(
        "\"outDir\": \"../out-js\",",
        "\"sourceMap\": true,\n    \"outDir\": \"../out-js\",",
    )
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

/// message-bundles slice 3 (#878): a message template's placeholders and
/// their ICU format kind (`"plain"`/`"plural"`/`"select"`/`"number"`/`"date"`),
/// sorted by placeholder name — for hover (`bynk-ide::symbols::describe_messages`).
/// A narrow, purpose-built public surface rather than exposing the internal
/// `icu` module's `FormatKind`/`IcuPlaceholder` types themselves, which stay
/// crate-private.
pub fn message_template_placeholder_summary(template: &str) -> Vec<(String, &'static str)> {
    icu::template_format_kinds(template)
        .into_iter()
        .map(|(name, kind)| (name.to_string(), kind.as_str()))
        .collect()
}

/// Compute the runtime import specifier for a module at `from_source`. For a
/// file at `commerce/payment.ts` the runtime sits two levels up, so this
/// returns `../runtime.js`; for a top-level file it returns `./runtime.js`.
pub(crate) fn runtime_import_for(from_source: &Path, ext: ImportExt) -> String {
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
pub(crate) fn emit(commons: &TypedCommons) -> String {
    // Emit the body first so the header can decide which runtime helpers to
    // import from what the body actually referenced (v0.110: the `__bynkBytes*`
    // helpers are imported only when a `Bytes` value is constructed/compared).
    // "What it referenced" comes from `dummy_ctx.runtime_use`, which the `Bytes`
    // lowerings write as they emit — not from scanning `body` for the helper's
    // name, which a user string literal or doc comment could also contain.
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
            emit_free_fn(&mut body, f, commons, None, false, &dummy_ctx.runtime_use);
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
    write_header_single(&mut out, commons, dummy_ctx.runtime_use.bytes(), uses_http);
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
        file_decl_index: crate::project::FileDeclIndex {
            types: HashMap::new(),
            fns: HashMap::new(),
            methods: HashMap::new(),
        },
        imported_from: HashMap::new(),
        imported_from_kind: HashMap::new(),
        imported_decl_paths: HashMap::new(),
        unit_kind: UnitKind::Commons,
        owning_context: None,
        exports_for_consumed: HashMap::new(),
        cross_context: bynk_check::resolver::CrossContextInfo::default(),
        target: BuildTarget::Bundle,
        local_agents: HashSet::new(),
        agent_given_deps: HashMap::new(),
        extra_import_lines: Vec::new(),
        agent_method_givens: HashMap::new(),
        actors: HashMap::new(),
        event_schema_versions: HashMap::new(),
        consumed_adapters: HashSet::new(),
        history_target_agents: HashSet::new(),
        imported_methods: HashMap::new(),
        runtime_use: Default::default(),
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
pub(crate) fn emit_project(
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
    // Events track, slice 0 (spine #936): an `event` is checker-visible as
    // a type (via `EventDecl::as_type_decl`, so exports/consumes/
    // construction all worked from day one), but nothing emitted its actual
    // TS declaration — this loop only ever matched `CommonsItem::Type`, so
    // a subscriber importing an event type across contexts (`from
    // Events(E)`, or `E` named in a cross-context signature) got a real
    // `tsc` "has no exported member" error. Reuses the identical synthetic
    // `TypeDecl` the checker already builds.
    for item in &commons.commons.items {
        if let CommonsItem::Event(e) = item {
            let t = e.as_type_decl();
            smb.borrow_mut().record(out.len(), t.span);
            emit_type(&mut out, &t, commons, ctx);
        }
    }
    for item in &commons.commons.items {
        if let CommonsItem::Fn(f) = item
            && let FnName::Free(_) = &f.name
        {
            smb.borrow_mut().record(out.len(), f.span);
            emit_free_fn(
                &mut out,
                f,
                commons,
                Some(&smb),
                ctx.contracts,
                &ctx.runtime_use,
            );
        }
    }
    // message-bundles slice 2 (#874): every `messages` block in the commons
    // is emitted together, once, as a single multi-locale bundle — not
    // per-item like the other behavioural kinds below — so the generated
    // `render` can dispatch across every declared locale's own table rather
    // than reading only the `@reference` one (slice 1's scope). Recorded at
    // the `@reference` block's own span, matching how a single-item emission
    // records at that item's span elsewhere in this loop.
    let messages_blocks: Vec<&MessagesDecl> = commons
        .commons
        .items
        .iter()
        .filter_map(|item| match item {
            CommonsItem::Messages(m) => Some(m),
            _ => None,
        })
        .collect();
    if let Some(reference) = messages_blocks
        .iter()
        .find(|m| m.annotations.iter().any(|a| a.name.name == "reference"))
    {
        smb.borrow_mut().record(out.len(), reference.span);
        emit_messages_bundle(&mut out, &messages_blocks, reference, &ctx.runtime_use);
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
    // Both injections below key on `ctx.runtime_use`, which the producers wrote as
    // they emitted. They used to key on `out.contains("<helper name>")`, which was
    // wrong in both directions: `out` also carries user string literals and doc
    // comments (a spurious import), and the ICU scan additionally depended on the
    // call being emitted with no space before its paren — so an unrelated
    // formatting change could silently drop a *required* import and produce a
    // module that does not compile. See `emitter::runtime_use`.
    // v0.110 (ADR 0142): import the `Bytes` runtime helpers iff the emitted body
    // actually references them. Injected into the existing runtime import line
    // (no new line, no body-column shift), so the source map computed above from
    // the pre-injection text stays valid.
    if ctx.runtime_use.bytes() {
        out = inject_runtime_imports(
            out,
            &runtime_import_for(&ctx.source_path, ctx.import_ext),
            BYTES_RUNTIME_IMPORTS,
        );
    }
    // message-bundles slice 3 (#878, Decision G): same mechanism, for the
    // three ICU-formatting runtime helpers. `emit_messages_bundle` (called
    // above, before this post-pass) is the only place that can reference
    // them; a project with no `plural`/`select`/`number`/`date` placeholder
    // anywhere never triggers this. All three are imported together
    // (mirrors `BYTES_RUNTIME_IMPORTS`'s own all-or-nothing shape) rather
    // than cherry-picked per-name — the emitted `tsconfig.json` has no
    // `noUnusedLocals`, so an unused named import is inert.
    if ctx.runtime_use.icu() {
        out = inject_runtime_imports(
            out,
            &runtime_import_for(&ctx.source_path, ctx.import_ext),
            MESSAGES_RUNTIME_IMPORTS,
        );
    }
    (out, source_map)
}

/// v0.110 (ADR 0142): append a set of runtime helpers to a module's existing
/// runtime import. Done as a post-pass so the decision keys on what the body
/// references, without a second emission or a source-map-shifting reorder.
/// Generalised in message-bundles slice 3 (#878) from a `Bytes`-only helper
/// to take `extra` as a parameter, shared with the ICU-formatting helpers.
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
/// named `runtime` — or anything like `"./my-runtime.js"` — and appending
/// `extra`'s bindings to that import would produce an unresolved export. The
/// caller already knows the exact path it emitted, so there is no reason to
/// guess.
pub(crate) fn inject_runtime_imports(out: String, runtime_specifier: &str, extra: &str) -> String {
    let mut result = String::with_capacity(out.len() + extra.len());
    let mut injected = false;
    let from_runtime = format!(" }} from \"{runtime_specifier}\"");
    for line in out.split_inclusive('\n') {
        if !injected
            && line.starts_with("import {")
            && line.contains(&from_runtime)
            && let Some(pos) = line.rfind(&from_runtime)
        {
            result.push_str(&line[..pos]);
            result.push_str(&missing_bindings(&line[..pos], extra));
            result.push_str(&line[pos..]);
            injected = true;
            continue;
        }
        result.push_str(line);
    }
    result
}

/// The subset of `extra` not already bound on `existing` — the head of an import
/// line, e.g. `import { Ok, Err, type Result`.
///
/// #914: an injection target may already import some of what a group carries. The
/// test-scaffold module lists `Ok`/`Err` in its fixed set but not `BoundaryError`,
/// so injecting the boundary group wholesale would emit `import { Ok, …, Ok, … }` —
/// a duplicate-identifier error, i.e. trading one uncompilable module for another.
/// Comparing on the bare name lets `type BoundaryError` match an existing
/// `BoundaryError` and vice versa.
///
/// Invariant: a group is a list of plain bindings, optionally `type`-prefixed —
/// **never an alias**. `bare("Foo as Ok")` is the whole phrase, so an aliased
/// binding on either side would compare unequal and inject a duplicate. No group
/// carries one today; keep it that way rather than teaching this to split on
/// `as`.
fn missing_bindings(existing: &str, extra: &str) -> String {
    fn bare(binding: &str) -> &str {
        binding
            .trim()
            .strip_prefix("type ")
            .unwrap_or(binding.trim())
    }
    let present: HashSet<&str> = existing
        .strip_prefix("import {")
        .unwrap_or(existing)
        .split(',')
        .map(bare)
        .collect();
    let wanted: Vec<&str> = extra
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty() && !present.contains(bare(b)))
        .collect();
    if wanted.is_empty() {
        String::new()
    } else {
        format!(", {}", wanted.join(", "))
    }
}

/// v0.22b: pre-order expression visitor — visits `e`, then every
/// sub-expression, including statements and tails of nested blocks. Driven by
/// `ast::expr_children`, the exhaustive total child iterator, rather than a
/// hand-matched recursion duplicating it — a new `ExprKind` variant fails to
/// compile in `expr_children` until it is taught to visit it, instead of
/// silently under-visiting here.
pub(crate) fn walk_exprs(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    for child in expr_children(e) {
        walk_exprs(child, f);
    }
}

/// v0.79: does this block contain a `~>` send anywhere — including nested
/// branches, match arms, and lambdas? Gates execution-context threading
/// (`deps.__exec`) so a context that never sends keeps byte-identical output.
///
/// A `~>` send is a [`Statement`] variant, not an [`ExprKind`] one, and a bare
/// `{ … }` block is only parseable in a handful of positions (an `if`/`else`
/// body, a `match` arm, a lambda body) — never as an arbitrary sub-expression
/// — so `Block`/`If`/`Match`/`Lambda` were already the complete reachable set
/// and the old `_ => false` tail never actually dropped a send. It is
/// rewritten to recurse over `expr_children`, the total child iterator,
/// anyway: a `Statement`-only construct like this is exactly the shape that
/// silently drifts if a later `ExprKind` variant *does* start admitting a
/// nested block and this list isn't updated to match — see
/// `block_writes_state`, whose traversal was converted alongside this one for
/// the same reason. Both now also enumerate `ExprKind` explicitly instead of
/// ending in a `_` arm, so that drift is a build failure rather than a silent
/// miss.
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
            // No variant below carries a `Block` *field*, so `expr_children`'s
            // total descent is complete for it — a block reached through a
            // child (a braced lambda body, say) comes back as an `Expr` and
            // re-enters this match at the `Block` arm above. A *new* variant
            // that holds a `Block` directly must be hand-matched up there
            // alongside `Block`/`If`/`Match`: appending it here loses the
            // `Statement::Send` tag (`expr_children` flattens a block to its
            // statements' values), and with it `deps.__exec` threading for a
            // context that does send.
            ExprKind::IntLit { .. }
            | ExprKind::FloatLit { .. }
            | ExprKind::DurationLit { .. }
            | ExprKind::StrLit(_)
            | ExprKind::InterpStr(_)
            | ExprKind::BoolLit(_)
            | ExprKind::Ident(_)
            | ExprKind::Call { .. }
            | ExprKind::Lambda(_)
            | ExprKind::BinOp(..)
            | ExprKind::UnaryOp(..)
            | ExprKind::Paren(_)
            | ExprKind::Ok(_)
            | ExprKind::Err(_)
            | ExprKind::Question(_)
            | ExprKind::ConstructorCall { .. }
            | ExprKind::RecordConstruction { .. }
            | ExprKind::FieldAccess { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::Is { .. }
            | ExprKind::Some(_)
            | ExprKind::None
            | ExprKind::UnitLit
            | ExprKind::RecordSpread { .. }
            | ExprKind::EffectPure(_)
            | ExprKind::Expect(_)
            | ExprKind::Val { .. }
            | ExprKind::Wire(_)
            | ExprKind::ListLit(_)
            | ExprKind::Observation(_)
            | ExprKind::Trace { .. } => expr_children(e).into_iter().any(expr),
        }
    }
    b.statements.iter().any(stmt) || expr(&b.tail)
}

/// Events track, slice 0 (spine #936): does this block contain an
/// `Events.emit[...]` call anywhere — including nested branches, match arms,
/// lambdas, and any other expression position (a `Paren`, an `Ok`/`Err`
/// wrapper, a `Call`/`RecordConstruction` argument, a `BinOp` operand, …)?
/// Gates release-at-commit buffer threading (`deps.__events`) so a handler
/// that never emits keeps byte-identical output, mirroring `block_uses_send`'s
/// gate on `deps.__exec`.
///
/// Driven off the exhaustive `walk_block_exprs`/`walk_exprs` visitor rather
/// than a hand-rolled `ExprKind` match — a bespoke match here previously
/// covered only `MethodCall`/`Block`/`If`/`Match`/`Lambda` and silently
/// disagreed with `lower_expr_into` (which recurses into every expression
/// position), so `do (Events.emit[E](event))` — one added paren — compiled
/// clean but emitted a body that referenced an undeclared `__events` local
/// (`tsc`-only failure, no bynk diagnostic). Riding the walker means this
/// can't drift from the lowering again: a new `ExprKind` variant fails to
/// compile here until `walk_exprs` itself is taught to visit it.
///
/// Syntactic, like `block_uses_send`: matches a bare-`Events`-receiver
/// `.emit` call by name, not by resolving the receiver against `given` — a
/// locally-shadowed `Events` would be a false positive, an accepted
/// approximation matching `block_uses_send`'s own precedent (it doesn't
/// verify `~>`'s target either).
pub(crate) fn block_uses_emit(b: &Block) -> bool {
    fn is_events_emit_call(receiver: &Expr, method: &Ident) -> bool {
        matches!(&receiver.kind, ExprKind::Ident(id) if id.name == "Events")
            && method.name == "emit"
    }
    let mut found = false;
    walk_block_exprs(b, &mut |e| {
        if !found
            && let ExprKind::MethodCall {
                receiver, method, ..
            } = &e.kind
            && is_events_emit_call(receiver, method)
        {
            found = true;
        }
    });
    found
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
    // `Block`/`If`/`Match` stay hand-matched so crossing a nested block
    // re-enters the statement-aware `block_writes_state` — an
    // `expr_children` descent flattens a block straight to its statements'
    // *values*, losing exactly the `Statement::Assign` tag `stmt` above
    // checks for. Everywhere else recurses over `expr_children`, the total
    // child iterator, rather than the previous `Paren`/`MethodCall`/`Call`/
    // `Lambda`-only list with a `_ => false` tail: the domain's `Effect`
    // typing means a mutating op can only actually reach those four
    // positions today, so this isn't a live-bug fix the way the `Lambda` arm
    // was — it closes the same *class* of gap pre-emptively, the way
    // `block_uses_send` and `walk_exprs` needed to for gaps that are live.
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
            // No variant below carries a `Block` *field*, so `expr_children`'s
            // total descent is complete for it — a block reached through a
            // child (a braced lambda body, say) comes back as an `Expr` and
            // re-enters this match at the `Block` arm above. A *new* variant
            // that holds a `Block` directly must be hand-matched up there
            // alongside `Block`/`If`/`Match`: appending it here loses the
            // `Statement::Assign` tag (`expr_children` flattens a block to its
            // statements' values), and with it the end-of-handler commit flush.
            ExprKind::IntLit { .. }
            | ExprKind::FloatLit { .. }
            | ExprKind::DurationLit { .. }
            | ExprKind::StrLit(_)
            | ExprKind::InterpStr(_)
            | ExprKind::BoolLit(_)
            | ExprKind::Ident(_)
            | ExprKind::Call { .. }
            | ExprKind::Lambda(_)
            | ExprKind::BinOp(..)
            | ExprKind::UnaryOp(..)
            | ExprKind::Paren(_)
            | ExprKind::Ok(_)
            | ExprKind::Err(_)
            | ExprKind::Question(_)
            | ExprKind::ConstructorCall { .. }
            | ExprKind::RecordConstruction { .. }
            | ExprKind::FieldAccess { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::Is { .. }
            | ExprKind::Some(_)
            | ExprKind::None
            | ExprKind::UnitLit
            | ExprKind::RecordSpread { .. }
            | ExprKind::EffectPure(_)
            | ExprKind::Expect(_)
            | ExprKind::Val { .. }
            | ExprKind::Wire(_)
            | ExprKind::ListLit(_)
            | ExprKind::Observation(_)
            | ExprKind::Trace { .. } => expr_children(e).into_iter().any(|c| expr(c, m)),
        }
    }
    b.statements.iter().any(|s| stmt(s, m)) || expr(&b.tail, m)
}

pub(crate) fn walk_block_exprs(b: &Block, f: &mut impl FnMut(&Expr)) {
    let mut exprs = Vec::new();
    for s in &b.statements {
        statement_exprs(s, &mut exprs);
    }
    exprs.push(&b.tail);
    for e in exprs {
        walk_exprs(e, f);
    }
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
        // An `event` registers into the `types` table and is checked over
        // the same record-field path as `CommonsItem::Type`'s `Record` arm.
        CommonsItem::Event(e) => e.body.fields.iter().any(|f| in_type_ref(&f.type_ref)),
        CommonsItem::Actor(_) | CommonsItem::Messages(_) => false,
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
            // v0.157 (ADR 0183): recurse into a generic application's arguments.
            TypeRef::App { args, .. } => args.iter().any(in_type_ref),
            TypeRef::Base(..)
            | TypeRef::Named(_)
            | TypeRef::QueueResult(_)
            | TypeRef::ValidationError(_)
            | TypeRef::JsonError(_)
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
        // An `event` registers into the `types` table and is checked over
        // the same record-field path as `CommonsItem::Type`'s `Record` arm.
        CommonsItem::Event(e) => e.body.fields.iter().any(|f| in_type_ref(&f.type_ref)),
        CommonsItem::Actor(_) | CommonsItem::Messages(_) => false,
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
            | TypeRef::History(a, _)
            | TypeRef::List(a, _) => in_type_ref(a),
            TypeRef::Fn(params, ret, _) => params.iter().any(in_type_ref) || in_type_ref(ret),
            // v0.157 (ADR 0183): recurse into a generic application's arguments.
            TypeRef::App { args, .. } => args.iter().any(in_type_ref),
            TypeRef::Base(..)
            | TypeRef::Named(_)
            | TypeRef::QueueResult(_)
            | TypeRef::ValidationError(_)
            | TypeRef::JsonError(_)
            | TypeRef::Unit(_) => false,
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
        // A `Connection` is a held resource storable only in a `store` field
        // (handled above) — never in a plain record field, so `Type`/`Event`
        // need no case here; `Actor`/`Messages` carry no `TypeRef` at all.
        CommonsItem::Type(_)
        | CommonsItem::Actor(_)
        | CommonsItem::Messages(_)
        | CommonsItem::Event(_) => false,
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
    emit_helpers_for_owner(
        out,
        &names,
        &commons.types,
        &ctx.commons_name,
        &ctx.runtime_use,
    );
    let insts: Vec<serialisation::GenericInst> = insts
        .into_iter()
        .filter(|i| !skip_insts.contains(&i.ts_name()))
        .collect();
    if !insts.is_empty() {
        emit_generic_helpers(out, &insts, &commons.types, &ctx.runtime_use);
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
            &ctx.runtime_use,
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
        //
        // #977: the field walk follows `local_boundary`, not `boundary_types_all`
        // — the same narrowing `emit_helpers_for_owner` applies just above, and
        // for the same reason. A boundary type this context does not *declare* is
        // either commons-owned (its codec, and its own instantiations, come from
        // the commons module) or consumed (its codec is regenerated below by
        // `emit_consumed_context_helpers`, whose `Qual` map reaches the owner's
        // `import type * as <ns>` alias). Walking a foreign type's fields here
        // emitted its instantiations *unqualified* — `Option<Region>` for a
        // consumed `Region` — and then seeded `emitted_insts` so the qualified
        // pass below skipped it, leaving `tsc --strict` with `TS2304: Cannot find
        // name 'Region'`. Handler signatures still walk in full: a *local*
        // handler naming `Option[ConsumedRegion]` directly is this module's own
        // boundary either way.
        let insts =
            collect_generic_instantiations(&services, &agents, &local_boundary, &commons.types);
        emit_generic_helpers(out, &insts, &commons.types, &ctx.runtime_use);

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
        emit_helpers_for_owner(
            out,
            &locally,
            &commons.types,
            ctx.commons_name.as_str(),
            &ctx.runtime_use,
        );
        let insts = collect_generic_instantiations(
            &HashMap::new(),
            &HashMap::new(),
            &locally,
            &commons.types,
        );
        emit_generic_helpers(out, &insts, &commons.types, &ctx.runtime_use);
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

    // #973: this context's own `from Events(E)` subscriptions, keyed by the
    // consumed context that declares each `E` — a subscriber calls no method
    // on the publisher, so `called_consumed_services` alone would never see
    // it, and the `continue` below on an empty `called_here` would skip the
    // event's payload type entirely (the root cause of #973: a subscriber's
    // generated module had no `deserialise_<Payload>` at all).
    let mut consumed_event_roots: HashMap<String, Vec<bynk_syntax::ast::TypeRef>> = HashMap::new();
    for item in &commons.commons.items {
        let CommonsItem::Service(svc) = item else {
            continue;
        };
        let bynk_syntax::ast::ServiceProtocol::Events { event_type, .. } = &svc.protocol else {
            continue;
        };
        let bynk_syntax::ast::TypeRef::Named(id) = event_type else {
            continue;
        };
        for (c, names) in &info.consumed_event_names {
            if names.contains(&id.name) {
                consumed_event_roots
                    .entry(c.clone())
                    .or_default()
                    .push(event_type.clone());
            }
        }
    }

    let empty_svcs: HashMap<String, bynk_check::resolver::CrossContextService> = HashMap::new();
    let empty_called: HashSet<String> = HashSet::new();
    let empty_event_roots: Vec<bynk_syntax::ast::TypeRef> = Vec::new();

    let mut consumed_keys: HashSet<&String> = info.consumed_services.keys().collect();
    consumed_keys.extend(consumed_event_roots.keys());
    let mut consumed_keys: Vec<&String> = consumed_keys.into_iter().collect();
    consumed_keys.sort();
    for c in consumed_keys {
        let svcs = info.consumed_services.get(c).unwrap_or(&empty_svcs);
        let event_roots = consumed_event_roots.get(c).unwrap_or(&empty_event_roots);
        if svcs.is_empty() && event_roots.is_empty() {
            continue;
        }
        let called_here = called.get(c).unwrap_or(&empty_called);
        if called_here.is_empty() && event_roots.is_empty() {
            continue;
        }
        let Some(types_table) = info.consumed_types.get(c) else {
            continue;
        };
        // The callee's exports — the set of types it *owns* and a consumer may
        // name. A closure type outside this set (a commons type the callee only
        // `uses`, e.g. `Money`) is the caller's own already, not the callee's to
        // hand out, so the caller never regenerates it under the callee's ns.
        let exports = ctx.exports_for_consumed.get(c);
        let owned = |n: &str| exports.is_some_and(|e| e.contains_key(n));

        // Roots: every called service's parameter and return types, plus (#973)
        // any event type this context subscribes to from `c` — a subscriber
        // participates in the event's contract as its receiving half, so its
        // payload is not an uncalled surface the way an unreached method is
        // (the narrowing this loop otherwise applies, mirroring ADR 0200
        // Decision E one layer up, at `called_consumed_services` above).
        let mut svc_names: Vec<&String> =
            svcs.keys().filter(|s| called_here.contains(*s)).collect();
        svc_names.sort();
        let mut roots: Vec<bynk_syntax::ast::TypeRef> = event_roots.clone();
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
            &ctx.runtime_use,
        );
        consumed_names_out.extend(to_emit);

        let to_emit_insts: Vec<serialisation::GenericInst> = cinsts
            .into_iter()
            .filter(|i| emitted_insts.insert(i.ts_name()))
            .collect();
        for i in &to_emit_insts {
            consumed_insts_out.push(i.ts_name());
        }
        emit_generic_helpers_qualified(out, &to_emit_insts, types_table, &qual, &ctx.runtime_use);
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
        if let Some(base) = commons
            .types
            .get(name)
            .and_then(|d| refined_or_opaque_base(d))
        {
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
    // A `messages` block declares no importable identifier of its own (its
    // `render` is synthesised separately), so `name()` is `None` there and it
    // contributes nothing to the local-name set.
    let local_to_file: HashSet<String> = commons
        .commons
        .items
        .iter()
        .filter_map(|i| i.name().map(|n| n.name.clone()))
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
            // Events track, slice 0 (spine #936): an `event`'s field types
            // are collected exactly like a `type`'s, via the same synthetic
            // `TypeDecl` `EventDecl::as_type_decl` builds.
            CommonsItem::Event(e) => {
                collect_refs_in_type_decl(&e.as_type_decl(), &local_to_file, ctx, &mut refs);
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
            // `MessageEntry.code`/`.template` are plain string literals with
            // no TypeRefs/exprs of their own to walk — but the generated
            // `render` (emit_messages) has a signature and body that name
            // `LocaleTag`/`Message`/`MessageArg` even though no expression in
            // this file's *source* does, so those three are registered here
            // by hand, the same way a real reference would be. `render`/
            // `renderArg` are deliberately NOT registered this way —
            // `render` collides with the generated function of the same
            // name, and both are instead imported together under
            // `emit_unit`'s (project.rs) hand-written, aliased extra import
            // line, bypassing this dedup/merge path entirely (importing
            // `renderArg` there too, alongside the aliased `render`, avoids a
            // duplicate import of it from here).
            CommonsItem::Messages(_) => {
                for name in ["LocaleTag", "Message", "MessageArg"] {
                    record_name_ref(name, &local_to_file, ctx, &mut refs);
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
            } = pattern.as_ref()
            {
                record_name_ref(&tn.name, local_to_file, ctx, out);
            }
        }
    }
}

/// If `name` at `span` is a bare reference to a variant of a sum type (per
/// the checker's expression type), return the owning sum's name — the same
/// test the lowering uses to qualify it as `Type.Variant` (see the
/// `ExprKind::Ident` arm of `lower_expr_into`).
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
    // Events track, slice 0 (spine #936): the bare event-type names this
    // context's own `from Events(E)` service headers name — see the
    // Workers type-only-import narrowing below.
    let subscribed_event_type_names: HashSet<String> = commons
        .commons
        .items
        .iter()
        .filter_map(|item| match item {
            CommonsItem::Service(s) => match &s.protocol {
                ServiceProtocol::Events {
                    event_type: TypeRef::Named(id),
                    ..
                } => Some(id.name.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
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
                let is_subscribed_event_type = ctx.target == BuildTarget::Workers
                    && subscribed_event_type_names.contains(n.as_str());
                if ctx.unit_kind == UnitKind::Context
                    && from_kind == Some(UnitKind::Commons)
                    && commons.types.contains_key(n.as_str())
                {
                    parts.push(format!("{n} as __Commons{n}"));
                } else if is_subscribed_event_type {
                    // Events track, slice 0 (spine #936): under Workers, a
                    // context deploys as its own separate Worker script —
                    // there is no shared module graph to import a peer
                    // context's *value* across (the #661 hazard this
                    // mirrors: a caller generates its own codec rather than
                    // importing the callee's runtime code). `from
                    // Events(E)`'s `E` is the one plain named type crossing
                    // a context boundary directly by name (every other
                    // cross-context reference goes through a generated
                    // Service-Binding codec instead) — used only in type
                    // position (`e: E`), so this specific name is type-only.
                    // Narrowly scoped to event types specifically, not every
                    // cross-context import: a `type`/`enum` crossing via
                    // `uses`/`consumes` (e.g. `bynk`'s `Method`) is often
                    // used as a *value* too (`Method.Get`), which a blanket
                    // `import type` would wrongly break.
                    parts.push(format!("type {}", ts_ident(n)));
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
        // Events track, slice 0 (spine #936): an agent whose own handler body
        // emits directly needs its Workers-mode DO fetch dispatcher to
        // rebuild `deps.__eventsDispatch` from `env.EVENTS_FANOUT` (mirrors
        // the `#527` `given`-provider rebuild — see `emit_agent`) — a
        // function does not survive the JSON wire any better than a
        // provider does.
        let has_agent_uses_emit = workers
            && commons.commons.items.iter().any(|i| match i {
                CommonsItem::Agent(a) => a.handlers.iter().any(|h| block_uses_emit(&h.body)),
                _ => false,
            });
        if has_agent_uses_emit {
            parts.push("dispatchToEventsFanout");
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
pub(crate) const BYTES_RUNTIME_IMPORTS: &str =
    ", __bynkBytesEqual, __bynkBytesToBase64, __bynkBytesFromBase64, __bynkBytesDecodeUtf8";

/// message-bundles slice 3 (#878, Decision G): the ICU-formatting runtime
/// helpers, appended to a module's import list when an emitted `messages`
/// bundle's `render` references any of them (`emit_icu_placeholder`,
/// `bynk-emit/src/emitter/emit.rs`).
const MESSAGES_RUNTIME_IMPORTS: &str = ", selectPluralArm, formatIcuNumber, formatIcuDate";

/// #914: the names an **inlined** boundary deserialiser builds directly, appended
/// to the import list of a module that curates its own — a Worker's `compose.ts`
/// and the test-scaffold modules. Every other module imports `Ok`/`Err`/`Result`
/// unconditionally, so most of this is inert there and never applied.
///
/// A codec for a *named* type delegates (`handlers.deserialise_Order(…)`) and needs
/// none of these; one for a base type or a `Bytes` inlines the construction. Two
/// arms — `Unit` and the runtime-owned error types — additionally annotate the
/// result (`Ok(undefined) as Result<void, BoundaryError>`), which `compose.ts`'s
/// structural list never carries; `Result` is in the group for those. The dedupe
/// in [`inject_runtime_imports`] makes it free wherever it is already imported.
pub(crate) const BOUNDARY_CODEC_RUNTIME_IMPORTS: &str =
    ", Ok, Err, type Result, type BoundaryError";

/// #914: the names the `Json.decode[T]` wrapper puts in the module — its own
/// `Result<T, JsonError>` signature and the `JsonValue` it parses into
/// (`lower_json_codec_call`, `bynk-emit/src/emitter/lower.rs`).
///
/// A sibling group rather than part of [`BOUNDARY_CODEC_RUNTIME_IMPORTS`]: the
/// producer is the wrapper, not the codec, and it names these whichever arm the
/// inner deserialiser takes — including the delegating ones, which set no
/// boundary-codec flag at all. `Json.encode` needs nothing for its own wrapper
/// text either (it lowers to a bare `JSON.stringify`).
///
/// The delegating arm — `Json.decode[SomeRecord]` / `Json.encode(someRecord)`
/// — used to be broken in a test-scaffold module for an unrelated reason
/// (issue #917): the call lowers to a bare `deserialise_SomeRecord(…)` /
/// `serialise_SomeRecord(…)`, and no such codec was emitted anywhere — the
/// unit module exports the record's interface and no codec of its own. Fixed
/// by generating the test module's *own* closure for every root a case body's
/// `Json` call reaches for (`RuntimeUse::note_json_codec_root`, drained by
/// `tests_emit.rs`'s `emit_test_module`), namespace-qualifying the TS type
/// positions through the target/`uses` unit's own namespace import
/// (`RuntimeUse::json_codec_qual`) — the same caller-generates-its-own-codec
/// pattern `emit_consumed_context_helpers` uses for a workers cross-context
/// caller's consumed-boundary types (#661). See
/// `918_json_decode_in_test_case` (base type, delegation-free) and
/// `919_json_decode_named_record_in_test_case` (named record, delegating).
pub(crate) const JSON_CODEC_RUNTIME_IMPORTS: &str =
    ", Ok, Err, type Result, type JsonValue, type JsonError";

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
pub(crate) fn agent_factory_name(agent: &str) -> String {
    format!("__make{agent}")
}

/// Lowering state that is genuinely **module-invariant**: the same value at
/// every body-lowering site within one emitted module, and never written by the
/// recursive lowering itself. Grouped out of [`LowerCtx`] so a new lowering kind
/// inherits the whole set wholesale instead of re-deriving each default.
///
/// Nothing the lowering mutates *per body* may move in here — see the
/// scratch-state fields at the bottom of [`LowerCtx`]. A `ModuleCtx` is still
/// built fresh alongside each `LowerCtx` today, but filing a per-body counter
/// under a name that says "module" is exactly how such state starts leaking
/// between handler bodies.
pub(crate) struct ModuleCtx<'a> {
    /// Typed-commons handle (used to look up receiver types for method-call
    /// UFCS lowering).
    commons: &'a TypedCommons,
    /// Cross-context info for v0.6 cross-context call lowering.
    cross_context: &'a bynk_check::resolver::CrossContextInfo,
    /// The emitted module's conditional-runtime-helper accumulator.
    ///
    /// The `Bytes` lowerings (kernel, `==`, base64 codec) call
    /// [`RuntimeUse::note_bytes`] through this, so the module's import line is
    /// decided from what lowering actually emitted rather than by scanning the
    /// generated text for the helper's own name.
    ///
    /// Required rather than optional: a missing accumulator would make
    /// `note_bytes` a silent no-op, which is exactly the failure this replaces —
    /// a module that references `__bynkBytesEqual` without importing it. A
    /// lowering whose imports are decided elsewhere (the test scaffolds) owns a
    /// throwaway one, which reads as the deliberate choice it is.
    runtime_use: &'a RuntimeUse,
    /// v0.8 build target. In workers mode cross-context calls lower to
    /// `callService(...)` instead of `deps.surface.<key>.<method>(...)`.
    target: BuildTarget,
    /// #527: agent → method → the method's `given` caps (mirrors
    /// [`crate::project::EmitProjectCtx::agent_method_givens`]). Consulted by
    /// the agent-call lowering to record capability requirements.
    agent_method_givens: HashMap<String, HashMap<String, Vec<bynk_syntax::ast::CapRef>>>,
    /// Events slice 3b (#978): each locally-declared event's resolved
    /// `@schema(N)` version (mirrors
    /// [`crate::project::EmitProjectCtx::event_schema_versions`]). Default-
    /// empty like `agent_method_givens`, not a required constructor
    /// parameter like `runtime_use` — a miss here degrades to `schemaVersion:
    /// 1`, exactly every event's behaviour before this map existed, not a
    /// hard failure the way a missing `runtime_use` would be.
    event_schema_versions: HashMap<String, i64>,
    /// #527: type names this context *rebrands* (`uses`-imported commons
    /// types re-exported as `T & { __ctxBrand }`). Drives brand-assertion
    /// casts where unbranded commons values meet branded local positions.
    rebranded_types: HashSet<String>,
    /// #527: fn names imported from a commons. Such a fn's signature uses the
    /// *unbranded* commons types, so calls whose return mentions a rebranded
    /// type are asserted back into the local (branded) namespace.
    commons_imported_fns: HashSet<String>,
    /// #934: true when the unit being emitted is the reserved first-party
    /// `bynk` adapter itself. `bynk` is a reserved namespace, so a capability
    /// literally named `Idempotency` declared *in this unit* is unambiguously
    /// the real one — used alongside `CrossContextInfo::flattened_caps` (the
    /// consumed-from-elsewhere case) to confirm a flattened `Idempotency`
    /// call is genuinely first-party before scoping its key, not a same-named
    /// capability some other adapter or context happens to declare.
    in_bynk_unit: bool,
}

impl<'a> ModuleCtx<'a> {
    fn new(
        commons: &'a TypedCommons,
        cross_context: &'a bynk_check::resolver::CrossContextInfo,
        runtime_use: &'a RuntimeUse,
    ) -> Self {
        Self {
            commons,
            cross_context,
            runtime_use,
            target: BuildTarget::Bundle,
            agent_method_givens: HashMap::new(),
            event_schema_versions: HashMap::new(),
            rebranded_types: HashSet::new(),
            commons_imported_fns: HashSet::new(),
            in_bynk_unit: false,
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
}

/// The lowering state shared by the four **capability-bearing** body kinds — a
/// service handler, a composed provider op, an agent handler, and a websocket
/// lifecycle DO method. Every other kind carries no `HandlerShared` at all, and
/// the [`LowerCtx`] accessors below hand those kinds the same defaults the flat
/// struct used to give them (an empty capability set, no scope, `deps`).
pub(crate) struct HandlerShared {
    /// Names of capabilities in scope as `given C1, C2, ...`. Used to lower
    /// `Capability.op(args)` calls to `deps.Capability.op(args)`.
    capabilities: HashSet<String>,
    /// #934: the calling handler's own qualified name (`<unit>.<service or
    /// agent>.<handler>`, e.g. `shop.reserve.ordering.call`). Read only by the
    /// `Idempotency.dedup`/`remember` lowering, which prefixes the
    /// developer-supplied key with it so two unrelated handlers using the same
    /// literal key never collide (design/tracks/idempotency-capability.md
    /// §3.4). `None` anywhere a capability call cannot occur (a plain method, a
    /// free fn, an invariant/transition predicate, a static field initialiser) —
    /// those kinds carry no `HandlerShared`, and the accessor reports `None`.
    handler_scope: Option<String>,
    /// Events track, slice 2 (spine #936): the qualified name of the unit
    /// this body is emitted into (`ctx.commons_name`), read by the
    /// `Events.emit[E](event)` lowering to mint the envelope's
    /// `publisherId`. Context-scoped rather than agent-scoped: `Events.emit`
    /// is legal from a plain, keyless service handler with no agent
    /// instance to report, so this is the only identity available
    /// uniformly at every legal emission site (an amendment to
    /// design/bynk-design-notes.md §7's "the publisher is the emitting
    /// agent" framing — see the events-envelope ADR). Always populated
    /// alongside `handler_scope` at every construction site; never `None`
    /// in practice for a body that could contain an `Events.emit` call.
    owning_context: String,
    /// v0.12: the receiver expression a capability call resolves against —
    /// `deps` in a handler body, `this.deps` in a composed provider body.
    cap_deps_expr: String,
    /// True if the current handler made at least one cross-context call
    /// (drives whether `deps` gets a `surface` field type).
    cross_context_used: bool,
    /// v0.9.2: set when the body instantiates a local agent. In workers mode
    /// this drives `env` (carrying the DO namespaces) into the handler's deps
    /// type so the agent factory can reach its Durable Object binding.
    agents_instantiated: bool,
    /// #527: capabilities required by agent methods this body calls, keyed by
    /// deps key. After body lowering these widen the handler's deps *type* to
    /// match the runtime value compose builds (which always carried them).
    agent_given_caps_used: std::collections::BTreeMap<String, bynk_syntax::ast::CapRef>,
}

impl Default for HandlerShared {
    fn default() -> Self {
        Self {
            capabilities: HashSet::new(),
            handler_scope: None,
            owning_context: String::new(),
            cap_deps_expr: "deps".to_string(),
            cross_context_used: false,
            agents_instantiated: false,
            agent_given_caps_used: std::collections::BTreeMap::new(),
        }
    }
}

/// The lowering state shared by the three **generated test-scaffold** body
/// kinds — a `stub`/`where`/`requires` predicate value, a unit test case, and an
/// integration test case.
#[derive(Default)]
pub(crate) struct TestShared {
    /// True when lowering **any** generated test-scaffold body — distinct from
    /// `assert_loc`, which is only ever `Some` for the two real `case` bodies
    /// and carries an unrelated payload (a diagnostic location). Kept as its own
    /// field (Locale capability track, slice 1, #844 review) rather than
    /// overloading `assert_loc.is_some()`, since that conflated "has a location"
    /// with "is test scaffolding" for a caller that has no location to give.
    test_scaffold: bool,
    /// v0.59: the source text and project-relative path of the file the body
    /// came from, so an `assert` can emit a real `path:line:col` location (for
    /// `--format json` click-through) rather than a bare byte offset. Stays
    /// `None` for a predicate scaffold, which emits no `assert`.
    assert_loc: Option<AssertLoc>,
}

/// The `store`-agent sub-state of an [`BodyMode::AgentHandler`] body: present
/// only when the hosting agent is a `store` agent, absent for a plain
/// state-record agent (whose handler reads `currentState`/`self.state` instead).
pub(crate) struct AgentStoreState {
    /// v0.81 (storage track): the name of the mutable working-state variable
    /// (`__state`) and the set of `Cell` field names. A bare `Cell` read lowers
    /// to `<var>.<cell>`, and a `cell := v` write lowers to `<var>.<cell> = <v>`
    /// — read-your-writes via the in-memory record, flushed once at handler end
    /// (ADR 0109).
    state: (String, HashSet<String>),
    /// v0.82 (ADR 0110): the agent's `store` `Map` field names. A method call
    /// whose receiver is one lowers to an entry operation over `__state.<map>`
    /// (a JSON-serialisable `Record<string, V>`), staged in the working record
    /// and flushed at commit like any other state field.
    maps: HashSet<String>,
    /// v0.83: the agent's `store` `Set` field names. A method call whose
    /// receiver is one lowers to an entry operation over `__state.<set>` (a
    /// `Record<string, boolean>`), staged in the working record.
    sets: HashSet<String>,
    /// v0.87 (ADR 0113): the agent's `store` `Cache` fields (name → ttl millis).
    /// A method call whose receiver is one lowers to an entry op over
    /// `__state.<cache>` (a `Record<string, { v, exp }>`), applying TTL expiry
    /// against the injected `Clock`.
    caches: HashMap<String, i64>,
    /// v0.95 (ADR 0121): the agent's `store` `Log` fields (name → optional
    /// `@retain` millis). `<log>.append` pushes `{ t: now(), v }` to
    /// `__state.<log>` (an array) and prunes past the retain horizon; the
    /// time-window roots / builders lower to a query pipeline over the array.
    logs: HashMap<String, Option<i64>>,
    /// v0.93 (ADR 0118): the agent's `@indexed` secondary indexes (map name →
    /// the value-record fields indexed on). A mutating op on the map maintains a
    /// sibling posting-list `Record<string, string[]>` per field (`<map>__idx_<f>`);
    /// an equality `filter` on an indexed field routes to a posting lookup.
    indexes: HashMap<String, Vec<String>>,
    /// v0.104/v0.105 (real-time track slice 3b): the agent's held `store Map[K,
    /// Connection]` fields (name → the connection's **frame type** `F`, e.g.
    /// `ServerFrame`). On Workers these persist `K → connId` in the durable state
    /// record; a method call whose receiver is one lowers to an entry op over
    /// `__state.<map>` (the connId record) with `connIdOf`/`resolveConnection<F>` —
    /// not the plain `Record<string, V>` ops (held maps are excluded from
    /// [`AgentStoreState::maps`]).
    held_maps: HashMap<String, String>,
}

/// What [`LowerCtx`] is lowering *right now*. One variant per real body-emission
/// site; each carries exactly the state that site populates and nothing else, so
/// "not applicable to this kind" is expressed in the type rather than left
/// indistinguishable from "deliberately defaulted".
pub(crate) enum BodyMode {
    /// A type's method body (`emit_method`).
    Method,
    /// A free function body (`emit_free_fn`).
    FreeFn,
    /// An agent field's static initialiser expression (`emit_agent`).
    StaticInit,
    /// v0.80: an agent invariant predicate. Carries the name of the
    /// proposed-state variable (the `commitState` parameter) and the set of
    /// state field names — a bare ident matching a state field lowers to
    /// `<var>.<field>`, since invariants read state fields directly (§14).
    Invariant {
        name: String,
        fields: HashSet<String>,
    },
    /// v0.116 (testing track slice 4): a `transition` predicate. Carries the JS
    /// names bound to the contextual `old` and `new` state records. The Bynk
    /// identifiers `old`/`new` lower to these (`new` is a JS reserved word, so
    /// both are renamed), and field access `old.<field>` reads off the `old`
    /// record.
    Transition { old: String, new: String },
    /// A service handler body (`emit_service`).
    ServiceHandler {
        handler: HandlerShared,
        /// v0.47: the `by` binder whose `.identity` is threaded through `deps`
        /// (so `<binder>.identity` lowers to `deps.identity` rather than the
        /// unit-value `undefined`).
        deps_identity_binder: Option<String>,
        /// v0.52: when lowering a multi-actor sum handler body, the `by` binder
        /// that names the resolved-actor value (threaded through `deps`, so the
        /// binder ident lowers to `deps.who` — the tagged union the body
        /// `match`es).
        actor_sum_binder: Option<String>,
    },
    /// A composed provider's operation body (`emit_provider`).
    ProviderOp { handler: HandlerShared },
    /// An agent handler body (`emit_agent`).
    AgentHandler {
        handler: HandlerShared,
        /// True when lowering an agent handler body. Used to rewrite
        /// `self.<keyField>` access into the appropriate local.
        in_agent_handler: bool,
        /// The name of the agent's `key id` field (so `self.<id>` resolves).
        agent_key_field: Option<String>,
        /// The `store`-agent working-record state, when the hosting agent is a
        /// `store` agent. Boxed: it is by far the largest payload in this enum,
        /// and every other body kind would otherwise pay for it.
        store: Option<Box<AgentStoreState>>,
    },
    /// A websocket lifecycle method on the hosting Durable Object
    /// (`emit_ws_do_method`).
    WsDoMethod {
        handler: HandlerShared,
        /// v0.47: as [`BodyMode::ServiceHandler::deps_identity_binder`].
        deps_identity_binder: Option<String>,
        /// v0.104 (real-time track slice 3b): when lowering a `from websocket`
        /// `on open` body **into its hosting Durable Object** (the agent the
        /// upgrade transfers the connection to), the name of that agent. A
        /// transfer call `<Agent>(<key>).method(args)` whose `<Agent>` is this
        /// self-agent lowers to a direct `this.method(args, deps)` self-call
        /// rather than the cross-instance `__make<Agent>(key)` factory — the
        /// connection is already in this DO, so it never crosses an RPC boundary
        /// (DECISION A).
        ws_self_agent: Option<String>,
    },
    /// A `stub`/`where`/`requires` predicate value lowered via
    /// `lower_block_to_async_body` — test/property/contract scaffolding, never a
    /// real production provider body.
    PredicateScaffold { test: TestShared },
    /// A unit test `case` body (`lower_test_case_body`).
    TestCase {
        test: TestShared,
        /// v0.117 (testing track slice 5): the name of the recorded-call trace
        /// object (`__obs`), over which an observation (`Cap.op called …`) and
        /// `trace(Cap.op)` are lowered.
        observation_trace: Option<String>,
        /// v0.7: the target context's local service names. A `service.call(args)`
        /// or `service(args)` invocation where `service` is in this set lowers to
        /// `<service>.call(args, deps)` so the test wires its `deps` through.
        test_services: HashSet<String>,
        /// v0.182 (#664): the ordered handler kinds of each test service, so a
        /// cron (`svc.schedule("…")`) or queue (`svc.message(m)`) address can
        /// recover the position index the emitted key encodes (`cron_<svc>_<i>` /
        /// `queue_…`). http keys are a pure function of verb + path and need no
        /// lookup here.
        test_service_handlers: HashMap<String, Vec<bynk_syntax::ast::HandlerKind>>,
    },
    /// An integration test `case` body (`lower_integration_case_body`).
    IntegrationCase {
        test: TestShared,
        /// v0.182 (Slice B, #667): the target's http service names. An http
        /// address on one of these lowers to a driver call
        /// (`__sysdrive_<svc>_<key>(args, sub)`) that drives a real
        /// `worker.fetch` with a signed credential, instead of the unit-tier
        /// direct handler call. Empty at the unit tier.
        system_http_services: std::collections::HashSet<String>,
        /// #707: the declared `(service, method, path)` http routes of the system
        /// target. A `(method, path)` call whose method is absent here but whose
        /// path is present is a **wrong-method** call — it drives the `405`
        /// fall-through through the generic `__sysdrive_wrongmethod_<svc>` driver.
        system_http_routes: std::collections::HashSet<(String, String, String)>,
        /// #708: for each declared `(service, method, path)` route that has a
        /// body param, the body's zero-based position among the route's
        /// positional call args (i.e. within `args[1..]`, matching handler-param
        /// declaration order) and its declared type. The raw driver
        /// (`__sysdrive_raw_*`, Slice C) forwards every slot as a `string`; a
        /// `Wire(…)` arg already lowers to that raw string, but a *typed* arg
        /// mixed into the same call must be converted: the body slot serialises
        /// through the same wire codec the typed driver uses
        /// (`JSON.stringify(serialise_expr_via(...))`), any other (path) slot
        /// just coerces via `String(...)`. Absent for a bodyless route.
        system_http_route_body:
            HashMap<(String, String, String), (usize, bynk_syntax::ast::TypeRef)>,
        /// #708: the type namespace (`<target>.`) `serialise_expr_via` needs to
        /// resolve a body param's custom codec when converting a typed arg for
        /// the raw driver. Mirrors the `type_ns` `emit_system_http_support`
        /// computes from the same suite target.
        system_http_type_ns: String,
    },
}

/// Per-body lowering context: what module we are emitting into ([`ModuleCtx`]),
/// what kind of body we are lowering ([`BodyMode`]), and the scratch state the
/// recursive lowering accumulates as it goes.
///
/// Everything below `mode` is deliberately **not** in `ModuleCtx`: a fresh
/// `LowerCtx` is built at every body-emission site and never reused across two
/// bodies, so these are implicitly reset per body today. Moving any of them up a
/// level would leak state between handlers in the same module — most visibly the
/// `next_tmp` counter, which would stop restarting `__r0` at each function and
/// so rename every generated temp in the emitted TypeScript.
pub(crate) struct LowerCtx<'a> {
    module: ModuleCtx<'a>,
    mode: BodyMode,
    /// Agent names declared in the surrounding context. Drives lowering of
    /// `Agent(key)` (to `new Agent(makeTestState(String(key)))`) and of
    /// `agent_instance.method(args)` (to `instance.method(args, deps)`) in
    /// service and agent-handler bodies. Populated by the caller for non-test
    /// emission and from the *test's own* agent set in test emission — which is
    /// why this is not a [`ModuleCtx`] field despite being module-wide at the
    /// nine non-test sites.
    pub local_agents: HashSet<String>,
    /// v0.154 (ADR 0178): the enclosing function/handler's resolved return type,
    /// set at each body-emission site that has one. The `?` lowering reads it to
    /// decide whether a declared error embedding (`embeds E as V`) converts the
    /// propagated `Err` — via the same `embedding_for` rule the checker used.
    /// Genuinely cross-cutting rather than kind-specific: it is saved/restored
    /// around lambda bodies, `?`-embedding and match arms *within* whichever
    /// kind is being lowered.
    return_ty: Option<bynk_check::checker::Ty>,
    next_tmp: u32,
    /// #908: a stack of per-block frames tracking `let`/`let <-` names that
    /// needed a fresh emitted identifier because the name was already bound
    /// by an enclosing (or the same) block's `let` — the checker allows
    /// re-binding a name (`let x = 1; let x = x + 1`, a deliberate ML-family
    /// idiom, ADR 0064), but each `let` still lowers to its own `const`, so
    /// without renaming a same-block re-`let` collides with the first
    /// (TS2451), and a nested block's re-`let` — while a legal *redeclaration*
    /// on its own — would put an RHS read of the outer binding in its own
    /// declaration's temporal dead zone. Pushed/popped in lock-step with
    /// [`emit_block_inner`] — the single choke point every block (function,
    /// lambda, if/else branch, match arm) lowers through — so a read
    /// (`lower_ident`, and the agent-dispatch receiver text) resolves a name
    /// by walking the stack innermost-out and falls back to the natural
    /// `ts_ident` name when no frame renamed it.
    pub shadow_scopes: Vec<HashMap<String, String>>,
    /// When an `is` receiver is not a simple, repeatable lvalue (e.g. a call
    /// like `parse(x) is Ok(n)`), it is evaluated once into a temp; the temp
    /// name is cached here keyed by the receiver expression's span so the
    /// `.tag` check and every pattern binding reference the *same* single
    /// evaluation. Simple receivers (idents / field chains) are never cached
    /// and continue to be rendered inline as before.
    is_receiver_temps: HashMap<bynk_syntax::span::Span, String>,
    /// Variable bindings that point at agent instances. Updated by the
    /// statement emitter when it sees `let x = AgentName(key)`. Used by
    /// the method-call lowering so `x.method(args)` resolves through
    /// the agent's class rather than via the receiver-namespace lookup.
    pub local_agent_vars: HashMap<String, String>,
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
    /// Slice 1 (ADR 0103): the source-map builder for the file being emitted, if
    /// any. The deep lowering chain records `(generated offset → source span)`
    /// checkpoints here; `emit_project` owns the `RefCell` and threads a shared
    /// borrow in. `None` for the single-file `emit()` path and any body emitted
    /// outside a project, where no map is produced.
    pub source_map: Option<&'a RefCell<SourceMapBuilder>>,
    /// T2.2 (R6.4): set at the two statement sites that emit a literal `await`
    /// (`EffectLet`, `Do`) and read-and-reset around a value-position `match`/`if`
    /// IIFE's own body construction — the flag a synchronous arrow reads to decide
    /// whether it must become `async` and be awaited at its call site. Replaces a
    /// scan of the built string for the substring `"await "`, which over-matched
    /// on a self-contained `async (...) => {...}` embedded as an arm's value (an
    /// iterator terminal like `forEach`) without anything in *this* arrow's own
    /// scope needing to await. Not isolated around a lambda body — a nested
    /// effectful lambda still marks the enclosing IIFE async, exactly as the old
    /// scan did (its own body text also contained `"await "`); closing that is a
    /// separate, unscoped defect, not this one.
    pub(crate) emitted_await: bool,
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
    fn new(module: ModuleCtx<'a>, mode: BodyMode) -> Self {
        Self {
            module,
            mode,
            local_agents: HashSet::new(),
            return_ty: None,
            // Every field below is per-body scratch state: a fresh `LowerCtx` is
            // built at each body-emission site and never reused, so these must
            // re-initialise here on every construction. In particular `next_tmp`
            // restarting at 0 is what makes each emitted function's temps begin
            // at `__r0`.
            next_tmp: 0,
            shadow_scopes: vec![HashMap::new()],
            is_receiver_temps: HashMap::new(),
            local_agent_vars: HashMap::new(),
            call_site_identity: None,
            call_site_no_credential: false,
            source_map: None,
            emitted_await: false,
        }
    }

    // ---- `ModuleCtx` passthroughs -----------------------------------------
    //
    // Returned with the `'a` module lifetime rather than the `&self` borrow, so
    // a `&mut self` lowering step can hold onto a commons/runtime handle across
    // its own recursive calls exactly as it did when these were plain fields.

    /// Typed-commons handle for the module being emitted.
    pub(crate) fn commons(&self) -> &'a TypedCommons {
        self.module.commons
    }

    /// Cross-context info for v0.6 cross-context call lowering.
    pub(crate) fn cross_context(&self) -> &'a bynk_check::resolver::CrossContextInfo {
        self.module.cross_context
    }

    /// The emitted module's conditional-runtime-helper accumulator.
    pub(crate) fn runtime_use(&self) -> &'a RuntimeUse {
        self.module.runtime_use
    }

    /// v0.8 build target.
    pub(crate) fn target(&self) -> BuildTarget {
        self.module.target
    }

    /// #527: type names this context rebrands.
    pub(crate) fn rebranded_types(&self) -> &HashSet<String> {
        &self.module.rebranded_types
    }

    /// #527: fn names imported from a commons.
    pub(crate) fn commons_imported_fns(&self) -> &HashSet<String> {
        &self.module.commons_imported_fns
    }

    /// #934: true when the unit being emitted is the first-party `bynk` adapter.
    pub(crate) fn in_bynk_unit(&self) -> bool {
        self.module.in_bynk_unit
    }

    // ---- capability-bearing (`HandlerShared`) state ------------------------
    //
    // Every accessor here reports the same default a non-handler kind used to
    // get from the flat struct (no capabilities, no scope, `deps`), so a caller
    // that does not care which kind it is in reads unchanged.

    fn handler(&self) -> Option<&HandlerShared> {
        match &self.mode {
            BodyMode::ServiceHandler { handler, .. }
            | BodyMode::ProviderOp { handler }
            | BodyMode::AgentHandler { handler, .. }
            | BodyMode::WsDoMethod { handler, .. } => Some(handler),
            _ => None,
        }
    }

    fn handler_mut(&mut self) -> Option<&mut HandlerShared> {
        match &mut self.mode {
            BodyMode::ServiceHandler { handler, .. }
            | BodyMode::ProviderOp { handler }
            | BodyMode::AgentHandler { handler, .. }
            | BodyMode::WsDoMethod { handler, .. } => Some(handler),
            _ => None,
        }
    }

    /// Whether `name` is a capability in scope as `given C1, C2, ...`.
    pub(crate) fn has_capability(&self, name: &str) -> bool {
        self.handler()
            .is_some_and(|h| h.capabilities.contains(name))
    }

    /// #934: the calling handler's own qualified name, if a capability call can
    /// occur in this body at all. `None` for every non-handler kind — the
    /// `Idempotency` key-scoping lowering treats that as a compiler bug and
    /// panics, exactly as it did when this was a flat `Option` field.
    pub(crate) fn handler_scope(&self) -> Option<&str> {
        self.handler().and_then(|h| h.handler_scope.as_deref())
    }

    /// Events track, slice 2: the qualified name of the unit this body is
    /// emitted into, for the `Events.emit[E](event)` lowering's
    /// `publisherId`. `None` for a body kind that carries no `HandlerShared`
    /// at all (an `Events.emit` call cannot occur there).
    pub(crate) fn owning_context(&self) -> Option<&str> {
        self.handler().map(|h| h.owning_context.as_str())
    }

    /// v0.12: the receiver expression a capability call resolves against.
    pub(crate) fn cap_deps_expr(&self) -> &str {
        self.handler().map_or("deps", |h| h.cap_deps_expr.as_str())
    }

    /// Note that this body made a cross-context call. A no-op in a body kind
    /// that carries no deps shape to widen (a plain method, a predicate, a test
    /// case) — those never read the flag back.
    pub(crate) fn note_cross_context_used(&mut self) {
        if let Some(h) = self.handler_mut() {
            h.cross_context_used = true;
        }
    }

    /// True if this handler made at least one cross-context call.
    pub(crate) fn cross_context_used(&self) -> bool {
        self.handler().is_some_and(|h| h.cross_context_used)
    }

    /// v0.9.2: true if this body instantiated a local agent.
    pub(crate) fn agents_instantiated(&self) -> bool {
        self.handler().is_some_and(|h| h.agents_instantiated)
    }

    /// #527: capabilities required by agent methods this body calls.
    pub(crate) fn agent_given_caps_used(
        &self,
    ) -> Option<&std::collections::BTreeMap<String, bynk_syntax::ast::CapRef>> {
        self.handler().map(|h| &h.agent_given_caps_used)
    }

    // ---- test-scaffold (`TestShared`) state --------------------------------

    fn test(&self) -> Option<&TestShared> {
        match &self.mode {
            BodyMode::PredicateScaffold { test }
            | BodyMode::TestCase { test, .. }
            | BodyMode::IntegrationCase { test, .. } => Some(test),
            _ => None,
        }
    }

    /// True when lowering **generated test-scaffold** TypeScript (a test-case
    /// body, or a `stub`/`where`/`requires` predicate value), where branded
    /// types are destructured into `any`-typed value bindings rather than
    /// referenced as types. Callers that emit a branded `as`-cast consult this
    /// to pick `unchecked_construct_test` (→ `(v as any)`) over the production
    /// `(v as T)` form, which cannot resolve `T` in the test module's scope.
    pub(crate) fn in_test_scaffold(&self) -> bool {
        self.test().is_some_and(|t| t.test_scaffold)
    }

    /// v0.59: the test body's source context, for `assert`/`expect` locations.
    pub(crate) fn assert_loc(&self) -> Option<&AssertLoc> {
        self.test().and_then(|t| t.assert_loc.as_ref())
    }

    // ---- single-kind state -------------------------------------------------

    /// v0.80: inside an invariant predicate, the proposed-state variable and the
    /// agent's state field names.
    pub(crate) fn invariant_state(&self) -> Option<(&str, &HashSet<String>)> {
        match &self.mode {
            BodyMode::Invariant { name, fields } => Some((name.as_str(), fields)),
            _ => None,
        }
    }

    /// v0.116: inside a `transition` predicate, the JS names bound to the
    /// contextual `old`/`new` state records.
    pub(crate) fn transition_states(&self) -> Option<(&str, &str)> {
        match &self.mode {
            BodyMode::Transition { old, new } => Some((old.as_str(), new.as_str())),
            _ => None,
        }
    }

    /// v0.117: the recorded-call trace object a test case's observations read.
    pub(crate) fn observation_trace(&self) -> Option<&str> {
        match &self.mode {
            BodyMode::TestCase {
                observation_trace, ..
            } => observation_trace.as_deref(),
            _ => None,
        }
    }

    fn agent_store(&self) -> Option<&AgentStoreState> {
        match &self.mode {
            BodyMode::AgentHandler { store, .. } => store.as_deref(),
            _ => None,
        }
    }

    /// v0.81: the mutable working-state variable a `store`-agent handler stages
    /// its writes into. `__state` is the name every real site uses; the fallback
    /// keeps the (defensive) non-store paths rendering as they did before.
    pub(crate) fn agent_store_var(&self) -> &str {
        self.agent_store().map_or("__state", |s| s.state.0.as_str())
    }

    /// v0.81: the working-state variable plus the `Cell` field names it holds.
    pub(crate) fn agent_store_cells(&self) -> Option<(&str, &HashSet<String>)> {
        self.agent_store().map(|s| (s.state.0.as_str(), &s.state.1))
    }

    /// v0.82: whether `name` is a persisted `store Map` field (held connection
    /// maps are deliberately excluded — they use the connId lowering).
    pub(crate) fn is_agent_store_map(&self, name: &str) -> bool {
        self.agent_store().is_some_and(|s| s.maps.contains(name))
    }

    /// v0.83: whether `name` is a `store Set` field.
    pub(crate) fn is_agent_store_set(&self, name: &str) -> bool {
        self.agent_store().is_some_and(|s| s.sets.contains(name))
    }

    /// v0.87: the ttl (millis) of the `store Cache` field `name`, if it is one.
    pub(crate) fn agent_store_cache_ttl(&self, name: &str) -> Option<i64> {
        self.agent_store().and_then(|s| s.caches.get(name).copied())
    }

    /// v0.95: the `@retain` horizon of the `store Log` field `name`, if it is
    /// one. The outer `Option` is "is a log"; the inner is "has a retain".
    pub(crate) fn agent_store_log_retain(&self, name: &str) -> Option<Option<i64>> {
        self.agent_store().and_then(|s| s.logs.get(name).copied())
    }

    /// v0.95: whether `name` is a `store Log` field.
    pub(crate) fn is_agent_store_log(&self, name: &str) -> bool {
        self.agent_store()
            .is_some_and(|s| s.logs.contains_key(name))
    }

    /// v0.93: the value-record fields the `store Map` `name` is `@indexed(by:)`
    /// on — empty when it has no secondary index.
    pub(crate) fn agent_store_index_fields(&self, name: &str) -> Vec<String> {
        self.agent_store()
            .and_then(|s| s.indexes.get(name).cloned())
            .unwrap_or_default()
    }

    /// v0.105: the connection **frame type** of the held `store Map[K,
    /// Connection]` field `name`, if it is one.
    pub(crate) fn agent_held_map_frame(&self, name: &str) -> Option<&String> {
        self.agent_store().and_then(|s| s.held_maps.get(name))
    }

    /// v0.105: whether `name` is a held `store Map[K, Connection]` field.
    pub(crate) fn is_agent_held_map(&self, name: &str) -> bool {
        self.agent_store()
            .is_some_and(|s| s.held_maps.contains_key(name))
    }

    /// True when lowering an agent handler body — drives the `self.<keyField>`
    /// rewrite.
    pub(crate) fn in_agent_handler(&self) -> bool {
        match &self.mode {
            BodyMode::AgentHandler {
                in_agent_handler, ..
            } => *in_agent_handler,
            _ => false,
        }
    }

    /// The name of the agent's `key id` field, inside an agent handler body.
    pub(crate) fn agent_key_field(&self) -> Option<&str> {
        match &self.mode {
            BodyMode::AgentHandler {
                agent_key_field, ..
            } => agent_key_field.as_deref(),
            _ => None,
        }
    }

    /// v0.104: the agent hosting the websocket lifecycle body being lowered.
    pub(crate) fn ws_self_agent(&self) -> Option<&str> {
        match &self.mode {
            BodyMode::WsDoMethod { ws_self_agent, .. } => ws_self_agent.as_deref(),
            _ => None,
        }
    }

    /// v0.47: the `by` binder whose `.identity` is threaded through `deps`.
    pub(crate) fn deps_identity_binder(&self) -> Option<&str> {
        match &self.mode {
            BodyMode::ServiceHandler {
                deps_identity_binder,
                ..
            }
            | BodyMode::WsDoMethod {
                deps_identity_binder,
                ..
            } => deps_identity_binder.as_deref(),
            _ => None,
        }
    }

    /// v0.52: the multi-actor sum handler's resolved-actor binder.
    pub(crate) fn actor_sum_binder(&self) -> Option<&str> {
        match &self.mode {
            BodyMode::ServiceHandler {
                actor_sum_binder, ..
            } => actor_sum_binder.as_deref(),
            _ => None,
        }
    }

    /// v0.7: whether `name` is a local service of the test case's target context.
    pub(crate) fn is_test_service(&self, name: &str) -> bool {
        match &self.mode {
            BodyMode::TestCase { test_services, .. } => test_services.contains(name),
            _ => false,
        }
    }

    /// v0.182 (#664): the ordered handler kinds of the test service `name`.
    pub(crate) fn test_service_handlers(
        &self,
        name: &str,
    ) -> Option<&[bynk_syntax::ast::HandlerKind]> {
        match &self.mode {
            BodyMode::TestCase {
                test_service_handlers,
                ..
            } => test_service_handlers.get(name).map(Vec::as_slice),
            _ => None,
        }
    }

    /// v0.182 (Slice B, #667): whether `name` is an http service of the system
    /// target being driven.
    pub(crate) fn is_system_http_service(&self, name: &str) -> bool {
        match &self.mode {
            BodyMode::IntegrationCase {
                system_http_services,
                ..
            } => system_http_services.contains(name),
            _ => false,
        }
    }

    /// #707: whether `(service, verb, path)` is a declared route of the system
    /// target — an undeclared one drives the `405` fall-through.
    pub(crate) fn has_system_http_route(&self, route: &(String, String, String)) -> bool {
        match &self.mode {
            BodyMode::IntegrationCase {
                system_http_routes, ..
            } => system_http_routes.contains(route),
            _ => false,
        }
    }

    /// #708: the body param position and declared type of a system http route.
    pub(crate) fn system_http_route_body(
        &self,
        route: &(String, String, String),
    ) -> Option<&(usize, bynk_syntax::ast::TypeRef)> {
        match &self.mode {
            BodyMode::IntegrationCase {
                system_http_route_body,
                ..
            } => system_http_route_body.get(route),
            _ => None,
        }
    }

    /// #708: the type namespace a system http body param's codec resolves in.
    pub(crate) fn system_http_type_ns(&self) -> &str {
        match &self.mode {
            BodyMode::IntegrationCase {
                system_http_type_ns,
                ..
            } => system_http_type_ns.as_str(),
            _ => "",
        }
    }

    /// Events track, slice 0 (spine #936): true when a bare `Events`
    /// receiver in this unit is genuinely the first-party `bynk.Events`
    /// capability — declared here because this unit *is* `bynk`, or
    /// flattened in from it (`consumes bynk { Events }`) — not some other,
    /// unrelated capability that merely happens to share the name. Mirrors
    /// #934's `Idempotency` distinction (`is_first_party` at the
    /// `Idempotency.dedup`/`remember` lowering site). Both the call-site
    /// interception (`lower.rs`) and the `__events` buffer declaration
    /// (`block_uses_emit`'s gate in `emit.rs`) must agree on this, or a
    /// custom same-named `Events` capability's calls get silently rewritten
    /// into a buffer nothing constructs a provider for.
    pub(crate) fn is_first_party_events(&self) -> bool {
        self.in_bynk_unit()
            || self
                .cross_context()
                .flattened_caps
                .get("Events")
                .map(String::as_str)
                == Some("bynk")
    }

    /// Events slice 3b (#978): the declared `@schema(N)` version of the
    /// locally-declared event `name`, or `1` if it has none (including if
    /// `name` isn't a locally-declared event at all — `Events.emit[E]` only
    /// ever names an owned event, checker-enforced, so a miss here can only
    /// mean a broken build already reported elsewhere, and this degrades to
    /// today's pre-existing output rather than panicking).
    pub(crate) fn event_schema_version(&self, name: &str) -> i64 {
        self.module
            .event_schema_versions
            .get(name)
            .copied()
            .unwrap_or(1)
    }

    /// Attach the file's source-map builder (slice 1, ADR 0103). Builder-style so
    /// the emission sites with no builder leave `LowerCtx::new(module, mode)`
    /// untouched — only the project-emission path that has one calls this.
    fn with_source_map(mut self, map: Option<&'a RefCell<SourceMapBuilder>>) -> Self {
        self.source_map = map;
        self
    }

    /// Record that this lowering emitted a reference to the `Bytes` runtime
    /// helpers, so the module imports them.
    fn note_bytes(&self) {
        self.runtime_use().note_bytes();
    }

    /// Record a checkpoint: generated text from `out_len` onward originates at
    /// `span`, until the next checkpoint (ADR 0103 D2, nearest-enclosing). A
    /// no-op when no builder is attached. `out_len` is the buffer length *before*
    /// the statement's text is appended.
    ///
    /// `out_len` only means something relative to the *top-level module
    /// buffer* the attached builder is tracking. A caller building an IIFE
    /// into its own local `String` — `lower_if`'s value-position wrapper,
    /// `build_match_iife`'s — before splicing it elsewhere must not call this
    /// with that buffer's own length; see [`Self::without_source_map`].
    fn record_span(&self, out_len: usize, span: bynk_syntax::span::Span) {
        if let Some(map) = self.source_map {
            map.borrow_mut().record(out_len, span);
        }
    }

    /// #4 review: run `f` with source-map recording suppressed, restoring it
    /// after. For lowering into a local IIFE buffer that will later be
    /// spliced into the real module text at some other offset — `record_span`
    /// has no way to know that offset, so a checkpoint taken here would
    /// silently corrupt the map with a position relative to the wrong
    /// buffer. `SourceMapBuilder::merge` already solves the equivalent
    /// problem one level up (a handler/test body's own local buffer, spliced
    /// into the module) by recording into a *sub*-builder and rebasing at the
    /// splice — but that needs a builder that outlives the call, and
    /// `source_map` is `Option<&'a RefCell<SourceMapBuilder>>` tied to the
    /// whole emission's lifetime, so a function-local sub-builder can't be
    /// substituted in. Suppressing instead of mis-recording means the
    /// nearest-enclosing-checkpoint rule (ADR 0103 D2) falls back to whatever
    /// was correctly mapped just before the IIFE started, rather than a wrong
    /// one silently taking over — degraded stepping through the IIFE's own
    /// lines in `bynkc test --inspect`, not a corrupted map.
    fn without_source_map<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.source_map.take();
        let result = f(self);
        self.source_map = saved;
        result
    }
    /// v0.9.2: lower an agent instantiation `AgentName(key)` to its factory
    /// call. Bundle/test mode passes only the key; workers mode also threads
    /// `deps.env` so the factory can reach the agent's DO namespace.
    fn agent_construct(&mut self, agent: &str, key_expr: &str) -> String {
        if let Some(h) = self.handler_mut() {
            h.agents_instantiated = true;
        }
        let factory = agent_factory_name(agent);
        if matches!(self.target(), BuildTarget::Workers) {
            format!("{factory}({key_expr}, deps.env)")
        } else {
            format!("{factory}({key_expr})")
        }
    }

    /// #527: note that the body calls `agent.method`, folding the method's
    /// `given` capabilities into this handler's requirement set. A no-op in a
    /// body kind that has no deps shape to widen — those never read it back.
    pub(crate) fn record_agent_call(&mut self, agent: &str, method: &str) {
        let givens = self
            .module
            .agent_method_givens
            .get(agent)
            .and_then(|m| m.get(method))
            .cloned()
            .unwrap_or_default();
        if let Some(h) = self.handler_mut() {
            for c in givens {
                h.agent_given_caps_used
                    .entry(c.key().to_string())
                    .or_insert(c);
            }
        }
    }
    fn fresh(&mut self) -> String {
        let n = self.next_tmp;
        self.next_tmp += 1;
        format!("__r{n}")
    }
    /// #908: bind a `let`/`let <-` LHS to its emitted JS identifier. Returns
    /// the natural `ts_ident` name unless `name` is already bound *anywhere*
    /// in the enclosing block chain — not only the current block. A nested
    /// block re-`let`-ing an outer name is ordinary, valid lexical shadowing
    /// in JS on its own, but this `let`'s own RHS may still read the outer
    /// binding (`let n = n + 1` one block in); a plain `const n` there
    /// would put the read inside its own declaration's temporal dead zone
    /// (JS hoists a block's `let`/`const` names to the top of that block),
    /// turning a correct read of the outer value into a TDZ ReferenceError.
    /// Renaming whenever *any* enclosing frame already has the name sidesteps
    /// that regardless of whether this particular RHS reads it. Allocates a
    /// fresh name via [`Self::fresh`] when so. `_` never collides (each is
    /// already a fresh throwaway) and is never registered, since it is never
    /// read.
    pub(crate) fn bind_local_name(&mut self, name: &str) -> String {
        if name == "_" {
            return self.fresh();
        }
        let natural = ts_ident(name);
        let js_name = if self.shadow_scopes.iter().any(|f| f.contains_key(name)) {
            self.fresh()
        } else {
            natural
        };
        self.shadow_scopes
            .last_mut()
            .expect("shadow_scopes always has a root frame")
            .insert(name.to_string(), js_name.clone());
        js_name
    }
    /// #908: the emitted JS identifier currently bound to a local name, if a
    /// `let` re-bind renamed it somewhere in the enclosing block chain.
    /// Walked innermost-out so a nested block sees an outer rename that was
    /// still active when it was entered. `None` means no rename applies —
    /// callers fall back to the natural `ts_ident` name.
    pub(crate) fn resolved_local_name(&self, name: &str) -> Option<String> {
        self.shadow_scopes
            .iter()
            .rev()
            .find_map(|f| f.get(name).cloned())
    }
    /// Whether `name` is bound by an enclosing local (a `let`, match-arm/`is`
    /// binding, or lambda param) rather than free to refer to a store field.
    /// A local always wins: store-field dispatch by bare receiver name must
    /// check this first, or a parameter/binding that happens to share a store
    /// field's name is silently treated as the store field.
    pub(crate) fn is_local(&self, name: &str) -> bool {
        self.shadow_scopes.iter().any(|f| f.contains_key(name))
    }
    /// #908: register a non-`let` binder (a match-arm/`is` pattern binding, or
    /// a lambda param) into the current frame under its natural `ts_ident`
    /// name — never renamed, since each such binder already lowers inside its
    /// own JS block/arrow scope with no risk of colliding with a sibling
    /// declaration of the same name. Without this, a read inside the binder's
    /// scope would fall through [`Self::resolved_local_name`]'s stack walk
    /// past this (unregistered) declaration to an outer `let` rename that is
    /// no longer the right value here — silently wrong output, not a `tsc`
    /// error. Every construct that introduces a binder outside `bind_local_name`
    /// (match arms, `is`, lambda params) must call this for each name it binds.
    pub(crate) fn declare_binder(&mut self, name: &str) {
        if name == "_" {
            return;
        }
        self.shadow_scopes
            .last_mut()
            .expect("shadow_scopes always has a root frame")
            .insert(name.to_string(), ts_ident(name));
    }
    /// Return a stable textual reference to an `is` receiver, used by the
    /// `.tag` check in `lower_is`. A simple, repeatable lvalue is lowered
    /// inline exactly as before (preserving rewrites such as `self.state` or
    /// capability access). A complex receiver (anything `value_text_for_is`
    /// could not render — e.g. a call) is evaluated once into a fresh temp
    /// hoisted into the returned `Lowered` and cached by span, so the bindings
    /// gathered later reference the same evaluation rather than re-running the
    /// expression.
    fn is_receiver_ref(&mut self, value: &Expr) -> Lowered {
        if let Some(t) = self.is_receiver_temps.get(&value.span) {
            return Lowered::bare(t.clone());
        }
        let mut pre = Pre::new();
        let lowered = pre.lower(value, self);
        if is_simple_is_receiver(value) {
            return pre.finish(lowered);
        }
        let tmp = self.fresh();
        pre.push(format!("const {tmp} = {lowered};"));
        self.is_receiver_temps.insert(value.span, tmp.clone());
        pre.finish(tmp)
    }

    /// v0.13: like `is_receiver_ref` but always lifts to a temp, even for a
    /// simple ident. A refined `is`-narrowing re-binds the value's name to the
    /// branded refined type (`const n = <temp> as Quantity`); that shadowing
    /// const cannot reference the same name (TDZ), so the value is captured in a
    /// temp first and both the check and the binding read the temp.
    fn is_receiver_ref_forced(&mut self, value: &Expr) -> Lowered {
        if let Some(t) = self.is_receiver_temps.get(&value.span) {
            return Lowered::bare(t.clone());
        }
        let mut pre = Pre::new();
        let lowered = pre.lower(value, self);
        let tmp = self.fresh();
        pre.push(format!("const {tmp} = {lowered};"));
        self.is_receiver_temps.insert(value.span, tmp.clone());
        pre.finish(tmp)
    }

    /// v0.13: true when `value is Name` is a *refinement* check — the value is a
    /// base/refined value and `Name` is a refined type — rather than a sum
    /// variant test. Mirrors the checker's disambiguation.
    fn is_refined_is_check(&self, value: &Expr, name: &str) -> bool {
        let value_baseish = matches!(
            self.commons().expr_types.get(&value.span),
            Some(Ty::Base(_))
                | Some(Ty::Named {
                    kind: NamedKind::Refined(_),
                    ..
                })
        );
        let name_refined = matches!(
            self.commons().types.get(name).map(|d| &d.body),
            Some(TypeBody::Refined { .. })
        );
        value_baseish && name_refined
    }
    /// Read-only counterpart for the binding gatherer (which returns no
    /// `Lowered`, so it has nowhere to hoist and cannot lift). If the receiver was already lifted to a temp during
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
        let ty = self.commons().expr_types.get(&e.span)?;
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
            && let Some(decl) = self.commons().types.get(name)
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
                args,
            }) => {
                let decl = self.commons().types.get(name)?;
                let TypeBody::Sum(s) = &decl.body else {
                    return None;
                };
                let v = s.variants.iter().find(|v| v.name.name == variant)?;
                let f = v.payload.get(idx)?;
                // #593: substitute the instantiation's type arguments into the
                // payload field type — a bare type parameter (`Loaded(value: T)`)
                // resolves to its concrete argument, exactly as the checker's
                // `variants_of` does. Plain resolve for a non-generic sum (empty
                // `args`), so a nested positional binding recovers the real field
                // name instead of falling back to the generic `"value"`.
                bynk_check::checker::instantiate_field_ty(
                    decl,
                    args,
                    &f.type_ref,
                    &self.commons().types,
                )
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
    ts_type_ref_with(
        r,
        Some(&|name| scope.contains(name).then(|| ns.to_string())),
    )
}

/// Like `ts_type_ref_qualified`, but each in-scope name can carry its *own*
/// namespace rather than one shared `ns` — needed when a signature mixes
/// names owned by the target unit with names reached only through a `uses`d
/// commons (e.g. a stub class implementing an adapter-sourced capability
/// whose return type lives in a commons the capability's own unit `uses`,
/// never in the target context itself — Locale capability track, slice 1,
/// #844). Qualifying such a name under the target's own namespace would
/// reference an export `emit_context_rebrands` never emits (it only rebrands
/// names the target's *own* lowered body references), so each name is
/// qualified under the namespace that actually exports it.
pub(crate) fn ts_type_ref_qualified_multi(
    r: &TypeRef,
    type_ns: &HashMap<String, String>,
) -> String {
    ts_type_ref_with(r, Some(&|name| type_ns.get(name).cloned()))
}

/// A name → owning-namespace lookup for `ts_type_ref_with`'s `qualify` arm.
type QualifyFn<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Shared renderer behind `ts_type_ref` (`qualify = None`) and the two
/// `ts_type_ref_qualified*` helpers above (`qualify = Some(name -> namespace)`).
/// With `None` it is output-identical to the historic `ts_type_ref`; the only
/// divergence is the `Named`/`App` arms, which qualify in-scope names when
/// `qualify` is set.
fn ts_type_ref_with(r: &TypeRef, qualify: Option<QualifyFn<'_>>) -> String {
    match r {
        TypeRef::Base(b, _) => ts_base(*b).to_string(),
        TypeRef::Named(id) => {
            if let Some(f) = qualify
                && let Some(ns) = f(&id.name)
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
            let head = if let Some(f) = qualify
                && let Some(ns) = f(&name.name)
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

/// #661 (Decision D)/#70 review: the one `PredKind` → runtime-check mapping,
/// shared by the owner-side check (`emit::emit_pred_check`, over a `value`
/// binding) and the boundary-side inline check
/// (`serialisation::emit_inline_pred_check`, over a `json` binding) — the
/// two used to hand-roll this mapping independently, pinned identical only by
/// a comment, so amending one (e.g. the `Matches` regex's `^(?:…)$` anchoring)
/// could silently drift from the other. `receiver` is the bound name the
/// generated condition reads (`value` or `json`); the returned message is the
/// same either side of the boundary by construction.
pub(crate) fn pred_condition_and_message(pred: &PredKind, receiver: &str) -> (String, String) {
    match pred {
        PredKind::NonNegative => (
            format!("{receiver} >= 0"),
            "must be non-negative".to_string(),
        ),
        PredKind::Positive => (format!("{receiver} > 0"), "must be positive".to_string()),
        PredKind::InRange(a, b) => {
            let (a, b) = (a.value, b.value);
            (
                format!("{receiver} >= {a} && {receiver} <= {b}"),
                format!("must be in range [{a}, {b}]"),
            )
        }
        PredKind::InRangeF(a, b) => {
            let (a, b) = (&a.lexeme, &b.lexeme);
            (
                format!("{receiver} >= {a} && {receiver} <= {b}"),
                format!("must be in range [{a}, {b}]"),
            )
        }
        PredKind::NonEmpty => (
            format!("{receiver}.length > 0"),
            "must be non-empty".to_string(),
        ),
        PredKind::MinLength(n) => (
            format!("{receiver}.length >= {n}"),
            format!("length must be at least {n}"),
        ),
        PredKind::MaxLength(n) => (
            format!("{receiver}.length <= {n}"),
            format!("length must be at most {n}"),
        ),
        PredKind::Length(n) => (
            format!("{receiver}.length === {n}"),
            format!("length must be exactly {n}"),
        ),
        PredKind::Matches(pat) => {
            let escaped = escape_ts_string(pat);
            (
                format!("new RegExp(\"^(?:\" + \"{escaped}\" + \")$\").test({receiver})"),
                format!("must match /{escaped}/"),
            )
        }
    }
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
    fn coverage_tsconfig_enables_source_maps() {
        // #854: the coverage remap consumes tsc's `.js.map`s, so the variant must
        // set `sourceMap` — a guard against a silent string-replace miss if the
        // base config's `outDir` line is ever reworded. The default stays map-free
        // so a normal `bynkc test` / deployment build ships no `.js.map`s.
        let cov = emit_tsconfig_with_source_maps();
        assert!(
            cov.contains("\"sourceMap\": true"),
            "coverage config: {cov}"
        );
        assert!(cov.contains("\"outDir\": \"../out-js\""));
        assert!(!emit_tsconfig().contains("sourceMap"));
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

/// Which conditional runtime helpers a module's import line ends up carrying.
///
/// These drive the single-file `emit()` path end-to-end (parse → resolve → check
/// → emit), so they exercise the real producers rather than the accumulator in
/// isolation. Before `RuntimeUse`, the decision was `body.contains("__bynkBytes")`
/// — a scan of the generated text — and `escapes_a_marker_in_a_string_literal`
/// below is the case that got wrong.
#[cfg(test)]
mod conditional_runtime_import_tests {
    use crate::testkit::{emit_bundle, emit_source};

    /// The import line is the first `import { … } from "./runtime.js"` in the
    /// emitted module.
    fn runtime_import_line(ts: &str) -> &str {
        ts.lines()
            .find(|l| l.starts_with("import {") && l.contains("runtime.js"))
            .unwrap_or("")
    }

    #[test]
    fn bytes_helpers_are_imported_when_a_bytes_value_is_built() {
        let ts = emit_source(
            "commons b\n\nfn decode(s: String) -> Option[Bytes] {\n  Bytes.fromBase64(s)\n}\n",
        );
        assert!(
            runtime_import_line(&ts).contains("__bynkBytesFromBase64"),
            "{ts}"
        );
    }

    #[test]
    fn bytes_helpers_are_imported_for_content_equality() {
        let ts = emit_source("commons b\n\nfn same(a: Bytes, b: Bytes) -> Bool {\n  a == b\n}\n");
        assert!(
            runtime_import_line(&ts).contains("__bynkBytesEqual"),
            "{ts}"
        );
    }

    #[test]
    fn bytes_helpers_are_absent_from_a_module_that_never_uses_bytes() {
        let ts = emit_source("commons b\n\nfn double(n: Int) -> Int {\n  n * 2\n}\n");
        assert!(!runtime_import_line(&ts).contains("__bynkBytes"), "{ts}");
    }

    /// The regression this replaced a text scan for: a `Bytes` helper name
    /// appearing inside a user **string literal** is not a reference to the
    /// helper, and must not pull the import in. `body.contains("__bynkBytes")`
    /// could not tell the two apart, because the literal is emitted verbatim
    /// into the same buffer it scanned.
    #[test]
    fn escapes_a_marker_in_a_string_literal() {
        let ts = emit_source("commons b\n\nfn label() -> String {\n  \"__bynkBytesEqual\"\n}\n");
        assert!(
            ts.contains("\"__bynkBytesEqual\""),
            "the literal should survive into the body: {ts}"
        );
        assert!(
            !runtime_import_line(&ts).contains("__bynkBytes"),
            "a marker inside a string literal is not a helper reference: {ts}"
        );
    }

    // -- the ICU formatters ---------------------------------------------------

    const ICU_HELPERS: [&str; 3] = ["selectPluralArm", "formatIcuNumber", "formatIcuDate"];

    /// The case the per-arm recording exists for. A `select` placeholder lowers to
    /// `Object.hasOwn` over an arm table and calls no formatter, so a bundle whose
    /// only ICU construct is a `select` must import none of the three — recording
    /// once per placeholder instead of per arm would import all three here.
    #[test]
    fn a_select_only_bundle_imports_no_icu_formatter() {
        let ts = emit_bundle(
            "messages \"en\" @reference {\n  \"greeting\" => \"{g, select, male {He} female {She} other {They}} liked this.\"\n}\n",
        );
        assert!(
            ts.contains("Object.hasOwn"),
            "the select arm table should have been emitted, else this proves nothing: {ts}"
        );
        for helper in ICU_HELPERS {
            assert!(
                !runtime_import_line(&ts).contains(helper),
                "a select-only bundle calls no formatter, so `{helper}` must not be imported: {ts}"
            );
        }
    }

    /// The opposite direction: a `plural` placeholder does call a formatter, and
    /// the three are imported as a group.
    #[test]
    fn a_plural_bundle_imports_the_icu_formatters() {
        let ts = emit_bundle(
            "messages \"en\" @reference {\n  \"cart\" => \"You have {n, plural, one {# item} other {# items}} in your cart\"\n}\n",
        );
        assert!(
            ts.contains("selectPluralArm("),
            "the plural dispatch should have been emitted: {ts}"
        );
        for helper in ICU_HELPERS {
            assert!(
                runtime_import_line(&ts).contains(helper),
                "`{helper}` should be imported for a plural bundle: {ts}"
            );
        }
    }

    /// A bundle with no ICU dispatch at all — a plain `{name}` placeholder goes
    /// through `renderArg`, not a formatter.
    #[test]
    fn a_plain_placeholder_bundle_imports_no_icu_formatter() {
        let ts =
            emit_bundle("messages \"en\" @reference {\n  \"hello\" => \"Hello, {name}!\"\n}\n");
        for helper in ICU_HELPERS {
            assert!(
                !runtime_import_line(&ts).contains(helper),
                "`{helper}` must not be imported for a bundle with no ICU dispatch: {ts}"
            );
        }
    }
}

/// #914: `inject_runtime_imports` must not add a binding the target line already
/// has. The test-scaffold module lists `Ok`/`Err`/`Result` but not
/// `BoundaryError`, so the boundary group is a partial overlap — injecting it
/// wholesale would emit a duplicate identifier, trading one uncompilable module
/// for another.
#[cfg(test)]
mod inject_runtime_imports_tests {
    use super::*;

    const SPEC: &str = "./runtime.js";

    fn line(bindings: &str) -> String {
        format!("import {{ {bindings} }} from \"{SPEC}\";\nconst x = 1;\n")
    }

    #[test]
    fn appends_bindings_that_are_absent() {
        let out = inject_runtime_imports(line("Ok, Err"), SPEC, BYTES_RUNTIME_IMPORTS);
        assert!(out.contains("Ok, Err, __bynkBytesEqual"), "{out}");
        assert!(out.contains("__bynkBytesDecodeUtf8 } from"), "{out}");
    }

    #[test]
    fn skips_bindings_already_present() {
        let out = inject_runtime_imports(
            line("Ok, Err, type Result"),
            SPEC,
            BOUNDARY_CODEC_RUNTIME_IMPORTS,
        );
        assert_eq!(
            out.matches("Ok").count(),
            1,
            "`Ok` was already imported and must not repeat: {out}"
        );
        assert!(
            out.contains("Ok, Err, type Result, type BoundaryError"),
            "{out}"
        );
    }

    /// The bare name is what collides, so `type BoundaryError` must match an
    /// existing `BoundaryError`.
    #[test]
    fn matches_a_type_prefixed_group_binding_against_a_bare_one() {
        let out = inject_runtime_imports(
            line("Ok, Err, type Result, BoundaryError"),
            SPEC,
            BOUNDARY_CODEC_RUNTIME_IMPORTS,
        );
        assert_eq!(
            out,
            line("Ok, Err, type Result, BoundaryError"),
            "every binding was already present, so the line is untouched"
        );
    }

    /// …and the other direction: a bare group binding against an existing
    /// `type`-prefixed one. This is the case that would regress if `bare` were
    /// applied to only one side of the comparison.
    #[test]
    fn matches_a_bare_group_binding_against_a_type_prefixed_one() {
        // A group whose bindings are bare, against a line that `type`-prefixes
        // them. `Result` is the realistic instance — the fixed test-scaffold
        // list writes `type Result`.
        let out = inject_runtime_imports(line("type Ok, type Err"), SPEC, ", Ok, Err");
        assert_eq!(
            out,
            line("type Ok, type Err"),
            "a bare binding must match an existing `type`-prefixed one: {out}"
        );
    }

    /// The two injections run back to back over the same line, so the second
    /// sees the first's output as `existing` — the overlap between the groups
    /// (`Ok`, `Err`, `type Result`) must not double up.
    #[test]
    fn composes_across_two_sequential_injections() {
        let out = inject_runtime_imports(
            line("Ok, Err, type Result"),
            SPEC,
            BOUNDARY_CODEC_RUNTIME_IMPORTS,
        );
        let out = inject_runtime_imports(out, SPEC, JSON_CODEC_RUNTIME_IMPORTS);
        assert_eq!(
            out,
            line("Ok, Err, type Result, type BoundaryError, type JsonValue, type JsonError"),
            "the shared bindings must be injected once: {out}"
        );
    }

    #[test]
    fn leaves_a_line_for_another_specifier_alone() {
        let other = "import { Ok } from \"./elsewhere.js\";\n".to_string();
        assert_eq!(
            inject_runtime_imports(other.clone(), SPEC, BYTES_RUNTIME_IMPORTS),
            other
        );
    }
}
