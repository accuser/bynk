//! Per-declaration emission — the functions `emit_project` drives to render
//! each top-level Karn declaration into TypeScript: type/refined/record/sum
//! declarations and their checks, attached methods and free functions,
//! capabilities, providers, services, contexts, and agents (plus the
//! worker-dispatch lowering helpers those emitters use). Split out of
//! `emitter.rs` (ADR 0060); the codec/reference/import/header helpers and the
//! `ts_*`/`LowerCtx` core stay in the parent and are reached via `use super::*`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::checker::TypedCommons;
use crate::project::EmitProjectCtx;

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
    let empty = crate::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty);
    // Methods are emitted as plain (non-async) members on an object literal;
    // any `Effect.pure(...)` in tail position must still wrap as
    // `Promise.resolve(...)` because there's no surrounding `async` to absorb
    // it. (Methods aren't expected to return `Effect[T]` in v0–v0.7.1.)
    emit_block_as_function_body(out, &f.body, &mut cx, INDENT_STEP * 2, false);
    writeln!(out, "  }},").unwrap();
}

pub(crate) fn emit_free_fn(out: &mut String, f: &FnDecl, commons: &TypedCommons) {
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
    let empty = crate::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(commons, &empty);
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
        let mut cx = LowerCtx::new(commons, &ctx.cross_context);
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
) {
    emit_doc_block(out, s.documentation.as_deref(), 0);
    writeln!(out, "export const {name} = {{", name = s.name.name).unwrap();
    let mut cron_idx = 0usize;
    let mut queue_idx = 0usize;
    for handler in &s.handlers {
        emit_doc_block(out, handler.documentation.as_deref(), INDENT_STEP);
        let kind_name = match &handler.kind {
            HandlerKind::Call => "call".to_string(),
            HandlerKind::Http { method, path } => http_handler_method_name(*method, path),
            HandlerKind::Cron { .. } => {
                let name = cron_handler_method_name(&s.name.name, cron_idx);
                cron_idx += 1;
                name
            }
            HandlerKind::Queue { .. } => {
                let name = queue_handler_method_name(&s.name.name, queue_idx);
                queue_idx += 1;
                name
            }
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
            .map(|c| c.key().to_string())
            .collect::<HashSet<_>>();
        cx.local_agents = ctx.local_agents.clone();
        cx.target = ctx.target;
        let async_tail = is_effectful_return(&handler.return_type);
        emit_block_as_function_body(
            &mut body_out,
            &handler.body,
            &mut cx,
            INDENT_STEP * 2,
            async_tail,
        );
        // Append the deps parameter (may include surface field if the body
        // made cross-context calls).
        let deps_ty =
            build_deps_object_ty_with_surface(&handler.given, &cx, &ctx.cross_context, ctx.target);
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

/// v0.15: the TypeScript deps-field type for a `given` capability reference.
/// A local capability uses its bare interface name; a cross-context one is
/// qualified with the providing context's import namespace
/// (`platform_time.Clock`).
fn cap_ref_ty(c: &CapRef, info: &crate::resolver::CrossContextInfo) -> String {
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
    info: &crate::resolver::CrossContextInfo,
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
    info: &crate::resolver::CrossContextInfo,
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
    cross_context: &crate::resolver::CrossContextInfo,
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
fn workers_env_ty(cross_context: &crate::resolver::CrossContextInfo, agents: &[String]) -> String {
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
fn has_consumed_service(cross_context: &crate::resolver::CrossContextInfo) -> bool {
    cross_context
        .consumed_services
        .values()
        .any(|svcs| !svcs.is_empty())
}

/// Build the TS type for the `surface` field in deps, naming each consumed
/// context by its surface key plus the consumed context's makeSurface type.
/// Only service-bearing consumed contexts contribute (a capability-only
/// consumed context has no `makeSurface`).
fn surface_ty(cross_context: &crate::resolver::CrossContextInfo) -> String {
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

    format!(
        "callService(deps.env.{binding}, \"{}\", {args_json}, {deser_ref})",
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
        // (`karn.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..) => unreachable!("function types are rejected at boundaries"),
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
/// karn layer guarantees the cast is sound.
pub(crate) fn param_cast(
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
        // v0.9.1: when both contexts brand the same commons type (e.g., both
        // see `Money` with their own `__ctxBrand`), a direct
        // `as <ns>.<Type>` cast is rejected by `tsc --strict` because the
        // brand discriminants are incompatible. Karn guarantees the value's
        // base type matches at the boundary, so route through `unknown` to
        // tell TypeScript to trust the structural Karn-side check.
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

pub(crate) fn emit_agent(
    out: &mut String,
    a: &AgentDecl,
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
) {
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
    // v0.9.2: per-agent state registry (bundle mode + `karnc test`) and the
    // zero-value factory used to initialise a fresh key's state.
    let registry = agent_registry_name(&a.name.name);
    let zero_fn = format!("__zeroOf{}State", a.name.name);
    writeln!(out, "const {registry} = new StateRegistry();").unwrap();
    // v0.11: build the fresh-state record. A field with an explicit initialiser
    // lowers its (static) expression; a field without one uses the v0.9.2
    // implicit zero.
    let zero_record = {
        let mut parts: Vec<String> = Vec::new();
        for f in &a.state_fields {
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
                crate::checker::zero_value_ts(&f.type_ref, f.refinement.as_ref(), &commons.types)
                    .unwrap_or_else(|| "undefined as never".to_string())
            };
            parts.push(format!("{}: {val}", f.name.name));
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
    writeln!(out, "    return stored ?? {zero_fn}();").unwrap();
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
            .map(|c| c.key().to_string())
            .collect::<HashSet<_>>();
        cx.local_agents = ctx.local_agents.clone();
        let async_tail = is_effectful_return(&h.return_type);
        emit_block_as_function_body(&mut body_out, &h.body, &mut cx, INDENT_STEP * 2, async_tail);
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
                // HTTP/cron/queue handlers are service-only (rejected in agents
                // by the parser); these arms are defensive and unreachable here.
                HandlerKind::Http { .. } | HandlerKind::Cron { .. } | HandlerKind::Queue { .. } => {
                    "call".to_string()
                }
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
    // v0.9.2: workers-mode DO dispatch. Method calls arrive as `fetch` requests
    // under `/_karn/agent/<method>`; decode `{ args, deps }`, invoke the
    // handler with deps as the trailing argument, and serialise the result.
    if matches!(ctx.target, BuildTarget::Workers) {
        writeln!(out, "  async fetch(request: Request): Promise<Response> {{").unwrap();
        writeln!(out, "    const url = new URL(request.url);").unwrap();
        writeln!(
            out,
            "    if (url.pathname.startsWith(\"/_karn/agent/\")) {{"
        )
        .unwrap();
        writeln!(
            out,
            "      const methodName = url.pathname.slice(\"/_karn/agent/\".length);"
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
