//! Per-declaration emission — the functions `emit_project` drives to render
//! each top-level Bynk declaration into TypeScript: type/refined/record/sum
//! declarations and their checks, attached methods and free functions,
//! capabilities, providers, services, contexts, and agents (plus the
//! worker-dispatch lowering helpers those emitters use). Split out of
//! `emitter.rs` (ADR 0060); the codec/reference/import/header helpers and the
//! `ts_*`/`LowerCtx` core stay in the parent and are reached via `use super::*`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::project::EmitProjectCtx;
use bynk_check::checker::TypedCommons;

use super::*;

pub(crate) fn emit_type(
    out: &mut String,
    t: &TypeDecl,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) {
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
pub(crate) fn emit_doc_block(out: &mut String, doc: Option<&str>, indent: usize) {
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

/// v0.93 (ADR 0118): deterministically order the `@indexed` map → fields entries
/// (by map name; fields keep their declaration order). `HashMap` iteration is
/// unordered, so emitted state fields would otherwise drift between runs.
fn sorted_index_fields(indexes: &HashMap<String, Vec<String>>) -> Vec<(&String, &Vec<String>)> {
    let mut entries: Vec<(&String, &Vec<String>)> = indexes.iter().collect();
    entries.sort_by_key(|(name, _)| name.to_string());
    entries
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
    // v0.21: validated `Float` values are finite — `.of` and the boundary
    // codec agree (ADR 0040); only in-language arithmetic is host-defined.
    if base == BaseType::Float {
        writeln!(out, "    if (!Number.isFinite(value)) {{").unwrap();
        writeln!(
            out,
            "      return Err({{ field: \"{name}\", message: \"must be a finite number\", value }});"
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
            let (a, b) = (a.value, b.value);
            writeln!(out, "    if (!(value >= {a} && value <= {b})) {{").unwrap();
            writeln!(
                out,
                "      return Err({{ field: \"{type_name}\", message: \"must be in range [{a}, {b}]\", value }});",
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        PredKind::InRangeF(a, b) => {
            let (a, b) = (&a.lexeme, &b.lexeme);
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
    let empty = bynk_check::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty);
    // Methods are emitted as plain (non-async) members on an object literal;
    // any `Effect.pure(...)` in tail position must still wrap as
    // `Promise.resolve(...)` because there's no surrounding `async` to absorb
    // it. (Methods aren't expected to return `Effect[T]` in v0–v0.7.1.)
    emit_block_as_function_body(out, &f.body, &mut cx, INDENT_STEP * 2, false);
    writeln!(out, "  }},").unwrap();
}

pub(crate) fn emit_free_fn(
    out: &mut String,
    f: &FnDecl,
    commons: &TypedCommons,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
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
    // v0.20a: erased TS generics — the type parameters print verbatim and
    // exist only at TS type-check time (no runtime dispatch).
    let generics = if f.type_params.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            f.type_params
                .iter()
                .map(|tp| tp.name.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    writeln!(
        out,
        "export {async_kw}function {name}{generics}({params}): {ret} {{",
        name = name.name,
        params = params.join(", "),
        ret = ts_type_ref(&f.return_type),
    )
    .unwrap();
    let empty = bynk_check::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty).with_source_map(source_map);
    let async_tail = is_effectful_return(&f.return_type);
    emit_block_as_function_body(out, &f.body, &mut cx, INDENT_STEP, async_tail);
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

pub(crate) fn is_effectful_return(r: &TypeRef) -> bool {
    matches!(r, TypeRef::Effect(_, _))
}

/// Synthesise a TypeScript-safe method name for an `on http METHOD path`
/// handler. The result is used both as the key on the service object and
/// as the identifier the Worker fetch handler invokes. Path parameter
/// segments (`:name`) become `Param_name` to remain distinct from literal
/// segments. (v0.9 §5.3)
pub(crate) fn http_handler_method_name(method: HttpMethod, path: &str) -> String {
    let mut s = format!("http_{}", method.as_str());
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        s.push('_');
        if let Some(rest) = seg.strip_prefix(':') {
            s.push_str("Param_");
            s.push_str(&sanitise_path_segment(rest));
        } else {
            s.push_str(&sanitise_path_segment(seg));
        }
    }
    s
}

/// Slice 3 (semantic-debugging track, ADR 0105): the debug-metadata sidecar's
/// `{ fn → label }` map — each emitted handler function paired with its Bynk
/// operation label, so the debugger can name a stack frame `GET "/"` rather than
/// `http_GET`. Built by re-walking the unit's handlers with the *same* naming
/// functions the emitter uses (so the keys match the emitted function names);
/// serialised as a JSON object (manual, like the source map — no serde). Returns
/// `None` when the unit declares no handlers.
pub(crate) fn collect_handler_labels(commons: &TypedCommons) -> Option<String> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for item in &commons.commons.items {
        match item {
            CommonsItem::Service(s) => {
                let (mut cron_idx, mut queue_idx) = (0usize, 0usize);
                for h in &s.handlers {
                    let pair = match &h.kind {
                        HandlerKind::Http { method, path } => (
                            http_handler_method_name(*method, path),
                            format!("{} \"{}\"", method.as_str(), path),
                        ),
                        HandlerKind::Cron { expr } => {
                            let n = cron_handler_method_name(&s.name.name, cron_idx);
                            cron_idx += 1;
                            (n, format!("cron \"{expr}\""))
                        }
                        HandlerKind::Message
                            if matches!(s.protocol, ServiceProtocol::WebSocket { .. }) =>
                        {
                            ("message".to_string(), "WebSocket message".to_string())
                        }
                        HandlerKind::Message => {
                            let n = queue_handler_method_name(&s.name.name, queue_idx);
                            queue_idx += 1;
                            (n, "message".to_string())
                        }
                        HandlerKind::Call => {
                            ("call".to_string(), handler_op_label("call", &h.params))
                        }
                        HandlerKind::Open => ("open".to_string(), "WebSocket open".to_string()),
                        HandlerKind::Close => ("close".to_string(), "WebSocket close".to_string()),
                    };
                    entries.push(pair);
                }
            }
            CommonsItem::Agent(a) => {
                for h in &a.handlers {
                    if let Some(name) = &h.method_name {
                        entries.push((name.name.clone(), handler_op_label(&name.name, &h.params)));
                    }
                }
            }
            _ => {}
        }
    }
    if entries.is_empty() {
        return None;
    }
    // Dedup by key (e.g. two services each with an `on call` emit a `call` method in
    // their own object — distinct in the emitted code, but one key here); keep the
    // first so the JSON object is well-formed.
    let mut seen = std::collections::HashSet::new();
    let mut out = String::from("{");
    let mut first = true;
    for (k, v) in &entries {
        if !seen.insert(k.clone()) {
            continue;
        }
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&source_map::json_string(k));
        out.push(':');
        out.push_str(&source_map::json_string(v));
    }
    out.push('}');
    Some(out)
}

/// `name(p1, p2)` from a handler's parameters — the operation label for `call` and
/// agent handlers (HTTP handlers use method + path instead).
fn handler_op_label(name: &str, params: &[Param]) -> String {
    let ps: Vec<String> = params.iter().map(|p| p.name.name.clone()).collect();
    format!("{}({})", name, ps.join(", "))
}

/// Synthesise a TypeScript-safe method name for an `on cron` handler (v0.10a):
/// `cron_<service>_<index>`, where `index` is the handler's position among the
/// service's cron handlers in declaration order. The same key is computed at
/// each emission site (the `handlers` method, the `compose` surface wrapper,
/// and the `scheduled` dispatcher) by walking handlers in the same order, so it
/// is collision-free and stable without encoding the schedule expression.
pub(crate) fn cron_handler_method_name(service: &str, index: usize) -> String {
    format!("cron_{service}_{index}")
}

/// v0.12: order a context's providers so each appears after the providers of
/// the capabilities it depends on (its `given`). Used by the composition root
/// to emit `const <Cap> = new <Provider>({ deps })` bindings in dependency
/// order. Cycles are rejected by the checker, so this terminates; the marker is
/// set before recursing as a defensive guard. Keyed by capability name.
pub(crate) fn topo_order_providers(
    providers: &std::collections::HashMap<String, ProviderDecl>,
) -> Vec<String> {
    fn visit(
        node: &str,
        providers: &std::collections::HashMap<String, ProviderDecl>,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if visited.contains(node) {
            return;
        }
        visited.insert(node.to_string());
        if let Some(p) = providers.get(node) {
            let mut deps: Vec<&str> = p
                .given
                .iter()
                .filter(|d| !d.is_cross_context())
                .map(|d| d.key())
                .filter(|n| providers.contains_key(*n))
                .collect();
            deps.sort_unstable();
            for d in deps {
                visit(d, providers, visited, order);
            }
        }
        order.push(node.to_string());
    }
    let mut order = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut keys: Vec<&String> = providers.keys().collect();
    keys.sort();
    for k in keys {
        visit(k, providers, &mut visited, &mut order);
    }
    order
}

/// Method name for an `on queue` handler (v0.10b): `queue_<service>_<index>`,
/// by the handler's position among the service's queue handlers. Computed
/// identically at the `handlers` method, the `compose` surface wrapper, and the
/// `queue` dispatcher (queue names are unique context-wide, but the index keeps
/// the key identifier-safe without sanitising the name).
pub(crate) fn queue_handler_method_name(service: &str, index: usize) -> String {
    format!("queue_{service}_{index}")
}

fn sanitise_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

// -- v0.5 emission --

pub(crate) fn emit_capability(out: &mut String, c: &CapabilityDecl) {
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

pub(crate) fn emit_provider(
    out: &mut String,
    p: &ProviderDecl,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    // v0.17: an external (bodiless) provider inside an adapter is supplied by
    // the adapter's binding — the compiler emits no class for it. Its symbol is
    // imported and constructed by the consumer's compose (§6.1, Phase 2).
    if p.external {
        return;
    }
    emit_doc_block(out, p.documentation.as_deref(), 0);
    writeln!(
        out,
        "export class {prov} implements {cap} {{",
        prov = p.provider_name.name,
        cap = p.capability.name,
    )
    .unwrap();
    // v0.12: a provider with `given` receives its dependencies through a
    // constructor; its bodies call them as `this.deps.<cap>`. The deps object
    // type lists exactly the provider's `given` capabilities.
    if !p.given.is_empty() {
        let deps_ty = p
            .given
            .iter()
            .map(|c| format!("{}: {}", c.key(), cap_ref_ty(c, &ctx.cross_context)))
            .collect::<Vec<_>>()
            .join("; ");
        writeln!(out, "  constructor(private deps: {{ {deps_ty} }}) {{}}").unwrap();
    }
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
        // v0.70: provider operation bodies lower directly into `out`, so attaching
        // the module builder records correct offsets — no splice merge needed.
        let mut cx = LowerCtx::new(commons, &ctx.cross_context).with_source_map(source_map);
        cx.local_agents = ctx.local_agents.clone();
        cx.target = ctx.target;
        // The provider's `given` capabilities are in scope in its bodies, and
        // resolve against the injected `this.deps`.
        cx.capabilities = p.given.iter().map(|c| c.key().to_string()).collect();
        if !p.given.is_empty() {
            cx.cap_deps_expr = "this.deps".to_string();
        }
        let async_tail = is_effectful_return(&op.return_type);
        emit_block_as_function_body(out, &op.body, &mut cx, INDENT_STEP * 2, async_tail);
        writeln!(out, "  }}").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    let factory = if p.given.is_empty() {
        format!("() => new {}()", p.provider_name.name)
    } else {
        format!("(deps: any) => new {}(deps)", p.provider_name.name)
    };
    writeln!(
        out,
        "export const {prov}Provider = {{ token: {cap}Token, factory: {factory} }};",
        prov = p.provider_name.name,
        cap = p.capability.name,
    )
    .unwrap();
    writeln!(out).unwrap();
}

pub(crate) fn emit_service(
    out: &mut String,
    s: &ServiceDecl,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    emit_doc_block(out, s.documentation.as_deref(), 0);
    writeln!(out, "export const {name} = {{", name = s.name.name).unwrap();
    let mut cron_idx = 0usize;
    let mut queue_idx = 0usize;
    let ws_proto = matches!(s.protocol, ServiceProtocol::WebSocket { .. });
    for handler in &s.handlers {
        // v0.104/v0.106 (real-time track slice 3b): on Workers a `from WebSocket`
        // lifecycle handler (`on open`/`on message`/`on close`) does not emit a
        // service-surface method — its body runs inside the hosting Durable Object
        // (`__wsOpen`/`__wsMessage`/`__wsClose`, DECISION A), driven by the edge
        // upgrade / `webSocketMessage` / `webSocketClose`, not from here. (On bundle
        // these are callable surface methods a `TestConnection` test drives.)
        let is_ws_handler = matches!(handler.kind, HandlerKind::Open | HandlerKind::Close)
            || (ws_proto && matches!(handler.kind, HandlerKind::Message));
        if is_ws_handler && matches!(ctx.target, BuildTarget::Workers) {
            continue;
        }
        emit_doc_block(out, handler.documentation.as_deref(), INDENT_STEP);
        let kind_name = match &handler.kind {
            HandlerKind::Call => "call".to_string(),
            HandlerKind::Http { method, path } => http_handler_method_name(*method, path),
            HandlerKind::Cron { .. } => {
                let name = cron_handler_method_name(&s.name.name, cron_idx);
                cron_idx += 1;
                name
            }
            // v0.106: a `from WebSocket` `on message` is the inbound surface method.
            HandlerKind::Message if ws_proto => "message".to_string(),
            HandlerKind::Message => {
                let name = queue_handler_method_name(&s.name.name, queue_idx);
                queue_idx += 1;
                name
            }
            // v0.103/v0.106: the WebSocket lifecycle surface methods.
            HandlerKind::Open => "open".to_string(),
            HandlerKind::Close => "close".to_string(),
        };
        // For service handlers the operation name is the handler kind
        // (e.g. `call`). v0.5 has only one handler kind, so the service is a
        // single-operation object literal.
        let mut params: Vec<String> = handler
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)))
            .collect();
        // v0.103/v0.106: a `from WebSocket` lifecycle handler receives the
        // `connection` as its first parameter (the synthetic binding the checker
        // added — the fresh socket for `on open`, the firing socket for `on
        // message`/`on close`); emit it so the lowered body's `connection` resolves.
        if is_ws_handler && let ServiceProtocol::WebSocket { out_type, .. } = &s.protocol {
            params.insert(
                0,
                format!("connection: Connection<{}>", ts_type_ref(out_type)),
            );
        }
        // Lower the body first so we can detect cross-context usage and
        // adjust the deps shape accordingly.
        let mut body_out = String::new();
        // v0.70: each handler body lowers into its own source-map sub-builder
        // (offsets relative to `body_out`), merged into the module builder at the
        // splice below so handler statements map per-statement, not to the
        // `service` declaration line.
        let body_smb = RefCell::new(SourceMapBuilder::new());
        let mut cx = LowerCtx::new(commons, &ctx.cross_context).with_source_map(Some(&body_smb));
        cx.capabilities = handler
            .given
            .iter()
            .map(|c| c.key().to_string())
            .collect::<HashSet<_>>();
        cx.local_agents = ctx.local_agents.clone();
        cx.target = ctx.target;
        // v0.52: a multi-actor sum handler's resolved actor is threaded through
        // `deps.who`; the binder ident lowers to it so the body can `match`. A
        // sum supersedes the single-actor Bearer identity path (the per-arm
        // identity comes from the match, not a single `deps.identity`).
        let sum_members = bynk_check::actors::sum_members_for(handler, &ctx.actors);
        // v0.47: a single Bearer handler's identity is threaded through `deps`;
        // tell the body lowering so `<binder>.identity` reads `deps.identity`.
        let bearer_seam = if sum_members.is_some() {
            None
        } else {
            bynk_check::actors::bearer_seam_for(handler, &ctx.actors)
        };
        cx.deps_identity_binder = bearer_seam.as_ref().and_then(|s| s.binder.clone());
        if sum_members.is_some()
            && let Some(by) = &handler.by_clause
            && let Some(binder) = &by.binder
        {
            cx.actor_sum_binder = Some(binder.name.clone());
        }
        // v0.54: a cross-context `on call … by c: Caller` handler reads a live
        // `CallerId` (the calling context's name) threaded through
        // `deps.identity`, exactly like the Bearer identity. Only when it binds.
        let caller_binder = if bearer_seam.is_none() && sum_members.is_none() {
            bynk_check::actors::caller_binder_for(handler, &ctx.actors)
        } else {
            None
        };
        if let Some(binder) = &caller_binder {
            cx.deps_identity_binder = Some(binder.clone());
        }
        let async_tail = is_effectful_return(&handler.return_type);
        emit_block_as_function_body(
            &mut body_out,
            &handler.body,
            &mut cx,
            INDENT_STEP * 2,
            async_tail,
        );
        // Append the deps parameter (may include surface field if the body
        // made cross-context calls). v0.47: a Bearer handler's deps also carries
        // the seam-minted `identity` — but only when a binder captures it
        // (v0.50: a binder-less Bearer handler verifies but mints no identity).
        let mut deps_ty =
            build_deps_object_ty_with_surface(&handler.given, &cx, &ctx.cross_context, ctx.target);
        if let Some(seam) = bearer_seam.as_ref().filter(|s| s.binder.is_some()) {
            let field = format!("identity: {}", seam.identity_type);
            deps_ty = if deps_ty == "{}" {
                format!("{{ {field} }}")
            } else {
                format!(
                    "{}; {field} }}",
                    deps_ty.trim_end().trim_end_matches('}').trim_end()
                )
            };
        }
        // v0.54: a Caller-binding call handler's deps carries the caller's
        // context name as its `CallerId` identity (a `string`).
        if caller_binder.is_some() {
            deps_ty = if deps_ty == "{}" {
                "{ identity: string }".to_string()
            } else {
                format!(
                    "{}; identity: string }}",
                    deps_ty.trim_end().trim_end_matches('}').trim_end()
                )
            };
        }
        // v0.52: a sum handler's deps carries the resolved-actor tagged union
        // (`who`), which the body `match`es. A binder-less sum is rejected by the
        // checker, so a sum handler always captures `who`.
        if let Some(members) = sum_members.as_ref() {
            let union = members
                .iter()
                .map(|m| match m.identity_type() {
                    Some(id) => format!("{{ tag: \"{}\", identity: {id} }}", m.actor_name),
                    None => format!("{{ tag: \"{}\" }}", m.actor_name),
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let field = format!("who: {union}");
            deps_ty = if deps_ty == "{}" {
                format!("{{ {field} }}")
            } else {
                format!(
                    "{}; {field} }}",
                    deps_ty.trim_end().trim_end_matches('}').trim_end()
                )
            };
        }
        // v0.79: a handler whose body uses `~>` receives the execution context
        // (`__exec`) in its deps, so the fire-and-forget send can hand its promise
        // to `waitUntil`. Gated on the body so non-sending handlers are unchanged.
        if crate::emitter::block_uses_send(&handler.body) {
            let field = "__exec: { waitUntil(promise: Promise<unknown>): void }";
            deps_ty = if deps_ty == "{}" {
                format!("{{ {field} }}")
            } else {
                format!(
                    "{}; {field} }}",
                    deps_ty.trim_end().trim_end_matches('}').trim_end()
                )
            };
        }
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
        let base = out.len();
        out.push_str(&body_out);
        if let Some(module) = source_map {
            module
                .borrow_mut()
                .merge(&body_smb.borrow(), &body_out, out, base, 0);
        }
        writeln!(out, "  }},").unwrap();
    }
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

/// v0.15: the TypeScript deps-field type for a `given` capability reference.
/// A local capability uses its bare interface name; a cross-context one is
/// qualified with the providing context's import namespace
/// (`platform_time.Clock`).
fn cap_ref_ty(c: &CapRef, info: &bynk_check::resolver::CrossContextInfo) -> String {
    match c.prefix().and_then(|p| info.resolve_prefix(&p)) {
        Some(consumed) => format!("{}.{}", qualified_to_ns(&consumed), c.key()),
        // v0.17: a bare flattened capability (`consumes U { Cap }`) keeps its
        // interface in the consumed unit's module — qualify the type there.
        None => match info.flattened_caps.get(c.key()) {
            Some(unit) => format!("{}.{}", qualified_to_ns(unit), c.key()),
            None => c.key().to_string(),
        },
    }
}

/// v0.15: collect the cross-context capabilities a context's **handlers**
/// (service + agent) reference via `given B.Cap`, as `(deps_key,
/// consumed_context)` pairs, deduplicated by key and sorted. These become
/// top-level deps fields (handlers access them as `deps.<key>`). Capabilities
/// used only by a provider are injected into that provider's constructor
/// instead, so they are excluded here.
pub(crate) fn cross_context_caps_used(
    commons: &TypedCommons,
    info: &bynk_check::resolver::CrossContextInfo,
) -> Vec<(String, String)> {
    let mut seen: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for item in &commons.commons.items {
        let handlers = match item {
            CommonsItem::Service(s) => &s.handlers,
            CommonsItem::Agent(a) => &a.handlers,
            _ => continue,
        };
        for h in handlers {
            for c in &h.given {
                if let Some(prefix) = c.prefix() {
                    if let Some(consumed) = info.resolve_prefix(&prefix) {
                        seen.entry(c.key().to_string()).or_insert(consumed);
                    }
                } else if let Some(unit) = info.flattened_caps.get(c.key()) {
                    // v0.17: a bare flattened capability is a cross-unit dep too.
                    seen.entry(c.key().to_string())
                        .or_insert_with(|| unit.clone());
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// v0.15: the set of consumed contexts whose capabilities this context
/// references anywhere (handlers *or* providers), so their namespaces are
/// imported for the capability interface types.
pub(crate) fn cross_context_cap_namespaces(
    commons: &TypedCommons,
    info: &bynk_check::resolver::CrossContextInfo,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut collect = |given: &[CapRef]| {
        for c in given {
            if let Some(prefix) = c.prefix()
                && let Some(consumed) = info.resolve_prefix(&prefix)
            {
                out.insert(consumed);
            } else if c.prefix().is_none()
                // v0.17: a bare flattened capability imports its interface from
                // the consumed unit's module.
                && let Some(unit) = info.flattened_caps.get(c.key())
            {
                out.insert(unit.clone());
            }
        }
    };
    for item in &commons.commons.items {
        match item {
            CommonsItem::Service(s) => s.handlers.iter().for_each(|h| collect(&h.given)),
            CommonsItem::Agent(a) => a.handlers.iter().for_each(|h| collect(&h.given)),
            CommonsItem::Provider(p) => collect(&p.given),
            _ => {}
        }
    }
    out
}

fn build_deps_object_ty_with_surface(
    given: &[CapRef],
    cx: &LowerCtx<'_>,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    target: BuildTarget,
) -> String {
    let mut parts: Vec<String> = given
        .iter()
        .map(|c| format!("{}: {}", c.key(), cap_ref_ty(c, cross_context)))
        .collect();
    match target {
        BuildTarget::Bundle => {
            if cx.cross_context_used {
                parts.push(format!("surface: {}", surface_ty(cross_context)));
            }
        }
        BuildTarget::Workers => {
            // v0.9.2: in workers mode `env` carries both consumed-context
            // Service Bindings and the local agents' Durable Object namespaces.
            // It is threaded into deps whenever the handler makes a
            // cross-context call or instantiates an agent.
            if cx.cross_context_used || cx.agents_instantiated {
                let agents = if cx.agents_instantiated {
                    sorted_local_agents(cx)
                } else {
                    Vec::new()
                };
                parts.push(format!("env: {}", workers_env_ty(cross_context, &agents)));
            }
        }
    }
    if parts.is_empty() {
        return "{}".to_string();
    }
    format!("{{ {} }}", parts.join("; "))
}

/// Local agent names in this commons, sorted — the DO bindings `env` exposes
/// in workers mode.
fn sorted_local_agents(cx: &LowerCtx<'_>) -> Vec<String> {
    let mut names: Vec<String> = cx
        .commons
        .commons
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Agent(a) => Some(a.name.name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names
}

/// Workers-mode deps.env shape: one Service Binding per consumed context and
/// one Durable Object namespace per local agent.
fn workers_env_ty(
    cross_context: &bynk_check::resolver::CrossContextInfo,
    agents: &[String],
) -> String {
    let mut consumed_sorted = cross_context.consumed_contexts.clone();
    consumed_sorted.sort();
    let mut entries: Vec<String> = consumed_sorted
        .iter()
        .map(|q| {
            let bind = crate::emitter::wrangler::consumed_binding_name(q);
            format!("{bind}: ServiceBinding")
        })
        .collect();
    for agent in agents {
        let bind = crate::emitter::wrangler::agent_binding_name(agent);
        entries.push(format!("{bind}: DurableObjectNamespace"));
    }
    if entries.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", entries.join("; "))
    }
}

/// v0.15: true when at least one consumed context exposes services (and thus
/// a `makeSurface`). A context may now consume another purely for its
/// capabilities, in which case there is no surface to thread.
fn has_consumed_service(cross_context: &bynk_check::resolver::CrossContextInfo) -> bool {
    cross_context
        .consumed_services
        .values()
        .any(|svcs| !svcs.is_empty())
}

/// Build the TS type for the `surface` field in deps, naming each consumed
/// context by its surface key plus the consumed context's makeSurface type.
/// Only service-bearing consumed contexts contribute (a capability-only
/// consumed context has no `makeSurface`).
fn surface_ty(cross_context: &bynk_check::resolver::CrossContextInfo) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();
    // Use alias if present, else the last segment of the qualified name.
    // Order: stable (sorted) so the diff is deterministic.
    let mut consumed_sorted: Vec<String> = cross_context
        .consumed_services
        .iter()
        .filter(|(_, svcs)| !svcs.is_empty())
        .map(|(q, _)| q.clone())
        .collect();
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
pub(crate) fn qualified_to_ns(q: &str) -> String {
    q.replace('.', "_")
}

/// The PascalCase name a context uses for its generated `Deps` interface:
/// `shortener.links` → `ShortenerLinks`.
fn context_pascal(name: &str) -> String {
    name.split('.')
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Emit the per-context `<Ctx>Deps` interface (v0.9.2 §4): the providers the
/// context contributes plus the surfaces of any consumed contexts. Replaces
/// the fragile `Parameters<typeof svc.call>[1]` indexing, which only resolved
/// correctly for single-argument service operations.
fn emit_context_deps_interface(
    out: &mut String,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) -> String {
    let deps_name = format!("{}Deps", context_pascal(&commons.commons.name.joined()));
    let mut fields: Vec<String> = commons
        .commons
        .items
        .iter()
        .filter_map(|i| match i {
            CommonsItem::Capability(c) => Some(format!("  readonly {n}: {n};", n = c.name.name)),
            _ => None,
        })
        .collect();
    // v0.15: cross-context capabilities the context consumes appear in deps,
    // typed against the providing context's namespace.
    for (key, consumed) in cross_context_caps_used(commons, &ctx.cross_context) {
        fields.push(format!(
            "  readonly {key}: {ns}.{key};",
            ns = qualified_to_ns(&consumed)
        ));
    }
    if !ctx.cross_context.consumed_contexts.is_empty() && has_consumed_service(&ctx.cross_context) {
        fields.push(format!(
            "  readonly surface: {};",
            surface_ty(&ctx.cross_context)
        ));
    }
    writeln!(out, "export interface {deps_name} {{").unwrap();
    for f in &fields {
        writeln!(out, "{f}").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    deps_name
}

/// Emit the `makeSurface(deps)` function for a context that exposes
/// services to other contexts (v0.6 §6.3 / §6.4).
pub(crate) fn emit_make_surface(out: &mut String, commons: &TypedCommons, ctx: &EmitProjectCtx) {
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
    let deps_name = emit_context_deps_interface(out, commons, ctx);
    writeln!(out, "export function makeSurface(deps: {deps_name}) {{").unwrap();
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

/// Lower a cross-context call in workers mode to a `callService(...)`
/// invocation. The argument types are looked up in the consumed context's
/// service signature so we know which serialise/deserialise helpers to
/// reference.
pub(crate) fn lower_workers_cross_context_call(
    consumed: &str,
    method: &Ident,
    args: &[Expr],
    stmts: &mut Vec<String>,
    cx: &mut LowerCtx<'_>,
) -> String {
    let info = cx.cross_context;
    let consumed_ns = qualified_to_ns(consumed);
    let binding = crate::emitter::wrangler::consumed_binding_name(consumed);

    // Look up the service signature on the consumed context.
    let svc = info
        .consumed_services
        .get(consumed)
        .and_then(|map| map.get(&method.name));

    // Build serialised-arg expression. If we know the parameter types, use
    // the owning context's serialise_<T> helper; otherwise fall back to
    // `value as JsonValue`.
    let mut args_serialised: Vec<String> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let lowered = lower_expr(a, stmts, cx);
        let param_ty = svc.and_then(|s| s.params.get(i)).map(|(_, t)| t);
        let serialised = match param_ty {
            Some(tr) => workers_serialise_expr(tr, &lowered, &consumed_ns),
            None => format!("{lowered} as JsonValue"),
        };
        args_serialised.push(serialised);
    }
    let args_json = if args_serialised.len() == 1 {
        args_serialised.into_iter().next().unwrap()
    } else {
        // Multi-arg: wrap into an object literal keyed by parameter names.
        let pairs: Vec<String> = svc
            .map(|s| {
                s.params
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| {
                        let serialised = args_serialised
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| "null".to_string());
                        format!("{name}: {serialised}")
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                args_serialised
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, s)| format!("arg{i}: {s}"))
                    .collect()
            });
        format!("{{ {} }}", pairs.join(", "))
    };

    // Deserialise the return value via the consumed context's helper.
    let deser_ref = match svc {
        Some(s) => workers_deserialise_ref(&s.return_type, &consumed_ns),
        None => "((j: any) => ({ tag: \"Ok\", value: j }))".to_string(),
    };

    // v0.54: stamp the calling context's qualified name so the callee's
    // `by c: Caller` handler reads a live `CallerId` (Q7). A compile-time
    // constant; the args body is unchanged.
    let caller = cx.commons.commons.name.joined().replace('"', "\\\"");
    format!(
        "callService(deps.env.{binding}, \"{}\", {args_json}, {deser_ref}, \"{caller}\")",
        method.name
    )
}

fn workers_serialise_expr(tr: &TypeRef, value: &str, owning_ns: &str) -> String {
    match tr {
        TypeRef::Base(_, _) => format!("{value} as JsonValue"),
        TypeRef::Named(id) => format!("{owning_ns}.serialise_{}({value})", id.name),
        TypeRef::Result(_, _, _) | TypeRef::Option(_, _) => {
            let inst = workers_inner_ts_name(tr);
            format!("{owning_ns}.serialise_{inst}({value})")
        }
        TypeRef::Effect(inner, _) => workers_serialise_expr(inner, value, owning_ns),
        _ => format!("{value} as JsonValue"),
    }
}

fn workers_deserialise_ref(tr: &TypeRef, owning_ns: &str) -> String {
    // Strip the Effect wrapper — the caller already awaits the Promise.
    let inner = match tr {
        TypeRef::Effect(t, _) => t.as_ref(),
        other => other,
    };
    match inner {
        TypeRef::Named(id) => format!("{owning_ns}.deserialise_{}", id.name),
        TypeRef::Result(_, _, _)
        | TypeRef::Option(_, _)
        | TypeRef::List(_, _)
        | TypeRef::Map(_, _, _) => {
            let inst = workers_inner_ts_name(inner);
            format!("{owning_ns}.deserialise_{inst}")
        }
        _ => "((j: any) => ({ tag: \"Ok\", value: j }))".to_string(),
    }
}

fn workers_inner_ts_name(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        // v0.20a: function types are confined to non-boundary positions
        // (`bynk.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..) | TypeRef::Query(..) | TypeRef::Stream(..) | TypeRef::Connection(..) => {
            unreachable!("function/query/stream types are rejected at boundaries")
        }
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!(
            "Result_{}_{}",
            workers_inner_ts_name(a),
            workers_inner_ts_name(b)
        ),
        TypeRef::Option(a, _) => format!("Option_{}", workers_inner_ts_name(a)),
        TypeRef::Effect(a, _) => format!("Effect_{}", workers_inner_ts_name(a)),
        TypeRef::HttpResult(a, _) => format!("HttpResult_{}", workers_inner_ts_name(a)),
        TypeRef::List(a, _) => format!("List_{}", workers_inner_ts_name(a)),
        TypeRef::Map(k, v, _) => format!(
            "Map_{}_{}",
            workers_inner_ts_name(k),
            workers_inner_ts_name(v)
        ),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "Unit".to_string(),
    }
}

/// If `receiver` is a dotted chain or single ident that matches one of the
/// current context's `consumes` clauses (by alias or qualified name), return
/// the consumed context's qualified name plus the surface key used to access
/// it through `deps.surface.<key>`.
pub(crate) fn cross_context_lowering_prefix(
    receiver: &Expr,
    cx: &LowerCtx<'_>,
) -> Option<(String, String)> {
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

pub(crate) fn flatten_emit_ident_chain(e: &Expr) -> Option<String> {
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
/// bynk layer guarantees the cast is sound.
pub(crate) fn param_cast(
    consumed: &str,
    info: &bynk_check::resolver::CrossContextInfo,
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
        // v0.9.1: when both contexts brand the same commons type (e.g., both
        // see `Money` with their own `__ctxBrand`), a direct
        // `as <ns>.<Type>` cast is rejected by `tsc --strict` because the
        // brand discriminants are incompatible. Bynk guarantees the value's
        // base type matches at the boundary, so route through `unknown` to
        // tell TypeScript to trust the structural Bynk-side check.
        return format!("({arg} as unknown as {ns}.{name})");
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

/// True when a type has a boundary deserialiser (ADR 0124 rehydration gate). A
/// non-codec type never legally reaches a `store` position, but a `Cell[()]`
/// could, so the gate skips anything `deserialise_expr` would reject.
fn is_codecable(t: &TypeRef) -> bool {
    matches!(
        t,
        TypeRef::Base(..)
            | TypeRef::Named(..)
            | TypeRef::Option(..)
            | TypeRef::List(..)
            | TypeRef::Result(..)
            | TypeRef::Map(..)
    )
}

/// Whether an agent emits a rehydration gate (and so imports `rehydrationViolation`).
/// Mirrors the per-field validation the gate builds, so the header import and the
/// emitted gate agree exactly (a mismatch is an unused / undefined import).
pub(crate) fn agent_needs_rehydrate(a: &AgentDecl, types: &HashMap<String, TypeDecl>) -> bool {
    // v0.105 (slice 3b-ii): a held `Map[K, Connection]` persists `K → connId` and
    // is rehydrated like any map — no value check (the connId is opaque) but the
    // textual `K` key is validated. The `Map` arm already counts that via the key,
    // so held maps need no special handling here.
    a.store_fields
        .iter()
        .any(|f| match f.kind.head.name.as_str() {
            "Cell" | "Log" => f.kind.args.first().is_some_and(is_codecable),
            "Map" | "Cache" => {
                f.kind.args.get(1).is_some_and(is_codecable)
                    || f.kind
                        .args
                        .first()
                        .is_some_and(|k| type_base_is_string(k, types))
            }
            "Set" => f
                .kind
                .args
                .first()
                .is_some_and(|t| type_base_is_string(t, types)),
            _ => false,
        })
}

/// v0.104/v0.105 (real-time track slice 3b): an agent has at least one `store
/// Map[K, V]` whose value `V` is a held resource (a `Connection`). On Workers a
/// live socket cannot serialise, so the durable record persists the **connection
/// id** (`Record<K, connId>`) and each `Connection` is re-resolved from its connId
/// via the hibernatable-socket API — so a stored connection survives DO eviction
/// (§2.9.6). Drives the `resolveConnection`/`connIdOf` imports and the held-map
/// realisation in `emit_agent`. (Bundle keeps held maps in the in-memory test
/// state record of `TestConnection`s — its tested behaviour — so this is
/// Workers-only.)
pub(crate) fn agent_has_held_storage(a: &AgentDecl) -> bool {
    !held_map_fields(a).is_empty()
}

/// True if a `store` field is a `Map[K, V]` whose value `V` is a held
/// `Connection` — the held maps split out of persistence on Workers.
fn is_held_map_field(f: &StoreField) -> bool {
    f.kind.head.name == "Map"
        && f.kind.args.len() == 2
        && crate::project::type_ref_is_held(&f.kind.args[1])
}

/// The agent's `store Map[K, V]` fields whose value is a held `Connection`, as
/// `(name, value-type)`. The value type is the held `Connection[F]` itself (so
/// its TS rendering is `Connection<F>`).
pub(crate) fn held_map_fields(a: &AgentDecl) -> Vec<(&Ident, &TypeRef)> {
    a.store_fields
        .iter()
        .filter(|f| is_held_map_field(f))
        .map(|f| (&f.name, &f.kind.args[1]))
        .collect()
}

/// The key type (`K`) of a two-argument store field (`Map`/`Cache`) named
/// `field`, used by the rehydration gate to validate textual keys (ADR 0124).
fn store_field_key_type<'a>(a: &'a AgentDecl, field: &str) -> Option<&'a TypeRef> {
    a.store_fields
        .iter()
        .find(|f| f.name.name == field && f.kind.args.len() == 2)
        .map(|f| &f.kind.args[0])
}

/// True when a type's base is `String` — directly, or through a named refined /
/// opaque alias. A textual key persists as its own string in a storage `Record`,
/// so the rehydration gate can validate it; a non-textual key persists as a
/// `String(k)` structural key, whose refinement validation is deferred (ADR 0124).
fn type_base_is_string(t: &TypeRef, types: &HashMap<String, TypeDecl>) -> bool {
    match t {
        TypeRef::Base(BaseType::String, _) => true,
        TypeRef::Named(id) => matches!(
            types.get(&id.name).map(|d| &d.body),
            Some(TypeBody::Refined {
                base: BaseType::String,
                ..
            }) | Some(TypeBody::Opaque {
                base: BaseType::String,
                ..
            })
        ),
        _ => false,
    }
}

pub(crate) fn emit_agent(
    out: &mut String,
    a: &AgentDecl,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    emit_doc_block(out, a.documentation.as_deref(), 0);
    let state_ty = format!("{}State", a.name.name);
    // v0.81 (storage track, ADR 0109): an agent's `Cell` fields ARE its state
    // record, so the whole state machinery (interface, zero factory, load/commit,
    // invariant gate) derives the record fields from the cells. Each `Cell[T]`
    // field becomes a `T`-typed record field carrying the cell's initialiser.
    // Handler bodies lower as bare reads / `:=` over `__state`, with an implicit
    // commit at handler end.
    let is_store_agent = true;
    let effective_fields: Vec<RecordField> = a
        .store_fields
        .iter()
        .filter(|f| f.kind.head.name == "Cell" && f.kind.args.len() == 1)
        .map(|f| RecordField {
            name: f.name.clone(),
            type_ref: f.kind.args[0].clone(),
            refinement: None,
            init: f.init.clone(),
            span: f.span,
        })
        .collect();
    // v0.82 (ADR 0110): `store Map[K, V]` fields are also state-record fields, but
    // persisted as a JSON-serialisable `Record<string, V>` (the value `Map` is a
    // JS `Map`, which does not serialise). Collected separately from `Cell` fields
    // since their TS type and zero differ; the working record is committed by the
    // same flush. `(name, V)`.
    // v0.104/v0.105 (real-time track slice 3b): on Workers, a `store Map[K,
    // Connection]` holds live sockets that cannot themselves be JSON-persisted.
    // v0.105 (slice 3b-ii) persists the **connection id** instead: the held map is a
    // durable `Record<string(K), connId>` in the state record, and each `Connection`
    // is re-resolved from its connId via the platform's hibernatable-socket API, so a
    // stored connection survives DO eviction (§2.9.6). The held maps are kept *out*
    // of `store_map_fields` (their value type and entry lowering differ from a plain
    // `Map[K, V]`) but are emitted into the state interface / zero / rehydration
    // key-check / load-commit as string records, and they trigger the commit flush
    // like any other persisted field. (Bundle keeps held maps in the in-memory test
    // state record of `TestConnection`s — its tested behaviour — so the connId
    // representation is Workers-only.)
    let is_workers = matches!(ctx.target, BuildTarget::Workers);
    let held_maps: Vec<(&Ident, &TypeRef)> = if is_workers {
        held_map_fields(a)
    } else {
        Vec::new()
    };
    let held_map_names: HashSet<String> = held_maps.iter().map(|(n, _)| n.name.clone()).collect();
    // The held map's lowering needs the connection's **frame type** `F` (for
    // `resolveConnection<F>`), not the `Connection<F>` wrapper — extract it.
    let held_maps_ts: HashMap<String, String> = held_maps
        .iter()
        .map(|(n, v)| {
            let f_ts = match v {
                TypeRef::Connection(inner, _) => ts_type_ref(inner),
                _ => ts_type_ref(v),
            };
            (n.name.clone(), f_ts)
        })
        .collect();
    let store_map_fields: Vec<(&Ident, &TypeRef)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Map" && f.kind.args.len() == 2)
            .filter(|f| !held_map_names.contains(&f.name.name))
            .map(|f| (&f.name, &f.kind.args[1]))
            .collect()
    } else {
        Vec::new()
    };
    let map_names: HashSet<String> = store_map_fields
        .iter()
        .map(|(n, _)| n.name.clone())
        .collect();
    // v0.93 (ADR 0118): `store Map[K, V] @indexed(by: f, …)` — each `by:` field
    // gets a maintained secondary index. `map name → [field, …]` (a deduped,
    // declaration-ordered list). The keys are validated against `V` in
    // `project::validate`; here we only read the surface to drive emission.
    let store_map_indexes: HashMap<String, Vec<String>> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Map" && f.kind.args.len() == 2)
            .filter_map(|f| {
                let mut fields: Vec<String> = Vec::new();
                for an in f.annotations.iter().filter(|an| an.name.name == "indexed") {
                    for arg in &an.args {
                        if arg.label.as_ref().map(|l| l.name.as_str()) == Some("by")
                            && let ExprKind::Ident(k) = &arg.value.kind
                            && !fields.contains(&k.name)
                        {
                            fields.push(k.name.clone());
                        }
                    }
                }
                (!fields.is_empty()).then(|| (f.name.name.clone(), fields))
            })
            .collect()
    } else {
        HashMap::new()
    };
    // v0.83 (ADR 0110): `store Set[T]` fields are state-record fields too,
    // persisted as a JSON-serialisable `Record<string, boolean>` (a JS `Set`
    // does not serialise). `(name, T)`; the element type is unused in the TS
    // representation but kept for symmetry with maps.
    let store_set_fields: Vec<(&Ident, &TypeRef)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Set" && f.kind.args.len() == 1)
            .map(|f| (&f.name, &f.kind.args[0]))
            .collect()
    } else {
        Vec::new()
    };
    let set_names: HashSet<String> = store_set_fields
        .iter()
        .map(|(n, _)| n.name.clone())
        .collect();
    // v0.87 (ADR 0113): `store Cache[K, V] @ttl(d)` fields — a value record plus
    // a per-entry expiry instant. `(name, V, ttl-millis)`; the ttl is the field's
    // `@ttl` Duration literal (validated by the checker).
    let store_cache_fields: Vec<(&Ident, &TypeRef, i64)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Cache" && f.kind.args.len() == 2)
            .filter_map(|f| {
                let ttl = f
                    .annotations
                    .iter()
                    .find(|an| an.name.name == "ttl")
                    .and_then(|an| match an.args.first().map(|arg| &arg.value.kind) {
                        Some(ExprKind::DurationLit { millis, .. }) => Some(*millis),
                        _ => None,
                    })?;
                Some((&f.name, &f.kind.args[1], ttl))
            })
            .collect()
    } else {
        Vec::new()
    };
    let cache_ttls: HashMap<String, i64> = store_cache_fields
        .iter()
        .map(|(n, _, ttl)| (n.name.clone(), *ttl))
        .collect();
    let cache_names: HashSet<String> = cache_ttls.keys().cloned().collect();
    // v0.95 (ADR 0121): `store Log[T] [@retain(d)]` fields — an ordered array of
    // `{ t, v }` entries. `(name, T, optional retain-millis)`; the retain (from
    // `@retain`) prunes on append.
    let store_log_fields: Vec<(&Ident, &TypeRef, Option<i64>)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Log" && f.kind.args.len() == 1)
            .map(|f| {
                let retain = f
                    .annotations
                    .iter()
                    .find(|an| an.name.name == "retain")
                    .and_then(|an| match an.args.first().map(|arg| &arg.value.kind) {
                        Some(ExprKind::DurationLit { millis, .. }) => Some(*millis),
                        _ => None,
                    });
                (&f.name, &f.kind.args[0], retain)
            })
            .collect()
    } else {
        Vec::new()
    };
    let log_retains: HashMap<String, Option<i64>> = store_log_fields
        .iter()
        .map(|(n, _, r)| (n.name.clone(), *r))
        .collect();
    let log_names: HashSet<String> = log_retains.keys().cloned().collect();
    // 1) State record type.
    writeln!(out, "export interface {state_ty} {{").unwrap();
    for f in &effective_fields {
        writeln!(
            out,
            "  readonly {name}: {ty};",
            name = f.name.name,
            ty = ts_type_ref(&f.type_ref),
        )
        .unwrap();
    }
    for (name, v) in &store_map_fields {
        writeln!(
            out,
            "  readonly {name}: Record<string, {v}>;",
            name = name.name,
            v = ts_type_ref(v),
        )
        .unwrap();
    }
    // v0.105 (slice 3b-ii): a held `Map[K, Connection]` persists `K → connId`.
    for (name, _) in &held_maps {
        writeln!(
            out,
            "  readonly {name}: Record<string, string>;",
            name = name.name,
        )
        .unwrap();
    }
    for (name, _) in &store_set_fields {
        writeln!(
            out,
            "  readonly {name}: Record<string, boolean>;",
            name = name.name,
        )
        .unwrap();
    }
    // v0.93 (ADR 0118): a sibling posting-list per `@indexed(by: f)` — field
    // value (stringified) → the primary keys whose value has it. Persisted and
    // committed wholesale with the map it indexes.
    for (map, fields) in sorted_index_fields(&store_map_indexes) {
        for f in fields {
            writeln!(out, "  readonly {map}__idx_{f}: Record<string, string[]>;").unwrap();
        }
    }
    for (name, v, _) in &store_cache_fields {
        writeln!(
            out,
            "  readonly {name}: Record<string, {{ v: {v}; exp: number }}>;",
            name = name.name,
            v = ts_type_ref(v),
        )
        .unwrap();
    }
    for (name, v, _) in &store_log_fields {
        writeln!(
            out,
            "  readonly {name}: Array<{{ t: number; v: {v} }}>;",
            name = name.name,
            v = ts_type_ref(v),
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    // v0.9.2: per-agent state registry (bundle mode + `bynkc test`) and the
    // zero-value factory used to initialise a fresh key's state.
    let registry = agent_registry_name(&a.name.name);
    let zero_fn = format!("__zeroOf{}State", a.name.name);
    writeln!(out, "const {registry} = new StateRegistry();").unwrap();
    // v0.11: build the fresh-state record. A field with an explicit initialiser
    // lowers its (static) expression; a field without one uses the v0.9.2
    // implicit zero.
    let zero_record = {
        let mut parts: Vec<String> = Vec::new();
        for f in &effective_fields {
            let val = if let Some(init) = &f.init {
                let mut stmts = Vec::new();
                let mut icx = LowerCtx::new(commons, &ctx.cross_context);
                icx.target = ctx.target;
                icx.local_agents = ctx.local_agents.clone();
                let expr = lower_expr(init, &mut stmts, &mut icx);
                // A static initialiser lowers to a pure expression (no setup
                // statements); if any appear, fall back to inlining them as a
                // comma sequence so the record stays valid.
                if stmts.is_empty() {
                    expr
                } else {
                    format!("({}, {expr})", stmts.join(", "))
                }
            } else {
                bynk_check::checker::zero_value_ts(
                    &f.type_ref,
                    f.refinement.as_ref(),
                    &commons.types,
                )
                .unwrap_or_else(|| "undefined as never".to_string())
            };
            parts.push(format!("{}: {val}", f.name.name));
        }
        // A fresh `store Map`/`store Set`/`store Cache` is the empty record.
        for (name, _) in &store_map_fields {
            parts.push(format!("{}: {{}}", name.name));
        }
        // A fresh held `Map[K, Connection]` (a `K → connId` record) is empty too.
        for (name, _) in &held_maps {
            parts.push(format!("{}: {{}}", name.name));
        }
        // A fresh `@indexed` posting-list is empty too (v0.93, ADR 0118).
        for (map, fields) in sorted_index_fields(&store_map_indexes) {
            for f in fields {
                parts.push(format!("{map}__idx_{f}: {{}}"));
            }
        }
        for (name, _) in &store_set_fields {
            parts.push(format!("{}: {{}}", name.name));
        }
        for (name, _, _) in &store_cache_fields {
            parts.push(format!("{}: {{}}", name.name));
        }
        // A fresh `store Log` is the empty array.
        for (name, _, _) in &store_log_fields {
            parts.push(format!("{}: []", name.name));
        }
        if parts.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", parts.join(", "))
        }
    };
    writeln!(
        out,
        "function {zero_fn}(): {state_ty} {{ return {zero_record}; }}"
    )
    .unwrap();
    writeln!(out).unwrap();
    // v0.96 (ADR 0124): the rehydration validation gate. `loadState` validates a
    // *loaded* (merged) state against the current type definition before any
    // handler reads it — the load-time twin of the commit-time invariant gate.
    // Each value position (a `Cell`'s `T`, a `Map`/`Cache`'s `V`, a `Log`'s `T`,
    // and a textual `Set` element / `Map` key) is run through the same boundary
    // deserialiser the HTTP/queue seams use; a failure is disposed of as an
    // internal `RehydrationViolation` fault (Q6), never a caller-facing 400.
    // (Non-textual `Map`/`Set` keys persist as structural string keys — refined-
    // key rehydration validation is a named follow-on, ADR 0124 D5.)
    let rehydrate_fn = format!("__rehydrate{}State", a.name.name);
    let agent_name = &a.name.name;
    let mut rehydrate_checks: Vec<String> = Vec::new();
    // The loaded record is statically typed (its fields are the agent's types),
    // but at runtime its bytes are untrusted, so each value is laundered to
    // `JsonValue` before the boundary deserialiser re-validates it.
    let push_value_check = |checks: &mut Vec<String>,
                            ty: &TypeRef,
                            value_expr: &str,
                            path: &str| {
        // Only codec-able types have a deserialiser; a non-storable type never
        // reaches a `store` position (the checker rejects it), but guard anyway.
        if !is_codecable(ty) {
            return;
        }
        let json = format!("({value_expr} as unknown as JsonValue)");
        let d = serialisation::deserialise_expr(ty, &json, path);
        checks.push(format!(
            "  {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}"
        ));
    };
    // `Cell[T]` — validate the field value against `T`.
    for f in &effective_fields {
        push_value_check(
            &mut rehydrate_checks,
            &f.type_ref,
            &format!("s.{}", f.name.name),
            &f.name.name,
        );
    }
    // `Map[K, V]` — validate each entry value against `V`, and each key against
    // `K` when `K` is textual (the key persists as a `String(k)` Record key).
    for (name, v) in &store_map_fields {
        if is_codecable(v) {
            rehydrate_checks.push(format!(
                "  for (const __v of Object.values(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(v, "(__v as unknown as JsonValue)", &name.name),
            ));
        }
        if let Some(k) = store_field_key_type(a, &name.name)
            && type_base_is_string(k, &commons.types)
        {
            rehydrate_checks.push(format!(
                "  for (const __k of Object.keys(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(k, "(__k as unknown as JsonValue)", &name.name),
            ));
        }
    }
    // v0.105 (slice 3b-ii): a held `Map[K, Connection]` persists `K → connId`. The
    // connId is an opaque platform string (no value check); validate each textual
    // `K` key, as for a plain map.
    for (name, _) in &held_maps {
        if let Some(k) = store_field_key_type(a, &name.name)
            && type_base_is_string(k, &commons.types)
        {
            rehydrate_checks.push(format!(
                "  for (const __k of Object.keys(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(k, "(__k as unknown as JsonValue)", &name.name),
            ));
        }
    }
    // `Set[T]` — the elements are the (textual) Record keys; validate when `T`
    // is textual, else defer (structural string key).
    for (name, t) in &store_set_fields {
        if type_base_is_string(t, &commons.types) {
            rehydrate_checks.push(format!(
                "  for (const __k of Object.keys(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(t, "(__k as unknown as JsonValue)", &name.name),
            ));
        }
    }
    // `Cache[K, V]` — validate each entry's `.v` against `V`.
    for (name, v, _) in &store_cache_fields {
        if is_codecable(v) {
            rehydrate_checks.push(format!(
                "  for (const __e of Object.values(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(v, "(__e.v as unknown as JsonValue)", &name.name),
            ));
        }
    }
    // `Log[T]` — validate each entry's `.v` against `T`.
    for (name, t, _) in &store_log_fields {
        if is_codecable(t) {
            rehydrate_checks.push(format!(
                "  for (const __e of s.{n}) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(t, "(__e.v as unknown as JsonValue)", &name.name),
            ));
        }
    }
    let has_rehydrate = agent_needs_rehydrate(a, &commons.types);
    if has_rehydrate {
        writeln!(out, "function {rehydrate_fn}(s: {state_ty}): void {{").unwrap();
        for c in &rehydrate_checks {
            writeln!(out, "{c}").unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
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
        "    const stored = await this.state.storage.get<{state_ty}>(\"state\");"
    )
    .unwrap();
    // v0.96 (ADR 0124): a fresh key takes its zero (valid by construction). For a
    // stored record, merge zero-then-stored — D4: a `store` field added in a later
    // deploy and absent from the persisted record takes its default, rather than
    // reading `undefined` — then run the rehydration validation gate on the merged
    // state before any handler reads it (D1/D2).
    writeln!(out, "    if (stored === undefined) return {zero_fn}();").unwrap();
    writeln!(out, "    const __merged = {{ ...{zero_fn}(), ...stored }};").unwrap();
    if has_rehydrate {
        writeln!(out, "    {rehydrate_fn}(__merged);").unwrap();
    }
    writeln!(out, "    return __merged;").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "  private async commitState(s: {state_ty}): Promise<void> {{"
    )
    .unwrap();
    // v0.80 (§14): evaluate each invariant against the proposed state `s` before
    // the write. A violation throws `InvariantViolation` *before* `storage.put`,
    // so the offending commit never persists (non-persistence of the offending
    // commit — not whole-handler rollback). The refusal is logged with the agent
    // type and invariant name (never the key — see ADR 0107) so it is
    // distinguishable from a crash in the logs.
    if !a.invariants.is_empty() {
        let field_names: HashSet<String> = effective_fields
            .iter()
            .map(|f| f.name.name.clone())
            .collect();
        for inv in &a.invariants {
            let mut cx = LowerCtx::new(commons, &ctx.cross_context);
            cx.invariant_state = Some(("s".to_string(), field_names.clone()));
            let mut pre = Vec::new();
            let pred = lower_expr(&inv.predicate, &mut pre, &mut cx);
            for s in &pre {
                writeln!(out, "    {s}").unwrap();
            }
            writeln!(out, "    if (!({pred})) {{").unwrap();
            writeln!(
                out,
                "      console.error(\"InvariantViolation {agent}.{name}\", {{ agent: \"{agent}\", invariant: \"{name}\" }});",
                agent = a.name.name,
                name = inv.name.name
            )
            .unwrap();
            writeln!(
                out,
                "      throw invariantViolation(\"{agent}\", \"{name}\");",
                agent = a.name.name,
                name = inv.name.name
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "    await this.state.storage.put(\"state\", s);").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
    // 3) Handlers.
    let cell_names: HashSet<String> = effective_fields
        .iter()
        .map(|f| f.name.name.clone())
        .collect();
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
        // v0.70: per-statement maps for the spliced handler body (see emit_service).
        let body_smb = RefCell::new(SourceMapBuilder::new());
        let mut cx = LowerCtx::new(commons, &ctx.cross_context).with_source_map(Some(&body_smb));
        cx.in_agent_handler = true;
        // v0.81: a store-agent handler reads/writes cells over a mutable working
        // record `__state`; a state-record handler uses `currentState`/`self.state`.
        // A store handler that performs any `:=` wraps its body in a closure so an
        // implicit commit runs at handler end on every (success) return path.
        // v0.105 (slice 3b-ii): a held `Map[K, Connection]` is persisted (a connId
        // record), so writing one (`put`/`remove`) must trigger the commit flush —
        // include the held maps in the write-detection set. (They are *not* in
        // `agent_store_maps`: their entry ops use the connId-resolution lowering, not
        // the plain `Record<string, V>` ops.)
        let writes_map_names: HashSet<String> = map_names.union(&held_map_names).cloned().collect();
        let writes_state = is_store_agent
            && block_writes_state(
                &h.body,
                (
                    &writes_map_names,
                    &set_names,
                    &cache_names,
                    &log_names,
                    &cell_names,
                ),
            );
        if is_store_agent {
            cx.agent_store_state = Some(("__state".to_string(), cell_names.clone()));
            cx.agent_store_maps = map_names.clone();
            cx.agent_store_sets = set_names.clone();
            cx.agent_store_caches = cache_ttls.clone();
            cx.agent_store_logs = log_retains.clone();
            cx.agent_store_indexes = store_map_indexes.clone();
            cx.agent_held_maps = held_maps_ts.clone();
        } else {
            cx.agent_state_var = Some("currentState".to_string());
        }
        cx.agent_key_field = Some(a.key_name.name.clone());
        cx.capabilities = h
            .given
            .iter()
            .map(|c| c.key().to_string())
            .collect::<HashSet<_>>();
        cx.local_agents = ctx.local_agents.clone();
        let async_tail = is_effectful_return(&h.return_type);
        // A writing store handler's body sits one level deeper, inside the
        // implicit-commit closure.
        let body_indent = if writes_state {
            INDENT_STEP * 3
        } else {
            INDENT_STEP * 2
        };
        emit_block_as_function_body(&mut body_out, &h.body, &mut cx, body_indent, async_tail);
        let deps_ty =
            build_deps_object_ty_with_surface(&h.given, &cx, &ctx.cross_context, ctx.target);
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
            .unwrap_or_else(|| match &h.kind {
                HandlerKind::Call => "call".to_string(),
                // HTTP/cron/queue/open handlers are service-only (rejected in
                // agents by the parser); these arms are defensive and unreachable
                // here.
                HandlerKind::Http { .. }
                | HandlerKind::Cron { .. }
                | HandlerKind::Message
                | HandlerKind::Open
                | HandlerKind::Close => "call".to_string(),
            });
        writeln!(
            out,
            "  {async_kw}{method}({params}): {ret} {{",
            params = params.join(", "),
        )
        .unwrap();
        // Load state at entry. A state-record handler binds `currentState` and
        // commits explicitly via `commit`. A store handler binds a mutable
        // working record `__state`; reads/writes go through it, and (if it writes)
        // the body is wrapped so `commitState` runs once at handler end — the
        // implicit, atomic commit (ADR 0109). A fault before that flush persists
        // nothing; the invariant gate inside `commitState` runs before the write.
        let splice = |out: &mut String| {
            let base = out.len();
            out.push_str(&body_out);
            if let Some(module) = source_map {
                module
                    .borrow_mut()
                    .merge(&body_smb.borrow(), &body_out, out, base, 0);
            }
        };
        if is_store_agent {
            if writes_state {
                writeln!(
                    out,
                    "    const __state = {{ ...(await this.loadState()) }};"
                )
                .unwrap();
                writeln!(out, "    const __result = await (async () => {{").unwrap();
                splice(out);
                writeln!(out, "    }})();").unwrap();
                writeln!(out, "    await this.commitState(__state);").unwrap();
                writeln!(out, "    return __result;").unwrap();
            } else {
                writeln!(out, "    const __state = await this.loadState();").unwrap();
                splice(out);
            }
        } else {
            writeln!(out, "    const currentState = await this.loadState();").unwrap();
            splice(out);
        }
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
    }
    // v0.104 (real-time track slice 3b): the `from WebSocket` `on open` handlers
    // whose connection transfers to *this* agent are hosted in this Durable Object
    // (DECISION A) — the upgrade is authenticated at the edge then forwarded here,
    // where the socket is accepted and the body runs as a `this`-self-call.
    let ws_open_hosts: Vec<WsOpenHost<'_>> = if is_workers {
        ws_open_hosts_for(&a.name.name, commons, &ctx.local_agents, &ctx.actors)
    } else {
        Vec::new()
    };
    for host in &ws_open_hosts {
        emit_ws_do_method(
            out,
            a,
            host,
            host.handler,
            &ws_open_do_method_name(host.service),
            commons,
            ctx,
            source_map,
        );
        if let Some(m) = host.message {
            emit_ws_do_method(
                out,
                a,
                host,
                m,
                &ws_message_do_method_name(host.service),
                commons,
                ctx,
                source_map,
            );
        }
        if let Some(c) = host.close {
            emit_ws_do_method(
                out,
                a,
                host,
                c,
                &ws_close_do_method_name(host.service),
                commons,
                ctx,
                source_map,
            );
        }
    }
    // v0.9.2: workers-mode DO dispatch. Method calls arrive as `fetch` requests
    // under `/_bynk/agent/<method>`; decode `{ args, deps }`, invoke the
    // handler with deps as the trailing argument, and serialise the result.
    if matches!(ctx.target, BuildTarget::Workers) {
        writeln!(out, "  async fetch(request: Request): Promise<Response> {{").unwrap();
        writeln!(out, "    const url = new URL(request.url);").unwrap();
        // v0.104 (slice 3b): a forwarded WebSocket upgrade. The edge has already
        // authenticated the actor (the body never runs unverified); accept the
        // socket here, run the on-open body, and return the `101` carrying the
        // client end. The verified identity and route arguments ride in a trusted
        // internal header (the DO is only reachable through the Worker).
        for host in &ws_open_hosts {
            emit_ws_open_fetch_branch(out, host);
        }
        writeln!(
            out,
            "    if (url.pathname.startsWith(\"/_bynk/agent/\")) {{"
        )
        .unwrap();
        writeln!(
            out,
            "      const methodName = url.pathname.slice(\"/_bynk/agent/\".length);"
        )
        .unwrap();
        writeln!(
            out,
            "      const {{ args, deps }} = (await request.json()) as {{ args: unknown[]; deps: unknown }};"
        )
        .unwrap();
        writeln!(
            out,
            "      const result = await (this as any)[methodName](...args, deps);"
        )
        .unwrap();
        writeln!(
            out,
            "      return new Response(JSON.stringify(result), {{ headers: {{ \"content-type\": \"application/json\" }} }});"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(
            out,
            "    return new Response(\"Not Found\", {{ status: 404 }});"
        )
        .unwrap();
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
        // v0.106 (slice 3b-iii): the inbound/close dispatch — Cloudflare calls
        // `webSocketMessage`/`webSocketClose` on a hibernatable socket. Decode the
        // frame against `in:` (reject-and-close on failure), recover the sender
        // identity + route args from the socket attachment, and run the body.
        for host in &ws_open_hosts {
            emit_ws_dispatch_handlers(out, host);
        }
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    // v0.9.2: agent-construction factory. Lowering of `AgentName(key)` calls
    // this. A present DO binding (workers) routes through `makeWorkersAgent`;
    // otherwise the bundle registry path is taken. The single `makeAgent`
    // helper keeps the call site target-agnostic.
    let key_ts = ts_type_ref(&a.key_type);
    let bind = crate::emitter::wrangler::agent_binding_name(&a.name.name);
    writeln!(
        out,
        "export function {factory}(key: {key_ts}, env?: {{ {bind}?: DurableObjectNamespace }}): {agent} {{",
        factory = agent_factory_name(&a.name.name),
        agent = a.name.name,
    )
    .unwrap();
    writeln!(
        out,
        "  return makeAgent({registry}, env?.{bind}, key, (state) => new {agent}(state));",
        agent = a.name.name,
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// v0.104 (real-time track slice 3b): a `from WebSocket` `on open` handler hosted
/// in a Durable Object (DECISION A) — the service it belongs to, the handler, the
/// protocol's `out` frame type (the `Connection`'s parameter), and the Bearer
/// seam (the edge authenticates with it; the DO method threads the verified
/// identity through `deps`).
struct WsOpenHost<'a> {
    service: &'a str,
    handler: &'a Handler,
    out_type: &'a TypeRef,
    // v0.106 (slice 3b-iii): the service's inbound `in` frame type and its optional
    // `on message`/`on close` handlers — run in this same DO (`webSocketMessage`/
    // `webSocketClose`), with identity + route args recovered from the socket
    // attachment the `on open` accept wrote.
    in_type: &'a TypeRef,
    message: Option<&'a Handler>,
    close: Option<&'a Handler>,
    seam: Option<bynk_check::actors::BearerSeam>,
}

impl WsOpenHost<'_> {
    /// True if the service has an inbound/close handler — then the `on open` accept
    /// writes the identity + route args into the socket attachment so a waking
    /// `webSocketMessage`/`webSocketClose` can recover them.
    fn has_inbound(&self) -> bool {
        self.message.is_some() || self.close.is_some()
    }
}

/// The DO method name a hosted `on open` lowers to (`__wsOpen_<Service>`). The
/// edge forwards the upgrade to `/_bynk/ws/open/<Service>`, which dispatches here.
fn ws_open_do_method_name(service: &str) -> String {
    format!("__wsOpen_{service}")
}

/// The DO method names a hosted `on message` / `on close` lower to.
fn ws_message_do_method_name(service: &str) -> String {
    format!("__wsMessage_{service}")
}
fn ws_close_do_method_name(service: &str) -> String {
    format!("__wsClose_{service}")
}

/// Collect the `on open` handlers in `commons` whose connection transfers to the
/// agent named `agent` — its statically-routable single-transfer target (D2). The
/// shape constraint (`bynk.ws.open_transfer_shape`) guarantees at most one such
/// transfer per handler, so the host DO is unambiguous.
fn ws_open_hosts_for<'a>(
    agent: &str,
    commons: &'a TypedCommons,
    local_agents: &HashSet<String>,
    actors: &HashMap<String, ActorDecl>,
) -> Vec<WsOpenHost<'a>> {
    let mut hosts = Vec::new();
    for item in &commons.commons.items {
        let CommonsItem::Service(s) = item else {
            continue;
        };
        let ServiceProtocol::WebSocket { out_type, in_type } = &s.protocol else {
            continue;
        };
        for h in &s.handlers {
            if !matches!(h.kind, HandlerKind::Open) {
                continue;
            }
            if let crate::emitter::websocket::WsOpenShape::One(t) =
                crate::emitter::websocket::analyse_open_shape(&h.body, local_agents)
                && t.agent == agent
            {
                hosts.push(WsOpenHost {
                    service: &s.name.name,
                    handler: h,
                    out_type,
                    in_type,
                    message: s
                        .handlers
                        .iter()
                        .find(|h| matches!(h.kind, HandlerKind::Message)),
                    close: s
                        .handlers
                        .iter()
                        .find(|h| matches!(h.kind, HandlerKind::Close)),
                    seam: bynk_check::actors::bearer_seam_for(h, actors),
                });
            }
        }
    }
    hosts
}

/// Emit the DO method that runs a hosted `from WebSocket` lifecycle body (`on
/// open`/`on message`/`on close`). The synthetic `connection` arrives as the first
/// parameter (the fresh socket for `on open`, the firing socket for `on message`/
/// `on close`), the handler's own parameters follow (for `on message`, the decoded
/// frame and any route values), and the verified identity rides in `deps`. The
/// body lowers with `ws_self_agent` set, so an agent transfer becomes a `this`
/// self-call rather than a cross-instance RPC.
#[allow(clippy::too_many_arguments)]
fn emit_ws_do_method(
    out: &mut String,
    agent: &AgentDecl,
    host: &WsOpenHost<'_>,
    h: &Handler,
    method: &str,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    let mut params = vec![format!(
        "connection: Connection<{}>",
        ts_type_ref(host.out_type)
    )];
    for p in &h.params {
        params.push(format!("{}: {}", p.name.name, ts_type_ref(&p.type_ref)));
    }
    let body_smb = RefCell::new(SourceMapBuilder::new());
    let mut cx = LowerCtx::new(commons, &ctx.cross_context).with_source_map(Some(&body_smb));
    cx.capabilities = h
        .given
        .iter()
        .map(|c| c.key().to_string())
        .collect::<HashSet<_>>();
    cx.local_agents = ctx.local_agents.clone();
    cx.target = ctx.target;
    cx.ws_self_agent = Some(agent.name.name.clone());
    cx.deps_identity_binder = host.seam.as_ref().and_then(|s| s.binder.clone());
    let async_tail = is_effectful_return(&h.return_type);
    let mut body_out = String::new();
    emit_block_as_function_body(&mut body_out, &h.body, &mut cx, INDENT_STEP * 2, async_tail);
    let mut deps_ty =
        build_deps_object_ty_with_surface(&h.given, &cx, &ctx.cross_context, ctx.target);
    if let Some(seam) = host.seam.as_ref().filter(|s| s.binder.is_some()) {
        let field = format!("identity: {}", seam.identity_type);
        deps_ty = if deps_ty == "{}" {
            format!("{{ {field} }}")
        } else {
            format!(
                "{}; {field} }}",
                deps_ty.trim_end().trim_end_matches('}').trim_end()
            )
        };
    }
    params.push(format!("deps: {deps_ty}"));
    let ret = ts_type_ref(&h.return_type);
    let async_kw = if async_tail { "async " } else { "" };
    writeln!(out, "  {async_kw}{method}({}): {ret} {{", params.join(", ")).unwrap();
    let base = out.len();
    out.push_str(&body_out);
    if let Some(module) = source_map {
        module
            .borrow_mut()
            .merge(&body_smb.borrow(), &body_out, out, base, 0);
    }
    writeln!(out, "  }}").unwrap();
    writeln!(out).unwrap();
}

/// Emit the `fetch` branch that completes a forwarded WebSocket upgrade for a
/// hosted `on open`. The edge has already verified the actor, so the body runs
/// authenticated; this accepts the socket, reconstructs the route arguments and
/// identity from the trusted internal header, runs the on-open body, and returns
/// the `101` handing the client end back.
fn emit_ws_open_fetch_branch(out: &mut String, host: &WsOpenHost<'_>) {
    let h = host.handler;
    let path = format!("/_bynk/ws/open/{}", host.service);
    let method = ws_open_do_method_name(host.service);
    writeln!(out, "    if (url.pathname === \"{path}\") {{").unwrap();
    writeln!(out, "      const __pair = newWebSocketPair();").unwrap();
    // The trusted internal header carries the route args, and the verified
    // identity only when the actor binds one (a binder-less `by` forwards none).
    let binds_identity = host.seam.as_ref().is_some_and(|s| s.binder.is_some());
    let payload_ty = if binds_identity {
        "{ args: unknown[]; identity: string }"
    } else {
        "{ args: unknown[] }"
    };
    writeln!(
        out,
        "      const __payload = JSON.parse(request.headers.get(\"X-Bynk-Ws-Open\") ?? \"{{}}\") as {payload_ty};"
    )
    .unwrap();
    // v0.105 (slice 3b-ii): accept the server socket into the DO *hibernatably*
    // (tagged with a fresh connId, attached for wake-time recovery) so a stored
    // connection survives eviction — not `server.accept()`, which is in-memory only.
    // v0.106 (slice 3b-iii): when the service has an inbound/close handler, also
    // attach the sender identity + route args so a waking `webSocketMessage`/
    // `webSocketClose` recovers them without re-authenticating.
    let meta_arg = if host.has_inbound() {
        let identity = if binds_identity {
            "__payload.identity"
        } else {
            "\"\""
        };
        format!(", {{ identity: {identity}, args: __payload.args }}")
    } else {
        String::new()
    };
    writeln!(
        out,
        "      const connection = acceptHibernatableConnection<{}>(this.state, __pair.server{meta_arg});",
        ts_type_ref(host.out_type)
    )
    .unwrap();
    let mut call_args = vec!["connection".to_string()];
    for (i, p) in h.params.iter().enumerate() {
        call_args.push(format!(
            "__payload.args[{i}] as {}",
            ts_type_ref(&p.type_ref)
        ));
    }
    let deps_arg = match host.seam.as_ref().filter(|s| s.binder.is_some()) {
        Some(seam) => format!(
            "{{ identity: __payload.identity as {} }}",
            seam.identity_type
        ),
        None => "{}".to_string(),
    };
    call_args.push(deps_arg);
    let await_kw = if is_effectful_return(&h.return_type) {
        "await "
    } else {
        ""
    };
    writeln!(
        out,
        "      {await_kw}this.{method}({});",
        call_args.join(", ")
    )
    .unwrap();
    writeln!(out, "      return webSocketUpgradeResponse(__pair.client);").unwrap();
    writeln!(out, "    }}").unwrap();
}

/// v0.106 (slice 3b-iii): the deps argument a hosted `on message`/`on close` body
/// receives — the verified identity recovered from the socket attachment, when the
/// actor binds one (else `{}`).
fn ws_attachment_deps_arg(seam: &Option<bynk_check::actors::BearerSeam>) -> String {
    match seam.as_ref().filter(|s| s.binder.is_some()) {
        Some(seam) => format!("{{ identity: __att.identity as {} }}", seam.identity_type),
        None => "{}".to_string(),
    }
}

/// v0.106 (slice 3b-iii): emit the hibernatable-WebSocket dispatch handlers
/// Cloudflare invokes on an accepted socket — `webSocketMessage` (an inbound frame)
/// and `webSocketClose`. Each recovers `{ connId, identity, args }` from the socket
/// attachment the `on open` accept wrote, re-wraps the firing socket as the
/// `connection`, and runs the corresponding DO-hosted body. `webSocketMessage`
/// decodes the raw frame against the service's `in:` type first — a malformed frame
/// closes the socket (`1003`/`1008`) and is never dispatched (the client-bytes trust
/// boundary).
fn emit_ws_dispatch_handlers(out: &mut String, host: &WsOpenHost<'_>) {
    if !host.has_inbound() {
        return;
    }
    let out_ts = ts_type_ref(host.out_type);
    let att_ty = "{ connId: string; identity: string; args: unknown[] }";
    // The firing socket's minimal structural surface (attachment + send/close), so
    // emitted code stays free of `@cloudflare/workers-types`.
    let ws_ty = "{ deserializeAttachment(): unknown; send(data: string): void; close(code?: number, reason?: string): void }";
    let deps_arg = ws_attachment_deps_arg(&host.seam);

    if let Some(m) = host.message {
        let method = ws_message_do_method_name(host.service);
        let decode = serialisation::deserialise_expr(host.in_type, "__json", "frame");
        writeln!(
            out,
            "  async webSocketMessage(ws: {ws_ty}, message: string | ArrayBuffer): Promise<void> {{"
        )
        .unwrap();
        writeln!(
            out,
            "    const __att = ws.deserializeAttachment() as {att_ty} | null;\n    if (__att === null) {{ ws.close(1011, \"no session\"); return; }}"
        )
        .unwrap();
        writeln!(
            out,
            "    const connection = new WorkersConnection<{out_ts}>(ws, __att.connId);"
        )
        .unwrap();
        writeln!(out, "    let __raw: string;").unwrap();
        writeln!(
            out,
            "    try {{ __raw = typeof message === \"string\" ? message : new TextDecoder().decode(message); }} catch {{ ws.close(1003, \"unreadable frame\"); return; }}"
        )
        .unwrap();
        writeln!(out, "    let __json: JsonValue;").unwrap();
        writeln!(
            out,
            "    try {{ __json = JSON.parse(__raw) as JsonValue; }} catch {{ ws.close(1003, \"malformed frame\"); return; }}"
        )
        .unwrap();
        writeln!(out, "    const __dec = {decode};").unwrap();
        writeln!(
            out,
            "    if (__dec.tag === \"Err\") {{ ws.close(1008, \"invalid frame\"); return; }}"
        )
        .unwrap();
        // The decoded frame fills the param typed as the service `in`; the rest are
        // route values recovered (positionally) from the attachment args.
        let mut call_args = vec!["connection".to_string()];
        let mut route_idx = 0usize;
        for p in &m.params {
            if crate::project::type_refs_match(&p.type_ref, host.in_type) {
                call_args.push("__dec.value".to_string());
            } else {
                call_args.push(format!(
                    "__att.args[{route_idx}] as {}",
                    ts_type_ref(&p.type_ref)
                ));
                route_idx += 1;
            }
        }
        call_args.push(deps_arg.clone());
        writeln!(out, "    await this.{method}({});", call_args.join(", ")).unwrap();
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
    }

    if let Some(c) = host.close {
        let method = ws_close_do_method_name(host.service);
        writeln!(
            out,
            "  async webSocketClose(ws: {ws_ty}, code: number, reason: string, wasClean: boolean): Promise<void> {{"
        )
        .unwrap();
        writeln!(out, "    void code; void reason; void wasClean;").unwrap();
        writeln!(
            out,
            "    const __att = ws.deserializeAttachment() as {att_ty} | null;\n    if (__att === null) {{ ws.close(1011, \"no session\"); return; }}"
        )
        .unwrap();
        writeln!(
            out,
            "    const connection = new WorkersConnection<{out_ts}>(ws, __att.connId);"
        )
        .unwrap();
        let mut call_args = vec!["connection".to_string()];
        for (i, p) in c.params.iter().enumerate() {
            call_args.push(format!("__att.args[{i}] as {}", ts_type_ref(&p.type_ref)));
        }
        call_args.push(deps_arg);
        writeln!(out, "    await this.{method}({});", call_args.join(", ")).unwrap();
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
    }
}
