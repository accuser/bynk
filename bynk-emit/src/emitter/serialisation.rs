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
use std::sync::Arc;

use bynk_syntax::ast::*;

use crate::emitter::RuntimeUse;
use bynk_check::wire_default::lower_field_default_wire;

/// #661: a *type qualifier* — maps a callee-owned type name to the type-only
/// namespace prefix (`"commerce_payment."`) the caller must use to *name* it,
/// while the caller generates that type's codec **locally** under a bare name.
///
/// A name absent from the map (or mapped to `""`) is named bare: the owner's
/// own module, a base/generic type, or a commons type the caller already
/// declares or imports locally. Only a consumed context's *own* boundary types
/// (`AuthId`, `PaymentError`) are qualified — the caller has no local
/// declaration to name, so the codec's type positions reach through the
/// `import type * as <ns>` alias. Codec *function* names are never qualified:
/// the caller's `deserialise_AuthId` calls its own local `deserialise_*`, which
/// is the whole point of the increment.
type Qual = std::collections::HashMap<String, String>;

/// The namespace prefix for a type name under `qual` (`""` when unqualified).
fn qual_prefix(qual: &Qual, name: &str) -> String {
    qual.get(name).cloned().unwrap_or_default()
}

// #855 (Phase 1): `collect_boundary_types`, `collect_type_names`,
// `recursive_generic_names`, `subst_type_ref`, `record_inst_fields`,
// `sum_inst_variants`, and `app_ts_name` moved to `bynk-check`'s wire IR
// (`bynk_check::wire`) as pure, AST-only walks with no TS emission — see that
// module's doc comment for the seam. Re-exported here under their original
// names (and, for `app_ts_name`, its original signature under an alias to
// its new `inst_codec_suffix` name) so every call site in this file and
// elsewhere in `bynk-emit` keeps compiling unchanged; `recursive_generic_names`,
// `collect_type_names`, and `subst_type_ref` had no callers outside the
// functions that moved with them, so they are not re-exported.
pub(crate) use bynk_check::wire::collect_boundary_types;
pub(crate) use bynk_check::wire::inst_codec_suffix as app_ts_name;
pub(crate) use bynk_check::wire::record_inst_fields;
pub(crate) use bynk_check::wire::sum_inst_variants;

// #855 (Phase 2 step 5): the scalar-codec decision vocabulary — which TS
// branch `emit_refined` takes is now read off a `WireScalar`'s
// `Revalidation` (built via `wire_type`) rather than re-derived inline from
// `qual`/`decl.body`. `json_kind_of` replaces `ts_base_for_serialisation`'s
// *classification*; this file keeps the TS-token spelling (`json_kind_ts`,
// below) per the seam in `wire.rs`'s module doc.
use bynk_check::wire::{
    BaseGuard, JsonKind, Provenance, Revalidation, WireBody, WireField, WireRef, WireSum, WireType,
    WireVariant, wire_ref, wire_type,
};

/// #855 (Phase 2 step 6): resolve one `TypeRef` occurrence to its [`WireRef`]
/// shape for `emit_field_deserialise` / `serialise_field_expr_via`. `wire_ref`
/// documents its `types` parameter as unconsulted (single-level resolution,
/// kept only for signature symmetry with the transitive walks) — an empty
/// table costs nothing (`HashMap::new()` does not allocate) and avoids
/// threading a real one through `emit_record_codec` / `emit_sum_codec` /
/// `emit_generic_helpers_qualified`, which is out of scope for this step
/// (records/sums/generic-helpers are steps 7/8/9).
fn wire_ref_of(t: &TypeRef) -> WireRef {
    wire_ref(t, &std::collections::HashMap::new())
}

/// Emit `serialise_<T>` and `deserialise_<T>` for every named type the
/// owner declares that crosses a boundary. `owner_qualified` is the
/// qualified name used as the brand path so that refinement-violation
/// messages identify the origin context.
pub(crate) fn emit_helpers_for_owner(
    out: &mut String,
    type_names: &[String],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    owner_qualified: &str,
    ru: &RuntimeUse,
) {
    emit_helpers_for_owner_qualified(out, type_names, types, owner_qualified, &Qual::new(), ru);
}

/// #661: as [`emit_helpers_for_owner`], but the caller supplies a type
/// `Qual`. With an empty qualifier this is the owner's own module (every
/// type named bare, refined validation through `.of`). With a non-empty one it
/// is a *consumer* generating its own view of another context's boundary
/// types: the qualified names reach through the `import type * as <ns>` alias,
/// and refined validation inlines (transparent) or casts structurally (opaque)
/// because the owner's `.of` is not importable.
pub(crate) fn emit_helpers_for_owner_qualified(
    out: &mut String,
    type_names: &[String],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    _owner_qualified: &str,
    qual: &Qual,
    ru: &RuntimeUse,
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
        emit_one(out, name, decl, types, qual, ru);
    }
    if emitted_any {
        writeln!(out).unwrap();
    }
}

fn emit_one(
    out: &mut String,
    name: &str,
    decl: &TypeDecl,
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
) {
    match &decl.body {
        TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {
            emit_refined(out, name, decl, types, qual, ru)
        }
        TypeBody::Record(_) => emit_record(out, name, decl, types, qual, ru),
        TypeBody::Sum(_) => emit_sum(out, name, decl, types, qual, ru),
    }
}

/// The TS token a [`JsonKind`] spells as. `json_kind_of` (`bynk_check::wire`)
/// replaces `ts_base_for_serialisation`'s *classification* of a `BaseType`;
/// this is the TS-spelling half the wire.rs seam keeps in `bynk-emit`. Also
/// doubles as the boundary `typeof` check string — for every `BaseType` the
/// two coincide (a bare `Int`/`Float`/`Duration`/`Instant`/`String`/`Bytes`/
/// `Bool` field is validated against exactly the JSON `typeof` its kind
/// implies), so a single call replaces what used to be two identical `match
/// base` blocks.
fn json_kind_ts(k: JsonKind) -> &'static str {
    match k {
        JsonKind::Number => "number",
        JsonKind::String => "string",
        JsonKind::Boolean => "boolean",
        JsonKind::Object => "object",
        JsonKind::Array => "array",
        JsonKind::Null => "null",
    }
}

/// v0.110 (ADR 0142 D5): the codec for a named opaque/refined type over
/// `Bytes` (`type Digest = Bytes`). Unlike the `number`-erased base types, a
/// `Bytes` does not round-trip as itself — it is base64-encoded on serialise
/// and decoded (rejecting a non-string or invalid-base64 wire value) on
/// deserialise. There are no `Bytes` refinement predicates, so there is no
/// `.of` re-validation to thread.
fn emit_bytes_named_codec(out: &mut String, name: &str, qual: &Qual, ru: &RuntimeUse) {
    ru.note_bytes();
    let ty = format!("{}{name}", qual_prefix(qual, name));
    writeln!(
        out,
        "export function serialise_{name}(value: {ty}): JsonValue {{"
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
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{ty}, BoundaryError> {{"
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
    writeln!(out, "  return Ok(__b.value as unknown as {ty});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// #855 (Phase 2 step 5): which of the four TS shapes a named scalar's codec
/// takes — owner `.of`, consumed-opaque structural cast, consumed-transparent
/// inline re-check, or the dedicated `Bytes` base64 codec — is now read off a
/// [`WireScalar`]'s [`Revalidation`], built once via
/// `bynk_check::wire::wire_type` from `decl` + this call's [`Provenance`],
/// instead of re-deriving `consumed`/`consumed_opaque`/`base == Bytes`
/// inline from `qual` + `decl.body` as this function used to. `wire_type`
/// only needs *Owned-vs-Consumed* + opaque-vs-transparent to pick a
/// `Revalidation` — the `owner_unit` string it carries otherwise is not
/// consumed by anything on this path (it exists for the Phase 4 peek), so
/// the qualifier prefix stands in for it here.
fn emit_refined(
    out: &mut String,
    name: &str,
    decl: &TypeDecl,
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
) {
    let qprefix = qual_prefix(qual, name);
    let ty = format!("{qprefix}{name}");
    let prov = if qprefix.is_empty() {
        Provenance::Owned
    } else {
        Provenance::Consumed {
            owner_unit: qprefix.trim_end_matches('.').to_string(),
        }
    };
    let scalar = match wire_type(name, decl, types, prov) {
        Some(WireType {
            body: WireBody::Scalar(s),
            ..
        }) => s,
        _ => unreachable!(
            "emit_refined is only ever called for a non-generic Refined/Opaque declaration"
        ),
    };

    // v0.110: a `Bytes`-based opaque/refined type has a bespoke base64 codec —
    // `emit_refined`'s early return to it, mirrored from `Revalidation` rather
    // than a bare `base == BaseType::Bytes` check.
    if scalar.revalidation == Revalidation::Base64Decode {
        emit_bytes_named_codec(out, name, qual, ru);
        return;
    }

    // #661: a *consumed* type (one the caller qualifies through the callee's
    // type-only namespace) has no importable `.of`, so its deserialiser cannot
    // route validation through the owner's constructor. An **opaque** consumed
    // type casts structurally after the base check (Decision C — its predicate
    // is the owner's secret and is not re-checked, which is sound because the
    // value was produced by the owner's typed code and skew is caught by the
    // v0.177 contract hash). A **transparent refined** consumed type inlines
    // its predicate checks (Decision D — the consumer knows the shape by
    // declaration, so it validates, just not through `.of`).
    let prim = json_kind_ts(scalar.json);
    let typeof_str = prim;
    writeln!(
        out,
        "export function serialise_{name}(value: {ty}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  return value as unknown as {prim};").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "export function deserialise_{name}(json: JsonValue, path: string = \"$\"): Result<{ty}, BoundaryError> {{"
    )
    .unwrap();
    writeln!(out, "  if (typeof json !== \"{typeof_str}\") {{").unwrap();
    writeln!(
        out,
        "    return Err({{ kind: \"StructuralMismatch\", path, expected: \"{typeof_str}\", actual: typeof json }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    match scalar.revalidation {
        Revalidation::StructuralOnly => {
            // Decision C: structural cast only — never reach for the owner's
            // `.of`, which would resurrect the value import this increment
            // removes and leak the opaque predicate into the consumer.
            writeln!(out, "  return Ok(json as unknown as {ty});").unwrap();
        }
        Revalidation::Inline => {
            // Decision D: a transparent refined consumed type validates inline.
            // The base-integrality / finiteness guards and the declared
            // predicates, in the same order the owner's `.of` applies them, but
            // wrapped as this codec's `BoundaryError` rather than a
            // `ValidationError`.
            emit_inline_refinement_checks(out, name, &scalar.base_guards, &scalar.predicates);
            writeln!(out, "  return Ok(json as unknown as {ty});").unwrap();
        }
        Revalidation::ViaConstructor => {
            // Owner's own module: re-validate via the type's own constructor
            // (`.of`), which applies the refinement. If the type has no
            // refinement, `.of` doesn't exist for refined-base types; fall back
            // to a direct cast.
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
        }
        Revalidation::Base64Decode => unreachable!("handled by the early return above"),
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// #661 (Decision D): inline the base-type and refinement checks a consumed
/// **transparent** refined type's deserialiser applies, reporting failures as
/// this codec's `BoundaryError` (`RefinementViolation` wrapping the same
/// `{ field, message, value }` the owner's `.of` would have produced). The
/// `typeof` guard is emitted by the caller; these are the checks that run once
/// the primitive type already matched. `json` is the value being validated.
/// #855 (Phase 2 step 5): driven off a [`WireScalar`]'s `base_guards` +
/// `predicates` (both **declaration order** — see `wire.rs`'s module doc)
/// rather than a `BaseType` + raw `Option<&Refinement>` pair.
fn emit_inline_refinement_checks(
    out: &mut String,
    name: &str,
    base_guards: &[BaseGuard],
    predicates: &[PredKind],
) {
    let violation = |msg: &str| {
        format!(
            "return Err({{ kind: \"RefinementViolation\", path, violation: {{ field: \"{name}\", message: \"{msg}\", value: json }} }});"
        )
    };
    // Base guards mirror `emit_refined_checks`: an `Int` is whole, a `Float`
    // finite. (`Duration`/`Instant` are not exposed as named refined bases.)
    for guard in base_guards {
        match guard {
            BaseGuard::Integral => {
                writeln!(out, "  if (!Number.isInteger(json)) {{").unwrap();
                writeln!(out, "    {}", violation("must be an integer")).unwrap();
                writeln!(out, "  }}").unwrap();
            }
            BaseGuard::Finite => {
                writeln!(out, "  if (!Number.isFinite(json)) {{").unwrap();
                writeln!(out, "    {}", violation("must be a finite number")).unwrap();
                writeln!(out, "  }}").unwrap();
            }
        }
    }
    for pred in predicates {
        emit_inline_pred_check(out, pred, &violation);
    }
}

/// #661 (Decision D): one refinement predicate as an inline `if (!…) return
/// Err(…)`, over the local `json` binding. The messages match
/// `emit::emit_pred_check` so a consumer-side rejection reads identically to an
/// owner-side one; only the error envelope differs (`BoundaryError` here,
/// `ValidationError` there).
fn emit_inline_pred_check(out: &mut String, pred: &PredKind, violation: &dyn Fn(&str) -> String) {
    let (cond, msg) = super::pred_condition_and_message(pred, "json");
    writeln!(out, "  if (!({cond})) {{").unwrap();
    writeln!(out, "    {}", violation(&msg)).unwrap();
    writeln!(out, "  }}").unwrap();
}

/// #855 (Phase 2 step 7): builds the [`WireField`] list via
/// `bynk_check::wire::wire_type` — the same declaration-order shape (field
/// name, [`WireRef`] shape, and raw `(Expr, TypeRef)` default) both the
/// codec and a future peek would derive — rather than re-walking
/// `body.fields` inline. `lower_field_default_wire` (this file) stays the
/// one place a default's *rendered* wire-JSON literal is produced, called
/// from `emit_record_codec` at the same point it always was (Part 1's
/// seam: the IR carries the raw default, the emitter renders it).
fn emit_record(
    out: &mut String,
    name: &str,
    decl: &TypeDecl,
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
) {
    let qprefix = qual_prefix(qual, name);
    let prov = if qprefix.is_empty() {
        Provenance::Owned
    } else {
        Provenance::Consumed {
            owner_unit: qprefix.trim_end_matches('.').to_string(),
        }
    };
    let fields = match wire_type(name, decl, types, prov) {
        Some(WireType {
            body: WireBody::Record { fields },
            ..
        }) => fields,
        _ => {
            unreachable!("emit_record is only ever called for a non-generic Record declaration")
        }
    };
    // #661: a consumed record's TS value type reaches through the type-only
    // namespace (`commerce_payment.Receipt`); the codec function name stays
    // bare and local. Its field codec calls are unqualified too — they resolve
    // to the caller's own locally-generated helpers.
    let ts_type = format!("{qprefix}{name}");
    emit_record_codec(out, name, &ts_type, &fields, types, ru);
}

/// v0.174 (#592): the shared record codec body. `fn_suffix` is the codec name
/// suffix (`Order`, or the monomorphised `Paginated_User`); `ts_type` is the
/// TypeScript value type the codec accepts / returns (`Order`, or the erased
/// generic `Paginated<User>`). The two coincide for a non-generic record and
/// diverge for a generic-record instantiation.
///
/// #855 (Phase 2 step 7): takes `&[WireField]` — the field's shape as a
/// [`WireRef`] (rendered via [`serialise_field_expr_wire`] /
/// [`emit_field_deserialise_wire`], no re-derivation from a raw `TypeRef`)
/// and its default as a raw `(Expr, TypeRef)`, lowered to its wire-JSON
/// literal right here via `lower_field_default_wire` — the same point it was
/// always called from, per Part 1's seam (`bynk-check` carries the boundary
/// fact, `bynk-emit` renders it). Events slice 3a (#972): a generic-record
/// instantiation's fields never carry a default (events are never generic),
/// so `default` is always `None` on that path; only `deserialise_<fn_suffix>`
/// consults it, `serialise_<fn_suffix>` is untouched (Decision B, #972).
fn emit_record_codec(
    out: &mut String,
    fn_suffix: &str,
    ts_type: &str,
    fields: &[WireField],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    ru: &RuntimeUse,
) {
    // serialise
    writeln!(
        out,
        "export function serialise_{fn_suffix}(value: {ts_type}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  return {{").unwrap();
    for field in fields {
        let expr =
            serialise_field_expr_wire(&field.shape, &format!("value.{}", field.name), "", ru);
        writeln!(out, "    {}: {expr},", field.name).unwrap();
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
    for field in fields {
        // Events slice 3a (#972): a defaulted field is read through a
        // pre-validated `__d_<field>` binding instead of the raw
        // `obj["<field>"]` access — `"fname" in obj`, not `!== undefined`,
        // is the only test that distinguishes a genuinely absent wire key
        // from one present with an explicit value (Decision D; this is also
        // what makes `Option[T]`'s two absences fall out with no
        // special-casing — a wire `{"kind":"None"}` already passed the `in`
        // test, so it flows through to a real `None`, untouched by the
        // default). Everything downstream (`emit_field_deserialise_wire`) is
        // unchanged either way.
        let default = field
            .default
            .as_ref()
            .and_then(|(e, t)| lower_field_default_wire(e, t, types).ok());
        let fname = &field.name;
        let access = if let Some(d) = &default {
            writeln!(
                out,
                "  const __d_{fname}: JsonValue = \"{fname}\" in obj ? obj[\"{fname}\"] : {d};"
            )
            .unwrap();
            format!("__d_{fname}")
        } else {
            format!("obj[\"{fname}\"]")
        };
        let sub_path = format!("`${{path}}.{}`", field.path_segment);
        emit_field_deserialise_wire(out, fname, &field.shape, &access, &sub_path, ru);
    }
    write!(out, "  return Ok({{ ").unwrap();
    let parts: Vec<String> = fields
        .iter()
        .map(|field| format!("{0}: __{0}", field.name))
        .collect();
    write!(out, "{}", parts.join(", ")).unwrap();
    writeln!(out, " }} as {ts_type});").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// #855 (Phase 2 step 8): builds the [`WireSum`] via
/// `bynk_check::wire::wire_type` — the same declaration-order variant/payload
/// shape a peek would derive — instead of re-walking `body.variants` inline.
fn emit_sum(
    out: &mut String,
    name: &str,
    decl: &TypeDecl,
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
) {
    // #661: a consumed sum's TS value type is namespace-qualified; the codec
    // function name and its per-variant codec calls stay bare and local. #593:
    // the codec body is the shared `emit_sum_codec` (also reused, unqualified,
    // for a generic-sum instantiation), so the qualified value type is threaded
    // in as its `ts_type`.
    let qprefix = qual_prefix(qual, name);
    let prov = if qprefix.is_empty() {
        Provenance::Owned
    } else {
        Provenance::Consumed {
            owner_unit: qprefix.trim_end_matches('.').to_string(),
        }
    };
    let sum = match wire_type(name, decl, types, prov) {
        Some(WireType {
            body: WireBody::Sum(s),
            ..
        }) => s,
        _ => unreachable!("emit_sum is only ever called for a non-generic Sum declaration"),
    };
    let ty = format!("{qprefix}{name}");
    emit_sum_codec(out, name, &ty, &sum, ru);
}

/// The serialise/deserialise pair for a sum type, over an already-resolved
/// [`WireSum`]. `fn_suffix` names the emitted functions (`Opt` / `Opt_Int`),
/// `ts_type` is their value type (`Opt` / `Opt<number>` / a namespace-qualified
/// `shop.Opt`). #593: a generic-sum instantiation reuses this with substituted
/// payload types, exactly as a generic record reuses [`emit_record_codec`].
///
/// #855 (Phase 2 step 8): the wire discriminant (`kind`) and in-memory
/// discriminant (`tag`) are read off `sum.wire_discriminant` /
/// `sum.memory_discriminant` rather than hard-coded string literals — the
/// same two values as before (`wire.rs`'s module doc: `memory_discriminant`
/// is the softest part of the seam, carried beside `wire_discriminant`
/// because this codec's whole job is translating between them), so this is
/// purely reading the fact from the IR, not a behaviour change.
fn emit_sum_codec(
    out: &mut String,
    fn_suffix: &str,
    ts_type: &str,
    sum: &WireSum,
    ru: &RuntimeUse,
) {
    let kind = sum.wire_discriminant;
    let tag = sum.memory_discriminant;
    writeln!(
        out,
        "export function serialise_{fn_suffix}(value: {ts_type}): JsonValue {{"
    )
    .unwrap();
    writeln!(out, "  switch (value.{tag}) {{").unwrap();
    for variant in &sum.variants {
        let vname = &variant.name;
        if variant.payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(out, "      return {{ {kind}: \"{vname}\" }};").unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            write!(out, "      return {{ {kind}: \"{vname}\"").unwrap();
            for field in &variant.payload {
                let expr = serialise_field_expr_wire(
                    &field.shape,
                    &format!("(value as any).{}", field.name),
                    "",
                    ru,
                );
                write!(out, ", {}: {expr}", field.name).unwrap();
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
    writeln!(out, "  const {kind} = obj[\"{kind}\"];").unwrap();
    writeln!(out, "  switch ({kind}) {{").unwrap();
    for variant in &sum.variants {
        let vname = &variant.name;
        if variant.payload.is_empty() {
            writeln!(out, "    case \"{vname}\":").unwrap();
            writeln!(
                out,
                "      return Ok({{ {tag}: \"{vname}\" }} as {ts_type});"
            )
            .unwrap();
        } else {
            writeln!(out, "    case \"{vname}\": {{").unwrap();
            for field in &variant.payload {
                let access = format!("obj[\"{}\"]", field.name);
                let sub_path = format!("`${{path}}.{}`", field.path_segment);
                emit_field_deserialise_wire(out, &field.name, &field.shape, &access, &sub_path, ru);
            }
            write!(out, "      return Ok({{ {tag}: \"{vname}\"").unwrap();
            for field in &variant.payload {
                write!(out, ", {0}: __{0}", field.name).unwrap();
            }
            writeln!(out, " }} as {ts_type});").unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "    default:").unwrap();
    writeln!(
        out,
        "      return Err({{ kind: \"StructuralMismatch\", path, expected: \"sum variant kind\", actual: String({kind}) }});"
    )
    .unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

/// Emit a let binding `__<field>` after destructuring & validating a
/// nested field.
///
/// #855 (Phase 2 step 6): dispatches on [`WireRef`] (via [`wire_ref_of`])
/// rather than matching `TypeRef` directly — the same resolution
/// `wire_ref` documents itself as mirroring one-for-one, **including** its
/// `Effect`/`HttpResult`/etc. field-position arm folding into the same
/// unchecked cast as `ValidationError`/`JsonError`/`QueueResult`. This is
/// the deserialise-side function `wire_ref`'s own doc names as the one it
/// agrees with exactly; contrast [`serialise_field_expr_via`], which does
/// not.
///
/// #855 (Phase 2 step 7): a thin `TypeRef` → [`WireRef`] wrapper over
/// [`emit_field_deserialise_wire`] — a record/sum field built from the IR
/// already carries its [`WireRef`] shape (`WireField::shape`) and calls that
/// directly, with no `TypeRef` to convert back from.
fn emit_field_deserialise(
    out: &mut String,
    name: &str,
    t: &TypeRef,
    json: &str,
    path_expr: &str,
    ru: &RuntimeUse,
) {
    emit_field_deserialise_wire(out, name, &wire_ref_of(t), json, path_expr, ru);
}

/// The [`WireRef`]-driven body [`emit_field_deserialise`] delegates to,
/// exposed directly for a caller that already holds a [`WireRef`] (a
/// [`WireField`]'s `shape`) rather than the `TypeRef` it was resolved from.
fn emit_field_deserialise_wire(
    out: &mut String,
    name: &str,
    wire: &WireRef,
    json: &str,
    path_expr: &str,
    ru: &RuntimeUse,
) {
    match wire {
        // v0.110 (ADR 0142 D5): a bare `Bytes` field is a base64 JSON string —
        // require a string, then decode (rejecting invalid base64), binding the
        // decoded `Uint8Array`. This is the one base type whose wire value is
        // not a direct cast of its erased representation.
        WireRef::Bytes => {
            ru.note_bytes();
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
        WireRef::Base {
            json: kind, guards, ..
        } => {
            let typeof_str = json_kind_ts(*kind);
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
            // v0.21: boundary `Float` values are finite (ADR 0040) —
            // `JSON.parse("1e999")` yields `Infinity`, which must not be
            // admitted from the wire.
            for guard in guards {
                match guard {
                    BaseGuard::Integral => {
                        writeln!(out, "  if (!Number.isInteger({json})) {{").unwrap();
                        writeln!(
                            out,
                            "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"integer\", actual: String({json}) }});"
                        )
                        .unwrap();
                        writeln!(out, "  }}").unwrap();
                    }
                    BaseGuard::Finite => {
                        writeln!(out, "  if (!Number.isFinite({json})) {{").unwrap();
                        writeln!(
                            out,
                            "    return Err({{ kind: \"StructuralMismatch\", path: {path_expr}, expected: \"finite number\", actual: String({json}) }});"
                        )
                        .unwrap();
                        writeln!(out, "  }}").unwrap();
                    }
                }
            }
            writeln!(out, "  const __{name} = {json};").unwrap();
        }
        // Named type (own module or generic instantiation, both keyed the
        // same way — `Named` for a declared type, `Inst` for `Result` /
        // `Option` / `List` / `Map` / a generic `App`): defer to its own
        // `deserialise_<key>`. Assumes it exists in scope (imported or
        // declared locally).
        WireRef::Named { name: type_name } => {
            writeln!(
                out,
                "  const __r_{name} = deserialise_{type_name}({json}, {path_expr});"
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        WireRef::Inst { key } => {
            writeln!(
                out,
                "  const __r_{name} = deserialise_{key}({json}, {path_expr});"
            )
            .unwrap();
            writeln!(out, "  if (__r_{name}.tag === \"Err\") return __r_{name};").unwrap();
            writeln!(out, "  const __{name} = __r_{name}.value;").unwrap();
        }
        WireRef::Unit => {
            writeln!(out, "  const __{name} = undefined;").unwrap();
        }
        // The runtime-owned error family, plus a stray field-position
        // `Effect` (see this function's doc): no generated codec to name, so
        // the value is cast through unchecked.
        WireRef::Unchecked { .. } => {
            writeln!(out, "  const __{name} = {json} as any;").unwrap();
        }
    }
}

fn serialise_field_expr(t: &TypeRef, value: &str, ru: &RuntimeUse) -> String {
    serialise_field_expr_via(t, value, "", ru)
}

/// The same dispatch, reaching its helpers through `ns` — `""` for a
/// module-local call, `"handlers."` from a Worker entry point that imports the
/// context's handlers as a namespace. Threading the prefix (rather than each
/// caller owning a parallel dispatch) is what keeps the boundary to **one**
/// codec path.
///
/// #855 (Phase 2 step 6): dispatches on [`WireRef`] (via [`wire_ref_of`]),
/// **except** `Effect`, which this function peels itself before consulting
/// the IR. `wire_ref`'s own doc names this exact asymmetry: a field-position
/// `Effect` is `Unchecked` under `wire_ref` (matching
/// [`emit_field_deserialise`], the resolver's one faithful consumer), but
/// this function has always *recursed* into the wrapped type instead — an
/// `Effect`-typed field serialises as its payload's codec, not as an opaque
/// cast. Routing `Effect` through `wire_ref` here would silently change that
/// to an unchecked cast, so it stays a manual peel rather than becoming a
/// second `WireRef` arm with no second consumer.
fn serialise_field_expr_via(t: &TypeRef, value: &str, ns: &str, ru: &RuntimeUse) -> String {
    if let TypeRef::Effect(inner, _) = t {
        return serialise_field_expr_via(inner, value, ns, ru);
    }
    serialise_field_expr_wire(&wire_ref_of(t), value, ns, ru)
}

/// The [`WireRef`]-driven body [`serialise_field_expr_via`] delegates to
/// (after its manual `Effect` peel — see that function's doc), exposed
/// directly for a caller that already holds a [`WireRef`] (a [`WireField`]'s
/// `shape`) rather than the `TypeRef` it was resolved from. #855 (Phase 2
/// step 7): a record/sum field's shape can never legally be a field-position
/// `Effect` (non-storable, non-boundary — rejected before it could reach a
/// declared field), so calling this directly for such a field, skipping the
/// `Effect` peel above, is not a second disagreement with `wire_ref` — it is
/// the same dispatch on a shape the peel could never have matched anyway.
fn serialise_field_expr_wire(wire: &WireRef, value: &str, ns: &str, ru: &RuntimeUse) -> String {
    match wire {
        // Named type or generic instantiation (`Result`/`Option`/`List`/`Map`/
        // a generic `App` all key the same way — see `wire_ref`'s doc):
        // serialise through its own `serialise_<key>`.
        WireRef::Named { name } => format!("{ns}serialise_{name}({value})"),
        WireRef::Inst { key } => format!("{ns}serialise_{key}({value})"),
        // v0.21: serialising a non-finite `Float` is a contract violation
        // (`JSON.stringify(NaN)` would silently produce `null`); the guard is
        // a self-contained IIFE so the module needs no extra runtime import.
        WireRef::Base {
            base: BaseType::Float,
            ..
        } => format!(
            "((v: number) => {{ if (!Number.isFinite(v)) throw new Error(\"non-finite Float at boundary\"); return v as JsonValue; }})({value})"
        ),
        // v0.110 (ADR 0142 D5): a `Bytes` is base64-encoded on the wire — the
        // one base type whose serialise is an encode, not a bare cast.
        WireRef::Bytes => {
            ru.note_bytes();
            format!("__bynkBytesToBase64({value}) as JsonValue")
        }
        WireRef::Base { .. } => format!("{value} as JsonValue"),
        // The runtime-owned error types have no *generated* codec — they are
        // declared by the runtime, not by a `TypeDecl` this emitter can walk, so
        // there is no `serialise_ValidationError` to name. They keep the
        // pass-through the whole boundary used before this increment; unifying
        // the user-type paths does not reach them. Their JSON shape is fixed by
        // the runtime (`errors.ts`), so the cast is not *wrong* — it is simply
        // unchecked, and it is the one remaining unchecked arm at the boundary.
        // (`Effect` never lands here via `serialise_field_expr_via` — it is
        // peeled above; a record/sum field can never legally be `Effect` in
        // the first place — see this function's doc.)
        WireRef::Unchecked { .. } => {
            format!("{value} as JsonValue")
        }
        WireRef::Unit => "null".to_string(),
    }
}

// #855 (Phase 1): `inner_ts_name` moved to `bynk_check::wire` as
// `codec_suffix`; `collect_codec_closure` moved verbatim (pure, AST-only —
// see that module's doc for the seam). Re-exported under their original
// names so call sites here and elsewhere in `bynk-emit` keep compiling
// unchanged.
pub(crate) use bynk_check::wire::codec_suffix as inner_ts_name;
pub(crate) use bynk_check::wire::collect_codec_closure;

/// v0.22b: an expression-form serialise for a codec target — the same
/// dispatch as a record field's serialisation.
pub(crate) fn serialise_expr(t: &TypeRef, value: &str, ru: &RuntimeUse) -> String {
    serialise_field_expr(t, value, ru)
}

/// v0.176 (#642): the one serialise dispatch for the workers cross-context
/// boundary, reaching helpers through `ns`. Replaces the two parallel
/// dispatches this boundary used to carry — `emit.rs`'s `workers_serialise_expr`
/// (which dropped `List`/`Map` to a bare `as JsonValue` cast) and
/// `workers_entry.rs`'s `serialise_call` (which did the same to `Bytes`, the
/// asymmetry that forced `bynk.types.bytes_at_workers_boundary`).
pub(crate) fn serialise_expr_via(t: &TypeRef, value: &str, ns: &str, ru: &RuntimeUse) -> String {
    serialise_field_expr_via(t, value, ns, ru)
}

/// v0.176 (#642): a deserialise **reference** for `ns`, shaped to
/// `callService`'s `deserialiseResult` parameter. The inline arms become a
/// lambda rather than the unvalidated `((j: any) => ({ tag: "Ok", value: j }))`
/// identity the caller path used to fall back to.
pub(crate) fn deserialise_ref_via(t: &TypeRef, ns: &str, ru: &RuntimeUse) -> String {
    match strip_effect(t) {
        TypeRef::Named(id) => format!("{ns}deserialise_{}", id.name),
        t @ (TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        | TypeRef::App { .. }) => format!("{ns}deserialise_{}", inner_ts_name(t)),
        other => format!(
            "(__j: JsonValue) => {}",
            deserialise_expr_via(other, "__j", "$", ns, ru)
        ),
    }
}

/// v0.176 (#642) follow-up: a serialise **reference** for `ns`, shaped to
/// `httpResultToResponse`'s serialiser parameter — the mirror image of
/// `deserialise_ref_via` above. Replaces `workers_entry.rs`'s
/// `http_value_serialiser`, a parallel dispatch that collapsed every base type
/// to `(v: any) => v as JsonValue`, dropping the `Float` non-finite guard and
/// `Bytes` base64 encoding that `serialise_field_expr_via` already carries.
pub(crate) fn serialise_ref_via(t: &TypeRef, ns: &str, ru: &RuntimeUse) -> String {
    match strip_effect(t) {
        TypeRef::Named(id) => format!("{ns}serialise_{}", id.name),
        t @ (TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        | TypeRef::App { .. }) => format!("{ns}serialise_{}", inner_ts_name(t)),
        other => format!(
            "(__v: any) => {}",
            serialise_field_expr_via(other, "__v", ns, ru)
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
pub(crate) fn deserialise_expr(t: &TypeRef, json: &str, path: &str, ru: &RuntimeUse) -> String {
    deserialise_expr_via(t, json, path, "", ru)
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
pub(crate) fn deserialise_expr_via(
    t: &TypeRef,
    json: &str,
    path: &str,
    ns: &str,
    ru: &RuntimeUse,
) -> String {
    // Every arm except the delegating ones — which call a `deserialise_<T>` in the
    // module's own namespace — builds `Ok(…)` / `Err(… as BoundaryError)` inline.
    // Recorded once here rather than per-arm: the delegating set is short and
    // closed, the inlining set is long, and erring the other way emits a module
    // that references an unimported name (#914). `Effect` recurses, so it lets the
    // inner type decide.
    if !matches!(
        t,
        TypeRef::Named(_)
            | TypeRef::Result(..)
            | TypeRef::Option(..)
            | TypeRef::List(..)
            | TypeRef::Map(..)
            | TypeRef::App { .. }
            | TypeRef::Effect(..)
    ) {
        ru.note_boundary_codec();
    }
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
        TypeRef::Effect(inner, _) => deserialise_expr_via(inner, json, path, ns, ru),
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
            ru.note_bytes();
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

// #855 (Phase 1): `collect_generic_instantiations` and `GenericInst` (now
// `WireInst` — see that type's doc for why its variant names are kept as
// `ResultInst`/`OptionInst`/… rather than the plan's bare spelling, deferred
// to Phase 2) moved to `bynk_check::wire`; `walk_generic_inst` moved with
// them (no callers outside the functions that moved). Re-exported under
// their original names so call sites here and elsewhere in `bynk-emit` keep
// compiling unchanged.
pub(crate) use bynk_check::wire::WireInst as GenericInst;
pub(crate) use bynk_check::wire::collect_generic_instantiations;

/// Emit specialised helpers for each `Result<A, B>` / `Option<A>`
/// instantiation. They delegate to the named-type serialisers for A and B.
/// v0.174 (#592): also emits a monomorphised record codec per generic
/// instantiation (`RecordInst`), which needs the declarations to substitute
/// its type parameters.
pub(crate) fn emit_generic_helpers(
    out: &mut String,
    insts: &[GenericInst],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    ru: &RuntimeUse,
) {
    emit_generic_helpers_qualified(out, insts, types, &Qual::new(), ru);
}

/// #661: as [`emit_generic_helpers`], but the value-type positions of each
/// specialised helper are named through the type `Qual` — so a consumer's
/// `deserialise_Result_AuthId_PaymentError` returns
/// `Result<commerce_payment.AuthId, commerce_payment.PaymentError>` while its
/// codec calls stay local. The codec *suffix* (`Result_AuthId_PaymentError`) is
/// namespace-independent by construction, which is exactly what keeps the
/// caller's and callee's names in agreement across the wire.
pub(crate) fn emit_generic_helpers_qualified(
    out: &mut String,
    insts: &[GenericInst],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
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
                    "{}{}<{}>",
                    qual_prefix(qual, name),
                    name,
                    args.iter()
                        .map(|a| ts_inner_type(a, qual))
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
                // Events slice 3a (#972): a generic record instantiation
                // never carries a field default — events are never generic
                // (`parse_event_decl` always builds zero type params), so
                // this path can never reach one. #855 (Phase 2 step 9): a
                // `WireField` for a substituted instantiation type is built
                // straight from `record_inst_fields`' resolved `TypeRef`s via
                // `wire_ref_of`, mirroring what `wire_type`/`wire_fields`
                // would derive for a non-generic declaration's own fields.
                let fields: Vec<WireField> = fields
                    .into_iter()
                    .map(|(n, t)| WireField {
                        shape: wire_ref_of(&t),
                        path_segment: n.clone(),
                        name: n,
                        default: None,
                    })
                    .collect();
                emit_record_codec(out, &fn_suffix, &ts_type, &fields, types, ru);
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
                        .map(|a| ts_inner_type(a, qual))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let variants = sum_inst_variants(name, args, types).unwrap_or_else(|| {
                    unreachable!("SumInst `{name}` is not a resolved generic sum")
                });
                // #855 (Phase 2 step 9): a `WireSum` for a substituted
                // instantiation is built straight from `sum_inst_variants`'
                // resolved `TypeRef`s via `wire_ref_of`, mirroring what
                // `wire_type`/`wire_sum` would derive for a non-generic
                // declaration's own variants. The wire/memory discriminants
                // are the same fixed `"kind"`/`"tag"` pair `wire_sum` uses.
                let sum = WireSum {
                    wire_discriminant: "kind",
                    memory_discriminant: "tag",
                    variants: variants
                        .into_iter()
                        .map(|(vname, payload)| WireVariant {
                            name: vname,
                            payload: payload
                                .into_iter()
                                .map(|(fname, t)| WireField {
                                    shape: wire_ref_of(&t),
                                    path_segment: fname.clone(),
                                    name: fname,
                                    default: None,
                                })
                                .collect(),
                        })
                        .collect(),
                };
                emit_sum_codec(out, &fn_suffix, &ts_type, &sum, ru);
            }
            GenericInst::ResultInst { ok, err } => {
                let ok_ts = inner_ts_name(ok);
                let err_ts = inner_ts_name(err);
                let ok_inner = ts_inner_type(ok, qual);
                let err_inner = ts_inner_type(err, qual);
                let serialise_ok = serialise_field_expr(ok, "value.value", ru);
                let serialise_err = serialise_field_expr(err, "value.error", ru);
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
                emit_field_deserialise(out, "v", ok, "obj[\"value\"]", "`${path}.value`", ru);
                writeln!(
                    out,
                    "    return Ok(Ok(__v) as Result<{ok_inner}, {err_inner}>);"
                )
                .unwrap();
                writeln!(out, "  }} else if (obj[\"kind\"] === \"Err\") {{").unwrap();
                emit_field_deserialise(out, "e", err, "obj[\"error\"]", "`${path}.error`", ru);
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
                let inner_ty = ts_inner_type(inner, qual);
                let serialise_inner = serialise_field_expr(inner, "value.value", ru);
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
                emit_field_deserialise(out, "v", inner, "obj[\"value\"]", "`${path}.value`", ru);
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
                let elem_ty = ts_inner_type(elem, qual);
                let serialise_elem = serialise_field_expr(elem, "v", ru);
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
                emit_field_deserialise(out, "el", elem, "item", "`${path}[${i}]`", ru);
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
                let key_ty = ts_inner_type(key, qual);
                let val_ty = ts_inner_type(val, qual);
                let serialise_key = serialise_field_expr(key, "k", ru);
                let serialise_val = serialise_field_expr(val, "v", ru);
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
                emit_field_deserialise(out, "k", key, "entryK", "`${path}[${i}][0]`", ru);
                emit_field_deserialise(out, "v", val, "entryV", "`${path}[${i}][1]`", ru);
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

/// #917: the qualified TS type renderer, exposed for the `Json.decode[T]`
/// wrapper — a test-scaffold module has no local declaration of the target
/// type, so its `Result<T, JsonError>` signature and `as T` cast must reach
/// it through the same type-only namespace (`qual`) the module's own
/// caller-generated codec helpers use. `qual` is empty on every other emission
/// path, where this renders identically to a bare type.
pub(crate) fn ts_type_ref_qualified(
    t: &TypeRef,
    qual: &std::collections::HashMap<String, String>,
) -> String {
    ts_inner_type(t, qual)
}

fn ts_inner_type(t: &TypeRef, qual: &Qual) -> String {
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
        // #661: a consumed generic record and its callee-owned arguments are
        // namespace-qualified.
        TypeRef::App { name, args, .. } => format!(
            "{}{}<{}>",
            qual_prefix(qual, &name.name),
            name.name,
            args.iter()
                .map(|a| ts_inner_type(a, qual))
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
        // #661: a callee-owned named type reaches through the type-only
        // namespace; everything the caller already declares maps to `""`.
        TypeRef::Named(id) => format!("{}{}", qual_prefix(qual, &id.name), id.name),
        TypeRef::Result(a, b, _) => format!(
            "Result<{}, {}>",
            ts_inner_type(a, qual),
            ts_inner_type(b, qual)
        ),
        TypeRef::Option(a, _) => format!("Option<{}>", ts_inner_type(a, qual)),
        TypeRef::Effect(a, _) => format!("Promise<{}>", ts_inner_type(a, qual)),
        TypeRef::HttpResult(a, _) => format!("HttpResult<{}>", ts_inner_type(a, qual)),
        TypeRef::List(a, _) => format!("readonly {}[]", ts_inner_type(a, qual)),
        TypeRef::Map(k, v, _) => {
            format!(
                "ReadonlyMap<{}, {}>",
                ts_inner_type(k, qual),
                ts_inner_type(v, qual)
            )
        }
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "void".to_string(),
    }
}

#[cfg(test)]
mod default_lowering_tests {
    use super::*;
    use std::collections::HashMap;

    /// Parses `src` as a single-file commons and returns its `types` table
    /// plus the field list of the `event`/`type` decl named `subject`, so
    /// each test can feed a real, parsed `(init, type_ref)` pair into
    /// [`lower_field_default_wire`] rather than hand-building AST nodes.
    fn parse_fields(
        src: &str,
        subject: &str,
    ) -> (HashMap<String, Arc<TypeDecl>>, Vec<RecordField>) {
        let tokens = bynk_syntax::lexer::tokenize(src).expect("tokenize");
        let unit = bynk_syntax::parser::parse_unit(&tokens, src).expect("parse");
        let items: Vec<CommonsItem> = match unit {
            bynk_syntax::ast::SourceUnit::Context(ctx) => ctx.items,
            bynk_syntax::ast::SourceUnit::Commons(commons) => commons.items,
            _ => panic!("expected a context or commons unit"),
        };
        let mut types: HashMap<String, Arc<TypeDecl>> = HashMap::new();
        let mut fields = None;
        for item in &items {
            match item {
                CommonsItem::Type(t) => {
                    types.insert(t.name.name.clone(), Arc::new(t.clone()));
                    if t.name.name == subject
                        && let TypeBody::Record(r) = &t.body
                    {
                        fields = Some(r.fields.clone());
                    }
                }
                CommonsItem::Event(e) => {
                    if e.name.name == subject {
                        fields = Some(e.body.fields.clone());
                    }
                    types.insert(e.name.name.clone(), Arc::new(e.as_type_decl()));
                }
                _ => {}
            }
        }
        (
            types,
            fields.unwrap_or_else(|| panic!("no decl named `{subject}`")),
        )
    }

    fn default_of<'a>(fields: &'a [RecordField], name: &str) -> (&'a Expr, &'a TypeRef) {
        let f = fields
            .iter()
            .find(|f| f.name.name == name)
            .unwrap_or_else(|| panic!("no field `{name}`"));
        (
            f.init
                .as_ref()
                .unwrap_or_else(|| panic!("field `{name}` has no default")),
            &f.type_ref,
        )
    }

    #[test]
    fn base_literals_lower_to_their_raw_wire_form() {
        let src = r#"
context test

event E = {
  a: Int = 5,
  b: Int = -5,
  c: String = "hi",
  d: Bool = true,
  e: Float = 1.5,
  f: Duration = 5.minutes,
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let cases = [
            ("a", "5"),
            ("b", "-5"),
            ("c", "\"hi\""),
            ("d", "true"),
            ("e", "1.5"),
            ("f", "300000"),
        ];
        for (name, expected) in cases {
            let (init, ty) = default_of(&fields, name);
            assert_eq!(
                lower_field_default_wire(init, ty, &types),
                Ok(expected.to_string()),
                "field `{name}`"
            );
        }
    }

    #[test]
    fn sum_variant_defaults_lower_to_a_bare_kind_object_not_a_qualified_reference() {
        let src = r#"
context test

type Region = enum { Domestic, International }

event E = {
  bare: Region = Domestic,
  qualified: Region = Region.Domestic,
}
"#;
        let (types, fields) = parse_fields(src, "E");
        for name in ["bare", "qualified"] {
            let (init, ty) = default_of(&fields, name);
            let got = lower_field_default_wire(init, ty, &types).unwrap();
            assert_eq!(got, "{ kind: \"Domestic\" }", "field `{name}`");
            assert!(
                !got.contains('.'),
                "field `{name}` must not contain a qualified reference: {got}"
            );
        }
    }

    #[test]
    fn payload_variant_default_recurses_into_declared_field_types() {
        let src = r#"
context test

type Outcome = | Won(prize: Int) | Lost

event E = {
  o: Outcome = Won(100),
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "o");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("{ kind: \"Won\", prize: 100 }".to_string())
        );
    }

    #[test]
    fn qualified_payload_variant_call_lowers_the_same_as_the_bare_call() {
        // Regression: `Outcome.Won(100)` parses to `ExprKind::MethodCall`
        // (confirmed by direct AST inspection), not `ConstructorCall` — a
        // match against `ConstructorCall` alone silently fell through to
        // "expected a variant" for this qualified spelling.
        let src = r#"
context test

type Outcome = | Won(prize: Int) | Lost

event E = {
  o: Outcome = Outcome.Won(100),
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "o");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("{ kind: \"Won\", prize: 100 }".to_string())
        );
    }

    #[test]
    fn opaque_unsafe_default_lowers_to_the_raw_literal() {
        // Regression: `OrderId.unsafe("x")` also parses to `MethodCall`, the
        // same shape as the qualified-variant case above.
        let src = r#"
context test

type OrderId = opaque String where MinLength(1)

event E = {
  id: OrderId = OrderId.unsafe("abc"),
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "id");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("\"abc\"".to_string())
        );
    }

    #[test]
    fn option_and_result_defaults_use_the_wire_kind_discriminant() {
        let src = r#"
context test

event E = {
  a: Option[Int] = Some(1),
  b: Option[Int] = None,
  c: Result[Int, String] = Ok(1),
  d: Result[Int, String] = Err("nope"),
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let cases = [
            ("a", "{ kind: \"Some\", value: 1 }"),
            ("b", "{ kind: \"None\" }"),
            ("c", "{ kind: \"Ok\", value: 1 }"),
            ("d", "{ kind: \"Err\", error: \"nope\" }"),
        ];
        for (name, expected) in cases {
            let (init, ty) = default_of(&fields, name);
            assert_eq!(
                lower_field_default_wire(init, ty, &types),
                Ok(expected.to_string()),
                "field `{name}`"
            );
        }
    }

    #[test]
    fn record_literal_default_lowers_to_a_plain_object() {
        let src = r#"
context test

type Region = enum { Domestic, International }
type Meta = { region: Region, note: String }

event E = {
  m: Meta = Meta { region: Region.Domestic, note: "x" },
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "m");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("{ region: { kind: \"Domestic\" }, note: \"x\" }".to_string())
        );
    }

    #[test]
    fn list_literal_default_recurses_per_element() {
        let src = r#"
context test

event E = {
  xs: List[Int] = [1, 2, 3],
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "xs");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("[1, 2, 3]".to_string())
        );
    }

    #[test]
    fn mismatched_shapes_return_err_not_panic() {
        let src = r#"
context test

type Region = enum { Domestic, International }

event E = {
  a: Int = "wrong",
  b: Region = International,
}
"#;
        let (types, fields) = parse_fields(src, "E");
        let (init, ty) = default_of(&fields, "a");
        assert!(lower_field_default_wire(init, ty, &types).is_err());
        // Sanity: a *valid* shape for the same sum still succeeds, proving
        // the harness itself is sound.
        let (init, ty) = default_of(&fields, "b");
        assert_eq!(
            lower_field_default_wire(init, ty, &types),
            Ok("{ kind: \"International\" }".to_string())
        );
    }
}
