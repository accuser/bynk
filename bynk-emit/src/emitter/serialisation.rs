//! Per-type serialise / deserialise helper generation for workers mode
//! (v0.8 §3.4 / §5.2).
//!
//! Every Bynk type that crosses a context boundary needs:
//!   - `serialise_<Type>(value): JsonValue` — structural lowering.
//!   - `deserialise_<Type>(json): Result<<Type>, BoundaryError>` —
//!     structural validation + refinement re-validation, then a nominal
//!     cast back to the receiving context's view.
//!
//! Helpers live in the *owning* module — commons modules emit helpers for
//! commons types, context modules emit helpers for the types they declare.

use std::fmt::Write as _;

use bynk_syntax::ast::*;

/// Compute the set of type names (transitively reachable) that need
/// serialise/deserialise helpers for this context: any type used in the
/// argument or return position of a service handler exposed by this
/// context, walked through record fields, sum payloads, and the generic
/// type parameters of Result/Option/Effect.
pub fn collect_boundary_types(
    types: &std::collections::HashMap<String, TypeDecl>,
    services: &std::collections::HashMap<String, ServiceDecl>,
    // v0.96 (ADR 0124): rehydration is a trust boundary — an agent's persisted
    // `store`-field types are validated on load, so they need their deserialisers
    // emitted. Register every store field's kind-argument types (the element /
    // key / value types of `Cell`/`Map`/`Set`/`Cache`/`Log`).
    agents: &std::collections::HashMap<String, AgentDecl>,
) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let recursive_set = recursive_generic_names(types);
    let recursive = &recursive_set;

    let mut svc_names: Vec<&String> = services.keys().collect();
    svc_names.sort();
    for name in svc_names {
        let service = &services[name];
        for h in &service.handlers {
            for p in &h.params {
                collect_type_names(&p.type_ref, &mut stack, types, recursive);
            }
            collect_type_names(&h.return_type, &mut stack, types, recursive);
        }
    }

    let mut agent_names: Vec<&String> = agents.keys().collect();
    agent_names.sort();
    for name in agent_names {
        for f in &agents[name].store_fields {
            for arg in &f.kind.args {
                collect_type_names(arg, &mut stack, types, recursive);
            }
        }
    }

    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(name.clone());
        let Some(decl) = types.get(&name) else {
            continue;
        };
        match &decl.body {
            TypeBody::Record(r) => {
                for f in &r.fields {
                    collect_type_names(&f.type_ref, &mut stack, types, recursive);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        collect_type_names(&p.type_ref, &mut stack, types, recursive);
                    }
                }
            }
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
        }
    }

    out.sort();
    out
}

/// v0.174 (#592): the set of generic-record names that are *recursive* — they
/// transitively contain themselves, so they have no finite monomorphised codec
/// (rejected at the boundary by the checker before emit). Precomputed once per
/// collector so the per-`App` guard in the codec walks is an O(1) membership test
/// rather than a fresh graph reachability walk at every occurrence.
fn recursive_generic_names(
    types: &std::collections::HashMap<String, TypeDecl>,
) -> std::collections::HashSet<String> {
    types
        .iter()
        .filter(|(_, d)| !d.type_params.is_empty())
        .map(|(n, _)| n.clone())
        .filter(|n| generic_record_is_recursive(n, types))
        .collect()
}

fn collect_type_names(
    t: &TypeRef,
    stack: &mut Vec<String>,
    types: &std::collections::HashMap<String, TypeDecl>,
    recursive: &std::collections::HashSet<String>,
) {
    match t {
        TypeRef::Named(id) => stack.push(id.name.clone()),
        // Query/Stream/Connection types carry no boundary-collectable user
        // types (non-boundary).
        TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => {}
        // v0.174 (#592): a generic-record instantiation is boundary-serialisable
        // through its monomorphised codec (`serialise_Paginated_User`). The
        // *named* helpers that codec calls come from its concrete field types —
        // the type arguments (`User`) and any non-parameter named field types
        // (`Envelope[T] = { meta: Metadata, … }`) — so walk the substituted
        // fields. A *recursive* generic record has no finite codec set and is
        // rejected at the boundary before emit; the guard here is defence in
        // depth so this walk can never fail to terminate.
        TypeRef::App { name, args, .. } => {
            if recursive.contains(&name.name) {
                return;
            }
            // #593: a generic-sum instantiation's codec (`serialise_ApiResult_User`)
            // likewise calls the named helpers of its *substituted variant
            // payloads* (`serialise_User` for `Loaded(value: T)` at `T = User`),
            // so walk those the same way records walk their fields.
            if let Some(fields) = record_inst_fields(&name.name, args, types) {
                for (_, ft) in &fields {
                    collect_type_names(ft, stack, types, recursive);
                }
            } else if let Some(variants) = sum_inst_variants(&name.name, args, types) {
                for (_, payload) in &variants {
                    for (_, ft) in payload {
                        collect_type_names(ft, stack, types, recursive);
                    }
                }
            }
        }
        // v0.20a: function types carry no user-named types to collect and are
        // rejected at boundaries anyway.
        TypeRef::Fn(..) => {}
        TypeRef::Result(a, b, _) => {
            collect_type_names(a, stack, types, recursive);
            collect_type_names(b, stack, types, recursive);
        }
        TypeRef::Option(a, _) => collect_type_names(a, stack, types, recursive),
        TypeRef::Effect(a, _) => collect_type_names(a, stack, types, recursive),
        TypeRef::HttpResult(a, _) => collect_type_names(a, stack, types, recursive),
        // v0.20b: collections serialise element-/entry-wise; their inner
        // named types need helpers.
        TypeRef::List(a, _) => collect_type_names(a, stack, types, recursive),
        TypeRef::Map(k, v, _) => {
            collect_type_names(k, stack, types, recursive);
            collect_type_names(v, stack, types, recursive);
        }
        TypeRef::Base(_, _)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
    }
}

/// v0.174 (#592): substitute a generic record's declared field type — replacing
/// each type-parameter name with the concrete argument type-ref — so a
/// per-instantiation codec sees fully concrete field types.
/// `Paginated[User]`'s `items: List[T]` becomes `items: List[User]`.
fn subst_type_ref(t: &TypeRef, subst: &std::collections::HashMap<String, TypeRef>) -> TypeRef {
    match t {
        TypeRef::Named(id) => match subst.get(&id.name) {
            Some(replacement) => replacement.clone(),
            None => t.clone(),
        },
        TypeRef::App { name, args, span } => TypeRef::App {
            name: name.clone(),
            args: args.iter().map(|a| subst_type_ref(a, subst)).collect(),
            span: *span,
        },
        TypeRef::Result(a, b, s) => TypeRef::Result(
            Box::new(subst_type_ref(a, subst)),
            Box::new(subst_type_ref(b, subst)),
            *s,
        ),
        TypeRef::Option(a, s) => TypeRef::Option(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::Effect(a, s) => TypeRef::Effect(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::HttpResult(a, s) => TypeRef::HttpResult(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::List(a, s) => TypeRef::List(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::Map(k, v, s) => TypeRef::Map(
            Box::new(subst_type_ref(k, subst)),
            Box::new(subst_type_ref(v, subst)),
            *s,
        ),
        TypeRef::Query(a, s) => TypeRef::Query(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::Stream(a, s) => TypeRef::Stream(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::Connection(a, s) => TypeRef::Connection(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::History(a, s) => TypeRef::History(Box::new(subst_type_ref(a, subst)), *s),
        TypeRef::Fn(ps, r, s) => TypeRef::Fn(
            ps.iter().map(|p| subst_type_ref(p, subst)).collect(),
            Box::new(subst_type_ref(r, subst)),
            *s,
        ),
        TypeRef::Base(..)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => t.clone(),
    }
}

/// v0.174 (#592): the concrete `(field-name, field-type)` list for a generic
/// record instantiation `Name[args…]` — the declared fields with every type
/// parameter substituted by the matching argument. Returns `None` if `name` is
/// not a declared generic record or the arity does not match (both guaranteed
/// impossible by the checker, so this is purely defensive).
fn record_inst_fields(
    name: &str,
    args: &[TypeRef],
    types: &std::collections::HashMap<String, TypeDecl>,
) -> Option<Vec<(String, TypeRef)>> {
    let decl = types.get(name)?;
    let TypeBody::Record(r) = &decl.body else {
        return None;
    };
    if decl.type_params.len() != args.len() {
        return None;
    }
    let subst: std::collections::HashMap<String, TypeRef> = decl
        .type_params
        .iter()
        .map(|p| p.name.name.clone())
        .zip(args.iter().cloned())
        .collect();
    Some(
        r.fields
            .iter()
            .map(|f| (f.name.name.clone(), subst_type_ref(&f.type_ref, &subst)))
            .collect(),
    )
}

/// #593: the concrete `(variant-name, [(field-name, field-type)])` list for a
/// generic sum instantiation `Name[args…]` — the declared variants with every
/// type parameter substituted by the matching argument. The sum analogue of
/// [`record_inst_fields`]; `None` (defensively) if `name` is not a declared
/// generic sum or the arity does not match.
#[allow(clippy::type_complexity)]
fn sum_inst_variants(
    name: &str,
    args: &[TypeRef],
    types: &std::collections::HashMap<String, TypeDecl>,
) -> Option<Vec<(String, Vec<(String, TypeRef)>)>> {
    let decl = types.get(name)?;
    let TypeBody::Sum(s) = &decl.body else {
        return None;
    };
    if decl.type_params.len() != args.len() {
        return None;
    }
    let subst: std::collections::HashMap<String, TypeRef> = decl
        .type_params
        .iter()
        .map(|p| p.name.name.clone())
        .zip(args.iter().cloned())
        .collect();
    Some(
        s.variants
            .iter()
            .map(|v| {
                (
                    v.name.name.clone(),
                    v.payload
                        .iter()
                        .map(|f| (f.name.name.clone(), subst_type_ref(&f.type_ref, &subst)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// v0.174 (#592): the monomorphised codec suffix for a generic-record
/// instantiation — `Paginated[User]` → `Paginated_User`,
/// `Pair[User, String]` → `Pair_User_String`. #593: shared with generic sums.
fn app_ts_name(name: &str, args: &[TypeRef]) -> String {
    let mut s = name.to_string();
    for a in args {
        s.push('_');
        s.push_str(&inner_ts_name(a));
    }
    s
}

/// Emit `serialise_<T>` and `deserialise_<T>` for every named type the
/// owner declares that crosses a boundary. `owner_qualified` is the
/// qualified name used as the brand path so that refinement-violation
/// messages identify the origin context.
pub fn emit_helpers_for_owner(
    out: &mut String,
    type_names: &[String],
    types: &std::collections::HashMap<String, TypeDecl>,
    _owner_qualified: &str,
) {
    // Only emit helpers for *named* types declared by this owner. Skip
    // unknown names — they belong to another module or to the runtime's
    // generic helpers (Result / Option).
    let mut emitted_any = false;
    for name in type_names {
        let Some(decl) = types.get(name) else {
            continue;
        };
        // v0.174 (#592): a generic record has no single `serialise_<Name>` —
        // each boundary instantiation gets its own monomorphised codec
        // (`serialise_Paginated_User`) via `emit_generic_helpers`. Never emit a
        // bare, un-parameterised helper for the declaration itself.
        if !decl.type_params.is_empty() {
            continue;
        }
        emitted_any = true;
        emit_one(out, name, decl);
    }
    if emitted_any {
        writeln!(out).unwrap();
    }
}

fn emit_one(out: &mut String, name: &str, decl: &TypeDecl) {
    match &decl.body {
        TypeBody::Refined { base, .. } => emit_refined(out, name, *base, decl),
        TypeBody::Opaque { base, .. } => emit_refined(out, name, *base, decl),
        TypeBody::Record(r) => emit_record(out, name, r),
        TypeBody::Sum(s) => emit_sum(out, name, s),
    }
}

fn ts_base_for_serialisation(b: BaseType) -> &'static str {
    match b {
        BaseType::Int => "number",
        BaseType::String => "string",
        BaseType::Bool => "boolean",
        BaseType::Float => "number",
        BaseType::Duration | BaseType::Instant => "number",
        // v0.110 (ADR 0142 D5): a `Bytes` wires as a base64 JSON string.
        BaseType::Bytes => "string",
    }
}

/// v0.110 (ADR 0142 D5): the codec for a named opaque/refined type over
/// `Bytes` (`type Digest = Bytes`). Unlike the `number`-erased base types, a
/// `Bytes` does not round-trip as itself — it is base64-encoded on serialise
/// and decoded (rejecting a non-string or invalid-base64 wire value) on
/// deserialise. There are no `Bytes` refinement predicates, so there is no
/// `.of` re-validation to thread.
fn emit_bytes_named_codec(out: &mut String, name: &str) {
    writeln!(
        out,
        "export function serialise_{name}(value: {name}): JsonValue {{"
    )
    .unwrap();
    writeln!(
        out,
        "  return __bynkBytesToBase64(value as unknown as Uint8Array);"
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{name}, BoundaryError> {{"
    )
    .unwrap();
    writeln!(out, "  if (typeof json !== \"string\") {{").unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"base64 string\", actual: typeof json }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  const __b = __bynkBytesFromBase64(json);").unwrap();
    writeln!(out, "  if (__b.tag === \"None\") {{").unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"base64 string\", actual: \"invalid base64\" }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  return Ok(__b.value as unknown as {name});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn emit_refined(out: &mut String, name: &str, base: BaseType, _decl: &TypeDecl) {
    // v0.110: a `Bytes`-based opaque/refined type has a bespoke base64 codec.
    if base == BaseType::Bytes {
        emit_bytes_named_codec(out, name);
        return;
    }
    let prim = ts_base_for_serialisation(base);
    let typeof_str = match base {
        BaseType::Int => "number",
        BaseType::String => "string",
        BaseType::Bool => "boolean",
        BaseType::Float => "number",
        BaseType::Duration | BaseType::Instant => "number",
        // Unreachable: the `Bytes` branch returns above.
        BaseType::Bytes => "string",
    };
    writeln!(
        out,
        "export function serialise_{name}(value: {name}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  return value as unknown as {prim};").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{name}, BoundaryError> {{"
    )
    .unwrap();
    writeln!(out, "  if (typeof json !== \"{typeof_str}\") {{").unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"{typeof_str}\", actual: typeof json }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    // Re-validate via the type's own constructor (`.of`), which applies
    // the refinement. If the type has no refinement, `.of` doesn't exist
    // for refined-base types; fall back to a direct cast.
    writeln!(
        out,
        "  const validated = (typeof ({name} as any).of === \"function\")"
    )
    .unwrap();
    writeln!(out, "    ? ({name} as any).of(json)").unwrap();
    writeln!(out, "    : Ok(json as unknown as {name});").unwrap();
    writeln!(out, "  if (validated.tag === \"Err\") {{").unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"RefinementViolation\", path, violation: validated.error }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  return Ok(validated.value as {name});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn emit_record(out: &mut String, name: &str, body: &RecordBody) {
    let fields: Vec<(String, TypeRef)> = body
        .fields
        .iter()
        .map(|f| (f.name.name.clone(), f.type_ref.clone()))
        .collect();
    emit_record_codec(out, name, name, &fields);
}

/// v0.174 (#592): the shared record codec body. `fn_suffix` is the codec name
/// suffix (`Order`, or the monomorphised `Paginated_User`); `ts_type` is the
/// TypeScript value type the codec accepts / returns (`Order`, or the erased
/// generic `Paginated<User>`). The two coincide for a non-generic record and
/// diverge for a generic-record instantiation.
fn emit_record_codec(
    out: &mut String,
    fn_suffix: &str,
    ts_type: &str,
    fields: &[(String, TypeRef)],
) {
    // serialise
    writeln!(
        out,
        "export function serialise_{fn_suffix}(value: {ts_type}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  return {{").unwrap();
    for (fname, type_ref) in fields {
        let expr = serialise_field_expr(type_ref, &format!("value.{fname}"));
        writeln!(out, "    {fname}: {expr},").unwrap();
    }
    writeln!(out, "  }};").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // deserialise
    writeln!(
        out,
        "export function deserialise_{fn_suffix}(json: JsonValue, path: string = \"$\"): Result<{ts_type}, BoundaryError> {{"
    )
    .unwrap();
    writeln!(
        out,
        "  if (typeof json !== \"object\" || json === null || Array.isArray(json)) {{"
    )
    .unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"object\", actual: typeof json }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  const obj = json as {{ [k: string]: JsonValue }};").unwrap();
    for (fname, type_ref) in fields {
        let access = format!("obj[\"{fname}\"]");
        let sub_path = format!("`${{path}}.{fname}`");
        emit_field_deserialise(out, fname, type_ref, &access, &sub_path);
    }
    write!(out, "  return Ok({{ ").unwrap();
    let parts: Vec<String> = fields
        .iter()
        .map(|(fname, _)| format!("{fname}: __{fname}"))
        .collect();
    write!(out, "{}", parts.join(", ")).unwrap();
    writeln!(out, " }} as {ts_type});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn emit_sum(out: &mut String, name: &str, body: &SumBody) {
    let variants: Vec<(String, Vec<(String, TypeRef)>)> = body
        .variants
        .iter()
        .map(|v| {
            (
                v.name.name.clone(),
                v.payload
                    .iter()
                    .map(|f| (f.name.name.clone(), f.type_ref.clone()))
                    .collect(),
            )
        })
        .collect();
    emit_sum_codec(out, name, name, &variants);
}

/// The serialise/deserialise pair for a sum type, over already-resolved variant
/// payloads. `fn_suffix` names the emitted functions (`Opt` / `Opt_Int`),
/// `ts_type` is their value type (`Opt` / `Opt<number>`). The wire discriminant
/// is `kind`; the in-memory discriminant is `tag`. #593: a generic-sum
/// instantiation reuses this with substituted payload types, exactly as a
/// generic record reuses [`emit_record_codec`].
fn emit_sum_codec(
    out: &mut String,
    fn_suffix: &str,
    ts_type: &str,
    variants: &[(String, Vec<(String, TypeRef)>)],
) {
    writeln!(
        out,
        "export function serialise_{fn_suffix}(value: {ts_type}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  switch (value.tag) {{").unwrap();
    for (vname, payload) in variants {
        if payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(out, "      return {{ kind: \"{vname}\" }};").unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            write!(out, "      return {{ kind: \"{vname}\"").unwrap();
            for (fname, type_ref) in payload {
                let expr = serialise_field_expr(type_ref, &format!("(value as any).{fname}"));
                write!(out, ", {fname}: {expr}").unwrap();
            }
            writeln!(out, " }};").unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "  }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "export function deserialise_{fn_suffix}(json: JsonValue, path: string = \"$\"): Result<{ts_type}, BoundaryError> {{"
    )
    .unwrap();
    writeln!(
        out,
        "  if (typeof json !== \"object\" || json === null || Array.isArray(json)) {{"
    )
    .unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"object\", actual: typeof json }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  const obj = json as {{ [k: string]: JsonValue }};").unwrap();
    writeln!(out, "  const kind = obj[\"kind\"];").unwrap();
    writeln!(out, "  switch (kind) {{").unwrap();
    for (vname, payload) in variants {
        if payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(out, "      return Ok({{ tag: \"{vname}\" }} as {ts_type});").unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            for (fname, type_ref) in payload {
                let access = format!("obj[\"{fname}\"]");
                let sub_path = format!("`${{path}}.{fname}`");
                emit_field_deserialise(out, fname, type_ref, &access, &sub_path);
            }
            write!(out, "      return Ok({{ tag: \"{vname}\"").unwrap();
            for (fname, _) in payload {
                write!(out, ", {fname}: __{fname}").unwrap();
            }
            writeln!(out, " }} as {ts_type});").unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "    default:").unwrap();
    writeln!(
        out,
        "      return Err({{ kind: \"StructuralMismatch\", path, expected: \"sum variant kind\", actual: String(kind) }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// Emit a let binding `__<field>` after destructuring & validating a
/// nested field.
fn emit_field_deserialise(out: &mut String, name: &str, t: &TypeRef, json: &str, path_expr: &str) {
    match t {
        // v0.20a: function types are confined to non-boundary positions
        // (`bynk.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..)
        | TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => {
            unreachable!("function/query/stream types are rejected at boundaries")
        }
        // v0.174 (#592): a generic-record instantiation delegates to its
        // monomorphised codec (`deserialise_Paginated_User`), exactly like a
        // named type delegates to its own `deserialise_<Name>`.
        TypeRef::App {
            name: app_name,
            args,
            ..
        } => {
            let inst = app_ts_name(&app_name.name, args);
            writeln!(
                out,
                "  const __r_{name} = deserialise_{inst}({json}, {path_expr});"
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        // v0.110 (ADR 0142 D5): a bare `Bytes` field is a base64 JSON string —
        // require a string, then decode (rejecting invalid base64), binding the
        // decoded `Uint8Array`. This is the one base type whose wire value is
        // not a direct cast of its erased representation.
        TypeRef::Base(BaseType::Bytes, _) => {
            writeln!(out, "  if (typeof {json} !== \"string\") {{").unwrap();
            writeln!(
                out,
                "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"base64 string\", actual: typeof {json} }});"
            )
            .unwrap();
            writeln!(out, "  }}").unwrap();
            writeln!(out, "  const __b_{name} = __bynkBytesFromBase64({json});").unwrap();
            writeln!(out, "  if (__b_{name}.tag === \"None\") {{").unwrap();
            writeln!(
                out,
                "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"base64 string\", actual: \"invalid base64\" }});"
            )
            .unwrap();
            writeln!(out, "  }}").unwrap();
            writeln!(out, "  const __{name} = __b_{name}.value;").unwrap();
        }
        TypeRef::Base(b, _) => {
            let typeof_str = match b {
                BaseType::Int => "number",
                BaseType::String => "string",
                BaseType::Bool => "boolean",
                BaseType::Float => "number",
                BaseType::Duration | BaseType::Instant => "number",
                // Unreachable: handled by the dedicated `Bytes` arm above.
                BaseType::Bytes => "string",
            };
            writeln!(out, "  if (typeof {json} !== \"{typeof_str}\") {{").unwrap();
            writeln!(
                out,
                "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"{typeof_str}\", actual: typeof {json} }});"
            )
            .unwrap();
            writeln!(out, "  }}").unwrap();
            // v0.22b: bare `Int` fields validate integrality (ADR 0049) —
            // with `Float` in the language there is no excuse for a
            // fractional `Int` from the wire. v0.90 (ADR 0114 D7): an `Instant`
            // is whole epoch milliseconds, so it validates integrality too.
            if *b == BaseType::Int || *b == BaseType::Instant {
                writeln!(out, "  if (!Number.isInteger({json})) {{").unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"integer\", actual: String({json}) }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
            }
            // v0.21: boundary `Float` values are finite (ADR 0040) —
            // `JSON.parse("1e999")` yields `Infinity`, which must not be
            // admitted from the wire.
            if *b == BaseType::Float {
                writeln!(out, "  if (!Number.isFinite({json})) {{").unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"finite number\", actual: String({json}) }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
            }
            writeln!(out, "  const __{name} = {json};").unwrap();
        }
        TypeRef::Named(id) => {
            // Defer to the type's own deserialiser. Assumes it exists in
            // scope (imported or declared locally).
            writeln!(
                out,
                "  const __r_{name} = deserialise_{}({json}, {path_expr});",
                id.name
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        TypeRef::Result(a, b, _) => {
            let ts_a = inner_ts_name(a);
            let ts_b = inner_ts_name(b);
            writeln!(
                out,
                "  const __r_{name} = deserialise_Result_{ts_a}_{ts_b}({json}, {path_expr});",
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        TypeRef::Option(a, _) => {
            let ts_a = inner_ts_name(a);
            writeln!(
                out,
                "  const __r_{name} = deserialise_Option_{ts_a}({json}, {path_expr});",
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        // v0.20b: collections delegate to their specialised helpers, exactly
        // like Result/Option instantiations.
        TypeRef::List(a, _) => {
            let ts_a = inner_ts_name(a);
            writeln!(
                out,
                "  const __r_{name} = deserialise_List_{ts_a}({json}, {path_expr});",
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        TypeRef::Map(k, v, _) => {
            let ts_k = inner_ts_name(k);
            let ts_v = inner_ts_name(v);
            writeln!(
                out,
                "  const __r_{name} = deserialise_Map_{ts_k}_{ts_v}({json}, {path_expr});",
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        TypeRef::Effect(_, _)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::HttpResult(_, _)
        | TypeRef::QueueResult(_) => {
            writeln!(out, "  const __{name} = {json} as any;").unwrap();
        }
        TypeRef::Unit(_) => {
            writeln!(out, "  const __{name} = undefined;").unwrap();
        }
    }
}

fn serialise_field_expr(t: &TypeRef, value: &str) -> String {
    serialise_field_expr_via(t, value, "")
}

/// The same dispatch, reaching its helpers through `ns` — `""` for a
/// module-local call, `"handlers."` from a Worker entry point that imports the
/// context's handlers as a namespace. Threading the prefix (rather than each
/// caller owning a parallel dispatch) is what keeps the boundary to **one**
/// codec path.
fn serialise_field_expr_via(t: &TypeRef, value: &str, ns: &str) -> String {
    match t {
        // v0.20a: function types are confined to non-boundary positions
        // (`bynk.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..)
        | TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => {
            unreachable!("function/query/stream types are rejected at boundaries")
        }
        // v0.174 (#592): a generic-record instantiation serialises through its
        // monomorphised codec (`serialise_Paginated_User`).
        TypeRef::App { name, args, .. } => {
            format!("{ns}serialise_{}({value})", app_ts_name(&name.name, args))
        }
        // v0.21: serialising a non-finite `Float` is a contract violation
        // (`JSON.stringify(NaN)` would silently produce `null`); the guard is
        // a self-contained IIFE so the module needs no extra runtime import.
        TypeRef::Base(BaseType::Float, _) => format!(
            "((v: number) => {{ if (!Number.isFinite(v)) throw new Error(\"non-finite Float at boundary\"); return v as JsonValue; }})({value})"
        ),
        // v0.110 (ADR 0142 D5): a `Bytes` is base64-encoded on the wire — the
        // one base type whose serialise is an encode, not a bare cast.
        TypeRef::Base(BaseType::Bytes, _) => {
            format!("__bynkBytesToBase64({value}) as JsonValue")
        }
        TypeRef::Base(_, _) => format!("{value} as JsonValue"),
        TypeRef::Named(id) => format!("{ns}serialise_{}({value})", id.name),
        TypeRef::Result(a, b, _) => format!(
            "{ns}serialise_Result_{}_{}({value})",
            inner_ts_name(a),
            inner_ts_name(b)
        ),
        TypeRef::Option(a, _) => format!("{ns}serialise_Option_{}({value})", inner_ts_name(a)),
        TypeRef::List(a, _) => format!("{ns}serialise_List_{}({value})", inner_ts_name(a)),
        TypeRef::Map(k, v, _) => format!(
            "{ns}serialise_Map_{}_{}({value})",
            inner_ts_name(k),
            inner_ts_name(v)
        ),
        // An `Effect` is stripped by the caller before it reaches a wire
        // position; reaching one here means the payload, not the wrapper.
        TypeRef::Effect(inner, _) => serialise_field_expr_via(inner, value, ns),
        // The runtime-owned error types have no *generated* codec — they are
        // declared by the runtime, not by a `TypeDecl` this emitter can walk, so
        // there is no `serialise_ValidationError` to name. They keep the
        // pass-through the whole boundary used before this increment; unifying
        // the user-type paths does not reach them. Their JSON shape is fixed by
        // the runtime (`errors.ts`), so the cast is not *wrong* — it is simply
        // unchecked, and it is the one remaining unchecked arm at the boundary.
        TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::HttpResult(_, _)
        | TypeRef::QueueResult(_) => {
            format!("{value} as JsonValue")
        }
        TypeRef::Unit(_) => "null".to_string(),
    }
}

fn inner_ts_name(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        // v0.20a: function types are confined to non-boundary positions
        // (`bynk.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..)
        | TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => {
            unreachable!("function/query/stream types are rejected at boundaries")
        }
        // v0.174 (#592): the codec suffix for a generic-record instantiation —
        // `Paginated[User]` → `Paginated_User`.
        TypeRef::App { name, args, .. } => app_ts_name(&name.name, args),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result_{}_{}", inner_ts_name(a), inner_ts_name(b)),
        TypeRef::Option(a, _) => format!("Option_{}", inner_ts_name(a)),
        TypeRef::Effect(a, _) => format!("Effect_{}", inner_ts_name(a)),
        TypeRef::HttpResult(a, _) => format!("HttpResult_{}", inner_ts_name(a)),
        TypeRef::List(a, _) => format!("List_{}", inner_ts_name(a)),
        TypeRef::Map(k, v, _) => format!("Map_{}_{}", inner_ts_name(k), inner_ts_name(v)),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "Unit".to_string(),
    }
}

/// v0.22b: the codec closure for a set of `Json.encode`/`Json.decode[T]`
/// target type-refs — the named types needing per-type helpers (transitively
/// through record fields and sum payloads) plus the generic instantiations
/// needing specialised helpers. The same closure logic as the boundary
/// collectors, rooted at expressions instead of service signatures.
pub fn collect_codec_closure(
    roots: &[TypeRef],
    types: &std::collections::HashMap<String, TypeDecl>,
) -> (Vec<String>, Vec<GenericInst>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut names: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let recursive_set = recursive_generic_names(types);
    let recursive = &recursive_set;
    for r in roots {
        collect_type_names(r, &mut stack, types, recursive);
    }
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        names.push(name.clone());
        let Some(decl) = types.get(&name) else {
            continue;
        };
        match &decl.body {
            TypeBody::Record(r) => {
                for f in &r.fields {
                    collect_type_names(&f.type_ref, &mut stack, types, recursive);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        collect_type_names(&p.type_ref, &mut stack, types, recursive);
                    }
                }
            }
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
        }
    }
    names.sort();

    let mut insts: Vec<GenericInst> = Vec::new();
    let mut inst_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in roots {
        walk_generic_inst(r, &mut insts, &mut inst_seen, types, recursive);
    }
    for name in &names {
        let Some(decl) = types.get(name) else {
            continue;
        };
        match &decl.body {
            TypeBody::Record(r) => {
                for f in &r.fields {
                    walk_generic_inst(&f.type_ref, &mut insts, &mut inst_seen, types, recursive);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        walk_generic_inst(
                            &p.type_ref,
                            &mut insts,
                            &mut inst_seen,
                            types,
                            recursive,
                        );
                    }
                }
            }
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
        }
    }
    (names, insts)
}

/// v0.22b: an expression-form serialise for a codec target — the same
/// dispatch as a record field's serialisation.
pub fn serialise_expr(t: &TypeRef, value: &str) -> String {
    serialise_field_expr(t, value)
}

/// v0.176 (#642): the one serialise dispatch for the workers cross-context
/// boundary, reaching helpers through `ns`. Replaces the two parallel
/// dispatches this boundary used to carry — `emit.rs`'s `workers_serialise_expr`
/// (which dropped `List`/`Map` to a bare `as JsonValue` cast) and
/// `workers_entry.rs`'s `serialise_call` (which did the same to `Bytes`, the
/// asymmetry that forced `bynk.types.bytes_at_workers_boundary`).
pub fn serialise_expr_via(t: &TypeRef, value: &str, ns: &str) -> String {
    serialise_field_expr_via(t, value, ns)
}

/// v0.176 (#642): a deserialise **reference** for `ns`, shaped to
/// `callService`'s `deserialiseResult` parameter. The inline arms become a
/// lambda rather than the unvalidated `((j: any) => ({ tag: "Ok", value: j }))`
/// identity the caller path used to fall back to.
pub fn deserialise_ref_via(t: &TypeRef, ns: &str) -> String {
    match strip_effect(t) {
        TypeRef::Named(id) => format!("{ns}deserialise_{}", id.name),
        t @ (TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        | TypeRef::App { .. }) => format!("{ns}deserialise_{}", inner_ts_name(t)),
        other => format!(
            "(__j: JsonValue) => {}",
            deserialise_expr_via(other, "__j", "$", ns)
        ),
    }
}

/// An `Effect[T]` in a handler signature wraps the *handler*, not the wire
/// payload — the caller awaits the Promise, so the codec is `T`'s.
fn strip_effect(t: &TypeRef) -> &TypeRef {
    match t {
        TypeRef::Effect(inner, _) => strip_effect(inner),
        other => other,
    }
}

/// v0.22b: an expression-form deserialise call for a codec target. Named
/// types and generic instantiations go through their (module-local)
/// helpers; bases inline the structural check.
pub fn deserialise_expr(t: &TypeRef, json: &str, path: &str) -> String {
    deserialise_expr_via(t, json, path, "")
}

/// v0.176 (#642): the one deserialise dispatch for the workers cross-context
/// boundary, reaching helpers through `ns`. Replaces `workers_entry.rs`'s
/// `deserialise_call`; the `Json.decode` entry (`deserialise_expr`) is the same
/// function with an empty prefix.
///
/// This carries two arms the `Json` codec path never needs, because the
/// checker's codec-domain rule rejects them there but the cross-context
/// boundary admits them: `Unit` (an `on call` may return `Effect[Result[(), E]]`)
/// and the runtime-owned error types.
pub fn deserialise_expr_via(t: &TypeRef, json: &str, path: &str, ns: &str) -> String {
    match t {
        TypeRef::Named(id) => format!("{ns}deserialise_{}({json}, \"{path}\")", id.name),
        TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        // v0.174 (#592): a generic-record instantiation decodes through its
        // monomorphised codec (`deserialise_Paginated_User`).
        | TypeRef::App { .. } => {
            format!("{ns}deserialise_{}({json}, \"{path}\")", inner_ts_name(t))
        }
        TypeRef::Effect(inner, _) => deserialise_expr_via(inner, json, path, ns),
        // A `()` carries no wire content — the wire slot is `null` and the value
        // is `undefined`. Nothing to validate, so `Ok` is the honest answer here
        // rather than an erosion.
        //
        // Reached only by a **bare** `()` in a wire position. A `Result`-wrapped
        // one — `on call () -> Effect[Result[(), E]]`, the common shape — strips
        // its `Effect` and then goes through `deserialise_Result_Unit_E`, whose
        // generated body handles the `Unit` payload itself (`emit_generic_helpers`),
        // so it never lands here. No fixture currently exercises this arm; it is
        // defensive, and saying so is more useful than implying coverage.
        TypeRef::Unit(_) => "Ok(undefined) as Result<void, BoundaryError>".to_string(),
        // The runtime-owned error types: no generated codec to name (see
        // `serialise_field_expr_via`). The one unchecked arm left at the boundary.
        TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::HttpResult(_, _)
        | TypeRef::QueueResult(_) => {
            format!("Ok({json} as any) as Result<any, BoundaryError>")
        }
        // v0.110 (ADR 0142 D5): a `Bytes` wires as a base64 string; decode it
        // (rejecting a non-string or invalid base64) to a `Uint8Array`.
        TypeRef::Base(BaseType::Bytes, _) => {
            format!(
                "((__v) => typeof __v === \"string\" ? ((__b) => __b.tag === \"Some\" ? Ok(__b.value) : Err({{ kind: \"StructuralMismatch\", path: \"{path}\", expected: \"base64 string\", actual: \"invalid base64\" }} as BoundaryError))(__bynkBytesFromBase64(__v)) : Err({{ kind: \"StructuralMismatch\", path: \"{path}\", expected: \"base64 string\", actual: typeof __v }} as BoundaryError))({json})"
            )
        }
        TypeRef::Base(b, _) => {
            let typeof_str = match b {
                BaseType::Int => "number",
                BaseType::String => "string",
                BaseType::Bool => "boolean",
                BaseType::Float => "number",
                BaseType::Duration | BaseType::Instant => "number",
                // Unreachable: handled by the dedicated `Bytes` arm above.
                BaseType::Bytes => "string",
            };
            let extra = match b {
                BaseType::Float => " && Number.isFinite(__v)",
                // v0.86 (ADR 0112 D6): a `Duration` is whole milliseconds —
                // reject a non-integer from the wire, as a refined `Int` does.
                BaseType::Int | BaseType::Duration | BaseType::Instant => {
                    " && Number.isInteger(__v)"
                }
                _ => "",
            };
            // v0.176 (#642): report what was *required*, not just the `typeof`
            // that was tested. For the arms carrying an `extra` predicate the two
            // differ, and reporting the bare `typeof` makes the error useless in
            // exactly the case the predicate exists to catch: a `3.5` for an `Int`
            // would read `expected: "number", actual: "number"`.
            let expected = match b {
                BaseType::Int | BaseType::Duration | BaseType::Instant => "integer",
                BaseType::Float => "finite number",
                _ => typeof_str,
            };
            let err = |actual: &str| {
                format!(
                    "Err({{ kind: \"StructuralMismatch\", path: \"{path}\", expected: \"{expected}\", actual: {actual} }} as BoundaryError)"
                )
            };
            if extra.is_empty() {
                return format!(
                    "((__v) => typeof __v === \"{typeof_str}\" ? Ok(__v) : {})({json})",
                    err("typeof __v")
                );
            }
            // The two failure modes are **not** the same error, and collapsing
            // them is what made both predecessors imprecise in opposite
            // directions. The `Json` path reported `typeof` for both, losing the
            // predicate failure's detail; the workers path reported
            // `String(value)` for both, which echoes an arbitrary caller-supplied
            // value into a 400 response body (an `Int` sent `"hunter2"` reported
            // `actual: "hunter2"`) and violates the ADR 0107 discipline of never
            // reporting the offending value.
            //
            // Split them and both problems go away. A wrong `typeof` reports the
            // `typeof` — the value could be anything, so it is never echoed. A
            // *failed predicate* means the `typeof` already matched, so the value
            // is provably a **number**: `String(__v)` is `"3.5"` for a
            // non-integer `Int`, and provably one of `"NaN"` / `"Infinity"` /
            // `"-Infinity"` for a non-finite `Float` — a closed set. That is
            // strictly more precise than either predecessor, with strictly less
            // exposure.
            let predicate = extra.trim_start_matches(" && ");
            format!(
                "((__v) => typeof __v !== \"{typeof_str}\" ? {} : {predicate} ? Ok(__v) : {})({json})",
                err("typeof __v"),
                err("String(__v)")
            )
        }
        // Everything else is rejected by the checker's codec-domain rule (the
        // `Json` path) or by the boundary rules (the workers path). Shared by
        // three callers, so the message names the type rather than one caller.
        other => unreachable!("non-codable type reached a codec lowering: {other:?}"),
    }
}

/// Collect the set of `Result<A, B>` / `Option<A>` instantiations used in
/// boundary positions so the emitter can synthesise the specialised
/// helpers. v0.18: an instantiation may also appear in the *fields* of a
/// boundary record or sum payload (e.g. the bynk surface's
/// `Request.contentType: Option[String]`) — the per-type serialisers
/// delegate to the specialised generic helpers, so walk those too.
pub fn collect_generic_instantiations(
    services: &std::collections::HashMap<String, ServiceDecl>,
    // v0.96 (ADR 0124): an agent's `store`-field element types are validated on
    // rehydration, so a `Cell[Option[Int]]` / `Log[List[T]]` needs its specialised
    // generic helper emitted just like a boundary signature does.
    agents: &std::collections::HashMap<String, AgentDecl>,
    boundary_type_names: &[String],
    types: &std::collections::HashMap<String, TypeDecl>,
) -> Vec<GenericInst> {
    let mut out: Vec<GenericInst> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let recursive_set = recursive_generic_names(types);
    let recursive = &recursive_set;
    // Iterate services in name order: `HashMap::values()` order varies per
    // process, and the *emission order* of the specialised helpers follows
    // first-encounter order here. Surfaced by the first fixture with
    // multiple same-file services carrying different instantiations (v0.23
    // #35 CI); latent since v0.8.
    let mut svc_names: Vec<&String> = services.keys().collect();
    svc_names.sort();
    for name in svc_names {
        let s = &services[name];
        for h in &s.handlers {
            for p in &h.params {
                walk_generic_inst(&p.type_ref, &mut out, &mut seen, types, recursive);
            }
            walk_generic_inst(&h.return_type, &mut out, &mut seen, types, recursive);
        }
    }
    let mut agent_names: Vec<&String> = agents.keys().collect();
    agent_names.sort();
    for name in agent_names {
        for f in &agents[name].store_fields {
            for arg in &f.kind.args {
                walk_generic_inst(arg, &mut out, &mut seen, types, recursive);
            }
        }
    }
    for name in boundary_type_names {
        let Some(decl) = types.get(name) else {
            continue;
        };
        // v0.174 (#592): never walk a *generic* declaration's own fields — they
        // are the declared, unsubstituted body (`Paginated[T] = { items: List[T]
        // }`), so walking `List[T]` would emit a bogus `serialise_List_T` over the
        // unbound type variable `T`. The instantiations a generic record needs
        // come from its *use* sites (`Paginated[User]`, a `TypeRef::App`), which
        // `walk_generic_inst` expands with concrete arguments. This mirrors the
        // `emit_helpers_for_owner` skip that keeps a bare `serialise_Paginated`
        // from being emitted.
        if !decl.type_params.is_empty() {
            continue;
        }
        match &decl.body {
            TypeBody::Record(r) => {
                for f in &r.fields {
                    walk_generic_inst(&f.type_ref, &mut out, &mut seen, types, recursive);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        walk_generic_inst(&p.type_ref, &mut out, &mut seen, types, recursive);
                    }
                }
            }
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
        }
    }
    out
}

#[derive(Debug, Clone)]
pub enum GenericInst {
    ResultInst {
        ok: TypeRef,
        err: TypeRef,
    },
    OptionInst {
        inner: TypeRef,
    },
    /// v0.20b: a `List[T]` boundary instantiation — element-wise wire format.
    ListInst {
        elem: TypeRef,
    },
    /// v0.20b: a `Map[K, V]` boundary instantiation — entries-array wire
    /// format (`[[k, v], …]`), insertion-ordered.
    MapInst {
        key: TypeRef,
        val: TypeRef,
    },
    /// v0.174 (#592): a generic user-record instantiation `Name[args…]` — a
    /// monomorphised per-instantiation record codec (`serialise_Paginated_User`)
    /// specialised to the concrete arguments (ADR 0183 Decision C's follow-on).
    RecordInst {
        name: String,
        args: Vec<TypeRef>,
    },
    /// #593: a generic user-sum instantiation `Name[args…]` — a monomorphised
    /// per-instantiation discriminated-union codec (`serialise_ApiResult_User`),
    /// the sum analogue of [`GenericInst::RecordInst`].
    SumInst {
        name: String,
        args: Vec<TypeRef>,
    },
}

impl GenericInst {
    pub fn ts_name(&self) -> String {
        match self {
            GenericInst::ResultInst { ok, err } => {
                format!("Result_{}_{}", inner_ts_name(ok), inner_ts_name(err))
            }
            GenericInst::OptionInst { inner } => {
                format!("Option_{}", inner_ts_name(inner))
            }
            GenericInst::ListInst { elem } => format!("List_{}", inner_ts_name(elem)),
            GenericInst::MapInst { key, val } => {
                format!("Map_{}_{}", inner_ts_name(key), inner_ts_name(val))
            }
            GenericInst::RecordInst { name, args } => app_ts_name(name, args),
            GenericInst::SumInst { name, args } => app_ts_name(name, args),
        }
    }
}

fn walk_generic_inst(
    t: &TypeRef,
    out: &mut Vec<GenericInst>,
    seen: &mut std::collections::HashSet<String>,
    types: &std::collections::HashMap<String, TypeDecl>,
    recursive: &std::collections::HashSet<String>,
) {
    match t {
        // v0.174 (#592): a generic-record instantiation needs a monomorphised
        // codec, and so do the generic instantiations reachable through its
        // concrete field types (`Paginated[User]` → `List[User]` →
        // `serialise_List_User`, `Envelope[Box[User]]` → `Box[User]` →
        // `serialise_Box_User`). Substitute the fields and walk them. A recursive
        // generic record (no finite codec set) is rejected at the boundary before
        // emit; the guard here is defence in depth so this walk always terminates
        // (the `seen` dedup alone cannot bound *polymorphic* recursion, whose
        // instantiations each carry a distinct name).
        TypeRef::App { name, args, .. } => {
            if recursive.contains(&name.name) {
                return;
            }
            // #593: an `App` names a generic record OR a generic sum; dispatch on
            // the declaration's body so the right monomorphised codec is emitted,
            // and walk the reachable instantiations through its concrete member
            // types (a sum's variant payloads, a record's fields).
            let is_sum = matches!(
                types.get(&name.name).map(|d| &d.body),
                Some(TypeBody::Sum(_))
            );
            let inst = if is_sum {
                GenericInst::SumInst {
                    name: name.name.clone(),
                    args: args.clone(),
                }
            } else {
                GenericInst::RecordInst {
                    name: name.name.clone(),
                    args: args.clone(),
                }
            };
            let key = inst.ts_name();
            if !seen.insert(key) {
                return;
            }
            out.push(inst);
            for a in args {
                walk_generic_inst(a, out, seen, types, recursive);
            }
            if is_sum {
                if let Some(variants) = sum_inst_variants(&name.name, args, types) {
                    for (_, payload) in &variants {
                        for (_, ft) in payload {
                            walk_generic_inst(ft, out, seen, types, recursive);
                        }
                    }
                }
            } else if let Some(fields) = record_inst_fields(&name.name, args, types) {
                for (_, ft) in &fields {
                    walk_generic_inst(ft, out, seen, types, recursive);
                }
            }
        }
        TypeRef::Result(a, b, _) => {
            let inst = GenericInst::ResultInst {
                ok: (**a).clone(),
                err: (**b).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(a, out, seen, types, recursive);
            walk_generic_inst(b, out, seen, types, recursive);
        }
        TypeRef::Option(a, _) => {
            let inst = GenericInst::OptionInst {
                inner: (**a).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(a, out, seen, types, recursive);
        }
        TypeRef::Effect(a, _) => walk_generic_inst(a, out, seen, types, recursive),
        TypeRef::HttpResult(a, _) => walk_generic_inst(a, out, seen, types, recursive),
        TypeRef::List(a, _) => {
            let inst = GenericInst::ListInst {
                elem: (**a).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(a, out, seen, types, recursive);
        }
        TypeRef::Map(k, v, _) => {
            let inst = GenericInst::MapInst {
                key: (**k).clone(),
                val: (**v).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(k, out, seen, types, recursive);
            walk_generic_inst(v, out, seen, types, recursive);
        }
        _ => {}
    }
}

/// Emit specialised helpers for each `Result<A, B>` / `Option<A>`
/// instantiation. They delegate to the named-type serialisers for A and B.
/// v0.174 (#592): also emits a monomorphised record codec per generic
/// instantiation (`RecordInst`), which needs the declarations to substitute
/// its type parameters.
pub fn emit_generic_helpers(
    out: &mut String,
    insts: &[GenericInst],
    types: &std::collections::HashMap<String, TypeDecl>,
) {
    for inst in insts {
        match inst {
            // v0.174 (#592): a generic-record instantiation `Paginated[User]`
            // emits `serialise_Paginated_User` / `deserialise_Paginated_User`,
            // its fields specialised to the concrete arguments. The value type
            // is the erased generic `Paginated<User>`.
            GenericInst::RecordInst { name, args } => {
                let fn_suffix = app_ts_name(name, args);
                let ts_type = format!(
                    "{}<{}>",
                    name,
                    args.iter()
                        .map(ts_inner_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                // `record_inst_fields` is `None` only for an unknown name, a
                // non-record body, or an arity mismatch — all of which the
                // resolver rejects (`generic_non_record` / `type_arg_count`)
                // before a `RecordInst` is ever collected. Panic loudly rather
                // than silently emit a call to an undefined codec (the file's
                // convention for a checker-guaranteed invariant).
                let fields = record_inst_fields(name, args, types).unwrap_or_else(|| {
                    unreachable!("RecordInst `{name}` is not a resolved generic record")
                });
                emit_record_codec(out, &fn_suffix, &ts_type, &fields);
            }
            // #593: a generic-sum instantiation `ApiResult[User]` emits
            // `serialise_ApiResult_User` / `deserialise_ApiResult_User`, its
            // variant payloads specialised to the concrete arguments. The value
            // type is the erased generic `ApiResult<User>`. Mirrors `RecordInst`.
            GenericInst::SumInst { name, args } => {
                let fn_suffix = app_ts_name(name, args);
                let ts_type = format!(
                    "{}<{}>",
                    name,
                    args.iter()
                        .map(ts_inner_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let variants = sum_inst_variants(name, args, types).unwrap_or_else(|| {
                    unreachable!("SumInst `{name}` is not a resolved generic sum")
                });
                emit_sum_codec(out, &fn_suffix, &ts_type, &variants);
            }
            GenericInst::ResultInst { ok, err } => {
                let ok_ts = inner_ts_name(ok);
                let err_ts = inner_ts_name(err);
                let ok_inner = ts_inner_type(ok);
                let err_inner = ts_inner_type(err);
                let serialise_ok = serialise_field_expr(ok, "value.value");
                let serialise_err = serialise_field_expr(err, "value.error");
                writeln!(
                    out,
                    "export function serialise_Result_{ok_ts}_{err_ts}(value: Result<{ok_inner}, {err_inner}>): JsonValue {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "  if (value.tag === \"Ok\") return {{ kind: \"Ok\", value: {serialise_ok} }};"
                )
                .unwrap();
                writeln!(out, "  return {{ kind: \"Err\", error: {serialise_err} }};").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();

                writeln!(
                    out,
                    "export function deserialise_Result_{ok_ts}_{err_ts}(json: JsonValue, path: string = \"$\"): Result<Result<{ok_inner}, {err_inner}>, BoundaryError> {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "  if (typeof json !== \"object\" || json === null || Array.isArray(json)) {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"object\", actual: typeof json }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  const obj = json as {{ [k: string]: JsonValue }};").unwrap();
                writeln!(out, "  if (obj[\"kind\"] === \"Ok\") {{").unwrap();
                emit_field_deserialise(out, "v", ok, "obj[\"value\"]", "`${path}.value`");
                writeln!(
                    out,
                    "    return Ok(Ok(__v) as Result<{ok_inner}, {err_inner}>);"
                )
                .unwrap();
                writeln!(out, "  }} else if (obj[\"kind\"] === \"Err\") {{").unwrap();
                emit_field_deserialise(out, "e", err, "obj[\"error\"]", "`${path}.error`");
                writeln!(
                    out,
                    "    return Ok(Err(__e) as Result<{ok_inner}, {err_inner}>);"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  return Err({{ kind: \"StructuralMismatch\", path, expected: \"Ok | Err\", actual: String(obj[\"kind\"]) }});").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();
            }
            GenericInst::OptionInst { inner } => {
                let inner_ts = inner_ts_name(inner);
                let inner_ty = ts_inner_type(inner);
                let serialise_inner = serialise_field_expr(inner, "value.value");
                writeln!(
                    out,
                    "export function serialise_Option_{inner_ts}(value: Option<{inner_ty}>): JsonValue {{"
                )
                .unwrap();
                writeln!(out, "  if (value.tag === \"Some\") return {{ kind: \"Some\", value: {serialise_inner} }};").unwrap();
                writeln!(out, "  return {{ kind: \"None\" }};").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();

                writeln!(
                    out,
                    "export function deserialise_Option_{inner_ts}(json: JsonValue, path: string = \"$\"): Result<Option<{inner_ty}>, BoundaryError> {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "  if (typeof json !== \"object\" || json === null || Array.isArray(json)) {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"object\", actual: typeof json }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  const obj = json as {{ [k: string]: JsonValue }};").unwrap();
                writeln!(out, "  if (obj[\"kind\"] === \"Some\") {{").unwrap();
                emit_field_deserialise(out, "v", inner, "obj[\"value\"]", "`${path}.value`");
                writeln!(out, "    return Ok(Some(__v) as Option<{inner_ty}>);").unwrap();
                writeln!(out, "  }} else if (obj[\"kind\"] === \"None\") {{").unwrap();
                writeln!(out, "    return Ok(None as Option<{inner_ty}>);").unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  return Err({{ kind: \"StructuralMismatch\", path, expected: \"Some | None\", actual: String(obj[\"kind\"]) }});").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();
            }
            // v0.20b: `List[T]` — element-wise wire format (a JSON array).
            GenericInst::ListInst { elem } => {
                let elem_ts = inner_ts_name(elem);
                let elem_ty = ts_inner_type(elem);
                let serialise_elem = serialise_field_expr(elem, "v");
                writeln!(
                    out,
                    "export function serialise_List_{elem_ts}(value: readonly {elem_ty}[]): JsonValue {{"
                )
                .unwrap();
                writeln!(out, "  return value.map((v) => {serialise_elem});").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();

                writeln!(
                    out,
                    "export function deserialise_List_{elem_ts}(json: JsonValue, path: string = \"$\"): Result<readonly {elem_ty}[], BoundaryError> {{"
                )
                .unwrap();
                writeln!(out, "  if (!Array.isArray(json)) {{").unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"array\", actual: typeof json }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  const out: {elem_ty}[] = [];").unwrap();
                writeln!(out, "  for (let i = 0; i < json.length; i++) {{").unwrap();
                // Bind the element before validating: `json[i]` with a
                // mutable index does not narrow under a typeof guard.
                writeln!(out, "  const item = json[i];").unwrap();
                emit_field_deserialise(out, "el", elem, "item", "`${path}[${i}]`");
                // The element deserialiser may come from the declaring
                // commons and return the *unbranded* record; this module's
                // element type may be the context's branded rebrand. Assert
                // the element like the Option codec above does (#527).
                writeln!(out, "  out.push(__el as {elem_ty});").unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  return Ok(out);").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();
            }
            // v0.20b: `Map[K, V]` — entries-array wire format `[[k, v], …]`,
            // uniform across String/Int keys and insertion-ordered
            // (normative, §7).
            GenericInst::MapInst { key, val } => {
                let key_ts = inner_ts_name(key);
                let val_ts = inner_ts_name(val);
                let key_ty = ts_inner_type(key);
                let val_ty = ts_inner_type(val);
                let serialise_key = serialise_field_expr(key, "k");
                let serialise_val = serialise_field_expr(val, "v");
                writeln!(
                    out,
                    "export function serialise_Map_{key_ts}_{val_ts}(value: ReadonlyMap<{key_ty}, {val_ty}>): JsonValue {{"
                )
                .unwrap();
                writeln!(out, "  const entries: JsonValue[] = [];").unwrap();
                writeln!(out, "  for (const [k, v] of value) {{").unwrap();
                writeln!(out, "    entries.push([{serialise_key}, {serialise_val}]);").unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  return entries;").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();

                writeln!(
                    out,
                    "export function deserialise_Map_{key_ts}_{val_ts}(json: JsonValue, path: string = \"$\"): Result<ReadonlyMap<{key_ty}, {val_ty}>, BoundaryError> {{"
                )
                .unwrap();
                writeln!(out, "  if (!Array.isArray(json)) {{").unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"array\", actual: typeof json }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  const out = new Map<{key_ty}, {val_ty}>();").unwrap();
                writeln!(out, "  for (let i = 0; i < json.length; i++) {{").unwrap();
                writeln!(out, "  const entry = json[i];").unwrap();
                writeln!(out, "  if (!Array.isArray(entry) || entry.length !== 2) {{").unwrap();
                writeln!(
                    out,
                    "    return Err({{ kind: \"StructuralMismatch\", path: `${{path}}[${{i}}]`, expected: \"[key, value] entry\", actual: typeof entry }});"
                )
                .unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  const entryK = entry[0];").unwrap();
                writeln!(out, "  const entryV = entry[1];").unwrap();
                emit_field_deserialise(out, "k", key, "entryK", "`${path}[${i}][0]`");
                emit_field_deserialise(out, "v", val, "entryV", "`${path}[${i}][1]`");
                // Same brand assertion as the List codec (#527).
                writeln!(out, "  out.set(__k as {key_ty}, __v as {val_ty});").unwrap();
                writeln!(out, "  }}").unwrap();
                writeln!(out, "  return Ok(out);").unwrap();
                writeln!(out, "}}").unwrap();
                writeln!(out).unwrap();
            }
        }
    }
}

fn ts_inner_type(t: &TypeRef) -> String {
    match t {
        // v0.20a: function types are confined to non-boundary positions
        // (`bynk.types.function_at_boundary`), so the serialisation machinery
        // can never legally see one.
        TypeRef::Fn(..)
        | TypeRef::Query(..)
        | TypeRef::Stream(..)
        | TypeRef::Connection(..)
        | TypeRef::History(..) => {
            unreachable!("function/query/stream types are rejected at boundaries")
        }
        // v0.174 (#592): a generic-record instantiation erases to the generic
        // interface applied to its concrete arguments (`Paginated<User>`).
        TypeRef::App { name, args, .. } => format!(
            "{}<{}>",
            name.name,
            args.iter()
                .map(ts_inner_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeRef::Base(b, _) => match b {
            BaseType::Int => "number".to_string(),
            BaseType::String => "string".to_string(),
            BaseType::Bool => "boolean".to_string(),
            BaseType::Float => "number".to_string(),
            BaseType::Duration | BaseType::Instant => "number".to_string(),
            // v0.110 (ADR 0142): `Bytes` erases to `Uint8Array`.
            BaseType::Bytes => "Uint8Array".to_string(),
        },
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result<{}, {}>", ts_inner_type(a), ts_inner_type(b)),
        TypeRef::Option(a, _) => format!("Option<{}>", ts_inner_type(a)),
        TypeRef::Effect(a, _) => format!("Promise<{}>", ts_inner_type(a)),
        TypeRef::HttpResult(a, _) => format!("HttpResult<{}>", ts_inner_type(a)),
        TypeRef::List(a, _) => format!("readonly {}[]", ts_inner_type(a)),
        TypeRef::Map(k, v, _) => {
            format!("ReadonlyMap<{}, {}>", ts_inner_type(k), ts_inner_type(v))
        }
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}
