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

use bynk_syntax::ast::{BaseType, PredKind, TypeBody, TypeDecl, TypeRef};

use crate::emitter::RuntimeUse;
use bynk_check::wire_default::lower_field_default_wire;
use bynk_ts::{
    TsArrowBody, TsBinaryOp, TsBindingName, TsDecl, TsExpr, TsLit, TsObjectEntry, TsParam, TsStmt,
    TsType, TsUnaryOp,
};

// #1435 (Arc E slice 1): this file's own local `TsExpr` builder set, the
// same "compose the real node algebra for this file's own repeated shapes"
// convention `workers.rs`/`workers_entry.rs` already established (#1321,
// #1323) — not part of the public node algebra, `bynk-ts` still owns every
// real constructor. Kept as this file's own private set rather than shared
// cross-file, matching this track's own established per-file scoping.

fn ident(s: impl Into<String>) -> TsExpr {
    TsExpr::Ident(s.into())
}

fn str_lit(s: impl Into<String>) -> TsExpr {
    TsExpr::Lit(TsLit::Str(s.into()))
}

fn member(object: TsExpr, property: impl Into<String>) -> TsExpr {
    TsExpr::Member {
        object: Box::new(object),
        property: property.into(),
    }
}

fn call(callee: TsExpr, args: Vec<TsExpr>) -> TsExpr {
    TsExpr::Call {
        callee: Box::new(callee),
        args,
    }
}

fn as_expr(expr: TsExpr, ty: TsType) -> TsExpr {
    TsExpr::As {
        expr: Box::new(expr),
        ty,
    }
}

// #1439 (Arc E slice 3): the statement-builder half of this file's own
// local node-builder set — slices 1/2 (#1436/#1438) only ever needed the
// `TsExpr` builders above (every prior conversion in this file returned one
// expression); this slice's `emit_field_deserialise_wire` returns a real
// `Vec<TsStmt>` per `WireRef` arm, so it needs the `TsStmt` half too. Same
// naming/shape convention `workers.rs`/`workers_entry.rs` already
// established for their own statement-builder sets (#1321/#1323).

fn not_expr(expr: TsExpr) -> TsExpr {
    TsExpr::Unary {
        op: TsUnaryOp::Not,
        expr: Box::new(expr),
    }
}

fn typeof_expr(expr: TsExpr) -> TsExpr {
    TsExpr::Unary {
        op: TsUnaryOp::Typeof,
        expr: Box::new(expr),
    }
}

fn binary(op: TsBinaryOp, left: TsExpr, right: TsExpr) -> TsExpr {
    TsExpr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn strict_eq(left: TsExpr, right: TsExpr) -> TsExpr {
    binary(TsBinaryOp::StrictEq, left, right)
}

fn strict_neq(left: TsExpr, right: TsExpr) -> TsExpr {
    binary(TsBinaryOp::StrictNotEq, left, right)
}

fn const_(name: impl Into<String>, init: TsExpr) -> TsStmt {
    TsStmt::const_stmt(TsBindingName::Ident(name.into()), None, init, None)
}

fn return_(expr: TsExpr) -> TsStmt {
    TsStmt::return_stmt(Some(expr), None)
}

fn if_(cond: TsExpr, then_branch: TsStmt) -> TsStmt {
    TsStmt::if_stmt(cond, then_branch, None)
}

fn block(stmts: Vec<TsStmt>) -> TsStmt {
    TsStmt::block(stmts, None)
}

/// `Err({ kind: "StructuralMismatch", path: <path_expr>, expected:
/// <expected>, actual: <actual> })` — the one wire-guard failure shape
/// every real arm of [`emit_field_deserialise_wire`] returns. Distinct from
/// `deserialise_expr_via`'s own `structural_mismatch` closure further down
/// this file in two ways that matter: `path` here is spliced in as an
/// already-rendered TS expression (`ident(path_expr)`), not a string
/// literal (`str_lit`) — this function's own `path_expr` parameter is
/// itself TS source text (a bare `path` identifier, or a template literal
/// like `` `${path}.value` ``), never a bare string to quote; and the
/// object is never cast `as BoundaryError` the way that closure's is (the
/// pre-conversion `writeln!` output this replaces never added one either —
/// only `deserialise_expr_via`'s own callers need the cast, since they
/// build a whole `Result<T, BoundaryError>` return value directly).
fn err_structural_mismatch(path_expr: &str, expected: &str, actual: TsExpr) -> TsExpr {
    call(
        ident("Err"),
        vec![TsExpr::object(vec![
            ("kind".to_string(), str_lit("StructuralMismatch")),
            ("path".to_string(), ident(path_expr)),
            ("expected".to_string(), str_lit(expected)),
            ("actual".to_string(), actual),
        ])],
    )
}

/// `Err({ kind: "StructuralMismatch", path, expected: <expected>, actual:
/// <actual> })` — [`emit_bytes_named_codec`]/[`emit_refined`]'s own
/// top-level boundary-guard failure shape (#1441, Arc E slice 4).
///
/// Deliberately a *second*, distinct helper rather than a call to
/// [`err_structural_mismatch`] above: that one's own `path_expr` parameter
/// is real call-site-supplied TS source text (a bare `path` identifier or a
/// template literal like `` `${path}.value` ``), rendered as an ordinary
/// `path: <path_expr>` property because every one of its real callers
/// (`emit_field_deserialise_wire`'s own field-level guards) passes a
/// rendered *sub*-path template, never the bare top-level `path` parameter
/// itself — confirmed empirically (no fixture anywhere renders the literal
/// text `path: path`). Every real call site *here*, by contrast, validates
/// a function's own top-level `path` parameter directly, which TypeScript's
/// object-literal shorthand prints as bare `path,` — confirmed byte-for-byte
/// against `bynkc/tests/fixtures/positive/254_multi_file_commons_workers_
/// codec/expected/money/cents.ts:26` (`emit_refined`'s own pre-conversion
/// text). [`TsExpr::object`]'s convenience constructor only builds
/// [`TsObjectEntry::Prop`] entries, which would print `path: path` instead
/// — a different, wrong byte sequence — so this builds the mixed
/// `Prop`/[`TsObjectEntry::Shorthand`] entry list directly.
fn err_structural_mismatch_top(expected: &str, actual: TsExpr) -> TsExpr {
    call(
        ident("Err"),
        // Review of #1442, finding 3: `TsExpr::object_entries` already
        // builds exactly this (a single-line object from a
        // `Vec<TsObjectEntry>`) — `TsExpr::object`'s own doc only rules out
        // the *other* convenience constructor, which takes `(String,
        // TsExpr)` pairs and can't represent `Shorthand`.
        vec![TsExpr::object_entries(vec![
            TsObjectEntry::Prop("kind".to_string(), str_lit("StructuralMismatch")),
            TsObjectEntry::Shorthand("path".to_string()),
            TsObjectEntry::Prop("expected".to_string(), str_lit(expected)),
            TsObjectEntry::Prop("actual".to_string(), actual),
        ])],
    )
}

/// Boundary-print — splices a real `Vec<bynk_ts::TsStmt>` into a still-
/// `write!`-based caller's own `out: &mut String` accumulator, one
/// `bynk_ts::print_stmt(&stmt, 1)` per statement. `depth: 1` is the
/// one-level indent (`"  "`) every real call site's own pre-conversion
/// `writeln!` calls used — confirmed against a real fixture's
/// before/after diff (#1439), not assumed, the same "verify, don't
/// assume" discipline #1437's own qualifier-prefix convention needed.
/// All 8 real call sites (`emit_record_codec`, `emit_sum_codec`,
/// `emit_generic_helpers_qualified`) sit at exactly this depth: a
/// generated function's own top-level statement list.
fn splice_stmts(out: &mut String, stmts: Vec<TsStmt>) {
    // Review of #1440: every other boundary-print site in `bynk-emit`
    // spells this `push_str`, not `write!` — matches that convention (an
    // infallible append needs no `.unwrap()`, and doesn't itself add a
    // `write!`-family line to the very `ts_writes` probe this slice drives
    // down).
    for stmt in &stmts {
        out.push_str(&bynk_ts::print_stmt(stmt, 1));
    }
}

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

/// #1437 (Arc E slice 2): [`crate::emitter::ts_type_ref_qualified_multi_ts_type`]
/// as a real `TsType`, over this file's own dotted-prefix [`Qual`] convention
/// — a real, empirically-found convention mismatch, not assumed compatible
/// from the two functions' identical `HashMap<String, String>` shapes alone.
/// `Qual`'s own values already carry the trailing separator (`"shop_payment."`,
/// this type's own doc), concatenated directly by [`qual_prefix`]; the
/// existing renderer's own qualify closure (`emitter.rs`'s `ts_type_ref_to_ts_type`,
/// `TypeRef::Named`'s arm) instead appends its OWN `.` (`format!("{ns}.{}",
/// id.name)`), so passing `Qual` through unmodified doubles the separator
/// (`shop_payment..PayError`) — caught by `coverage_behaviour.rs`'s real `tsc
/// --strict` run (`TS1003: Identifier expected`), not by the byte-golden
/// fixture corpus, which has no consumed-context boundary type reaching this
/// exact generic-instantiation path. An absent/empty `Qual` entry (this
/// type's own "named bare" case) maps to `None` here, not `Some("")` — the
/// latter would render a stray leading `.` the same way a non-empty value's
/// doubled separator does. `pub(crate)`, not private: `emitter/lower.rs`'s
/// own `Json.decode[T]` test-scaffold arm is this file's one external
/// caller, replacing its own direct call to the now-deleted
/// `ts_type_ref_qualified`.
pub(crate) fn qualified_ts_type(t: &TypeRef, qual: &Qual) -> bynk_ts::TsType {
    // Review of #1438: the deleted `ts_inner_type` carried an explicit
    // `unreachable!()` for these five variants (`bynk.types.
    // function_at_boundary` rejects them before the serialisation machinery
    // ever sees one); `ts_type_ref_qualified_multi_ts_type` has no such arm
    // (it also serves non-boundary positions, where all five are legal), so
    // folding into it silently turned a loud panic into a plausible-looking
    // annotation next to a `serialise_*`/`deserialise_*` helper that cannot
    // codec it. Unreachable today, made loud rather than silently relied on
    // — the same precedent this crate's own unreachable-invariant
    // `debug_assert!`s already set (e.g. the empty-args `TypeRef::App` one
    // in `emitter.rs`).
    debug_assert!(
        !matches!(
            t,
            TypeRef::Fn(..)
                | TypeRef::Query(..)
                | TypeRef::Stream(..)
                | TypeRef::Connection(..)
                | TypeRef::History(..)
        ),
        "function/query/stream types are rejected at boundaries"
    );
    let bare: Qual = qual
        .iter()
        .filter_map(|(k, v)| {
            let ns = v.trim_end_matches('.');
            (!ns.is_empty()).then(|| (k.clone(), ns.to_string()))
        })
        .collect();
    crate::emitter::ts_type_ref_qualified_multi_ts_type(t, &bare)
}

#[cfg(test)]
mod qualified_ts_type_tests {
    use super::*;
    use bynk_syntax::ast::Ident;
    use bynk_syntax::span::Span;

    fn named(name: &str) -> TypeRef {
        TypeRef::Named(Ident {
            name: name.to_string(),
            span: Span::new(0, 0),
        })
    }

    /// Review of #1438, finding 2: the exact regression this slice
    /// introduced and fixed (`shop_payment..PayError`, a doubled
    /// separator) — pinned directly, not left to depend on a real `tsc`
    /// run in `bynkc/tests/coverage_behaviour.rs`.
    #[test]
    fn dotted_qual_entry_renders_a_single_separator() {
        let qual: Qual = [("PayError".to_string(), "shop_payment.".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            bynk_ts::print_type(&qualified_ts_type(&named("PayError"), &qual)),
            "shop_payment.PayError"
        );
    }

    #[test]
    fn absent_qual_entry_renders_bare() {
        let qual: Qual = Qual::new();
        assert_eq!(
            bynk_ts::print_type(&qualified_ts_type(&named("PayError"), &qual)),
            "PayError"
        );
    }

    /// An empty-string entry is `Qual`'s own "named bare" convention (this
    /// type's own doc) — must map to `None`, not `Some("")`, or it renders
    /// a stray leading `.`.
    #[test]
    fn empty_qual_entry_renders_bare_not_a_stray_dot() {
        let qual: Qual = [("PayError".to_string(), String::new())]
            .into_iter()
            .collect();
        assert_eq!(
            bynk_ts::print_type(&qualified_ts_type(&named("PayError"), &qual)),
            "PayError"
        );
    }
}

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
    BaseGuard, JsonKind, Provenance, Revalidation, UncheckedReason, WireBody, WireField, WireRef,
    WireSum, WireType, WireVariant, wire_ref, wire_type,
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
        // #1441 (Arc E slice 4): `emit_refined` itself now builds real
        // `bynk_ts::TsDecl` nodes (no `out: &mut String` parameter) — this
        // call site is the one place that still needs `out`, so it prints
        // each returned declaration at the boundary. `TsStmt::decl(..,
        // None)` + `bynk_ts::print_stmt(&stmt, 0)` is the established
        // pattern for printing one top-level `TsDecl` on its own (no
        // dedicated `print_decl` entry point exists or is needed — this
        // exact idiom is already `emit_free_fn`'s/`emit_agent`'s own real
        // precedent, `emitter/emit.rs`/`emitter.rs`), followed by the same
        // blank-line separator (`writeln!(out).unwrap()`) the
        // pre-conversion `writeln!` calls always emitted after each
        // function.
        TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {
            for d in emit_refined(name, decl, types, qual, ru) {
                out.push_str(&bynk_ts::print_stmt(&TsStmt::decl(d, None), 0));
                writeln!(out).unwrap();
            }
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

/// `path: string = "$"` — the one JS default-valued parameter anywhere in
/// this crate's generated output (#1441, Arc E slice 4). [`TsParam`] carries
/// no `default: Option<TsExpr>` field (its own doc names only `optional` —
/// "nothing in the grounding file needs a default value"), and adding one
/// would touch every one of the ~120 existing `TsParam { .. }` literal
/// construction sites across `bynk-emit`/`bynk-ts` for the sake of this
/// single real occurrence — wildly disproportionate to a 4-function slice.
/// `name` is already an established "raw-text slot" for a case bigger than
/// a bare identifier: [`bynk_ts::TsDecl::Import`]'s own `names` field docs
/// the identical move for a `"type KVNamespace"` specifier. Splicing the
/// whole clause in as one opaque `name` (with `ty: None`, so
/// `render_params` appends nothing further) reuses that precedent rather
/// than inventing a new one.
fn deserialise_path_param() -> TsParam {
    TsParam {
        name: "path: string = \"$\"".to_string(),
        ty: None,
        optional: false,
    }
}

/// v0.110 (ADR 0142 D5): the codec for a named opaque/refined type over
/// `Bytes` (`type Digest = Bytes`). Unlike the `number`-erased base types, a
/// `Bytes` does not round-trip as itself — it is base64-encoded on serialise
/// and decoded (rejecting a non-string or invalid-base64 wire value) on
/// deserialise. There are no `Bytes` refinement predicates, so there is no
/// `.of` re-validation to thread.
///
/// #1441 (Arc E slice 4): returns the `[serialise, deserialise]` pair as
/// real `bynk_ts::TsDecl` nodes instead of writing into an `out: &mut
/// String` — `emit_refined`'s own early return threads them straight
/// through unchanged (its only real caller, confirmed by grep), and
/// `emit_one` prints both at the boundary. A `(TsDecl, TsDecl)` pair would
/// say the same thing but `Vec<TsDecl>` is what the caller (`emit_one`,
/// looping to print+blank-line each declaration) actually wants to iterate,
/// and matches `emit_refined`'s own identical return shape below exactly —
/// one shape for both halves of this slice's call tree, not two.
fn emit_bytes_named_codec(name: &str, qual: &Qual, ru: &RuntimeUse) -> Vec<TsDecl> {
    ru.note_bytes();
    let ty = format!("{}{name}", qual_prefix(qual, name));

    let serialise = TsDecl::Export(Box::new(TsDecl::Function {
        name: format!("serialise_{name}"),
        generics: Vec::new(),
        params: vec![TsParam {
            name: "value".to_string(),
            ty: Some(TsType::named(ty.clone())),
            optional: false,
        }],
        return_type: Some(TsType::named("JsonValue")),
        body: vec![return_(call(
            ident("__bynkBytesToBase64"),
            vec![as_expr(
                as_expr(ident("value"), TsType::named("unknown")),
                TsType::named("Uint8Array"),
            )],
        ))],
        is_async: false,
        inline: false,
    }));

    let deserialise = TsDecl::Export(Box::new(TsDecl::Function {
        name: format!("deserialise_{name}"),
        generics: Vec::new(),
        params: vec![
            TsParam {
                name: "json".to_string(),
                ty: Some(TsType::named("JsonValue")),
                optional: false,
            },
            deserialise_path_param(),
        ],
        return_type: Some(TsType::named(format!("Result<{ty}, BoundaryError>"))),
        body: vec![
            if_(
                strict_neq(typeof_expr(ident("json")), str_lit("string")),
                block(vec![return_(err_structural_mismatch_top(
                    "base64 string",
                    typeof_expr(ident("json")),
                ))]),
            ),
            const_(
                "__b",
                call(ident("__bynkBytesFromBase64"), vec![ident("json")]),
            ),
            if_(
                strict_eq(member(ident("__b"), "tag"), str_lit("None")),
                block(vec![return_(err_structural_mismatch_top(
                    "base64 string",
                    str_lit("invalid base64"),
                ))]),
            ),
            return_(call(
                ident("Ok"),
                vec![as_expr(
                    as_expr(member(ident("__b"), "value"), TsType::named("unknown")),
                    TsType::named(ty.clone()),
                )],
            )),
        ],
        is_async: false,
        inline: false,
    }));

    vec![serialise, deserialise]
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
///
/// #1441 (Arc E slice 4): returns the `[serialise, deserialise]` pair as
/// real `bynk_ts::TsDecl` nodes — see [`emit_bytes_named_codec`]'s own doc
/// for why `Vec<TsDecl>`, not a `(TsDecl, TsDecl)` pair, and why the two
/// share one return shape. The `Revalidation::ViaConstructor` arm's runtime
/// `.of` probe (below) stays one opaque [`TsStmt::raw`] block: its inline
/// object type (`{ of?: (json: unknown) => Result<unknown,
/// ValidationError> }`) needs a *named* function-type parameter (`json`),
/// but [`bynk_ts::TsType::Fn`]'s own doc is explicit that it only ever
/// numbers parameters positionally (`a0`, `a1`, …) — real TS grammar
/// requires *some* name, but not this exact one, so building it as a real
/// `TsType::Fn` would print `(a0: unknown) => ...` and break zero-diff. The
/// ternary itself is multi-line (`bynk_ts::TsExpr::Conditional`'s own
/// printer always renders `test ? consequent : alternate` on one line, no
/// multiline form). Both are real, checked gaps in the existing algebra —
/// not a default to opaque without checking — and closing either is
/// disproportionate to this one call site, so the whole `const validated =
/// …;` assignment stays hand-formatted text, the same "genuinely out of
/// scope" precedent `workers.rs`'s own `claim_predicate_to_js` call already
/// set (#1321). The `if (validated.tag === "Err") { … }
/// return Ok(validated.value as {name});` tail that follows has no such
/// gap, so it builds real nodes like everything else in this function.
fn emit_refined(
    name: &str,
    decl: &TypeDecl,
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    qual: &Qual,
    ru: &RuntimeUse,
) -> Vec<TsDecl> {
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
        return emit_bytes_named_codec(name, qual, ru);
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

    let serialise = TsDecl::Export(Box::new(TsDecl::Function {
        name: format!("serialise_{name}"),
        generics: Vec::new(),
        params: vec![TsParam {
            name: "value".to_string(),
            ty: Some(TsType::named(ty.clone())),
            optional: false,
        }],
        return_type: Some(TsType::named("JsonValue")),
        body: vec![return_(as_expr(
            as_expr(ident("value"), TsType::named("unknown")),
            TsType::named(prim),
        ))],
        is_async: false,
        inline: false,
    }));

    let mut body = vec![if_(
        strict_neq(typeof_expr(ident("json")), str_lit(typeof_str)),
        block(vec![return_(err_structural_mismatch_top(
            typeof_str,
            typeof_expr(ident("json")),
        ))]),
    )];
    match scalar.revalidation {
        Revalidation::StructuralOnly => {
            // Decision C: structural cast only — never reach for the owner's
            // `.of`, which would resurrect the value import this increment
            // removes and leak the opaque predicate into the consumer.
            body.push(return_(call(
                ident("Ok"),
                vec![as_expr(
                    as_expr(ident("json"), TsType::named("unknown")),
                    TsType::named(ty.clone()),
                )],
            )));
        }
        Revalidation::Inline => {
            // Decision D: a transparent refined consumed type validates inline.
            // The base-integrality / finiteness guards and the declared
            // predicates, in the same order the owner's `.of` applies them, but
            // wrapped as this codec's `BoundaryError` rather than a
            // `ValidationError`.
            body.extend(emit_inline_refinement_checks(
                name,
                &scalar.base_guards,
                &scalar.predicates,
            ));
            body.push(return_(call(
                ident("Ok"),
                vec![as_expr(
                    as_expr(ident("json"), TsType::named("unknown")),
                    TsType::named(ty.clone()),
                )],
            )));
        }
        Revalidation::ViaConstructor => {
            // Owner's own module: re-validate via the type's own constructor
            // (`.of`), which applies the refinement. If the type has no
            // refinement, `.of` doesn't exist for refined-base types; fall back
            // to a direct cast.
            // P7.2: `name`'s declared type doesn't statically know about a
            // conditional `.of` constructor (that's the whole reason this probes
            // for it at runtime) — a marker type stating exactly the shape this
            // code actually depends on, not a blanket escape. `.error`'s real
            // type is `ValidationError` (a real, exported runtime interface,
            // `runtime/src/errors.ts`, already referenced by name elsewhere in
            // this file) — a first attempt typed it `unknown` and broke real
            // `tsc --strict` fixtures against this function's own declared
            // `violation: ValidationError` return shape. The `else` branch is
            // annotated to match the same `Result<{name}, ValidationError>`
            // union so both ternary arms unify to one type instead of one
            // arm's `Ok(...)` inferring a narrower `Result<{name}, never>`.
            //
            // This whole statement stays one opaque `TsStmt::raw` block — see
            // this function's own doc for why (a real, checked gap in both
            // `TsType::Fn`'s named-parameter support and
            // `TsExpr::Conditional`'s single-line-only printer, not a default
            // to opaque). Baked-in indent matches this body's own one-level
            // depth exactly, the same contract `print_stmt`'s own doc holds
            // every `Raw` statement to.
            body.push(TsStmt::raw(
                format!(
                    "  const validated = (typeof ({name} as unknown as {{ of?: (json: unknown) => Result<unknown, ValidationError> }}).of === \"function\")\n    ? ({name} as unknown as {{ of: (json: unknown) => Result<unknown, ValidationError> }}).of(json)\n    : (Ok(json as unknown as {name}) as Result<unknown, ValidationError>);\n"
                ),
                None,
            ));
            body.push(if_(
                strict_eq(member(ident("validated"), "tag"), str_lit("Err")),
                block(vec![return_(call(
                    ident("Err"),
                    vec![TsExpr::object_entries(vec![
                        TsObjectEntry::Prop("kind".to_string(), str_lit("RefinementViolation")),
                        TsObjectEntry::Shorthand("path".to_string()),
                        TsObjectEntry::Prop(
                            "violation".to_string(),
                            member(ident("validated"), "error"),
                        ),
                    ])],
                ))]),
            ));
            body.push(return_(call(
                ident("Ok"),
                vec![as_expr(
                    member(ident("validated"), "value"),
                    TsType::named(name.to_string()),
                )],
            )));
        }
        Revalidation::Base64Decode => unreachable!("handled by the early return above"),
    }

    let deserialise = TsDecl::Export(Box::new(TsDecl::Function {
        name: format!("deserialise_{name}"),
        generics: Vec::new(),
        params: vec![
            TsParam {
                name: "json".to_string(),
                ty: Some(TsType::named("JsonValue")),
                optional: false,
            },
            deserialise_path_param(),
        ],
        return_type: Some(TsType::named(format!("Result<{ty}, BoundaryError>"))),
        body,
        is_async: false,
        inline: false,
    }));

    vec![serialise, deserialise]
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
///
/// #1441 (Arc E slice 4): returns a real `Vec<bynk_ts::TsStmt>` — the
/// statement-builder shape #1439 already established for
/// `emit_field_deserialise_wire` — feeding straight into `emit_refined`'s
/// own `Revalidation::Inline` arm, rather than writing into an `out: &mut
/// String`. `violation` returns a `TsStmt` (a `return Err(...)` statement)
/// instead of the pre-conversion `String`, for the same reason: both real
/// call sites (the base-guard loop below, `emit_inline_pred_check`'s own
/// loop) place it directly inside a real `block(vec![...])`.
fn emit_inline_refinement_checks(
    name: &str,
    base_guards: &[BaseGuard],
    predicates: &[PredKind],
) -> Vec<TsStmt> {
    // Review of #1442, finding 1: `msg` reaches here two ways — a static,
    // hand-written literal from the base-guard arms below (never needs
    // escaping) and, via `emit_inline_pred_check`, `pred_condition_and_
    // message`'s own `PredKind::Matches` message, which is ALREADY run
    // through `escape_ts_string` (that function's own doc names both
    // callers' shared "splice unescaped" contract). `str_lit`'s own
    // `TsLit::Str` re-escapes `\`/`"` unconditionally, so passing an
    // already-escaped `Matches` message through it doubled every backslash
    // (`must match /\d{4}/` → `must match /\\d{4}/`) — a real, silent
    // output change no fixture caught (no fixture reaches this predicate
    // through the inline-consumer path with a backslash/quote in its
    // pattern). `TsLit::Raw` (pre-quoted, no further escaping) matches
    // `pred_condition_and_message`'s own contract exactly, and is a no-op
    // for the base-guard arms' own escaping-free text.
    let violation = |msg: &str| -> TsStmt {
        return_(call(
            ident("Err"),
            vec![TsExpr::object_entries(vec![
                TsObjectEntry::Prop("kind".to_string(), str_lit("RefinementViolation")),
                TsObjectEntry::Shorthand("path".to_string()),
                TsObjectEntry::Prop(
                    "violation".to_string(),
                    TsExpr::object(vec![
                        ("field".to_string(), str_lit(name)),
                        (
                            "message".to_string(),
                            TsExpr::Lit(TsLit::Raw(format!("\"{msg}\""))),
                        ),
                        ("value".to_string(), ident("json")),
                    ]),
                ),
            ])],
        ))
    };
    // Base guards mirror `emit_refined_checks`: an `Int` is whole, a `Float`
    // finite. (`Duration`/`Instant` are not exposed as named refined bases.)
    let mut stmts = Vec::new();
    for guard in base_guards {
        match guard {
            BaseGuard::Integral => stmts.push(if_(
                not_expr(call(
                    member(ident("Number"), "isInteger"),
                    vec![ident("json")],
                )),
                block(vec![violation("must be an integer")]),
            )),
            BaseGuard::Finite => stmts.push(if_(
                not_expr(call(
                    member(ident("Number"), "isFinite"),
                    vec![ident("json")],
                )),
                block(vec![violation("must be a finite number")]),
            )),
        }
    }
    for pred in predicates {
        stmts.extend(emit_inline_pred_check(pred, &violation));
    }
    stmts
}

/// #661 (Decision D): one refinement predicate as an inline `if (!…) return
/// Err(…)`, over the local `json` binding. The messages match
/// `emit::emit_pred_check` so a consumer-side rejection reads identically to an
/// owner-side one; only the error envelope differs (`BoundaryError` here,
/// `ValidationError` there).
///
/// #1441 (Arc E slice 4): returns `Vec<TsStmt>` (one `if`), same treatment
/// as [`emit_inline_refinement_checks`] above. `super::pred_condition_and_
/// message`'s own `cond` stays opaque text spliced through [`ident`] — it
/// is `emitter.rs`'s `String`-returning `pred_condition_and_message`,
/// confirmed by Arc B/P7.9's own proposal as genuinely out of that slice's
/// scope, the same "cross-file `String`-producing dependency genuinely out
/// of scope stays opaque" precedent `workers.rs`'s own `claim_predicate_
/// to_js` call already set (#1321). Wrapped in an explicit
/// [`TsExpr::Paren`] before negating — `cond` is often a compound boolean
/// expression (e.g. `value.length >= 3 && value.length <= 10`), and unlike
/// `Binary`/`As`, a bare `TsExpr::Ident` is never parenthesised by the
/// printer's own unary-operand precedence logic (it has no way to know the
/// opaque text it holds isn't atomic) — the original `if (!({cond}))`
/// always wrapped explicitly for exactly this reason, and dropping the
/// parens would both change the emitted bytes and, for a compound `cond`,
/// the actual runtime meaning.
fn emit_inline_pred_check(pred: &PredKind, violation: &dyn Fn(&str) -> TsStmt) -> Vec<TsStmt> {
    let (cond, msg) = super::pred_condition_and_message(pred, "json");
    vec![if_(
        not_expr(TsExpr::Paren(Box::new(ident(cond)))),
        block(vec![violation(&msg)]),
    )]
}

/// Review of #1442, finding 1/2: direct coverage for
/// [`emit_inline_refinement_checks`]/[`emit_inline_pred_check`], the two
/// branches no fixture exercises. `matches_predicate_with_a_backslash_is_
/// not_double_escaped` pins the actual regression (a `Matches` message
/// already run through `escape_ts_string` was silently re-escaped by
/// `str_lit`'s own `TsLit::Str`); `base_guard_finite_renders` closes the
/// coverage gap the review named alongside it (`Number.isFinite(json)`
/// appears in zero fixtures, unlike its `Integral` sibling).
#[cfg(test)]
mod emit_inline_refinement_checks_tests {
    use super::*;

    fn render(stmts: Vec<TsStmt>) -> String {
        stmts
            .iter()
            .map(|s| bynk_ts::print_stmt(s, 1))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn matches_predicate_with_a_backslash_is_not_double_escaped() {
        // Pattern `\d{4}` (one real backslash) — `escape_ts_string` escapes
        // it to a valid JS string literal, `\\d{4}` (two real backslash
        // characters, correct and final); the bug re-escaped that already-
        // escaped text through `TsLit::Str`, doubling it again to `\\\\d{4}`
        // (four).
        let stmts =
            emit_inline_refinement_checks("name", &[], &[PredKind::Matches(r"\d{4}".to_string())]);
        let out = render(stmts);
        assert!(
            out.contains(r"must match /\\d{4}/"),
            "expected exactly one escaping round in the message, got: {out}"
        );
        assert!(
            !out.contains(r"must match /\\\\d{4}/"),
            "message was double-escaped: {out}"
        );
    }

    #[test]
    fn base_guard_finite_renders() {
        let stmts = emit_inline_refinement_checks("name", &[BaseGuard::Finite], &[]);
        assert_eq!(
            render(stmts),
            "  if (!Number.isFinite(json)) {\n    return Err({ kind: \"RefinementViolation\", path, violation: { field: \"name\", message: \"must be a finite number\", value: json } });\n  }\n"
        );
    }
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
        // #1435 (Arc E slice 1): `serialise_field_expr_wire` now returns a
        // real `bynk_ts::TsExpr` — this caller stays `write!`-based (a
        // different cluster of this same file, out of this slice's scope),
        // so it prints at the boundary, the same "opaque consumer needs its
        // own print" treatment `lower.rs`/`tests_emit.rs`'s own call sites
        // use.
        let expr = bynk_ts::print_expr(&serialise_field_expr_wire(
            &field.shape,
            &format!("value.{}", field.name),
            "",
            ru,
        ));
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
        // #1439 (Arc E slice 3): boundary-print — `emit_field_deserialise_wire`
        // now returns a real `Vec<bynk_ts::TsStmt>`, spliced in via
        // `splice_stmts` at this function's own one-level indent.
        splice_stmts(
            out,
            emit_field_deserialise_wire(fname, &field.shape, &access, &sub_path, ru),
        );
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
                // P7.2: deferred, not narrowed. A first attempt used
                // `Record<string, unknown>` here and broke a real `tsc --strict`
                // fixture (180/188/etc., the `Float` boundary guard): the
                // extracted field's value flows into `serialise_field_expr_wire`'s
                // own shape-specific helpers (e.g. a `(v: number) => ...`
                // finiteness check for `Float`), which need the field's real
                // per-shape type, not a blanket `unknown` — correctly narrowing
                // needs deriving that type from `field.shape` itself, not a
                // generic record cast.
                // #1435 (Arc E slice 1): boundary-print, same treatment as
                // `emit_record_codec`'s own identical call site above.
                let expr = bynk_ts::print_expr(&serialise_field_expr_wire(
                    &field.shape,
                    &format!("(value as any).{}", field.name),
                    "",
                    ru,
                ));
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
                // #1439 (Arc E slice 3): boundary-print, same treatment as
                // `emit_record_codec`'s own identical call site above.
                splice_stmts(
                    out,
                    emit_field_deserialise_wire(&field.name, &field.shape, &access, &sub_path, ru),
                );
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
///
/// #1439 (Arc E slice 3): returns the same `Vec<bynk_ts::TsStmt>`
/// [`emit_field_deserialise_wire`] now does, rather than writing into an
/// `out: &mut String` — this function's only job is resolving `t` to a
/// [`WireRef`] before delegating, so it carries the signature change with
/// zero body change beyond the return.
fn emit_field_deserialise(
    name: &str,
    t: &TypeRef,
    json: &str,
    path_expr: &str,
    ru: &RuntimeUse,
) -> Vec<TsStmt> {
    emit_field_deserialise_wire(name, &wire_ref_of(t), json, path_expr, ru)
}

/// The [`WireRef`]-driven body [`emit_field_deserialise`] delegates to,
/// exposed directly for a caller that already holds a [`WireRef`] (a
/// [`WireField`]'s `shape`) rather than the `TypeRef` it was resolved from.
///
/// #1439 (Arc E slice 3): returns a real `Vec<bynk_ts::TsStmt>` — a
/// *sequence* of statements (one or more guard `if`s, then a `const`
/// binding), not one expression, unlike slices 1/2's own `-> TsExpr`
/// conversions (#1436/#1438) — hence `Vec`, not a single node. All 8 real
/// call sites (`emit_record_codec`, `emit_sum_codec`,
/// `emit_generic_helpers_qualified`) are still `write!`-based, so each
/// splices the result back in via [`splice_stmts`] at the same one-level
/// indent the original `writeln!` calls always used, confirmed
/// byte-identical against the fixture corpus.
fn emit_field_deserialise_wire(
    name: &str,
    wire: &WireRef,
    json: &str,
    path_expr: &str,
    ru: &RuntimeUse,
) -> Vec<TsStmt> {
    match wire {
        // v0.110 (ADR 0142 D5): a bare `Bytes` field is a base64 JSON string —
        // require a string, then decode (rejecting invalid base64), binding the
        // decoded `Uint8Array`. This is the one base type whose wire value is
        // not a direct cast of its erased representation.
        WireRef::Bytes => {
            ru.note_bytes();
            vec![
                if_(
                    strict_neq(typeof_expr(ident(json)), str_lit("string")),
                    block(vec![return_(err_structural_mismatch(
                        path_expr,
                        "base64 string",
                        typeof_expr(ident(json)),
                    ))]),
                ),
                const_(
                    format!("__b_{name}"),
                    call(ident("__bynkBytesFromBase64"), vec![ident(json)]),
                ),
                if_(
                    strict_eq(member(ident(format!("__b_{name}")), "tag"), str_lit("None")),
                    block(vec![return_(err_structural_mismatch(
                        path_expr,
                        "base64 string",
                        str_lit("invalid base64"),
                    ))]),
                ),
                const_(
                    format!("__{name}"),
                    member(ident(format!("__b_{name}")), "value"),
                ),
            ]
        }
        WireRef::Base {
            json: kind, guards, ..
        } => {
            let typeof_str = json_kind_ts(*kind);
            let mut stmts = vec![if_(
                strict_neq(typeof_expr(ident(json)), str_lit(typeof_str)),
                block(vec![return_(err_structural_mismatch(
                    path_expr,
                    typeof_str,
                    typeof_expr(ident(json)),
                ))]),
            )];
            // v0.22b: bare `Int` fields validate integrality (ADR 0049) —
            // with `Float` in the language there is no excuse for a
            // fractional `Int` from the wire. v0.90 (ADR 0114 D7): an `Instant`
            // is whole epoch milliseconds, so it validates integrality too.
            // v0.21: boundary `Float` values are finite (ADR 0040) —
            // `JSON.parse("1e999")` yields `Infinity`, which must not be
            // admitted from the wire.
            for guard in guards {
                let (method, expected) = match guard {
                    BaseGuard::Integral => ("isInteger", "integer"),
                    BaseGuard::Finite => ("isFinite", "finite number"),
                };
                stmts.push(if_(
                    not_expr(call(member(ident("Number"), method), vec![ident(json)])),
                    block(vec![return_(err_structural_mismatch(
                        path_expr,
                        expected,
                        call(ident("String"), vec![ident(json)]),
                    ))]),
                ));
            }
            stmts.push(const_(format!("__{name}"), ident(json)));
            stmts
        }
        // Named type (own module or generic instantiation, both keyed the
        // same way — `Named` for a declared type, `Inst` for `Result` /
        // `Option` / `List` / `Map` / a generic `App`): defer to its own
        // `deserialise_<key>`. Assumes it exists in scope (imported or
        // declared locally).
        // Review of #1440: `Named`/`Inst` differ only in which callee they
        // name (`type_name`/`key`, both a bare `String` — `bynk-check/src/
        // wire.rs`'s own `WireRef` fields) — collapsed via an or-pattern
        // rather than kept as two ~identical arms, the same "one shape, one
        // arm" discipline this tree's own exhaustiveness rules already
        // enforce for the printer side.
        WireRef::Named { name: type_name } | WireRef::Inst { key: type_name } => vec![
            const_(
                format!("__r_{name}"),
                call(
                    ident(format!("deserialise_{type_name}")),
                    vec![ident(json), ident(path_expr)],
                ),
            ),
            if_(
                strict_eq(member(ident(format!("__r_{name}")), "tag"), str_lit("Err")),
                return_(ident(format!("__r_{name}"))),
            ),
            const_(
                format!("__{name}"),
                member(ident(format!("__r_{name}")), "value"),
            ),
        ],
        WireRef::Unit => vec![const_(format!("__{name}"), ident("undefined"))],
        // The runtime-owned error family, plus a stray field-position
        // `Effect` (see this function's doc): no generated codec to name, so
        // the value is cast through unchecked.
        //
        // #1319: closes the `ts_any` residual for the four runtime-owned
        // error types — each now casts through its own real, exported
        // runtime type (`bynk-emit/runtime/src/errors.ts`/`queue.ts`/
        // `http.ts`), gated into the module's own import list the same way
        // `JsonError`/`HttpResult` already were (`write_header`'s
        // `file_mentions_json_error`/`file_mentions_http_result`, now joined
        // by `file_mentions_queue_result`; `ValidationError` is imported
        // unconditionally). `Effect` stays `any`: a stray field-position
        // `Effect[T]` genuinely has no shape to derive one from (this
        // function's own doc explains why it reaches here at all), and
        // narrowing it is not part of this residual — `unknown` would
        // compile but silently drop the fact that it's still open, which is
        // worse than leaving `any` visible.
        //
        // #1439 (Arc E slice 3): the cast is now a real `TsExpr::As` node
        // (nested `As`-under-`As` for the four bridged-through-`unknown`
        // reasons, mirroring `serialise_field_expr_wire`'s own
        // `WireRef::Unchecked` arm's identical shape) rather than a raw
        // `"as any"`/`"as unknown as X"` string glued onto `{json} {cast}`.
        // Per the accepted proposal's own explicit note: this moves the
        // *construction* of the `Effect` arm's `any` from a string to a
        // real node, but does NOT close the `ts_any` residual — `xtask`'s
        // probe was deliberately widened (#1322's review) to also match
        // `named("any"` in Rust source, so the count is expected to stay at
        // 31, not drop to 30.
        WireRef::Unchecked { reason } => {
            let value = match reason {
                UncheckedReason::Effect => as_expr(ident(json), TsType::named("any")),
                UncheckedReason::ValidationError => as_expr(
                    as_expr(ident(json), TsType::named("unknown")),
                    TsType::named("ValidationError"),
                ),
                UncheckedReason::JsonError => as_expr(
                    as_expr(ident(json), TsType::named("unknown")),
                    TsType::named("JsonError"),
                ),
                UncheckedReason::HttpResult => as_expr(
                    as_expr(ident(json), TsType::named("unknown")),
                    TsType::named_with_args("HttpResult", vec![TsType::named("unknown")]),
                ),
                UncheckedReason::QueueResult => as_expr(
                    as_expr(ident(json), TsType::named("unknown")),
                    TsType::named("QueueResult"),
                ),
            };
            vec![const_(format!("__{name}"), value)]
        }
    }
}

/// #1439 (Arc E slice 3): direct unit coverage for
/// [`emit_field_deserialise_wire`], byte-checked against the exact strings
/// its pre-conversion `writeln!` body used to produce. The project-form
/// fixture corpus reaches most of these arms already (`bless_positive_
/// fixtures`/`positive_fixtures` passing with zero fixture diff is the
/// stronger, end-to-end proof for those), but a real gap check (per this
/// issue's own "confirm this at implementation time" instruction) found
/// `WireRef::Unchecked { reason: UncheckedReason::Effect }` is NOT reached
/// by any real fixture — `1319_runtime_error_type_fields/expected/
/// errortypes.ts` exercises the other four `Unchecked` reasons (via a
/// record with one field of each runtime-owned error type), but a
/// field-position `Effect` never appears in that fixture or any other in
/// the corpus (a record/sum field can never legally *be* `Effect` per
/// `wire_ref`'s own doc — this arm is reached only via the `TypeRef` path
/// this file's callers don't exercise for that specific shape). This
/// module closes that gap directly rather than leaving the arm asserted
/// only by inspection.
#[cfg(test)]
mod emit_field_deserialise_wire_tests {
    use super::*;
    use bynk_check::wire::Expected;

    fn render(stmts: Vec<TsStmt>) -> String {
        stmts
            .iter()
            .map(|s| bynk_ts::print_stmt(s, 1))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn unchecked_effect_casts_through_any() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire(
            "name",
            &WireRef::Unchecked {
                reason: UncheckedReason::Effect,
            },
            "json",
            "path",
            &ru,
        );
        assert_eq!(render(stmts), "  const __name = json as any;\n");
    }

    #[test]
    fn unchecked_validation_error_casts_through_unknown() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire(
            "name",
            &WireRef::Unchecked {
                reason: UncheckedReason::ValidationError,
            },
            "json",
            "path",
            &ru,
        );
        assert_eq!(
            render(stmts),
            "  const __name = json as unknown as ValidationError;\n"
        );
    }

    #[test]
    fn unchecked_http_result_names_unknown_element_type() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire(
            "name",
            &WireRef::Unchecked {
                reason: UncheckedReason::HttpResult,
            },
            "json",
            "path",
            &ru,
        );
        assert_eq!(
            render(stmts),
            "  const __name = json as unknown as HttpResult<unknown>;\n"
        );
    }

    #[test]
    fn unit_arm_binds_undefined() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire("name", &WireRef::Unit, "json", "path", &ru);
        assert_eq!(render(stmts), "  const __name = undefined;\n");
    }

    #[test]
    fn named_arm_propagates_the_error_branch() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire(
            "name",
            &WireRef::Named {
                name: "Foo".to_string(),
            },
            "json",
            "path",
            &ru,
        );
        assert_eq!(
            render(stmts),
            "  const __r_name = deserialise_Foo(json, path);\n  if (__r_name.tag === \"Err\") return __r_name;\n  const __name = __r_name.value;\n"
        );
    }

    #[test]
    fn bytes_arm_decodes_base64_with_two_guards() {
        let ru = RuntimeUse::default();
        let stmts = emit_field_deserialise_wire("name", &WireRef::Bytes, "json", "path", &ru);
        assert_eq!(
            render(stmts),
            "  if (typeof json !== \"string\") {\n    return Err({ kind: \"StructuralMismatch\", path: path, expected: \"base64 string\", actual: typeof json });\n  }\n  const __b_name = __bynkBytesFromBase64(json);\n  if (__b_name.tag === \"None\") {\n    return Err({ kind: \"StructuralMismatch\", path: path, expected: \"base64 string\", actual: \"invalid base64\" });\n  }\n  const __name = __b_name.value;\n"
        );
    }

    #[test]
    fn base_arm_with_integral_and_finite_guards() {
        let ru = RuntimeUse::default();
        let wire = WireRef::Base {
            base: BaseType::Float,
            json: JsonKind::Number,
            guards: vec![BaseGuard::Integral, BaseGuard::Finite],
            expected: Expected::Json(JsonKind::Number),
        };
        let stmts = emit_field_deserialise_wire("name", &wire, "json", "path", &ru);
        assert_eq!(
            render(stmts),
            "  if (typeof json !== \"number\") {\n    return Err({ kind: \"StructuralMismatch\", path: path, expected: \"number\", actual: typeof json });\n  }\n  if (!Number.isInteger(json)) {\n    return Err({ kind: \"StructuralMismatch\", path: path, expected: \"integer\", actual: String(json) });\n  }\n  if (!Number.isFinite(json)) {\n    return Err({ kind: \"StructuralMismatch\", path: path, expected: \"finite number\", actual: String(json) });\n  }\n  const __name = json;\n"
        );
    }
}

fn serialise_field_expr(t: &TypeRef, value: &str, ru: &RuntimeUse) -> TsExpr {
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
fn serialise_field_expr_via(t: &TypeRef, value: &str, ns: &str, ru: &RuntimeUse) -> TsExpr {
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
fn serialise_field_expr_wire(wire: &WireRef, value: &str, ns: &str, ru: &RuntimeUse) -> TsExpr {
    match wire {
        // Named type or generic instantiation (`Result`/`Option`/`List`/`Map`/
        // a generic `App` all key the same way — see `wire_ref`'s doc):
        // serialise through its own `serialise_<key>`.
        WireRef::Named { name } => call(ident(format!("{ns}serialise_{name}")), vec![ident(value)]),
        WireRef::Inst { key } => call(ident(format!("{ns}serialise_{key}")), vec![ident(value)]),
        // v0.21: serialising a non-finite `Float` is a contract violation
        // (`JSON.stringify(NaN)` would silently produce `null`); the guard is
        // a self-contained IIFE so the module needs no extra runtime import.
        //
        // #1435 (Arc E slice 1): the first real [`TsArrowBody::Block`] site
        // in this tree — a statement body (`if`/`throw`, then `return`), not
        // reducible to one expression the way every other arm here is. See
        // that type's own doc (`bynk-ts/src/program.rs`) for why this widens
        // the existing `Arrow.body` field rather than adding a new `TsExpr`
        // variant.
        WireRef::Base {
            base: BaseType::Float,
            ..
        } => call(
            TsExpr::Arrow {
                params: vec![TsParam {
                    name: "v".to_string(),
                    ty: Some(TsType::named("number")),
                    optional: false,
                }],
                is_async: false,
                generics: Vec::new(),
                return_type: None,
                body: Box::new(TsArrowBody::Block(vec![
                    TsStmt::if_stmt(
                        TsExpr::Unary {
                            op: TsUnaryOp::Not,
                            expr: Box::new(call(
                                member(ident("Number"), "isFinite"),
                                vec![ident("v")],
                            )),
                        },
                        TsStmt::throw_stmt(
                            TsExpr::New {
                                callee: Box::new(ident("Error")),
                                args: vec![str_lit("non-finite Float at boundary")],
                            },
                            None,
                        ),
                        None,
                    ),
                    TsStmt::return_stmt(
                        Some(as_expr(ident("v"), TsType::named("JsonValue"))),
                        None,
                    ),
                ])),
            },
            vec![ident(value)],
        ),
        // v0.110 (ADR 0142 D5): a `Bytes` is base64-encoded on the wire — the
        // one base type whose serialise is an encode, not a bare cast.
        WireRef::Bytes => {
            ru.note_bytes();
            as_expr(
                call(ident("__bynkBytesToBase64"), vec![ident(value)]),
                TsType::named("JsonValue"),
            )
        }
        WireRef::Base { .. } => as_expr(ident(value), TsType::named("JsonValue")),
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
        //
        // Review of #1319/#1320, finding 1's own tsc run (a real fixture with
        // one of these types finally reached this arm — none had before):
        // `value as JsonValue` fails `tsc --strict` (TS2352) for
        // `ValidationError`/`JsonError` specifically — real interfaces, not
        // structurally close enough to `JsonValue`'s own union for a direct
        // cast; `HttpResult<T>`/`QueueResult`'s own union shapes happen to
        // pass today, but that is a heuristic, not a guarantee this residual
        // should depend on. Bridging through `unknown` (the fix `tsc` itself
        // names) is safe and uniform for all five reasons, `Effect` included.
        //
        // A nested `As`-under-`As` (not one opaque double-cast string): safe
        // here because this function's own result is never rendered through
        // `render_operand` (nothing downstream uses it as a further Member/
        // Unary/Binary/Index operand) — every real caller either splices it
        // into a `Call`/`Object`/`Const` position via `render_expr`'s own
        // plain recursion, or stringifies the whole tree at a print boundary
        // (`bynk_ts::print_expr`), and `render_expr`'s own `As` arm only
        // parenthesises a `Binary`/`Arrow`/`Conditional` inner expression —
        // never a nested `As` — so this renders as the intended, parenless
        // `value as unknown as JsonValue`.
        WireRef::Unchecked { .. } => as_expr(
            as_expr(ident(value), TsType::named("unknown")),
            TsType::named("JsonValue"),
        ),
        WireRef::Unit => TsExpr::Lit(TsLit::Null),
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
pub(crate) fn serialise_expr(t: &TypeRef, value: &str, ru: &RuntimeUse) -> TsExpr {
    serialise_field_expr(t, value, ru)
}

/// v0.176 (#642): the one serialise dispatch for the workers cross-context
/// boundary, reaching helpers through `ns`. Replaces the two parallel
/// dispatches this boundary used to carry — `emit.rs`'s `workers_serialise_expr`
/// (which dropped `List`/`Map` to a bare `as JsonValue` cast) and
/// `workers_entry.rs`'s `serialise_call` (which did the same to `Bytes`, the
/// asymmetry that forced `bynk.types.bytes_at_workers_boundary`).
pub(crate) fn serialise_expr_via(t: &TypeRef, value: &str, ns: &str, ru: &RuntimeUse) -> TsExpr {
    serialise_field_expr_via(t, value, ns, ru)
}

/// v0.176 (#642): a deserialise **reference** for `ns`, shaped to
/// `callService`'s `deserialiseResult` parameter. The inline arms become a
/// lambda rather than the unvalidated `((j: any) => ({ tag: "Ok", value: j }))`
/// identity the caller path used to fall back to.
pub(crate) fn deserialise_ref_via(t: &TypeRef, ns: &str, ru: &RuntimeUse) -> TsExpr {
    match strip_effect(t) {
        TypeRef::Named(id) => ident(format!("{ns}deserialise_{}", id.name)),
        t @ (TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        | TypeRef::App { .. }) => ident(format!("{ns}deserialise_{}", inner_ts_name(t))),
        other => TsExpr::Arrow {
            params: vec![TsParam {
                name: "__j".to_string(),
                ty: Some(TsType::named("JsonValue")),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            body: Box::new(TsArrowBody::Expr(Box::new(deserialise_expr_via(
                other, "__j", "$", ns, ru,
            )))),
        },
    }
}

/// v0.176 (#642) follow-up: a serialise **reference** for `ns`, shaped to
/// `httpResultToResponse`'s serialiser parameter — the mirror image of
/// `deserialise_ref_via` above. Replaces `workers_entry.rs`'s
/// `http_value_serialiser`, a parallel dispatch that collapsed every base type
/// to `(v: any) => v as JsonValue`, dropping the `Float` non-finite guard and
/// `Bytes` base64 encoding that `serialise_field_expr_via` already carries.
pub(crate) fn serialise_ref_via(t: &TypeRef, ns: &str, ru: &RuntimeUse) -> TsExpr {
    match strip_effect(t) {
        TypeRef::Named(id) => ident(format!("{ns}serialise_{}", id.name)),
        t @ (TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        | TypeRef::App { .. }) => ident(format!("{ns}serialise_{}", inner_ts_name(t))),
        other => TsExpr::Arrow {
            params: vec![TsParam {
                name: "__v".to_string(),
                ty: Some(TsType::named(crate::emitter::ts_type_ref(other))),
                optional: false,
            }],
            is_async: false,
            generics: Vec::new(),
            return_type: None,
            body: Box::new(TsArrowBody::Expr(Box::new(serialise_field_expr_via(
                other, "__v", ns, ru,
            )))),
        },
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
pub(crate) fn deserialise_expr(t: &TypeRef, json: &str, path: &str, ru: &RuntimeUse) -> TsExpr {
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
) -> TsExpr {
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
    // The runtime-owned-error-type arms below (`ValidationError`/`JsonError`/
    // `QueueResult`/`HttpResult`) all share this one shape: `Ok(json as
    // unknown as {ty}) as Result<{ty}, BoundaryError>` — the deserialise-side
    // mirror of `serialise_field_expr_wire`'s own `WireRef::Unchecked` arm
    // (see that arm's own doc for why a nested `As`-under-`As` is safe here:
    // this function's result is never rendered through `render_operand`).
    let ok_unknown_cast = |ty: &str| -> TsExpr {
        as_expr(
            call(
                ident("Ok"),
                vec![as_expr(
                    as_expr(ident(json), TsType::named("unknown")),
                    TsType::named(ty.to_string()),
                )],
            ),
            TsType::named(format!("Result<{ty}, BoundaryError>")),
        )
    };
    // `Err({ kind: "StructuralMismatch", path, expected, actual } as
    // BoundaryError)` — shared by the `Bytes` arm (below) and the plain
    // `Base` arm further down, the one error shape both build.
    let structural_mismatch = |expected: &str, actual: TsExpr| -> TsExpr {
        call(
            ident("Err"),
            vec![as_expr(
                TsExpr::object(vec![
                    ("kind".to_string(), str_lit("StructuralMismatch")),
                    ("path".to_string(), str_lit(path)),
                    ("expected".to_string(), str_lit(expected)),
                    ("actual".to_string(), actual),
                ]),
                TsType::named("BoundaryError"),
            )],
        )
    };
    match t {
        TypeRef::Named(id) => call(
            ident(format!("{ns}deserialise_{}", id.name)),
            vec![ident(json), str_lit(path)],
        ),
        TypeRef::Result(..)
        | TypeRef::Option(..)
        | TypeRef::List(..)
        | TypeRef::Map(..)
        // v0.174 (#592): a generic-record instantiation decodes through its
        // monomorphised codec (`deserialise_Paginated_User`).
        | TypeRef::App { .. } => call(
            ident(format!("{ns}deserialise_{}", inner_ts_name(t))),
            vec![ident(json), str_lit(path)],
        ),
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
        TypeRef::Unit(_) => as_expr(
            call(ident("Ok"), vec![ident("undefined")]),
            TsType::named("Result<void, BoundaryError>"),
        ),
        // The runtime-owned error types: no generated codec to name, so the
        // deserialised value casts through unchecked — same shape as
        // `emit_field_deserialise_wire`'s `WireRef::Unchecked` arm.
        //
        // #1319: closes the `ts_any` residual — each arm now casts through
        // its own real, exported runtime type instead of sharing one `any`
        // arm, since `t` (the matched `TypeRef` itself) already tells this
        // function precisely which of the four it is; no information is
        // missing here the way it briefly was one layer down at the
        // `WireRef::Unchecked`-with-a-`reason`-field call site. Bridged
        // through `unknown` — the same fix, for the same TS2352 reason, as
        // `emit_field_deserialise_wire`'s own `WireRef::Unchecked` arm (this
        // function's `json` is not always statically a `JsonValue` the way
        // that arm's is, but nothing about a real-vs-real interface cast
        // gets *safer* when the source type is narrower, so the same
        // defensive bridge applies uniformly). No fixture currently reaches
        // this function for one of these four types directly (the field
        // path above is what real code exercises today); named rather than
        // silently assumed correct, the same "not exercised, defensive"
        // precedent this file's own `TypeRef::Unit` arm above already sets.
        TypeRef::ValidationError(_) => ok_unknown_cast("ValidationError"),
        TypeRef::JsonError(_) => ok_unknown_cast("JsonError"),
        // Review of #1319/#1320, finding 1: `HttpResult<T>` (`bynk-emit/
        // runtime/src/http.ts:6`) is generic with no default type argument
        // — a bare `HttpResult` in type position is `tsc`'s TS2314, not the
        // `any`-compatible cast the other three residual types get. This
        // arm already holds the element type (`TypeRef::HttpResult(Box<
        // TypeRef>, Span)`, `bynk-syntax/src/ast.rs:2196`), so it renders
        // through `ts_type_ref` — the same real-type-name printer every
        // other `HttpResult<...>` position in this file already uses —
        // instead of dropping the payload.
        TypeRef::HttpResult(inner, _) => {
            let ty = format!("HttpResult<{}>", crate::emitter::ts_type_ref(inner));
            ok_unknown_cast(&ty)
        }
        TypeRef::QueueResult(_) => ok_unknown_cast("QueueResult"),
        // v0.110 (ADR 0142 D5): a `Bytes` wires as a base64 string; decode it
        // (rejecting a non-string or invalid base64) to a `Uint8Array`.
        TypeRef::Base(BaseType::Bytes, _) => {
            ru.note_bytes();
            call(
                TsExpr::Arrow {
                    params: vec![TsParam {
                        name: "__v".to_string(),
                        ty: None,
                        optional: false,
                    }],
                    is_async: false,
                    generics: Vec::new(),
                    return_type: None,
                    body: Box::new(TsArrowBody::Expr(Box::new(TsExpr::Conditional {
                        test: Box::new(TsExpr::Binary {
                            op: TsBinaryOp::StrictEq,
                            left: Box::new(TsExpr::Unary {
                                op: TsUnaryOp::Typeof,
                                expr: Box::new(ident("__v")),
                            }),
                            right: Box::new(str_lit("string")),
                        }),
                        consequent: Box::new(call(
                            TsExpr::Arrow {
                                params: vec![TsParam {
                                    name: "__b".to_string(),
                                    ty: None,
                                    optional: false,
                                }],
                                is_async: false,
                                generics: Vec::new(),
                                return_type: None,
                                body: Box::new(TsArrowBody::Expr(Box::new(
                                    TsExpr::Conditional {
                                        test: Box::new(TsExpr::Binary {
                                            op: TsBinaryOp::StrictEq,
                                            left: Box::new(member(ident("__b"), "tag")),
                                            right: Box::new(str_lit("Some")),
                                        }),
                                        consequent: Box::new(call(
                                            ident("Ok"),
                                            vec![member(ident("__b"), "value")],
                                        )),
                                        alternate: Box::new(structural_mismatch(
                                            "base64 string",
                                            str_lit("invalid base64"),
                                        )),
                                    },
                                ))),
                            },
                            vec![call(ident("__bynkBytesFromBase64"), vec![ident("__v")])],
                        )),
                        alternate: Box::new(structural_mismatch(
                            "base64 string",
                            TsExpr::Unary {
                                op: TsUnaryOp::Typeof,
                                expr: Box::new(ident("__v")),
                            },
                        )),
                    }))),
                },
                vec![ident(json)],
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
            // `Some(predicate)`: the arm needs a second, narrower runtime check
            // beyond `typeof` — a real `TsExpr`, not text, now that `extra`'s
            // only past job (interpolating into a hand-built `format!`) is
            // gone.
            let predicate: Option<TsExpr> = match b {
                BaseType::Float => Some(call(member(ident("Number"), "isFinite"), vec![ident("__v")])),
                // v0.86 (ADR 0112 D6): a `Duration` is whole milliseconds —
                // reject a non-integer from the wire, as a refined `Int` does.
                BaseType::Int | BaseType::Duration | BaseType::Instant => {
                    Some(call(member(ident("Number"), "isInteger"), vec![ident("__v")]))
                }
                _ => None,
            };
            // v0.176 (#642): report what was *required*, not just the `typeof`
            // that was tested. For the arms carrying a `predicate` the two
            // differ, and reporting the bare `typeof` makes the error useless in
            // exactly the case the predicate exists to catch: a `3.5` for an `Int`
            // would read `expected: "number", actual: "number"`.
            let expected = match b {
                BaseType::Int | BaseType::Duration | BaseType::Instant => "integer",
                BaseType::Float => "finite number",
                _ => typeof_str,
            };
            let typeof_v = || TsExpr::Unary {
                op: TsUnaryOp::Typeof,
                expr: Box::new(ident("__v")),
            };
            let Some(predicate) = predicate else {
                return call(
                    TsExpr::Arrow {
                        params: vec![TsParam {
                            name: "__v".to_string(),
                            ty: None,
                            optional: false,
                        }],
                        is_async: false,
                        generics: Vec::new(),
                        return_type: None,
                        body: Box::new(TsArrowBody::Expr(Box::new(TsExpr::Conditional {
                            test: Box::new(TsExpr::Binary {
                                op: TsBinaryOp::StrictEq,
                                left: Box::new(typeof_v()),
                                right: Box::new(str_lit(typeof_str)),
                            }),
                            consequent: Box::new(call(ident("Ok"), vec![ident("__v")])),
                            alternate: Box::new(structural_mismatch(expected, typeof_v())),
                        }))),
                    },
                    vec![ident(json)],
                );
            };
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
            call(
                TsExpr::Arrow {
                    params: vec![TsParam {
                        name: "__v".to_string(),
                        ty: None,
                        optional: false,
                    }],
                    is_async: false,
                    generics: Vec::new(),
                    return_type: None,
                    body: Box::new(TsArrowBody::Expr(Box::new(TsExpr::Conditional {
                        test: Box::new(TsExpr::Binary {
                            op: TsBinaryOp::StrictNotEq,
                            left: Box::new(typeof_v()),
                            right: Box::new(str_lit(typeof_str)),
                        }),
                        consequent: Box::new(structural_mismatch(expected, typeof_v())),
                        alternate: Box::new(TsExpr::Conditional {
                            test: Box::new(predicate),
                            consequent: Box::new(call(ident("Ok"), vec![ident("__v")])),
                            alternate: Box::new(structural_mismatch(
                                expected,
                                call(ident("String"), vec![ident("__v")]),
                            )),
                        }),
                    }))),
                },
                vec![ident(json)],
            )
        }
        // Everything else is rejected by the checker's codec-domain rule (the
        // `Json` path) or by the boundary rules (the workers path). Shared by
        // three callers, so the message names the type rather than one caller.
        other => unreachable!("non-codable type reached a codec lowering: {other:?}"),
    }
}

/// Review of #1319/#1320, finding 2: no unit test covered any of
/// `deserialise_expr_via`'s own four runtime-owned-error-type arms — this
/// path is not reached by any current fixture (see those arms' own doc),
/// so a fixture-corpus regression would not have caught a text-level
/// mistake here the way it caught the `HttpResult<unknown>` gap one layer
/// down. Pins the exact generated text directly.
#[cfg(test)]
mod deserialise_expr_via_error_type_tests {
    use super::*;
    use bynk_syntax::span::Span;

    fn sp() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn validation_error_casts_through_unknown() {
        let ru = RuntimeUse::default();
        let t = TypeRef::ValidationError(sp());
        assert_eq!(
            bynk_ts::print_expr(&deserialise_expr(&t, "json", "path", &ru)),
            "Ok(json as unknown as ValidationError) as Result<ValidationError, BoundaryError>"
        );
    }

    #[test]
    fn json_error_casts_through_unknown() {
        let ru = RuntimeUse::default();
        let t = TypeRef::JsonError(sp());
        assert_eq!(
            bynk_ts::print_expr(&deserialise_expr(&t, "json", "path", &ru)),
            "Ok(json as unknown as JsonError) as Result<JsonError, BoundaryError>"
        );
    }

    #[test]
    fn http_result_names_its_real_element_type_and_casts_through_unknown() {
        let ru = RuntimeUse::default();
        let t = TypeRef::HttpResult(Box::new(TypeRef::Base(BaseType::Int, sp())), sp());
        assert_eq!(
            bynk_ts::print_expr(&deserialise_expr(&t, "json", "path", &ru)),
            "Ok(json as unknown as HttpResult<number>) as Result<HttpResult<number>, BoundaryError>"
        );
    }

    #[test]
    fn queue_result_casts_through_unknown() {
        let ru = RuntimeUse::default();
        let t = TypeRef::QueueResult(sp());
        assert_eq!(
            bynk_ts::print_expr(&deserialise_expr(&t, "json", "path", &ru)),
            "Ok(json as unknown as QueueResult) as Result<QueueResult, BoundaryError>"
        );
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
                        .map(|a| bynk_ts::print_type(&qualified_ts_type(a, qual)))
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
                        .map(|a| bynk_ts::print_type(&qualified_ts_type(a, qual)))
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
                let ok_inner = bynk_ts::print_type(&qualified_ts_type(ok, qual));
                let err_inner = bynk_ts::print_type(&qualified_ts_type(err, qual));
                // #1435 (Arc E slice 1): boundary-print — `emit_generic_helpers`
                // stays `write!`-based (out of this slice's scope).
                let serialise_ok =
                    bynk_ts::print_expr(&serialise_field_expr(ok, "value.value", ru));
                let serialise_err =
                    bynk_ts::print_expr(&serialise_field_expr(err, "value.error", ru));
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
                // #1439 (Arc E slice 3): boundary-print, same treatment as
                // `emit_record_codec`'s own call site above.
                splice_stmts(
                    out,
                    emit_field_deserialise("v", ok, "obj[\"value\"]", "`${path}.value`", ru),
                );
                writeln!(
                    out,
                    "    return Ok(Ok(__v) as Result<{ok_inner}, {err_inner}>);"
                )
                .unwrap();
                writeln!(out, "  }} else if (obj[\"kind\"] === \"Err\") {{").unwrap();
                // #1439 (Arc E slice 3): boundary-print, same treatment as above.
                splice_stmts(
                    out,
                    emit_field_deserialise("e", err, "obj[\"error\"]", "`${path}.error`", ru),
                );
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
                let inner_ty = bynk_ts::print_type(&qualified_ts_type(inner, qual));
                // #1435 (Arc E slice 1): boundary-print, same treatment as
                // `ResultInst` above.
                let serialise_inner =
                    bynk_ts::print_expr(&serialise_field_expr(inner, "value.value", ru));
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
                // #1439 (Arc E slice 3): boundary-print, same treatment as above.
                splice_stmts(
                    out,
                    emit_field_deserialise("v", inner, "obj[\"value\"]", "`${path}.value`", ru),
                );
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
                let elem_ty = bynk_ts::print_type(&qualified_ts_type(elem, qual));
                // #1435 (Arc E slice 1): boundary-print, same treatment as above.
                let serialise_elem = bynk_ts::print_expr(&serialise_field_expr(elem, "v", ru));
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
                // #1439 (Arc E slice 3): boundary-print, same treatment as above.
                splice_stmts(
                    out,
                    emit_field_deserialise("el", elem, "item", "`${path}[${i}]`", ru),
                );
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
                let key_ty = bynk_ts::print_type(&qualified_ts_type(key, qual));
                let val_ty = bynk_ts::print_type(&qualified_ts_type(val, qual));
                // #1435 (Arc E slice 1): boundary-print, same treatment as above.
                let serialise_key = bynk_ts::print_expr(&serialise_field_expr(key, "k", ru));
                let serialise_val = bynk_ts::print_expr(&serialise_field_expr(val, "v", ru));
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
                // #1439 (Arc E slice 3): boundary-print, same treatment as above.
                splice_stmts(
                    out,
                    emit_field_deserialise("k", key, "entryK", "`${path}[${i}][0]`", ru),
                );
                splice_stmts(
                    out,
                    emit_field_deserialise("v", val, "entryV", "`${path}[${i}][1]`", ru),
                );
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

#[cfg(test)]
mod default_lowering_tests {
    use super::*;
    use bynk_syntax::ast::{CommonsItem, Expr, RecordField};
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
