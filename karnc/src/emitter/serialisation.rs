//! Per-type serialise / deserialise helper generation for workers mode
//! (v0.8 §3.4 / §5.2).
//!
//! Every Karn type that crosses a context boundary needs:
//!   - `serialise_<Type>(value): JsonValue` — structural lowering.
//!   - `deserialise_<Type>(json): Result<<Type>, BoundaryError>` —
//!     structural validation + refinement re-validation, then a nominal
//!     cast back to the receiving context's view.
//!
//! Helpers live in the *owning* module — commons modules emit helpers for
//! commons types, context modules emit helpers for the types they declare.

use std::fmt::Write as _;

use crate::ast::*;

/// Compute the set of type names (transitively reachable) that need
/// serialise/deserialise helpers for this context: any type used in the
/// argument or return position of a service handler exposed by this
/// context, walked through record fields, sum payloads, and the generic
/// type parameters of Result/Option/Effect.
pub fn collect_boundary_types(
    types: &std::collections::HashMap<String, TypeDecl>,
    services: &std::collections::HashMap<String, ServiceDecl>,
) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();

    for service in services.values() {
        for h in &service.handlers {
            for p in &h.params {
                collect_type_names(&p.type_ref, &mut stack);
            }
            collect_type_names(&h.return_type, &mut stack);
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
                    collect_type_names(&f.type_ref, &mut stack);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        collect_type_names(&p.type_ref, &mut stack);
                    }
                }
            }
            TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
        }
    }

    out.sort();
    out
}

fn collect_type_names(t: &TypeRef, stack: &mut Vec<String>) {
    match t {
        TypeRef::Named(id) => stack.push(id.name.clone()),
        TypeRef::Result(a, b, _) => {
            collect_type_names(a, stack);
            collect_type_names(b, stack);
        }
        TypeRef::Option(a, _) => collect_type_names(a, stack),
        TypeRef::Effect(a, _) => collect_type_names(a, stack),
        TypeRef::HttpResult(a, _) => collect_type_names(a, stack),
        TypeRef::Base(_, _) | TypeRef::ValidationError(_) | TypeRef::Unit(_) => {}
    }
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
    }
}

fn emit_refined(out: &mut String, name: &str, base: BaseType, _decl: &TypeDecl) {
    let prim = ts_base_for_serialisation(base);
    let typeof_str = match base {
        BaseType::Int => "number",
        BaseType::String => "string",
        BaseType::Bool => "boolean",
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
    // serialise
    writeln!(
        out,
        "export function serialise_{name}(value: {name}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  return {{").unwrap();
    for f in &body.fields {
        let fname = &f.name.name;
        let expr = serialise_field_expr(&f.type_ref, &format!("value.{fname}"));
        writeln!(out, "    {fname}: {expr},").unwrap();
    }
    writeln!(out, "  }};").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // deserialise
    writeln!(
        out,
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{name}, BoundaryError> {{"
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
    for f in &body.fields {
        let fname = &f.name.name;
        let access = format!("obj[\"{fname}\"]");
        let sub_path = format!("`${{path}}.{fname}`");
        emit_field_deserialise(out, fname, &f.type_ref, &access, &sub_path);
    }
    write!(out, "  return Ok({{ ").unwrap();
    let parts: Vec<String> = body
        .fields
        .iter()
        .map(|f| format!("{}: __{}", f.name.name, f.name.name))
        .collect();
    write!(out, "{}", parts.join(", ")).unwrap();
    writeln!(out, " }} as {name});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn emit_sum(out: &mut String, name: &str, body: &SumBody) {
    writeln!(
        out,
        "export function serialise_{name}(value: {name}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  switch (value.tag) {{").unwrap();
    for v in &body.variants {
        let vname = &v.name.name;
        if v.payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(out, "      return {{ kind: \"{vname}\" }};").unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            write!(out, "      return {{ kind: \"{vname}\"").unwrap();
            for f in &v.payload {
                let fname = &f.name.name;
                let expr = serialise_field_expr(&f.type_ref, &format!("(value as any).{fname}"));
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
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{name}, BoundaryError> {{"
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
    for v in &body.variants {
        let vname = &v.name.name;
        if v.payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(out, "      return Ok({{ tag: \"{vname}\" }} as {name});").unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            for f in &v.payload {
                let fname = &f.name.name;
                let access = format!("obj[\"{fname}\"]");
                let sub_path = format!("`${{path}}.{fname}`");
                emit_field_deserialise(out, fname, &f.type_ref, &access, &sub_path);
            }
            write!(out, "      return Ok({{ tag: \"{vname}\"").unwrap();
            for f in &v.payload {
                let fname = &f.name.name;
                write!(out, ", {fname}: __{fname}").unwrap();
            }
            writeln!(out, " }} as {name});").unwrap();
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
        TypeRef::Base(b, _) => {
            let typeof_str = match b {
                BaseType::Int => "number",
                BaseType::String => "string",
                BaseType::Bool => "boolean",
            };
            writeln!(out, "  if (typeof {json} !== \"{typeof_str}\") {{").unwrap();
            writeln!(
                out,
                "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"{typeof_str}\", actual: typeof {json} }});"
            )
            .unwrap();
            writeln!(out, "  }}").unwrap();
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
        TypeRef::Effect(_, _) | TypeRef::ValidationError(_) | TypeRef::HttpResult(_, _) => {
            writeln!(out, "  const __{name} = {json} as any;").unwrap();
        }
        TypeRef::Unit(_) => {
            writeln!(out, "  const __{name} = undefined;").unwrap();
        }
    }
}

fn serialise_field_expr(t: &TypeRef, value: &str) -> String {
    match t {
        TypeRef::Base(_, _) => format!("{value} as JsonValue"),
        TypeRef::Named(id) => format!("serialise_{}({value})", id.name),
        TypeRef::Result(a, b, _) => format!(
            "serialise_Result_{}_{}({value})",
            inner_ts_name(a),
            inner_ts_name(b)
        ),
        TypeRef::Option(a, _) => format!("serialise_Option_{}({value})", inner_ts_name(a)),
        TypeRef::Effect(_, _) | TypeRef::ValidationError(_) | TypeRef::HttpResult(_, _) => {
            format!("{value} as JsonValue")
        }
        TypeRef::Unit(_) => "null".to_string(),
    }
}

fn inner_ts_name(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result_{}_{}", inner_ts_name(a), inner_ts_name(b)),
        TypeRef::Option(a, _) => format!("Option_{}", inner_ts_name(a)),
        TypeRef::Effect(a, _) => format!("Effect_{}", inner_ts_name(a)),
        TypeRef::HttpResult(a, _) => format!("HttpResult_{}", inner_ts_name(a)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "Unit".to_string(),
    }
}

/// Collect the set of `Result<A, B>` / `Option<A>` instantiations used in
/// boundary positions so the emitter can synthesise the specialised
/// helpers. v0.18: an instantiation may also appear in the *fields* of a
/// boundary record or sum payload (e.g. the karn surface's
/// `Request.contentType: Option[String]`) — the per-type serialisers
/// delegate to the specialised generic helpers, so walk those too.
pub fn collect_generic_instantiations(
    services: &std::collections::HashMap<String, ServiceDecl>,
    boundary_type_names: &[String],
    types: &std::collections::HashMap<String, TypeDecl>,
) -> Vec<GenericInst> {
    let mut out: Vec<GenericInst> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in services.values() {
        for h in &s.handlers {
            for p in &h.params {
                walk_generic_inst(&p.type_ref, &mut out, &mut seen);
            }
            walk_generic_inst(&h.return_type, &mut out, &mut seen);
        }
    }
    for name in boundary_type_names {
        let Some(decl) = types.get(name) else {
            continue;
        };
        match &decl.body {
            TypeBody::Record(r) => {
                for f in &r.fields {
                    walk_generic_inst(&f.type_ref, &mut out, &mut seen);
                }
            }
            TypeBody::Sum(s) => {
                for v in &s.variants {
                    for p in &v.payload {
                        walk_generic_inst(&p.type_ref, &mut out, &mut seen);
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
    ResultInst { ok: TypeRef, err: TypeRef },
    OptionInst { inner: TypeRef },
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
        }
    }
}

fn walk_generic_inst(
    t: &TypeRef,
    out: &mut Vec<GenericInst>,
    seen: &mut std::collections::HashSet<String>,
) {
    match t {
        TypeRef::Result(a, b, _) => {
            let inst = GenericInst::ResultInst {
                ok: (**a).clone(),
                err: (**b).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(a, out, seen);
            walk_generic_inst(b, out, seen);
        }
        TypeRef::Option(a, _) => {
            let inst = GenericInst::OptionInst {
                inner: (**a).clone(),
            };
            let key = inst.ts_name();
            if seen.insert(key) {
                out.push(inst);
            }
            walk_generic_inst(a, out, seen);
        }
        TypeRef::Effect(a, _) => walk_generic_inst(a, out, seen),
        TypeRef::HttpResult(a, _) => walk_generic_inst(a, out, seen),
        _ => {}
    }
}

/// Emit specialised helpers for each `Result<A, B>` / `Option<A>`
/// instantiation. They delegate to the named-type serialisers for A and B.
pub fn emit_generic_helpers(out: &mut String, insts: &[GenericInst]) {
    for inst in insts {
        match inst {
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
        }
    }
}

fn ts_inner_type(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => match b {
            BaseType::Int => "number".to_string(),
            BaseType::String => "string".to_string(),
            BaseType::Bool => "boolean".to_string(),
        },
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result<{}, {}>", ts_inner_type(a), ts_inner_type(b)),
        TypeRef::Option(a, _) => format!("Option<{}>", ts_inner_type(a)),
        TypeRef::Effect(a, _) => format!("Promise<{}>", ts_inner_type(a)),
        TypeRef::HttpResult(a, _) => format!("HttpResult<{}>", ts_inner_type(a)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}
