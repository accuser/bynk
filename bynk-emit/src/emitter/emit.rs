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
use std::sync::Arc;

use crate::project::EmitProjectCtx;
use bynk_check::actors::ActorDecl;
use bynk_check::checker::{TyId, TypedCommons, Types};
use bynk_syntax::ast::{
    AgentDecl, BaseType, CapabilityDecl, CommonsItem, Expr, ExprKind, FnDecl, FnName, Handler,
    HttpMethod, Ident, MessageEntry, MessagesDecl, Param, PredKind, ProviderDecl, RecordField,
    Refinement, ServiceDecl, StoreField, TypeBody, TypeDecl, TypeParam, TypeRef,
};

use crate::ir::lower::{
    HandlerSignatureIr, body_writes_state, is_effectful_return, lower_actor_seam_ir,
    lower_handler_kind_ir, lower_protocol_ir_from_commons,
};
use crate::ir::{
    ActorSeamIr, FnSig, IrHandlerKind, IrHttpMethod, OpSig, ProtocolIr, StoreFieldIr, StoreKindIr,
    TypeShape,
};

use super::*;

/// P6.x (#1188): reads `shape` — `t`'s already-lowered `bynk-emit::ir::TypeShape`
/// — instead of walking `t.body` (the AST's own `TypeBody`) directly. Doesn't
/// spell that AST module's path literally in this comment on purpose: this
/// file is otherwise invisible to the `ast_importers` probe
/// (`xtask/src/greenfield_status.rs`), which matches on the literal string,
/// comments included — see `design/tracks/the-ir.md` §5's own note on this
/// exact blind spot.
/// `t` itself is still read for what `TypeShape` deliberately doesn't carry:
/// `.documentation`/`.name`/`.type_params` (header/namespace-object emission,
/// including this type's own attached methods via `emit_attached_methods`,
/// which stays on the untouched, unconverted body-lowering path — see that
/// function's own doc comment).
pub(crate) fn emit_type(
    out: &mut String,
    t: &TypeDecl,
    shape: &TypeShape,
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
    match shape {
        // `Opaque` and `Refined` lower almost identically: a branded base type
        // alias plus an `of` constructor object. The one difference (ADR 0182)
        // is `unsafe`: opaque exposes it (its defining commons needs a
        // representation-level constructor), a refined/alias type does not —
        // `TypeShape::Refined::opaque` already carries that distinction
        // (P6.6's own Decision A unified the AST's two `TypeBody` variants
        // into this one `TypeShape` variant).
        TypeShape::Refined {
            base,
            refinement,
            opaque,
        } => emit_refined_type(
            out,
            t,
            RefinedShape {
                base: *base,
                refinement: refinement.as_ref(),
                is_opaque: *opaque,
            },
            commons,
            &brand_prefix,
            &ctx.runtime_use,
        ),
        TypeShape::Record { fields } => emit_record_type(out, t, fields, commons, &ctx.runtime_use),
        // `embeds` is not read here — today's emitter has no reader for
        // `SumBody::embeds` anywhere (confirmed by grep; `embeds` is checker-
        // enforced construction-time only), so `TypeShape::Sum::embeds` stays
        // unread by this slice too, not a gap introduced by it.
        TypeShape::Sum { variants, .. } => {
            emit_sum_type(out, t, variants, commons, &ctx.runtime_use)
        }
    }
}

/// Emit a doc block as a JSDoc-style comment at the given indent. Each line
/// of the doc body is prefixed with ` * `; empty lines become ` *`.
///
/// Doc-block bodies are copied verbatim from source, so a stray `*/` would
/// otherwise close the JSDoc comment early and let the trailing text land as
/// executable top-level TypeScript (issue #720). Escape it to `*\/`, which
/// renders identically but can no longer terminate the comment.
///
/// #1333 (R7.1): builds a real [`bynk_ts::TsStmt`] (`TsStmtKind::DocComment`)
/// and prints it through [`bynk_ts::print_stmt`], instead of `writeln!`-ing
/// the JSDoc text by hand — the escaping/blank-line rules above now live on
/// that variant's own doc and its printer arm; this function's own signature
/// and every one of its 14 real callers are unchanged, the same P7.9 pattern
/// `ts_type_ref`/`ts_ty` already used (`indent` is always an exact multiple
/// of `INDENT_STEP` in every real call, so `indent / INDENT_STEP` is a
/// lossless conversion to the printer's own 2-space-per-depth unit).
pub(crate) fn emit_doc_block(out: &mut String, doc: Option<&str>, indent: usize) {
    let Some(doc) = doc else { return };
    debug_assert!(
        indent.is_multiple_of(INDENT_STEP),
        "emit_doc_block: indent {indent} is not a multiple of INDENT_STEP"
    );
    let stmt = bynk_ts::TsStmt::doc_comment(doc, None);
    out.push_str(&bynk_ts::print_stmt(&stmt, indent / INDENT_STEP));
}

/// v0.93 (ADR 0118): deterministically order the `@indexed` map → fields entries
/// (by map name; fields keep their declaration order). `HashMap` iteration is
/// unordered, so emitted state fields would otherwise drift between runs.
fn sorted_index_fields(indexes: &HashMap<String, Vec<String>>) -> Vec<(&String, &Vec<String>)> {
    let mut entries: Vec<(&String, &Vec<String>)> = indexes.iter().collect();
    entries.sort_by_key(|(name, _)| name.to_string());
    entries
}

/// The parts of a refined-or-opaque `TypeBody` that its lowering reads.
///
/// Honest about what this is: `emit_refined_type` reached eight arguments when it
/// gained `runtime_use`, and grouping three of them keeps it under
/// `clippy::too_many_arguments` without another `#[allow]`. The three do cohere —
/// all come out of the same `t.body` the caller already matched on — but the
/// function still takes six parameters, so this is a lint fix that reads well,
/// not a decomposition.
struct RefinedShape<'a> {
    base: BaseType,
    refinement: Option<&'a Refinement>,
    /// ADR 0182: opaque and refined lower identically apart from `unsafe`, which
    /// only opaque exposes (its defining commons needs a representation-level
    /// constructor).
    is_opaque: bool,
}

/// #1339 (R7.1): `export type {name} = {base} & { readonly __brand: "..."
/// };` is a real [`bynk_ts::TsDecl::TypeAlias`] over a new
/// [`bynk_ts::TsType::Intersection`] (this file's own first real need for
/// it — `TsType` had `Named`/`Array`/`Object`/`Fn`/`Union`, nothing for
/// `A & B`); `export const {name} = { of(...) {...}, unsafe?(...) {...},
/// ...attachedMethods }` is a real [`bynk_ts::TsDecl::ConstDecl`] whose
/// `init` is one [`bynk_ts::TsExpr::multiline_object_entries`] — `of`'s own
/// signature/return-statement are real nodes; `emit_refined_checks`'s own
/// output (still `out: &mut String`, unaffected by this slice, the P7.9/
/// step-1 pattern applied one level down) is captured and carried as one
/// [`bynk_ts::TsStmtKind::Raw`] statement — the same carrier #1337 added for
/// `lower.rs`'s own permanently-excluded body text, reused here for a
/// different, narrower reason: this is legitimately real-node-shaped
/// content (every guard is already built and printed via real
/// `bynk_ts::TsStmt`/`print_stmt` calls inside `emit_refined_checks`
/// itself), just still `String`-typed at its own call boundary because that
/// function's own signature wasn't part of this slice's scope. `emit_
/// attached_methods`'s own real `Vec<TsObjectEntry>` (#1337) is appended
/// directly into the SAME entries list — no more per-entry `print_object_
/// entry` splice loop for this function, now that it owns the whole
/// object's own construction. This function's own exact signature is
/// unchanged, the same P7.9/step-1 pattern.
fn emit_refined_type(
    out: &mut String,
    t: &TypeDecl,
    shape: RefinedShape<'_>,
    commons: &TypedCommons,
    brand_prefix: &str,
    runtime_use: &RuntimeUse,
) {
    let RefinedShape {
        base,
        refinement,
        is_opaque,
    } = shape;
    let name = t.name.name.clone();
    let base_ty = bynk_ts::TsType::named(ts_base(base));

    let brand_ty = bynk_ts::TsType::Object(vec![bynk_ts::TsTypeMember::readonly_prop(
        "__brand",
        bynk_ts::TsType::named(format!("\"{brand_prefix}{name}\"")),
    )]);
    let type_alias = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::TypeAlias {
            name: name.clone(),
            type_params: Vec::new(),
            ty: bynk_ts::TsType::intersection(vec![base_ty.clone(), brand_ty]),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&type_alias, 0));
    writeln!(out).unwrap();

    let checks_text = {
        let mut checks = String::new();
        emit_refined_checks(&mut checks, t, base, refinement);
        checks
    };
    let cast_to_name = bynk_ts::TsExpr::As {
        expr: Box::new(bynk_ts::TsExpr::Ident("value".to_string())),
        ty: bynk_ts::TsType::named(name.clone()),
    };
    let mut entries: Vec<bynk_ts::TsObjectEntry> = vec![bynk_ts::TsObjectEntry::Method {
        name: "of".to_string(),
        is_async: false,
        generics: Vec::new(),
        params: vec![bynk_ts::TsParam {
            name: "value".to_string(),
            ty: Some(base_ty.clone()),
            optional: false,
        }],
        return_type: Some(bynk_ts::TsType::named_with_args(
            "Result",
            vec![
                bynk_ts::TsType::named(name.clone()),
                bynk_ts::TsType::named("ValidationError"),
            ],
        )),
        doc: None,
        inline: false,
        body: vec![
            bynk_ts::TsStmt::raw(checks_text, None),
            bynk_ts::TsStmt::return_stmt(
                Some(bynk_ts::TsExpr::Call {
                    callee: Box::new(bynk_ts::TsExpr::Ident("Ok".to_string())),
                    args: vec![cast_to_name.clone()],
                }),
                None,
            ),
        ],
    }];
    // ADR 0182: only opaque types expose a public `unsafe` constructor. A refined
    // or alias type omits it — host code cannot bypass the predicate — and its
    // admitted/generated values brand via an inline `as` cast instead (see
    // `unchecked_construct`).
    if is_opaque {
        entries.push(bynk_ts::TsObjectEntry::Method {
            name: "unsafe".to_string(),
            is_async: false,
            generics: Vec::new(),
            params: vec![bynk_ts::TsParam {
                name: "value".to_string(),
                ty: Some(base_ty),
                optional: false,
            }],
            return_type: Some(bynk_ts::TsType::named(name.clone())),
            doc: None,
            inline: false,
            body: vec![bynk_ts::TsStmt::return_stmt(Some(cast_to_name), None)],
        });
    }
    entries.extend(emit_attached_methods(
        &t.name.name,
        &t.type_params,
        commons,
        runtime_use,
    ));
    let const_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::ConstDecl {
            name,
            ty: None,
            init: bynk_ts::TsExpr::multiline_object_entries(entries),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&const_decl, 0));
    writeln!(out).unwrap();
}

/// #1335 (R7.1): each guard is a real [`bynk_ts::TsStmt::If`] wrapping a
/// [`bynk_ts::TsStmt::Return`], printed at depth 2 (`out`'s own 4-space
/// convention here) through [`bynk_ts::print_stmt`], instead of hand-rolled
/// `writeln!`s — `emit_refined_checks` and `emit_pred_check`'s own exact
/// signatures are unchanged, the same P7.9/step-1 (#1333) pattern.
fn emit_refined_checks(
    out: &mut String,
    t: &TypeDecl,
    base: BaseType,
    refinement: Option<&Refinement>,
) {
    let name = &t.name.name;
    if base == BaseType::Int {
        out.push_str(&print_numeric_guard_stmt(
            name,
            "isInteger",
            "must be an integer",
        ));
    }
    // v0.21: validated `Float` values are finite — `.of` and the boundary
    // codec agree (ADR 0040); only in-language arithmetic is host-defined.
    if base == BaseType::Float {
        out.push_str(&print_numeric_guard_stmt(
            name,
            "isFinite",
            "must be a finite number",
        ));
    }
    if let Some(r) = refinement {
        for pred in &r.predicates {
            emit_pred_check(out, name, &pred.kind);
        }
    }
}

/// `if (!Number.{method}(value)) { return Err({ field: "{name}", message:
/// "{message}", value }); }` — the `Int`/`Float` base-type guard shared by
/// both `emit_refined_checks` call sites, `field`/`message` both real
/// string literals with nothing to escape (a Bynk type name; a fixed,
/// hand-written English message).
fn print_numeric_guard_stmt(name: &str, method: &str, message: &str) -> String {
    let cond = bynk_ts::TsExpr::Unary {
        op: bynk_ts::TsUnaryOp::Not,
        expr: Box::new(bynk_ts::TsExpr::Call {
            callee: Box::new(bynk_ts::TsExpr::Member {
                object: Box::new(bynk_ts::TsExpr::Ident("Number".to_string())),
                property: method.to_string(),
            }),
            args: vec![bynk_ts::TsExpr::Ident("value".to_string())],
        }),
    };
    print_guard_if_stmt(
        cond,
        bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(name.to_string())),
        bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(message.to_string())),
    )
}

/// `if (<cond>) { return Err({ field: <field>, message: <message>, value
/// }); }` at depth 2 — the one real shape both `print_numeric_guard_stmt`
/// and `emit_pred_check` build, differing only in `cond`/`field`/`message`.
fn print_guard_if_stmt(
    cond: bynk_ts::TsExpr,
    field: bynk_ts::TsExpr,
    message: bynk_ts::TsExpr,
) -> String {
    let err_obj = bynk_ts::TsExpr::object_entries(vec![
        bynk_ts::TsObjectEntry::Prop("field".to_string(), field),
        bynk_ts::TsObjectEntry::Prop("message".to_string(), message),
        bynk_ts::TsObjectEntry::Shorthand("value".to_string()),
    ]);
    let return_stmt = bynk_ts::TsStmt::return_stmt(
        Some(bynk_ts::TsExpr::Call {
            callee: Box::new(bynk_ts::TsExpr::Ident("Err".to_string())),
            args: vec![err_obj],
        }),
        None,
    );
    let if_stmt =
        bynk_ts::TsStmt::if_stmt(cond, bynk_ts::TsStmt::block(vec![return_stmt], None), None);
    bynk_ts::print_stmt(&if_stmt, 2)
}

fn emit_pred_check(out: &mut String, type_name: &str, pred: &PredKind) {
    let (cond, msg) = crate::emitter::pred_condition_and_message(pred, "value");
    // `cond` is opaque, already-formed JS condition text (e.g.
    // `value >= 0`, or, for `PredKind::Matches`, a `RegExp(...)` expression
    // whose own pattern text is already `escape_ts_string`-escaped) —
    // carried as a raw `Ident` wrapped in the real `!(...)` this crate's
    // own `Not`/`Paren` already represent, reproducing `if (!({cond}))`
    // exactly. `msg` gets the SAME opaque, pre-quoted treatment, not
    // `TsLit::Str` — deviating from the accepted proposal's own Decision B
    // ("msg as an ordinary TsLit::Str"), because `PredKind::Matches`'s own
    // message embeds that same already-`escape_ts_string`-escaped pattern
    // text directly (`format!("must match /{escaped}/")`); running it a
    // second time through `TsLit::Str`'s own renderer (which re-applies the
    // identical escaper) would double-escape every backslash the pattern
    // contains — a real correctness bug for any `Matches` predicate whose
    // pattern needs one. Every other `PredKind` arm's own message is plain,
    // already-safe English text with nothing to escape, so this is a
    // uniform, always-correct choice, not a narrow special case.
    let cond_expr = bynk_ts::TsExpr::Unary {
        op: bynk_ts::TsUnaryOp::Not,
        expr: Box::new(bynk_ts::TsExpr::Paren(Box::new(bynk_ts::TsExpr::Ident(
            cond,
        )))),
    };
    out.push_str(&print_guard_if_stmt(
        cond_expr,
        bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(type_name.to_string())),
        bynk_ts::TsExpr::Ident(format!("\"{msg}\"")),
    ));
}

/// v0.157 (ADR 0183): the erased TS type-parameter list for a generic
/// declaration (`<A, B>`), or `""` when non-generic. The same erasure used by
/// generic functions.
///
/// #1333 (R7.1): each name prints through a real [`bynk_ts::TsType::named`]
/// via [`bynk_ts::print_type`] — P7.9's own established entry point for one
/// type fragment — instead of splicing the raw `&str` directly; this
/// function's own `-> String` signature and every one of its 5 real callers
/// are unchanged.
pub(crate) fn ts_type_params(params: &[TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let names: Vec<String> = params
        .iter()
        .map(|p| bynk_ts::print_type(&bynk_ts::TsType::named(p.name.name.as_str())))
        .collect();
    format!("<{}>", names.join(", "))
}

/// #1339 (R7.1): `export interface {name}{params} { readonly {field}: {ty};
/// ... }` is a real [`bynk_ts::TsDecl::Interface`] (`type_params` bare
/// names, matching `ts_type_params`'s own convention exactly; each field a
/// [`bynk_ts::TsTypeMember::readonly_prop`], reusing [`TsTypeMember`]'s own
/// existing `readonly` field rather than a bespoke one — every real field
/// here is `readonly`), printed through [`bynk_ts::print_stmt`] at depth 0.
/// Field types route through `ts_ty_to_ts_type` (the real-node sibling
/// `ts_ty` itself already wraps, P7.9) instead of the opaque pre-printed
/// `String` `ts_ty` returns — a real node, not text. This function's own
/// exact signature is unchanged, the same P7.9/step-1 pattern.
fn emit_record_type(
    out: &mut String,
    t: &TypeDecl,
    fields: &[(String, TyId)],
    commons: &TypedCommons,
    runtime_use: &RuntimeUse,
) {
    let type_params: Vec<String> = t.type_params.iter().map(|p| p.name.name.clone()).collect();
    let members: Vec<bynk_ts::TsTypeMember> = fields
        .iter()
        .map(|(name, ty)| {
            bynk_ts::TsTypeMember::readonly_prop(
                name.clone(),
                ts_ty_to_ts_type(*ty, &commons.ty_intern),
            )
        })
        .collect();
    let interface = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::Interface {
            name: t.name.name.clone(),
            type_params,
            members,
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&interface, 0));
    writeln!(out).unwrap();
    writeln!(out, "export const {name} = {{", name = t.name.name).unwrap();
    for entry in emit_attached_methods(&t.name.name, &t.type_params, commons, runtime_use) {
        out.push_str(&bynk_ts::print_object_entry(&entry, 0));
    }
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();
}

/// #1339 (R7.1): `export type {name}{params} =\n  | {...}\n  | {...};` is a
/// real [`bynk_ts::TsDecl::TypeAlias`] over a new
/// [`bynk_ts::TsType::multiline_union`] (see its own doc for the exact
/// leading-pipe/spacing rules this reproduces byte-for-byte); `export const
/// {name} = { {tag}: ..., ... }` is a real [`bynk_ts::TsDecl::ConstDecl`]
/// whose `init` is one [`bynk_ts::TsExpr::multiline_object_entries`] — a
/// nullary variant is a real cast expression, a payload variant a real
/// [`bynk_ts::TsExpr::Arrow`] (`generics`/`return_type`, #1339's own two
/// gaps beyond the accepted proposal's own three) whose object-literal body
/// is wrapped in an explicit [`bynk_ts::TsExpr::Paren`] — `Arrow`'s own
/// renderer does not auto-parenthesise an object-literal body the way real
/// JS/TS syntax requires to disambiguate it from a block, so this call site
/// must ask for the parens itself, the same "explicit `Paren` always prints
/// its own literal parens" precedent #1323 already established. `emit_
/// attached_methods`'s own real `Vec<TsObjectEntry>` (#1337) is appended
/// directly into the SAME entries list, the same "own the whole object's
/// construction, drop the per-entry splice loop" pattern `emit_refined_
/// type`'s own conversion just used. This function's own exact signature
/// is unchanged, the same P7.9/step-1 pattern.
fn emit_sum_type(
    out: &mut String,
    t: &TypeDecl,
    variants: &[(String, Vec<(String, TyId)>)],
    commons: &TypedCommons,
    runtime_use: &RuntimeUse,
) {
    // #593: a generic sum erases to a TypeScript generic discriminated union,
    // exactly as a generic record erases to `interface Name<T>`. The type
    // parameters ride the `export type` header and each payload constructor
    // (`Some: <T>(v: T): Opt<T> => …`); a payload-less constructor stays a
    // constant, cast to the all-`never` instantiation (`Opt<never>`), which is
    // assignable to every `Opt<X>` since the nullary arm names no parameter.
    // Both are empty for a non-generic sum, keeping its output identical.
    let name = t.name.name.clone();
    let type_params: Vec<String> = t.type_params.iter().map(|p| p.name.name.clone()).collect();
    let never_ty = if type_params.is_empty() {
        bynk_ts::TsType::named(name.clone())
    } else {
        bynk_ts::TsType::named_with_args(
            name.clone(),
            type_params
                .iter()
                .map(|_| bynk_ts::TsType::named("never"))
                .collect(),
        )
    };
    let self_ty = if type_params.is_empty() {
        bynk_ts::TsType::named(name.clone())
    } else {
        bynk_ts::TsType::named_with_args(
            name.clone(),
            type_params
                .iter()
                .map(|p| bynk_ts::TsType::named(p.clone()))
                .collect(),
        )
    };

    let variant_types: Vec<bynk_ts::TsType> = variants
        .iter()
        .map(|(tag, payload)| {
            let mut members = vec![bynk_ts::TsTypeMember::readonly_prop(
                "tag",
                bynk_ts::TsType::named(format!("\"{tag}\"")),
            )];
            members.extend(payload.iter().map(|(field, ty)| {
                bynk_ts::TsTypeMember::readonly_prop(
                    field.clone(),
                    ts_ty_to_ts_type(*ty, &commons.ty_intern),
                )
            }));
            bynk_ts::TsType::Object(members)
        })
        .collect();
    let type_alias = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::TypeAlias {
            name: name.clone(),
            type_params: type_params.clone(),
            ty: bynk_ts::TsType::multiline_union(variant_types),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&type_alias, 0));
    writeln!(out).unwrap();

    let mut entries: Vec<bynk_ts::TsObjectEntry> = Vec::new();
    for (tag, payload) in variants {
        if payload.is_empty() {
            let tag_obj = bynk_ts::TsExpr::object_entries(vec![bynk_ts::TsObjectEntry::Prop(
                "tag".to_string(),
                bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(tag.clone())),
            )]);
            entries.push(bynk_ts::TsObjectEntry::Prop(
                tag.clone(),
                bynk_ts::TsExpr::As {
                    expr: Box::new(tag_obj),
                    ty: never_ty.clone(),
                },
            ));
        } else {
            let ctor_params: Vec<bynk_ts::TsParam> = payload
                .iter()
                .map(|(field, ty)| bynk_ts::TsParam {
                    name: field.clone(),
                    ty: Some(ts_ty_to_ts_type(*ty, &commons.ty_intern)),
                    optional: false,
                })
                .collect();
            let mut obj_entries = vec![bynk_ts::TsObjectEntry::Prop(
                "tag".to_string(),
                bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(tag.clone())),
            )];
            obj_entries.extend(
                payload
                    .iter()
                    .map(|(field, _)| bynk_ts::TsObjectEntry::Shorthand(field.clone())),
            );
            let ctor_body =
                bynk_ts::TsExpr::Paren(Box::new(bynk_ts::TsExpr::object_entries(obj_entries)));
            entries.push(bynk_ts::TsObjectEntry::Prop(
                tag.clone(),
                bynk_ts::TsExpr::Arrow {
                    params: ctor_params,
                    is_async: false,
                    generics: type_params.clone(),
                    return_type: Some(self_ty.clone()),
                    body: Box::new(ctor_body),
                },
            ));
        }
    }
    entries.extend(emit_attached_methods(
        &t.name.name,
        &t.type_params,
        commons,
        runtime_use,
    ));
    let const_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::ConstDecl {
            name,
            ty: None,
            init: bynk_ts::TsExpr::multiline_object_entries(entries),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&const_decl, 0));
    writeln!(out).unwrap();
}

/// #1337: returns real `bynk_ts::TsObjectEntry`s (was `out: &mut String`) —
/// so `emit_refined_type`/`emit_record_type`/`emit_sum_type` (still
/// unconverted) can splice them via `bynk_ts::print_object_entry`, and so a
/// future slice converting those three can append them directly into a real
/// `TsExpr::Object`'s own entries, with no opaque-entries carrier needed at
/// all (the question this slice's own accepted proposal was asked to
/// resolve).
fn emit_attached_methods(
    type_name: &str,
    type_params: &[TypeParam],
    commons: &TypedCommons,
    runtime_use: &RuntimeUse,
) -> Vec<bynk_ts::TsObjectEntry> {
    let mut entries = Vec::new();
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
        entries.push(emit_method(
            f,
            type_name,
            type_params,
            method_name,
            commons,
            runtime_use,
        ));
    }
    entries
}

/// v0.132.1 (#481): the consumer-context rebrand of a `uses`-imported refined
/// type carries a value-side const (`.of` / `.unsafe`, emitted inline in
/// `emit_context_rebrands`). A commons may *also* attach user-defined methods
/// to the type (`fn Cents.fromInt(...)`, merged into the const by
/// [`emit_attached_methods`] in the commons' *own* output). Those methods must
/// reach the consumer too, or a call like `Cents.fromInt(n)` — which the
/// checker accepts — fails `tsc` on the rebranded const. Forward each attached
/// method by delegating to the value-imported `__Commons{type_name}` (a value
/// import, so the delegate target exists at runtime); the `as unknown as`
/// return cast bridges the commons brand to the context brand, exactly like the
/// inline `.of` / `.unsafe` forwarders. Covers static *and* instance methods
/// (a consumer-branded `self` is a structural subtype of `__Commons{name}`, so
/// no argument cast is needed).
///
/// `methods` are the imported type's attached method signatures, threaded in
/// via [`EmitProjectCtx::imported_methods`] (the consumer context's own
/// `TypedCommons` merges the imported *types* but not their fn items). They
/// arrive pre-sorted by name so emission is deterministic.
///
/// P6.18: reads [`FnSig`] (the declaring unit's own resolved `params`/
/// `return_ty`) rather than a raw `FnDecl`'s `TypeRef`s — `self`'s own type
/// still comes from `type_name` directly, the *consumer* context's own
/// rebranded name, never resolved through `FnSig` at all (a method's own
/// generic receiver plays no part in what gets forwarded here).
///
/// #1337: fully self-contained, no `emitter/lower.rs` dependency at all
/// (unlike [`emit_method`]'s own opaque body) — the body here is one real,
/// statically-known statement shape, so it converts to a real
/// `bynk_ts::TsStmt::Return`, no opaque carrier needed. `out: &mut String`
/// kept (the P7.9/step-1 pattern): `emitter.rs`'s own real caller
/// (`emit_context_rebrands`) is still unconverted, so this stays a "build a
/// real node internally, print just that fragment" helper rather than
/// returning entries the caller isn't ready to consume structurally.
pub(crate) fn emit_forwarded_methods(
    out: &mut String,
    type_name: &str,
    methods: &[FnSig],
    tys: &Arc<Types>,
) {
    for f in methods {
        let mut params: Vec<bynk_ts::TsParam> = Vec::new();
        let mut args: Vec<bynk_ts::TsExpr> = Vec::new();
        if f.has_self {
            params.push(bynk_ts::TsParam {
                name: "self".to_string(),
                ty: Some(bynk_ts::TsType::named(type_name)),
                optional: false,
            });
            args.push(bynk_ts::TsExpr::Ident("self".to_string()));
        }
        for (name, ty) in &f.params {
            let ident = ts_ident(name);
            params.push(bynk_ts::TsParam {
                name: ident.clone(),
                ty: Some(bynk_ts::TsType::named(ts_ty(*ty, tys))),
                optional: false,
            });
            args.push(bynk_ts::TsExpr::Ident(ident));
        }
        let ret = ts_ty(f.return_ty, tys);
        let call = bynk_ts::TsExpr::Call {
            callee: Box::new(bynk_ts::TsExpr::Member {
                object: Box::new(bynk_ts::TsExpr::Ident(format!("__Commons{type_name}"))),
                property: f.name.clone(),
            }),
            args,
        };
        let body = vec![bynk_ts::TsStmt::return_stmt(
            Some(bynk_ts::TsExpr::As {
                expr: Box::new(bynk_ts::TsExpr::As {
                    expr: Box::new(call),
                    ty: bynk_ts::TsType::named("unknown"),
                }),
                ty: bynk_ts::TsType::named(ret.clone()),
            }),
            None,
        )];
        let entry = bynk_ts::TsObjectEntry::Method {
            name: f.name.clone(),
            is_async: false,
            // `FnSig` (P6.18's own note) never carries generics here — a
            // forwarded method's own receiver is always the consumer
            // context's own concrete rebranded name, never resolved
            // through a generic receiver.
            generics: Vec::new(),
            params,
            return_type: Some(bynk_ts::TsType::named(ret)),
            // `FnSig` carries no doc text either — nothing forwards it.
            doc: None,
            // #1337's own one real `inline: true` site: the pre-conversion
            // `writeln!` always built the whole entry — signature and
            // one-statement body alike — on one physical line, unlike
            // every other real `Method` entry in this tree.
            inline: true,
            body,
        };
        out.push_str(&bynk_ts::print_object_entry(&entry, 0));
    }
}

/// #1337: returns one real `bynk_ts::TsObjectEntry::Method` (was
/// `out: &mut String`) — the per-entry builder [`emit_attached_methods`]
/// calls once per matching attached method.
///
/// The method's own body is NOT built here at all — it's delegated
/// wholesale to `emit_block_as_function_body_with_return`
/// (`emitter/lower.rs:201`), the one splice boundary ADR
/// `arc-c-lower-rs-permanent-exclusion` names as a *permanent*, deliberate
/// exclusion from this tree (`lower.rs` is the compiler's own second
/// code-generation pass, comprehensive language-surface work Arc C was
/// never scoped to cover) — captured into a `String` at the exact same
/// absolute indent the pre-conversion code always passed
/// (`INDENT_STEP * 2`), then carried as one opaque `bynk_ts::TsStmt::raw`
/// statement, printed exactly as given with no reinterpretation.
fn emit_method(
    f: &FnDecl,
    type_name: &str,
    type_params: &[TypeParam],
    method_name: &Ident,
    commons: &TypedCommons,
    runtime_use: &RuntimeUse,
) -> bynk_ts::TsObjectEntry {
    // #1337: a real gap the accepted proposal's own citation missed (it
    // searched for `///`-style doc markers; this language's own doc block
    // is `---`-delimited, per the lexer — `Timestamp.diff`/`Timestamp.add`
    // in `65_money_uses_time` both carry one) — caught by the zero-diff
    // fixture check, not reasoned about in the abstract.
    let doc = f.documentation.clone();
    // #594: a method on a generic type erases to a generic namespace-object
    // member. The namespace `const` cannot itself carry `<T>`, so the type's own
    // parameters are threaded onto *each* method alongside the method's own
    // (`map<T, U>(self: Box<T>, …)`). `ts_type_params` is empty for a
    // non-generic type, keeping the pre-#594 output byte-identical.
    let self_ty_args = ts_type_params(type_params);
    let mut method_generics: Vec<TypeParam> = type_params.to_vec();
    method_generics.extend(f.type_params.iter().cloned());
    // #1337: a real gap the accepted proposal's own citation missed (it
    // searched the project-form fixture corpus only; the one real site,
    // `402_generic_instance_method`, is single-file form) — caught by the
    // zero-diff fixture check, not reasoned about in the abstract. Bare
    // names only, matching `ts_type_params`'s own rendering exactly (see
    // `TsObjectEntry::Method::generics`'s own doc for why a full
    // `Vec<TsParam>` would be the wrong shape here).
    //
    // Review of #1338: must be a bare, UNESCAPED clone, not `ts_ident`.
    // `ts_ident` is for value identifiers and renames reserved words like
    // `deps`/`static`/`package` — all legal Bynk *type*-parameter names
    // (`parse_optional_type_params` accepts any identifier). `self_ty_args`
    // (via `ts_type_params`, a few lines above) and every `ts_type_ref`
    // param/return type reference these same names unescaped, so escaping
    // only the declaration site here would desync it from its own uses:
    // `map<__id_deps>(self: Box<deps>): Box<deps>` — `deps` undeclared,
    // `__id_deps` unused, a real tsc error the pre-conversion code never
    // had (it built `map<deps>(self: Box<deps>)`, self-consistent).
    let generics: Vec<String> = method_generics
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();
    let mut params: Vec<bynk_ts::TsParam> = Vec::new();
    if f.has_self {
        params.push(bynk_ts::TsParam {
            name: "self".to_string(),
            ty: Some(bynk_ts::TsType::named(format!("{type_name}{self_ty_args}"))),
            optional: false,
        });
    }
    for p in &f.params {
        params.push(bynk_ts::TsParam {
            name: ts_ident(&p.name.name),
            ty: Some(bynk_ts::TsType::named(ts_type_ref(&p.type_ref))),
            optional: false,
        });
    }
    let empty = bynk_check::resolver::CrossContextInfo::default();
    let mut cx = LowerCtx::new(
        ModuleCtx::new(commons, &empty, runtime_use),
        BodyMode::Method,
    );
    // Methods are emitted as plain (non-async) members on an object literal;
    // any `Effect.pure(...)` in tail position must still wrap as
    // `Promise.resolve(...)` because there's no surrounding `async` to absorb
    // it. (Methods aren't expected to return `Effect[T]` in v0–v0.7.1.)
    let mut body_text = String::new();
    emit_block_as_function_body_with_return(
        &mut body_text,
        &f.body,
        &mut cx,
        INDENT_STEP * 2,
        false,
        Some(&f.return_type),
    );
    bynk_ts::TsObjectEntry::Method {
        name: method_name.name.clone(),
        is_async: false,
        generics,
        params,
        return_type: Some(bynk_ts::TsType::named(ts_type_ref(&f.return_type))),
        doc,
        inline: false,
        body: vec![bynk_ts::TsStmt::raw(body_text, None)],
    }
}

/// #1351 (R7.1): `export {async}function {name}{generics}({params}): {ret} {
/// <body> }` is a real [`bynk_ts::TsDecl::Function`] — `params`/`return_type`
/// route through the already-real [`ts_type_ref_to_ts_type`] (P7.9, #1315;
/// this file's own directly-callable private-sibling-visibility precedent,
/// #1339) instead of the opaque pre-printed `String` `ts_type_ref` returns;
/// `generics` is `TsDecl::Function`'s own real gap this proposal closes
/// (bare names, matching every other real generics-list precedent in this
/// crate). The function's own BODY — whichever of the two still-unconverted
/// sources built it (the ordinary lowered block, or
/// `emit_contract_guarded_body`'s own guard-wrapped one, NEITHER converted
/// by this proposal) — is captured into a fresh buffer and carried as ONE
/// opaque [`bynk_ts::TsStmtKind::Raw`] statement, unchanged text, the exact
/// precedent #1337 established for `emit_method`'s own wholesale-delegated
/// body. This function's own exact signature is unchanged, the P7.9/step-1
/// pattern — it never owned a `Verbatim` construction site.
///
/// Review-caught-before-merge, #1351: the body can no longer lower directly
/// into `out` the way the pre-conversion code did (source-map checkpoints
/// were correct there only because the body's own text landed at its real,
/// final position in `out` as it was written). Once the body is captured
/// into its own local `body_text` buffer for embedding as a `Raw`
/// statement, any `record_span` call made *during* that lowering would
/// record an offset relative to `body_text`'s own 0-based length — wrong
/// once spliced elsewhere. Uses the same local-sub-builder-then-`merge`
/// pattern `emit_service`'s own handler-body lowering already established
/// (`LowerCtx::record_span`'s own doc: "a caller building ... into its own
/// local `String` ... before splicing it elsewhere must not call this with
/// that buffer's own length") — `body_smb` collects checkpoints relative to
/// `body_text`, then `merge`s into the real `source_map`, rebased at
/// `body_text`'s own splice offset within the fully-printed declaration.
pub(crate) fn emit_free_fn(
    out: &mut String,
    f: &FnDecl,
    commons: &TypedCommons,
    source_map: Option<&RefCell<SourceMapBuilder>>,
    // v0.115: emit the contract call-site guard (dev/test profile). Stripped
    // (false) in the deploy build for zero runtime cost (DECISION J).
    contracts: bool,
    runtime_use: &RuntimeUse,
) {
    let FnName::Free(name) = &f.name else {
        return;
    };
    emit_doc_block(out, f.documentation.as_deref(), 0);
    let params: Vec<bynk_ts::TsParam> = f
        .params
        .iter()
        .map(|p| bynk_ts::TsParam {
            name: ts_ident(&p.name.name),
            ty: Some(ts_type_ref_to_ts_type(&p.type_ref, None)),
            optional: false,
        })
        .collect();
    let is_async = is_effectful_return(&f.return_type);
    // v0.20a: erased TS generics — the type parameters print verbatim and
    // exist only at TS type-check time (no runtime dispatch).
    let generics: Vec<String> = f
        .type_params
        .iter()
        .map(|tp| tp.name.name.clone())
        .collect();

    let empty = bynk_check::resolver::CrossContextInfo::default();
    let body_smb = RefCell::new(SourceMapBuilder::new());
    let mut cx = LowerCtx::new(
        ModuleCtx::new(commons, &empty, runtime_use),
        BodyMode::FreeFn,
    )
    .with_source_map(Some(&body_smb));
    let async_tail = is_effectful_return(&f.return_type);
    let guarded = contracts && (!f.requires.is_empty() || !f.ensures.is_empty());
    let mut body_text = String::new();
    if guarded {
        emit_contract_guarded_body(&mut body_text, f, &mut cx, async_tail);
    } else {
        emit_block_as_function_body_with_return(
            &mut body_text,
            &f.body,
            &mut cx,
            INDENT_STEP,
            async_tail,
            Some(&f.return_type),
        );
    }

    let func_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::Function {
            name: ts_ident(&name.name),
            generics,
            params,
            return_type: Some(ts_type_ref_to_ts_type(&f.return_type, None)),
            body: vec![bynk_ts::TsStmt::raw(body_text.clone(), None)],
            is_async,
        })),
        None,
    );
    let printed = bynk_ts::print_stmt(&func_decl, 0);
    // `Raw`'s own text is spliced verbatim (`printer.rs`'s own documented
    // guarantee), so its offset within `printed` is exact arithmetic, not a
    // search: everything before it is the header/opening-brace text, and
    // everything after it is the fixed closing `"}\n"` `TsDecl::Function`'s
    // own render arm always appends.
    //
    // Review of #1352, finding 1: this arithmetic silently encodes 3
    // invariants of a renderer in another crate (Raw splices verbatim,
    // `TsDecl::Function` always ends in exactly `"}\n"`, `print_stmt` is
    // called at depth 0) — if any of those ever changed, the failure mode
    // is not a compile error or a text diff, it's a silently wrong source
    // map, the exact bug class this function exists to fix. Guarded loudly
    // rather than trusted silently.
    debug_assert!(
        printed.ends_with(&format!("{body_text}}}\n")),
        "emit_free_fn: Raw body is no longer the verbatim tail of the printed decl"
    );
    let body_offset_in_printed = printed.len() - body_text.len() - "}\n".len();
    let base = out.len() + body_offset_in_printed;
    out.push_str(&printed);
    if let Some(module) = source_map {
        module
            .borrow_mut()
            .merge(&body_smb.borrow(), &body_text, out, base, 0);
    }
    writeln!(out).unwrap();
}

/// #1353 (R7.1): closes step (4) of the design pass's own decomposition
/// order — `emit_free_fn`'s own outer wrapper landed in #1352; this converts
/// `emit_contract_guarded_body`'s own real remainder. Each precondition/
/// postcondition guard (`if (!(pred)) { const __e = new Error(`...`);
/// __e.name = "BynkContractError"; throw __e; }`) is a real
/// [`bynk_ts::TsStmt::if_stmt`] over a real [`bynk_ts::TsStmt::inline_block`]
/// of 3 real statements (`const_stmt`/`assign`/[`bynk_ts::TsStmt::throw_stmt`]
/// — `Throw` is this proposal's own real gap, `bynk_ts::TsStmtKind` had no
/// bare `throw` before now), printed through [`bynk_ts::print_stmt`]; the
/// trailing `return result;` is a real [`bynk_ts::TsStmt::return_stmt`].
///
/// `pred` (`Pre::lower`'s own return) and each hoisted `pre.stmts()` line
/// stay opaque — carried the same way #1335's own `emit_pred_check` carries
/// `pred_condition_and_message`'s `cond` (an `Ident` wrapped in this crate's
/// real `Unary::Not`/`Paren`), but for a DIFFERENT, stronger reason: `Pre`
/// lives in `emitter/lower.rs` (`pre.lower` calls `lower_expr`, the general
/// expression lowerer) — the one splice boundary ADR
/// `arc-c-lower-rs-permanent-exclusion` names a *permanent* Arc C exclusion,
/// not temporarily-unconverted-but-convertible-later machinery like
/// `pred_condition_and_message` (which lives in `emitter.rs` proper). The
/// error message (backtick-quoted, with its own already-escaped `\``
/// clause-name delimiters and `${param}=...`-style interpolations already
/// baked in as literal text by `param_dump`) is carried the same way as one
/// opaque [`bynk_ts::TsExpr::template_lit`] part with zero real `exprs` —
/// `parts` prints verbatim, so this reproduces the exact byte output with no
/// decomposition needed.
///
/// The result-capturing IIFE (`const result = (async () => { <body> }
/// )();`/`const result = (() => { <body> })();`) stays ONE opaque
/// [`bynk_ts::TsStmt::raw`] statement — not decomposed into a real
/// `Const`/`Arrow`/`Call` — matching #1352's own identical treatment of
/// `emit_free_fn`'s unguarded body one level up: `TsExpr::Arrow`'s own
/// `body` is expression-only (#1327's own deliberate choice — a real
/// block-body variant "would have needed new flatten-every-nested-statement
/// ... printer machinery, disproportionate" for an analogous single site),
/// and this IIFE's own body is a genuine multi-statement block, not an
/// expression. `emit_block_as_function_body_with_return`'s own output
/// (still unconverted, out of scope) is captured into a local buffer inside
/// that one opaque statement, the same way it always has been.
///
/// This function's own exact `out: &mut String` signature is unchanged, the
/// P7.9/step-1 pattern one level deeper than #1352's own conversion — it
/// never owned a `Verbatim` construction site (still spliced into
/// `emit_free_fn`'s own `body_text` buffer, itself carried as `emit_free_
/// fn`'s own `Raw` statement).
///
/// v0.115: emit a contracted free function's body behind the dev/test call-site
/// guard (DECISION J). Preconditions (`requires`) are checked on entry; the body
/// runs into a captured `result`; postconditions (`ensures`) are checked over
/// `result` before it is returned. Each violation throws with the clause name
/// and the offending argument/`result` values. This wrapper is emitted only in
/// the dev/test profile and is O(1) in code size (one guard, not per call site).
fn emit_contract_guarded_body(out: &mut String, f: &FnDecl, cx: &mut LowerCtx, async_tail: bool) {
    let FnName::Free(name) = &f.name else {
        return;
    };
    let fn_name = name.name.clone();
    // A `${…}` interpolation of each in-scope value for the failure report.
    let param_dump = |extra_result: bool| -> String {
        let mut parts: Vec<String> = f
            .params
            .iter()
            .filter(|p| p.name.name != "_")
            .map(|p| format!("{n}=${{{v}}}", n = p.name.name, v = ts_ident(&p.name.name)))
            .collect();
        if extra_result {
            parts.push("result=${result}".to_string());
        }
        parts.join(", ")
    };
    // Precondition guards — parameters are the TS function params, in scope.
    for c in &f.requires {
        let mut pre = Pre::new();
        let pred = pre.lower(&c.predicate, cx);
        for s in pre.stmts() {
            out.push_str(&bynk_ts::print_stmt(
                &bynk_ts::TsStmt::raw(format!("  {s}\n"), None),
                0,
            ));
        }
        let msg = format!(
            "contract violated: precondition \\`{clause}\\` of {fn_name} ({dump})",
            clause = c.name.name,
            dump = param_dump(false),
        );
        out.push_str(&bynk_ts::print_stmt(
            &contract_guard_if_stmt(&pred, &msg),
            1,
        ));
    }
    // Run the original body, capturing its value as `result` for the `ensures`.
    //
    // Written directly into `out`, NOT captured into a local buffer and
    // wrapped as one opaque `TsStmt::raw` the way #1339's own `emit_refined_
    // type` does for `emit_refined_checks`'s output — that precedent is safe
    // only because `emit_refined_checks` never touches source-map recording.
    // `emit_block_as_function_body_with_return` calls `cx.record_span(out.
    // len(), ...)` internally: a local buffer's own length starts at 0, so
    // any checkpoint recorded during its lowering would land at the WRONG
    // offset once its text is later spliced into `out` at a non-zero
    // position — the exact bug #1352 already found and fixed one level up
    // (in `emit_free_fn`, for `body_text` itself); introducing a second
    // local buffer HERE reproduces it one level deeper, silently, since
    // `out` (this function's own parameter, = `emit_free_fn`'s `body_text`)
    // is the one buffer whose offsets `emit_free_fn`'s own `body_smb`/
    // `merge` machinery is already correctly rebased against. Writing here
    // exactly as the pre-conversion code did — directly into `out`, no
    // wrapping — keeps every `record_span` call's own `out.len()` correct
    // by construction, with no merge of its own needed at this level.
    if async_tail {
        writeln!(out, "  const result = await (async () => {{").unwrap();
    } else {
        writeln!(out, "  const result = (() => {{").unwrap();
    }
    emit_block_as_function_body_with_return(
        out,
        &f.body,
        cx,
        INDENT_STEP * 2,
        async_tail,
        Some(&f.return_type),
    );
    writeln!(out, "  }})();").unwrap();
    // Postcondition guards — `result` (and the parameters) are in scope.
    for c in &f.ensures {
        let mut pre = Pre::new();
        let pred = pre.lower(&c.predicate, cx);
        for s in pre.stmts() {
            out.push_str(&bynk_ts::print_stmt(
                &bynk_ts::TsStmt::raw(format!("  {s}\n"), None),
                0,
            ));
        }
        let msg = format!(
            "contract violated: postcondition \\`{clause}\\` of {fn_name} ({dump})",
            clause = c.name.name,
            dump = param_dump(true),
        );
        out.push_str(&bynk_ts::print_stmt(
            &contract_guard_if_stmt(&pred, &msg),
            1,
        ));
    }
    out.push_str(&bynk_ts::print_stmt(
        &bynk_ts::TsStmt::return_stmt(Some(bynk_ts::TsExpr::Ident("result".to_string())), None),
        1,
    ));
}

/// Shared builder for the contract call-site guard's own real `if (!(pred))
/// { const __e = new Error(`msg`); __e.name = "BynkContractError"; throw
/// __e; }` shape — identical for a `requires`/`ensures` clause, differing
/// only in `pred`/`msg`, both still-opaque text (see `emit_contract_guarded_
/// body`'s own doc for why).
fn contract_guard_if_stmt(pred: &str, msg: &str) -> bynk_ts::TsStmt {
    let cond = bynk_ts::TsExpr::Unary {
        op: bynk_ts::TsUnaryOp::Not,
        expr: Box::new(bynk_ts::TsExpr::Paren(Box::new(bynk_ts::TsExpr::Ident(
            pred.to_string(),
        )))),
    };
    let new_error = bynk_ts::TsExpr::New {
        callee: Box::new(bynk_ts::TsExpr::Ident("Error".to_string())),
        args: vec![bynk_ts::TsExpr::template_lit(
            vec![msg.to_string()],
            Vec::new(),
        )],
    };
    let const_e = bynk_ts::TsStmt::const_stmt(
        bynk_ts::TsBindingName::Ident("__e".to_string()),
        None,
        new_error,
        None,
    );
    let assign_name = bynk_ts::TsStmt::assign(
        bynk_ts::TsExpr::Member {
            object: Box::new(bynk_ts::TsExpr::Ident("__e".to_string())),
            property: "name".to_string(),
        },
        bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str("BynkContractError".to_string())),
        None,
    );
    let throw_e = bynk_ts::TsStmt::throw_stmt(bynk_ts::TsExpr::Ident("__e".to_string()), None);
    let then_branch = bynk_ts::TsStmt::inline_block(vec![const_e, assign_name, throw_e], None);
    bynk_ts::TsStmt::if_stmt(cond, then_branch, None)
}

#[cfg(test)]
mod emit_free_fn_tests {
    use crate::testkit::emit_source;

    /// #1351: pins `emit_free_fn`'s own real generic/non-generic output
    /// byte-for-byte against `198_generic_identity_compose`'s own real
    /// `expected.ts` (`bynkc/tests/fixtures/positive/198_generic_identity_compose`)
    /// — a generic single-param function, a multi-generic multi-param one
    /// (function-typed params, exercising `TsParam.ty`'s own `Fn` arm), and
    /// a plain non-generic one, transcribed directly from that fixture.
    #[test]
    fn generic_and_plain_free_functions_match_the_real_fixtures_own_bytes() {
        let ts = emit_source(
            r#"
commons generic

fn identity[A](x: A) -> A {
  x
}

fn compose[A, B, C](f: A -> B, g: B -> C, x: A) -> C {
  g(f(x))
}

fn demo() -> Int {
  identity(5)
}
"#,
        );
        assert!(
            ts.contains("export function identity<A>(x: A): A {\n  return x;\n}\n"),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export function compose<A, B, C>(f: (a0: A) => B, g: (a0: B) => C, x: A): C {\n  \
                 return g(f(x));\n}\n"
            ),
            "{ts}"
        );
        assert!(
            ts.contains("export function demo(): number {\n  return identity(5);\n}\n"),
            "{ts}"
        );
    }

    /// #1351: pins the `async`/effectful-return case — `is_effectful_return`
    /// gates both the header's own `async` keyword and `async_tail`'s body
    /// lowering, unchanged by this conversion.
    #[test]
    fn an_effectful_free_function_emits_async_and_no_generics() {
        let ts = emit_source(
            r#"
commons effectful

fn compute() -> Effect[Int] {
  1
}
"#,
        );
        assert!(
            ts.contains("export async function compute(): Promise<number> {\n"),
            "{ts}"
        );
    }
}

/// Synthesise a TypeScript-safe method name for an `on http METHOD path`
/// handler. The result is used both as the key on the service object and
/// as the identifier the Worker fetch handler invokes. Path parameter
/// segments (`:name`) become `Param_name` to remain distinct from literal
/// segments. (v0.9 §5.3)
pub(crate) fn http_handler_method_name(method: HttpMethod, path: &str) -> String {
    http_handler_method_name_from_str(method.as_str(), path)
}

/// P6.51 (design/tracks/the-ir.md §6b): [`http_handler_method_name`]'s own
/// `IrHttpMethod` sibling, for the call sites that already hold an
/// `IrHandlerKind::Http`'s own resolved method rather than the raw AST
/// `HandlerKind::Http`'s. Both share [`http_handler_method_name_from_str`],
/// since the two `HttpMethod`/`IrHttpMethod` enums render to identical
/// strings via their own respective `as_str()`.
pub(crate) fn http_handler_method_name_ir(method: IrHttpMethod, path: &str) -> String {
    http_handler_method_name_from_str(method.as_str(), path)
}

fn http_handler_method_name_from_str(method: &str, path: &str) -> String {
    let mut s = format!("http_{method}");
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
                    let pair = match lower_handler_kind_ir(&h.kind) {
                        IrHandlerKind::Http { method, path } => (
                            http_handler_method_name_ir(method, &path),
                            format!("{} \"{}\"", method.as_str(), path),
                        ),
                        IrHandlerKind::Cron { expr } => {
                            let n = cron_handler_method_name(&s.name.name, cron_idx);
                            cron_idx += 1;
                            (n, format!("cron \"{expr}\""))
                        }
                        IrHandlerKind::Message
                            if matches!(
                                lower_protocol_ir_from_commons(&s.protocol, commons),
                                ProtocolIr::WebSocket { .. }
                            ) =>
                        {
                            ("message".to_string(), "WebSocket message".to_string())
                        }
                        IrHandlerKind::Message => {
                            let n = queue_handler_method_name(&s.name.name, queue_idx);
                            queue_idx += 1;
                            (n, "message".to_string())
                        }
                        IrHandlerKind::Call => {
                            ("call".to_string(), handler_op_label("call", &h.params))
                        }
                        IrHandlerKind::Open => ("open".to_string(), "WebSocket open".to_string()),
                        IrHandlerKind::Close => {
                            ("close".to_string(), "WebSocket close".to_string())
                        }
                        // Events track, slice 0 (spine #936): exactly one
                        // `on event` per `from Events(E)` service (no
                        // pattern-refinement fan-out yet, so no index).
                        IrHandlerKind::Event => ("event".to_string(), "event".to_string()),
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
        out.push_str(&bynk_project::json_string(k));
        out.push(':');
        out.push_str(&bynk_project::json_string(v));
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
            let given = crate::ir::lower::lower_provider_given_ir(p);
            let mut deps: Vec<&str> = given
                .iter()
                .filter(|d| d.context.is_none())
                .map(|d| d.name.as_str())
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

/// One message entry's TS renderer: `(params) => <expr>`. A literal-only
/// template collapses to a plain string; a template with placeholders becomes
/// a `+`-concatenation, each placeholder substituted from `params` via
/// `renderArg` (imported from `bynk.locale`, never duplicated here) when
/// present, else left as the literal `{name}` token — total, never throws,
/// matching `render`'s own totality contract.
///
/// message-bundles slice 3 (#878): an ICU-dispatch placeholder (`inner`
/// contains a `,`) instead becomes `emit_icu_placeholder`'s codegen, which
/// needs `locale_tag` to construct `Intl.*` at runtime. `Err(_)` (a
/// malformed ICU construct) is unreachable in practice — the checker
/// (`bynk.messages.malformed_icu_syntax`) already rejects it before emission
/// runs on an error-free project — but is handled totally, matching this
/// function's own "never throws" contract, rather than `unreachable!()`.
fn emit_message_entry_renderer(
    out: &mut String,
    entry: &MessageEntry,
    locale_tag: &str,
    runtime_use: &RuntimeUse,
) {
    write!(out, "(params: ReadonlyMap<string, MessageArg>): string => ").unwrap();
    let segments = split_template(&entry.template);
    let parts: Vec<String> = segments
        .iter()
        .map(|s| match s {
            TemplateSegment::Literal(lit) => format!("\"{}\"", escape_ts_string(lit)),
            TemplateSegment::Placeholder { inner, .. } => match inner.find(',') {
                None => {
                    let key = format!("\"{}\"", escape_ts_string(inner));
                    let fallback = format!("\"{{{}}}\"", escape_ts_string(inner));
                    format!(
                        "(params.get({key}) !== undefined ? renderArg(params.get({key}) as MessageArg) : {fallback})"
                    )
                }
                Some(_) => match icu::parse_icu_placeholder(inner) {
                    Ok(p) => emit_icu_placeholder(&p, locale_tag, runtime_use),
                    Err(_) => {
                        let name = inner.split(',').next().unwrap_or(inner).trim();
                        format!("\"{{{}}}\"", escape_ts_string(name))
                    }
                },
            },
        })
        .collect();
    if parts.is_empty() {
        write!(out, "\"\"").unwrap();
    } else {
        write!(out, "{}", parts.join(" + ")).unwrap();
    }
}

/// message-bundles slice 3 (#878): codegen for one ICU-dispatch placeholder.
/// Each kind emits as an IIFE (matching this file's own "non-tail dispatch
/// becomes an IIFE" idiom) guarding on the looked-up `MessageArg`'s `.tag`
/// before touching `.value` — TS narrows `__arg` to the matching variant
/// after the guard, so no cast is needed. Falls back to the literal
/// `"{name}"` text when the param is missing or the wrong `MessageArg`
/// variant, matching the plain-placeholder fallback's own convention.
fn emit_icu_placeholder(
    p: &icu::IcuPlaceholder<'_>,
    locale_tag: &str,
    runtime_use: &RuntimeUse,
) -> String {
    // Recorded per-arm, not once up front: a `select` placeholder emits
    // `Object.hasOwn` over an arm table and no formatter at all, so noting here
    // would import the three helpers into a select-only bundle that never calls
    // them. The three are imported as a group, so one note per helper-emitting
    // arm is enough.
    let tag_lit = format!("\"{}\"", escape_ts_string(locale_tag));
    let key = format!("\"{}\"", escape_ts_string(p.name));
    let fallback = format!("\"{{{}}}\"", escape_ts_string(p.name));
    let arg = format!("params.get({key})");
    match &p.kind {
        icu::PlaceholderKind::Plural { arms } => {
            runtime_use.note_icu();
            let arms_obj: Vec<String> = arms
                .iter()
                .map(|(cat, segs)| {
                    format!(
                        "\"{}\": {}",
                        cat.as_str(),
                        emit_sub_message(segs, &tag_lit, runtime_use)
                    )
                })
                .collect();
            format!(
                "((__arg) => __arg === undefined || (__arg.tag !== \"Whole\" && __arg.tag !== \"Num\") ? {fallback} : selectPluralArm({tag_lit}, __arg.value, {{ {} }}))({arg})",
                arms_obj.join(", ")
            )
        }
        icu::PlaceholderKind::Select { arms } => {
            let arms_obj: Vec<String> = arms
                .iter()
                .map(|(k, segs)| {
                    format!(
                        "\"{}\": {}",
                        escape_ts_string(k),
                        emit_sub_message(segs, &tag_lit, runtime_use)
                    )
                })
                .collect();
            // `Object.hasOwn` (not `?? __arms["other"]`): the arm table is an
            // object literal, so a runtime `__arg.value` naming an
            // `Object.prototype` member (`"constructor"`, `"toString"`,
            // `"__proto__"`) would resolve off the prototype chain and never
            // reach the mandatory `other` arm — `??` only fires on
            // null/undefined, and an inherited method is neither (#900). The
            // dispatch's real question is own-property presence. `Object.hasOwn`
            // is ES2022, which `emit_tsconfig` already targets.
            format!(
                "((__arg) => {{ if (__arg === undefined || __arg.tag !== \"Text\") {{ return {fallback}; }} const __arms: Record<string, string> = {{ {} }}; return Object.hasOwn(__arms, __arg.value) ? __arms[__arg.value] : __arms[\"other\"]; }})({arg})",
                arms_obj.join(", ")
            )
        }
        icu::PlaceholderKind::Number { style } => {
            runtime_use.note_icu();
            let style_arg = style
                .map(|s| format!(", \"{}\"", s.as_str()))
                .unwrap_or_default();
            format!(
                "((__arg) => __arg !== undefined && (__arg.tag === \"Whole\" || __arg.tag === \"Num\") ? formatIcuNumber({tag_lit}, __arg.value{style_arg}) : {fallback})({arg})"
            )
        }
        icu::PlaceholderKind::Date { style } => {
            runtime_use.note_icu();
            let style_arg = style
                .map(|s| format!(", \"{}\"", s.as_str()))
                .unwrap_or_default();
            format!(
                "((__arg) => __arg !== undefined && __arg.tag === \"Moment\" ? formatIcuDate({tag_lit}, __arg.value{style_arg}) : {fallback})({arg})"
            )
        }
    }
}

/// Turns one `plural`/`select` arm's sub-message into a `+`-joined TS
/// expression, evaluated inside `emit_icu_placeholder`'s IIFE where `__arg`
/// is already in scope and narrowed. `Hash` only ever occurs in a `plural`
/// arm (the parser's `allow_hash` guarantees a `select` arm never contains
/// one), where `__arg.value: number` after narrowing.
fn emit_sub_message(segs: &[icu::SubSegment], tag_lit: &str, runtime_use: &RuntimeUse) -> String {
    if segs.is_empty() {
        return "\"\"".to_string();
    }
    segs.iter()
        .map(|seg| match seg {
            icu::SubSegment::Literal(s) => format!("\"{}\"", escape_ts_string(s)),
            // A bare `#` renders the argument as a number. Recorded here even
            // though the only arm that can contain one is `plural` (see this
            // function's doc), which records for itself: the note belongs with
            // the emission, so this stays correct if that invariant ever moves.
            icu::SubSegment::Hash => {
                runtime_use.note_icu();
                format!("formatIcuNumber({tag_lit}, __arg.value)")
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Compiles every `messages` block in one commons into one `code ->
/// renderer` lookup per locale, a shared dispatch object keyed by locale tag,
/// and one bundle-scoped `render(tag, msg)` that looks up the resolved
/// `tag`'s own table, else the `@reference` locale's, else falls back to
/// `bynk.locale`'s own `render` (imported under the private alias
/// `__bynkLocaleRender` — `emit_unit`, project.rs — since this file's own
/// `render` would otherwise collide with the imported name) for any code no
/// declared locale covers. Also exports the bundle's declared-locale set and
/// reference tag (message-bundles slice 2, #874, ADR 0273) — the
/// precondition Locale's own slice 2 (negotiation) needs before it can
/// start.
///
/// `blocks` is every `CommonsItem::Messages` in the commons, in source
/// order; `reference` is the one block among them carrying `@reference` —
/// the caller (`emit_project`) already found it to decide *whether* to call
/// this function at all, so it's passed in rather than re-derived here (PR
/// #875 review — a harmless but needless second scan).
/// #1355 (R7.1): converts this function's own outer construction to real
/// `bynk_ts` nodes, with two deliberate, named opaque carve-outs.
///
/// **`messagesByLocale`'s own header/type-annotation/closing-brace stay
/// hand-written text** — `Record<string, Record<string, (params: ReadonlyMap
/// <string, MessageArg>) => string>>` needs its inner function type's one
/// parameter named `params`; `bynk_ts::TsType::Fn`'s own `params` are
/// deliberately anonymous, printer-numbered positionally (`a0`, `a1`, …) —
/// the same "an odd, one-off type shape stays opaque text" precedent P7.9
/// (#1315) already used for `Query[T]`'s own extra-paren-wrapped shape, not
/// a general `TsType::Fn` redesign for this one call site. Each LOCALE's own
/// entry (and each per-code entry nested inside it) IS a real
/// [`bynk_ts::TsObjectEntry`], though — printed through the shared
/// [`bynk_ts::print_object_entry`] fragment entry point directly into the
/// hand-written wrapper, the same "return/print real entries into a
/// still-hand-written enclosing literal" shape `emit_attached_methods`'s own
/// callers already use (#1337) — chosen here specifically because
/// `emit_doc_block`'s own per-locale doc comment has no real node shape to
/// intersperse with today (`TsObjectEntry::Prop` carries no `doc` field the
/// way `Method`'s own does; adding one for this single narrow need would be
/// disproportionate). Each per-code entry's own VALUE —
/// `emit_message_entry_renderer`'s output, one of step (11)'s own named,
/// not-yet-proposed ICU-formatting cluster — stays opaque, carried as a
/// [`bynk_ts::TsExpr::Ident`] wrapping already-formed JS, the established
/// "call an unconverted sibling helper, carry its text opaquely" pattern
/// this whole track uses.
///
/// **`messagesReferenceLocale`/`messagesLocales`/`render` convert fully** —
/// no opaque carve-outs, every shape they need (`TsExpr::As`, `TsExpr::
/// Index`, `TsExpr::Conditional`, `TsBinaryOp::NullishCoalescing`, `TsStmt::
/// If`/`Return`) already exists.
pub(crate) fn emit_messages_bundle(
    out: &mut String,
    blocks: &[&MessagesDecl],
    reference: &MessagesDecl,
    runtime_use: &RuntimeUse,
) {
    // One `code -> renderer` table per locale, inlined into a single
    // `messagesByLocale` object literal keyed by tag. No per-locale `const
    // __messages_<tag>` binding: a locale tag can be `"pt-BR"`, which is not a
    // valid TS identifier, so a named binding would be a syntax error — the
    // object is keyed by the tag *string* and needs no binding of its own.
    writeln!(
        out,
        "const messagesByLocale: Record<string, Record<string, (params: ReadonlyMap<string, MessageArg>) => string>> = {{"
    )
    .unwrap();
    for m in blocks {
        emit_doc_block(out, m.documentation.as_deref(), INDENT_STEP);
        let code_entries: Vec<bynk_ts::TsObjectEntry> = m
            .entries
            .iter()
            .map(|entry| {
                let mut renderer_text = String::new();
                emit_message_entry_renderer(&mut renderer_text, entry, &m.tag, runtime_use);
                bynk_ts::TsObjectEntry::Prop(
                    format!("\"{}\"", escape_ts_string(&entry.code)),
                    bynk_ts::TsExpr::Ident(renderer_text),
                )
            })
            .collect();
        let locale_entry = bynk_ts::TsObjectEntry::Prop(
            format!("\"{}\"", escape_ts_string(&m.tag)),
            bynk_ts::TsExpr::multiline_object_entries(code_entries),
        );
        out.push_str(&bynk_ts::print_object_entry(&locale_entry, 0));
    }
    writeln!(out, "}};").unwrap();
    writeln!(out).unwrap();

    // `("tag" as string) as LocaleTag` — the inner `as` is wrapped in an
    // explicit `Paren`: `as` is left-associative, so the un-parenthesised
    // `"tag" as string as LocaleTag` is the identical cast grammatically,
    // but the pre-conversion text always parenthesised it, and `As`'s own
    // renderer does not auto-add parens around a nested `As` operand. The
    // same "explicit `Paren` always prints its own literal parens"
    // precedent #1323 established.
    //
    // Review of #1356, finding 1: `tag` is the RAW locale tag, not
    // `escape_ts_string`-escaped — `TsLit::Str`'s own renderer already
    // escapes (byte-identical to `escape_ts_string`, deliberately, P7.8),
    // so pre-escaping here would double-escape any backslash/quote a tag
    // ever contains, corrupting the literal's own decoded value. Not
    // reachable today (`LocaleTag`'s own `Matches(...)` refinement rejects
    // anything but letters/digits/hyphens) but a real, latent bug had the
    // caller pre-escaped, the same class of hazard #1335's own `msg`
    // deviation was written to avoid. `messagesByLocale`'s own `Prop` KEY a
    // few lines up is the opposite, correct case: a key is spliced
    // verbatim, never run through a printer escaper, so it needs the
    // explicit `escape_ts_string` call to be safe at all.
    let tag_cast = |tag: &str| bynk_ts::TsExpr::As {
        expr: Box::new(bynk_ts::TsExpr::Paren(Box::new(bynk_ts::TsExpr::As {
            expr: Box::new(bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(tag.to_string()))),
            ty: bynk_ts::TsType::named("string"),
        }))),
        ty: bynk_ts::TsType::named("LocaleTag"),
    };
    let ref_locale_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::ConstDecl {
            name: "messagesReferenceLocale".to_string(),
            ty: Some(bynk_ts::TsType::named("LocaleTag")),
            init: tag_cast(&reference.tag),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&ref_locale_decl, 0));

    let locale_list: Vec<bynk_ts::TsExpr> = blocks.iter().map(|m| tag_cast(&m.tag)).collect();
    let locales_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::ConstDecl {
            name: "messagesLocales".to_string(),
            ty: Some(bynk_ts::TsType::readonly_array(bynk_ts::TsType::named(
                "LocaleTag",
            ))),
            init: bynk_ts::TsExpr::array(locale_list),
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&locales_decl, 0));
    writeln!(out).unwrap();

    let ident = |s: &str| bynk_ts::TsExpr::Ident(s.to_string());
    let member = |object: bynk_ts::TsExpr, property: &str| bynk_ts::TsExpr::Member {
        object: Box::new(object),
        property: property.to_string(),
    };
    let index = |object: bynk_ts::TsExpr, index: bynk_ts::TsExpr| bynk_ts::TsExpr::Index {
        object: Box::new(object),
        index: Box::new(index),
    };
    let not_undefined = |e: bynk_ts::TsExpr| bynk_ts::TsExpr::Binary {
        op: bynk_ts::TsBinaryOp::StrictNotEq,
        left: Box::new(e),
        right: Box::new(ident("undefined")),
    };

    let local_table_decl = bynk_ts::TsStmt::const_stmt(
        bynk_ts::TsBindingName::Ident("__localeTable".to_string()),
        None,
        index(ident("messagesByLocale"), ident("tag")),
        None,
    );
    let reference_table_decl = bynk_ts::TsStmt::const_stmt(
        bynk_ts::TsBindingName::Ident("__referenceTable".to_string()),
        None,
        index(ident("messagesByLocale"), ident("messagesReferenceLocale")),
        None,
    );
    let entry_ternary = bynk_ts::TsExpr::Paren(Box::new(bynk_ts::TsExpr::Conditional {
        test: Box::new(not_undefined(ident("__localeTable"))),
        consequent: Box::new(index(ident("__localeTable"), member(ident("msg"), "code"))),
        alternate: Box::new(ident("undefined")),
    }));
    let entry_decl = bynk_ts::TsStmt::const_stmt(
        bynk_ts::TsBindingName::Ident("__entry".to_string()),
        None,
        bynk_ts::TsExpr::Binary {
            op: bynk_ts::TsBinaryOp::NullishCoalescing,
            left: Box::new(entry_ternary),
            right: Box::new(index(
                ident("__referenceTable"),
                member(ident("msg"), "code"),
            )),
        },
        None,
    );
    let if_entry = bynk_ts::TsStmt::if_stmt(
        not_undefined(ident("__entry")),
        bynk_ts::TsStmt::block(
            vec![bynk_ts::TsStmt::return_stmt(
                Some(bynk_ts::TsExpr::Call {
                    callee: Box::new(ident("__entry")),
                    args: vec![member(ident("msg"), "params")],
                }),
                None,
            )],
            None,
        ),
        None,
    );
    let fallback_return = bynk_ts::TsStmt::return_stmt(
        Some(bynk_ts::TsExpr::Call {
            callee: Box::new(ident("__bynkLocaleRender")),
            args: vec![ident("tag"), ident("msg")],
        }),
        None,
    );

    let render_fn = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::Function {
            name: "render".to_string(),
            generics: Vec::new(),
            params: vec![
                bynk_ts::TsParam {
                    name: "tag".to_string(),
                    ty: Some(bynk_ts::TsType::named("LocaleTag")),
                    optional: false,
                },
                bynk_ts::TsParam {
                    name: "msg".to_string(),
                    ty: Some(bynk_ts::TsType::named("Message")),
                    optional: false,
                },
            ],
            return_type: Some(bynk_ts::TsType::named("string")),
            body: vec![
                local_table_decl,
                reference_table_decl,
                entry_decl,
                if_entry,
                fallback_return,
            ],
            is_async: false,
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&render_fn, 0));
    writeln!(out).unwrap();
}

// -- v0.5 emission --

/// P6.x (#1193, slice 3 of #1187, narrowed — `Provider` deferred, see this
/// issue's own Framing): reads each op's resolved `params`/`return_ty` off
/// `ops` (`bynk-emit::ir::OpSig`, `lower_capability_item_ir`'s own return
/// value) through `ts_ty`, instead of walking `CapabilityOp::params`/
/// `return_type` `TypeRef`s through `ts_type_ref` directly. `c` is still
/// needed alongside `ops` — `IrItem::Capability::def` is a bare `String`,
/// carrying neither `c.documentation`/`op.documentation` nor the `Arc` back
/// to the declaration #1188 could rely on for `Type` ([DECISION A]).
/// `ops`/`c.ops` are zipped by index: both are built in declaration order
/// (`IrItem::Capability::ops`'s own doc comment), never reordered by either
/// side.
///
/// Accepted divergence (review of #1194, widening #1193's own disclosure):
/// `resolve_type_ref` returns `None` — falling back to `Unit`/`void` here —
/// for two shapes `ts_type_ref` rendered directly before this slice: an
/// unresolvable type name (`void` instead of the raw bad name) and
/// `TypeRef::History` (`void` instead of `ts_type_ref`'s own `"never"`,
/// `emitter.rs:4127`). Both are reachable only through a capability op,
/// since the resolver skips `CommonsItem::Capability` outright — no program
/// the checker actually validates can reach either.
/// #1357 (R7.1): both declarations this function builds are now real
/// `bynk_ts` nodes. `export interface {Name} { op<T>(params): ret; ... }` is
/// a real [`bynk_ts::TsDecl::Interface`] over real, per-op
/// [`bynk_ts::TsTypeMember::Method`] entries — `generics`/`doc` are this
/// slice's own real gap (bare names for `generics`, matching every other
/// real generics-list precedent in this crate; `doc` mirrors
/// [`bynk_ts::TsObjectEntry::Method.doc`]'s own identical field, #1337) —
/// params route through the already-real [`ts_ty_to_ts_type`] (P7.9,
/// #1315) instead of the opaque pre-printed `String` `ts_ty` returns. The
/// injection token (`export const {Name}Token: unique symbol =
/// Symbol("{Name}");`) is a real [`bynk_ts::TsDecl::ConstDecl`] — `unique
/// symbol` stays one opaque `TsType::named` string, the same "an odd,
/// one-off type shape stays opaque text" precedent P7.9 already used for
/// `Query[T]`'s own extra-paren-wrapped shape (nothing else in this crate
/// builds a `unique symbol` type). This function's own exact signature is
/// unchanged, the P7.9/step-1 pattern — it never owned a `Verbatim`
/// construction site.
pub(crate) fn emit_capability(
    out: &mut String,
    c: &CapabilityDecl,
    ops: &[OpSig],
    commons: &TypedCommons,
) {
    emit_doc_block(out, c.documentation.as_deref(), 0);
    // Review of #1194: `zip` silently truncates to the shorter side — this
    // guards the by-index pairing invariant `ops`/`c.ops` are documented
    // (not enforced) to share, so a future second caller that passes a
    // mismatched `ops` loses trailing interface methods loudly instead of
    // silently.
    debug_assert_eq!(
        c.ops.len(),
        ops.len(),
        "`ops` is zipped with `c.ops` by index — the two must be the same lowering"
    );
    let members: Vec<bynk_ts::TsTypeMember> = c
        .ops
        .iter()
        .zip(ops)
        .map(|(op, sig)| {
            let params: Vec<bynk_ts::TsParam> = sig
                .params
                .iter()
                .map(|(name, ty)| bynk_ts::TsParam {
                    name: ts_ident(name),
                    ty: Some(ts_ty_to_ts_type(*ty, &commons.ty_intern)),
                    optional: false,
                })
                .collect();
            // #926 (Decision C): a genuine generic TS interface method, no
            // monomorphisation/erasure. Reads `op.type_params` (not
            // `sig.type_params`): it only extracts each `TypeParam`'s own
            // name, no type resolution, and `OpSig::type_params` exists for
            // `lower_op_sig_ir`'s own rigid-variable scoping, not for this
            // to render from (Decision A/B, #1193).
            bynk_ts::TsTypeMember::Method {
                name: op.name.name.clone(),
                generics: op
                    .type_params
                    .iter()
                    .map(|tp| tp.name.name.clone())
                    .collect(),
                params,
                ret: ts_ty_to_ts_type(sig.return_ty, &commons.ty_intern),
                doc: op.documentation.clone(),
            }
        })
        .collect();
    let interface = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::Interface {
            name: c.name.name.clone(),
            type_params: Vec::new(),
            members,
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&interface, 0));
    writeln!(out).unwrap();

    // Injection token (symbol carrying the interface type).
    let token_decl = bynk_ts::TsStmt::decl(
        bynk_ts::TsDecl::Export(Box::new(bynk_ts::TsDecl::ConstDecl {
            name: format!("{}Token", c.name.name),
            ty: Some(bynk_ts::TsType::named("unique symbol")),
            init: bynk_ts::TsExpr::Call {
                callee: Box::new(bynk_ts::TsExpr::Ident("Symbol".to_string())),
                args: vec![bynk_ts::TsExpr::Lit(bynk_ts::TsLit::Str(
                    c.name.name.clone(),
                ))],
            },
        })),
        None,
    );
    out.push_str(&bynk_ts::print_stmt(&token_decl, 0));
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
    //
    // P7.2 (review of #1300): computed once here and reused below (the
    // capability-scope collection a few lines down, and the factory's own
    // `deps_ty` further below) — previously recomputed independently at each
    // site from the same `p.given`, three calls into the same pure function
    // for the same answer.
    let given_ir = crate::ir::lower::lower_provider_given_ir(p);
    if !p.given.is_empty() {
        let deps_ty = given_ir
            .iter()
            .map(|c| format!("{}: {}", c.name, cap_ref_ty(c, &ctx.cross_context)))
            .collect::<Vec<_>>()
            .join("; ");
        // The field is declared separately and assigned in the constructor body
        // rather than written as a parameter property (`constructor(private deps:
        // …)`). A parameter property is the one type-directed construct pure
        // type-stripping cannot erase — Node's `--experimental-strip-types` throws
        // `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX` on it — so this de-sugared form keeps
        // the emitter's whole surface strip-removable (the strip-only emission
        // invariant, ADR 0136). The field stays typed for the `tsc`/strict path,
        // and under ES2022 `useDefineForClassFields` the declaration defines
        // `this.deps` before the body assigns the real value; the end state is
        // correct and strips to `constructor(deps) { this.deps = deps; }`.
        writeln!(out, "  private deps: {{ {deps_ty} }};").unwrap();
        writeln!(
            out,
            "  constructor(deps: {{ {deps_ty} }}) {{ this.deps = deps; }}"
        )
        .unwrap();
    }
    for op in &p.ops {
        let params: Vec<String> = op
            .params
            .iter()
            .map(|p| format!("{}: {}", ts_ident(&p.name.name), ts_type_ref(&p.type_ref)))
            .collect();
        // P6.55 (design/tracks/the-ir.md §6b): computed once, not twice —
        // `async_tail` below used to call `is_effectful_return(&op.return_type)`
        // again for the identical value (the same duplicate-computation
        // pattern P6.54 fixed in `emit_agent`).
        let effectful = is_effectful_return(&op.return_type);
        let async_kw = if effectful { "async " } else { "" };
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
        let mut module = ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use);
        module.agent_method_givens = ctx.agent_method_givens.clone();
        module.event_schema_versions = ctx.event_schema_versions.clone();
        module.set_rebrand_info(commons, ctx);
        module.target = ctx.target;
        module.in_bynk_unit = ctx.commons_name == "bynk";
        let mut cx = LowerCtx::new(
            module,
            BodyMode::ProviderOp {
                handler: HandlerShared {
                    // The provider's `given` capabilities are in scope in its
                    // bodies, and resolve against the injected `this.deps`.
                    capabilities: given_ir.iter().map(|c| c.name.clone()).collect(),
                    cap_deps_expr: if p.given.is_empty() {
                        "deps".to_string()
                    } else {
                        "this.deps".to_string()
                    },
                    // #934: a provider's own op body can itself call a `given`
                    // capability (e.g. a provider that dedups its own work via
                    // `Idempotency`).
                    handler_scope: Some(format!(
                        "{}.provides.{}.{}",
                        ctx.commons_name, p.capability.name, op.name.name
                    )),
                    owning_context: ctx.commons_name.clone(),
                    ..HandlerShared::default()
                },
            },
        )
        .with_source_map(source_map);
        cx.local_agents = ctx.local_agents.clone();
        emit_block_as_function_body_with_return(
            out,
            &op.body,
            &mut cx,
            INDENT_STEP * 2,
            effectful,
            Some(&op.return_type),
        );
        writeln!(out, "  }}").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    let factory = if p.given.is_empty() {
        format!("() => new {}()", p.provider_name.name)
    } else {
        // The same `deps_ty` the constructor above declares for `deps`,
        // recomputed from the `given_ir` hoisted at the top of this function
        // (that `deps_ty` binding itself doesn't escape its own block).
        let deps_ty = given_ir
            .iter()
            .map(|c| format!("{}: {}", c.name, cap_ref_ty(c, &ctx.cross_context)))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "(deps: {{ {deps_ty} }}) => new {}(deps)",
            p.provider_name.name
        )
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

/// Append one field to a `deps` object-type string under construction —
/// `emit_service`'s own actor-seam/`__exec`/events-dispatch widening all
/// repeat this exact splice: `{}` becomes `{ field }`; anything else gets
/// `field` spliced in before the closing brace. Review of #1209: gathering
/// the actor-seam widening into one `match` (below) made this splice's own
/// repetition newly visible — six copies in `emit_service` alone before
/// this helper.
fn append_deps_field(deps_ty: &str, field: &str) -> String {
    if deps_ty == "{}" {
        format!("{{ {field} }}")
    } else {
        format!(
            "{}; {field} }}",
            deps_ty.trim_end().trim_end_matches('}').trim_end()
        )
    }
}

pub(crate) fn emit_service(
    out: &mut String,
    s: &ServiceDecl,
    protocol: &ProtocolIr,
    signatures: &[HandlerSignatureIr],
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    let tys = commons.tys();
    emit_doc_block(out, s.documentation.as_deref(), 0);
    writeln!(out, "export const {name} = {{", name = s.name.name).unwrap();
    let mut cron_idx = 0usize;
    let mut queue_idx = 0usize;
    let ws_proto = matches!(protocol, ProtocolIr::WebSocket { .. });
    // #1187's slice 5: `s.handlers`/`signatures` are the same list in the
    // same declaration order — `signatures` is built by mapping `s.handlers`
    // 1:1 at the one call site (`emitter.rs`) — the same zip-by-index
    // precedent `emit_capability`'s own `c.ops`/`ops` pairing already
    // established (#1193).
    for (handler, (ir_params, _ir_given, ir_ret, ir_effectful)) in s.handlers.iter().zip(signatures)
    {
        // v0.104/v0.106 (real-time track slice 3b): on Workers a `from websocket`
        // lifecycle handler (`on open`/`on message`/`on close`) does not emit a
        // service-surface method — its body runs inside the hosting Durable Object
        // (`__wsOpen`/`__wsMessage`/`__wsClose`, DECISION A), driven by the edge
        // upgrade / `webSocketMessage` / `webSocketClose`, not from here. (On bundle
        // these are callable surface methods a `TestConnection` test drives.)
        let handler_kind_ir = lower_handler_kind_ir(&handler.kind);
        let is_ws_handler = matches!(handler_kind_ir, IrHandlerKind::Open | IrHandlerKind::Close)
            || (ws_proto && matches!(handler_kind_ir, IrHandlerKind::Message));
        // Events track, slice 0 (spine #936): unlike the WebSocket lifecycle
        // handlers above, `on event` has no *other* place its body could go
        // on Workers — a WS lifecycle handler's real body lives in the
        // hosting agent's DO (`emit_ws_do_method`); a subscriber service has
        // no such alternate home. So the method is still emitted here on
        // every target — real delivery (the fan-out DO calling in) reaches
        // it directly, not through `compose.ts`'s HTTP-routable surface,
        // which never exposes it regardless (no HTTP method/route to route
        // from).
        if is_ws_handler && matches!(ctx.target, BuildTarget::Workers) {
            continue;
        }
        emit_doc_block(out, handler.documentation.as_deref(), INDENT_STEP);
        let kind_name = match &handler_kind_ir {
            IrHandlerKind::Call => "call".to_string(),
            IrHandlerKind::Http { method, path } => http_handler_method_name_ir(*method, path),
            IrHandlerKind::Cron { .. } => {
                let name = cron_handler_method_name(&s.name.name, cron_idx);
                cron_idx += 1;
                name
            }
            // v0.106: a `from websocket` `on message` is the inbound surface method.
            IrHandlerKind::Message if ws_proto => "message".to_string(),
            IrHandlerKind::Message => {
                let name = queue_handler_method_name(&s.name.name, queue_idx);
                queue_idx += 1;
                name
            }
            // v0.103/v0.106: the WebSocket lifecycle surface methods.
            IrHandlerKind::Open => "open".to_string(),
            IrHandlerKind::Close => "close".to_string(),
            // Events track, slice 0 (spine #936): exactly one `on event` per
            // `from Events(E)` service.
            IrHandlerKind::Event => "event".to_string(),
        };
        // For service handlers the operation name is the handler kind
        // (e.g. `call`). v0.5 has only one handler kind, so the service is a
        // single-operation object literal.
        let mut params: Vec<String> = ir_params
            .iter()
            .map(|(name, ty)| format!("{}: {}", ts_ident(name), ts_ty(*ty, tys)))
            .collect();
        // v0.103/v0.106: a `from websocket` lifecycle handler receives the
        // `connection` as its first parameter (the synthetic binding the checker
        // added — the fresh socket for `on open`, the firing socket for `on
        // message`/`on close`); emit it so the lowered body's `connection` resolves.
        if is_ws_handler && let ProtocolIr::WebSocket { out_ty, .. } = protocol {
            params.insert(
                0,
                format!("connection: Connection<{}>", ts_ty(*out_ty, tys)),
            );
        }
        // Events track, slice 4 (spine #936): a `via schema(N)` guard needs
        // `env.schemaVersion` regardless of whether the subscriber declared
        // the optional `env: EventEnvelope` parameter (slice 2) — a bare
        // `on event(e: E)` handler has no envelope in scope otherwise. When
        // missing, insert a synthetic parameter in the same position a real
        // `env` would occupy, under a name distinct from anything a user
        // could write, so it can never collide with a declared one. The two
        // envelope-forwarding call sites (`workers.rs`, `project.rs`) widen
        // their own condition to match, so the value actually arrives here.
        let schema_dispatch_env_binder = if handler_kind_ir == IrHandlerKind::Event
            && let ProtocolIr::Events {
                schema_dispatch: Some(_),
                ..
            } = protocol
        {
            match ir_params.get(1) {
                Some((env_param_name, _)) => Some(ts_ident(env_param_name)),
                None => {
                    params.insert(1, "__bynkSchemaEnv: EventEnvelope".to_string());
                    Some("__bynkSchemaEnv".to_string())
                }
            }
        } else {
            None
        };
        // Lower the body first so we can detect cross-context usage and
        // adjust the deps shape accordingly.
        let mut body_out = String::new();
        // v0.70: each handler body lowers into its own source-map sub-builder
        // (offsets relative to `body_out`), merged into the module builder at the
        // splice below so handler statements map per-statement, not to the
        // `service` declaration line.
        let body_smb = RefCell::new(SourceMapBuilder::new());
        // v0.52: a multi-actor sum handler's resolved actor is threaded through
        // `deps.who`; the binder ident lowers to it so the body can `match`. A
        // sum supersedes the single-actor Bearer identity path (the per-arm
        // identity comes from the match, not a single `deps.identity`).
        // v0.47/v0.151/v0.52/v0.54: a handler's own actor-verification seam —
        // Bearer/Oidc/Caller thread their identity through `deps.identity`,
        // a sum threads the resolved-actor tagged union through `deps.who`
        // (the body `match`es it). `lower_actor_seam_ir`'s own doc comment
        // (`crate::ir::ActorSeamIr`) has the full grounding for why sum is
        // tried first (it can otherwise collide with Bearer) and why the
        // other three are mutually exclusive by construction.
        let seam = lower_actor_seam_ir(handler, &ctx.actors);
        let deps_identity_binder = match &seam {
            ActorSeamIr::Caller(binder) => Some(binder.clone()),
            ActorSeamIr::Bearer(s) => s.binder.clone(),
            ActorSeamIr::Oidc(s) => s.binder.clone(),
            ActorSeamIr::Sum(_) | ActorSeamIr::None => None,
        };
        let actor_sum_binder = if matches!(seam, ActorSeamIr::Sum(_)) {
            handler
                .by_clause
                .as_ref()
                .and_then(|by| by.binder.as_ref())
                .map(|binder| binder.name.clone())
        } else {
            None
        };
        let mut module = ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use);
        module.in_bynk_unit = ctx.commons_name == "bynk";
        module.agent_method_givens = ctx.agent_method_givens.clone();
        module.event_schema_versions = ctx.event_schema_versions.clone();
        module.set_rebrand_info(commons, ctx);
        module.target = ctx.target;
        let mut cx = LowerCtx::new(
            module,
            BodyMode::ServiceHandler {
                handler: HandlerShared {
                    capabilities: crate::ir::lower::lower_handler_given_ir(handler)
                        .into_iter()
                        .map(|c| c.name)
                        .collect::<HashSet<_>>(),
                    // #934: the qualified handler name `Idempotency.dedup`/
                    // `remember` key scoping prefixes onto the developer-supplied
                    // key.
                    handler_scope: Some(format!(
                        "{}.{}.{}",
                        ctx.commons_name, s.name.name, kind_name
                    )),
                    owning_context: ctx.commons_name.clone(),
                    ..HandlerShared::default()
                },
                deps_identity_binder,
                actor_sum_binder,
            },
        )
        .with_source_map(Some(&body_smb));
        cx.local_agents = ctx.local_agents.clone();
        let async_tail = *ir_effectful;
        emit_block_as_function_body_with_return(
            &mut body_out,
            &handler.body,
            &mut cx,
            INDENT_STEP * 2,
            async_tail,
            Some(&handler.return_type),
        );
        // Events track, slice 1 (spine #936): a `from Events(E { ... })`
        // subscription filter is deliver-and-filter (ADR 0286, unchanged by
        // this slice) — the fan-out mechanism still delivers every emission
        // of `E` to every subscriber; this subscriber's own generated
        // handler evaluates the pattern as a guard and no-ops if it doesn't
        // match. Prologue, not a wrapper edit, so it covers all three
        // delivery paths (Cloudflare Workers, Bundle/node, Bundle/browser)
        // in one place, since they all call into this same generated method.
        // `handler.params[0]`'s type is guaranteed to equal the header's
        // event type by `check_service_protocols`'s param-type-agreement
        // check, so testing its own name (read off `ir_params`, the
        // already-lowered signature `emit_service`'s own loop already
        // holds — P6.54) here is sound.
        if handler_kind_ir == IrHandlerKind::Event
            && let ProtocolIr::Events {
                pattern: Some(pattern),
                ..
            } = protocol
            && let Some((param_name, _)) = ir_params.first()
            && let Some(guard) = event_pattern_guard_ir(&ts_ident(param_name), Some(pattern))
        {
            let prologue = format!(
                "{}if (!({guard})) return undefined;\n",
                " ".repeat(INDENT_STEP * 2)
            );
            body_out.insert_str(0, &prologue);
        }
        // Events track, slice 4 (spine #936): a `via schema(N)` guard,
        // independent of and in addition to the payload-pattern guard above
        // — a service may carry either, both, or neither. Same
        // prologue technique, same three-delivery-path coverage. The
        // envelope binder is either the user's own declared `env` name or
        // the synthetic one inserted above.
        if let ProtocolIr::Events {
            schema_dispatch: Some(version),
            ..
        } = protocol
            && let Some(env_binder) = &schema_dispatch_env_binder
        {
            let prologue = format!(
                "{}if (!({env_binder}.schemaVersion === {version})) return undefined;\n",
                " ".repeat(INDENT_STEP * 2)
            );
            body_out.insert_str(0, &prologue);
        }
        // Append the deps parameter (may include surface field if the body
        // made cross-context calls). v0.47: a Bearer handler's deps also carries
        // the seam-minted `identity` — but only when a binder captures it
        // (v0.50: a binder-less Bearer handler verifies but mints no identity).
        let mut deps_ty = build_deps_object_ty_with_surface(
            &effective_given(&crate::ir::lower::lower_handler_given_ir(handler), &cx),
            &cx,
            &ctx.cross_context,
            ctx.target,
        );
        // v0.47/v0.151/v0.52/v0.54: widen `deps` for whichever actor seam
        // this handler resolved to — at most one arm ever fires, `seam`
        // being an enum rather than four independent optionals (a stronger
        // guarantee than the four-`if`-in-a-row shape this replaced, whose
        // mutual exclusion depended on the resolver-priority `if`s above
        // rather than the type system).
        match &seam {
            ActorSeamIr::Bearer(s) if s.binder.is_some() => {
                deps_ty = append_deps_field(&deps_ty, &format!("identity: {}", s.identity_type));
            }
            // v0.151: an `Oidc`-binding handler threads its `sub`-minted
            // identity into deps exactly like Bearer.
            ActorSeamIr::Oidc(s) if s.binder.is_some() => {
                deps_ty = append_deps_field(&deps_ty, &format!("identity: {}", s.identity_type));
            }
            // v0.54: a Caller-binding call handler's deps carries the
            // caller's context name as its `CallerId` identity (a `string`).
            ActorSeamIr::Caller(_) => {
                deps_ty = append_deps_field(&deps_ty, "identity: string");
            }
            // v0.52: a sum handler's deps carries the resolved-actor tagged
            // union (`who`), which the body `match`es. A binder-less sum is
            // rejected by the checker, so a sum handler always captures `who`.
            ActorSeamIr::Sum(members) => {
                let union = members
                    .iter()
                    .map(|m| match m.identity_type() {
                        Some(id) => format!("{{ tag: \"{}\", identity: {id} }}", m.actor_name),
                        None => format!("{{ tag: \"{}\" }}", m.actor_name),
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                deps_ty = append_deps_field(&deps_ty, &format!("who: {union}"));
            }
            // A binder-less Bearer/Oidc, or no seam at all — nothing to widen.
            ActorSeamIr::Bearer(_) | ActorSeamIr::Oidc(_) | ActorSeamIr::None => {}
        }
        // v0.79: a handler whose body uses `~>` receives the execution context
        // (`__exec`) in its deps, so the fire-and-forget send can hand its promise
        // to `waitUntil`. Gated on the body so non-sending handlers are unchanged.
        if crate::emitter::block_uses_send(&handler.body) {
            deps_ty = append_deps_field(
                &deps_ty,
                "__exec: { waitUntil(promise: Promise<unknown>): void }",
            );
        }
        // Events track, slice 0 (spine #936): a handler whose body uses
        // `Events.emit` receives a compose-supplied `__eventsDispatch`
        // callback in its deps — the actual fanout delivery mechanism
        // (a Cloudflare Durable Object per publishing context, or an
        // in-process dispatch on Bundle/node) lives at the composition
        // layer, which already knows how to build any capability's
        // provider; the handler itself only needs to hand its buffered
        // `__events` to whatever composed it. Mirrors `__exec`'s threading
        // for `~>` exactly. Gated on more than this handler's own body: a
        // handler with no direct `Events.emit` call but that invokes a
        // *local agent* method which itself emits (`Ledger(id).bump(...)`)
        // still needs `__eventsDispatch` threaded through, to hand onward —
        // `cx.agent_given_caps_used` is exactly the existing mechanism that
        // already propagates an invoked agent method's other `given`
        // capabilities up to the caller (`effective_given`, just below);
        // `Events` rides the same path since it's declared as an ordinary
        // `given Events` on the agent handler.
        let needs_events_dispatch = cx.is_first_party_events()
            && (crate::emitter::block_uses_emit(&handler.body, &commons.callees)
                || cx
                    .agent_given_caps_used()
                    .is_some_and(|m| m.contains_key("Events")));
        if needs_events_dispatch {
            deps_ty = append_deps_field(
                &deps_ty,
                &format!(
                    "__eventsDispatch: (events: Array<{}>) => Promise<void>",
                    crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
                ),
            );
        }
        params.push(format!("deps: {deps_ty}"));
        let ret = ts_ty(*ir_ret, tys);
        let async_kw = if *ir_effectful { "async " } else { "" };
        writeln!(
            out,
            "  {async_kw}{op}({params}): {ret} {{",
            op = kind_name,
            params = params.join(", "),
        )
        .unwrap();
        // Events track, slice 0 (spine #936): release-at-commit (events.md
        // §3.0) needs a completion boundary services never had before — the
        // body runs inside an IIFE so its own `return`s resolve `__result`
        // rather than the outer method, and only a handler that completes
        // without throwing reaches past it, then flushes `__events` to
        // whatever composed it (`deps.__eventsDispatch`, threaded above).
        // Gated on `block_uses_emit` specifically (not the broader
        // `needs_events_dispatch` above) — a handler that only *forwards*
        // `__eventsDispatch` to an agent it calls has nothing of its own to
        // buffer or flush, so it keeps byte-identical output, mirroring
        // `__exec`'s gate on `block_uses_send`.
        let body_emits_directly = crate::emitter::block_uses_emit(&handler.body, &commons.callees);
        if body_emits_directly {
            writeln!(
                out,
                "    const __events: Array<{}> = [];",
                crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
            )
            .unwrap();
            writeln!(out, "    const __result = await (async () => {{").unwrap();
        }
        let base = out.len();
        out.push_str(&body_out);
        if let Some(module) = source_map {
            module
                .borrow_mut()
                .merge(&body_smb.borrow(), &body_out, out, base, 0);
        }
        if body_emits_directly {
            writeln!(out, "    }})();").unwrap();
            writeln!(out, "    if (__events.length > 0) {{").unwrap();
            writeln!(out, "      await deps.__eventsDispatch(__events);").unwrap();
            writeln!(out, "    }}").unwrap();
            writeln!(out, "    return __result;").unwrap();
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
fn cap_ref_ty(c: &crate::ir::CapRefIr, info: &bynk_check::resolver::CrossContextInfo) -> String {
    match c.context.as_deref().and_then(|p| info.resolve_prefix(p)) {
        Some(consumed) => format!("{}.{}", qualified_to_ns(&consumed), c.name),
        // v0.17: a bare flattened capability (`consumes U { Cap }`) keeps its
        // interface in the consumed unit's module — qualify the type there.
        None => match info.flattened_caps.get(&c.name) {
            Some(unit) => format!("{}.{}", qualified_to_ns(unit), c.name),
            None => c.name.clone(),
        },
    }
}

/// Events track, slice 0 (spine #936): does any service or agent handler in
/// this commons emit — the same predicate `block_uses_emit` answers per
/// handler body, rolled up to "does the context-level `__eventsDispatch`
/// deps field need to exist at all." Syntactic, like its sibling: a false
/// positive here just adds an unused interface field, not a miscompile.
pub(crate) fn commons_uses_emit(commons: &TypedCommons) -> bool {
    commons.commons.items.iter().any(|item| match item {
        CommonsItem::Service(s) => s
            .handlers
            .iter()
            .any(|h| crate::emitter::block_uses_emit(&h.body, &commons.callees)),
        CommonsItem::Agent(a) => a
            .handlers
            .iter()
            .any(|h| crate::emitter::block_uses_emit(&h.body, &commons.callees)),
        _ => false,
    })
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
            for c in crate::ir::lower::lower_handler_given_ir(h) {
                // Events track, slice 0 (spine #936): `Events.emit` is
                // intercepted entirely at the call site (release-at-commit
                // buffering) and never calls through a constructed provider
                // — unlike every other capability, there is no
                // `EventsProvider` for compose to build, so the first-party
                // `Events` must not appear in any context's deps interface.
                let is_first_party_events = c.name == "Events"
                    && info.flattened_caps.get(&c.name).map(String::as_str) == Some("bynk");
                if is_first_party_events {
                    continue;
                }
                if let Some(prefix) = &c.context {
                    if let Some(consumed) = info.resolve_prefix(prefix) {
                        seen.entry(c.name.clone()).or_insert(consumed);
                    }
                } else if let Some(unit) = info.flattened_caps.get(&c.name) {
                    // v0.17: a bare flattened capability is a cross-unit dep too.
                    seen.entry(c.name.clone()).or_insert_with(|| unit.clone());
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
    let mut collect = |given: Vec<crate::ir::CapRefIr>| {
        for c in given {
            if let Some(prefix) = &c.context
                && let Some(consumed) = info.resolve_prefix(prefix)
            {
                out.insert(consumed);
            } else if c.context.is_none()
                // v0.17: a bare flattened capability imports its interface from
                // the consumed unit's module.
                && let Some(unit) = info.flattened_caps.get(&c.name)
            {
                out.insert(unit.clone());
            }
        }
    };
    for item in &commons.commons.items {
        match item {
            CommonsItem::Service(s) => s
                .handlers
                .iter()
                .for_each(|h| collect(crate::ir::lower::lower_handler_given_ir(h))),
            CommonsItem::Agent(a) => a
                .handlers
                .iter()
                .for_each(|h| collect(crate::ir::lower::lower_handler_given_ir(h))),
            CommonsItem::Provider(p) => collect(crate::ir::lower::lower_provider_given_ir(p)),
            _ => {}
        }
    }
    out
}

/// #527: a handler's *effective* `given` list — its declared capabilities
/// plus those required by the local-agent methods its body calls (recorded
/// during lowering). Compose always instantiates the full capability union
/// into the runtime deps value, so this widening only brings the deps *type*
/// in line with the value; without it, forwarding `deps` to an agent method
/// with `given` failed `tsc --strict` (and under-documented the dependency).
fn effective_given(
    declared: &[crate::ir::CapRefIr],
    cx: &LowerCtx<'_>,
) -> Vec<crate::ir::CapRefIr> {
    let mut out = declared.to_vec();
    let have: HashSet<String> = declared.iter().map(|c| c.name.clone()).collect();
    for (key, cap) in cx.agent_given_caps_used().into_iter().flatten() {
        if !have.contains(key) {
            out.push(cap.clone());
        }
    }
    // Events track, slice 0 (spine #936): `Events.emit` is intercepted
    // entirely at the call site (release-at-commit buffering; see the
    // `Events`/`emit` special case in `lower.rs`) and never calls through a
    // constructed provider — unlike every other capability, there is no
    // `EventsProvider` for compose to build. So `Events` must not appear in
    // `deps` at all, or compose would need to construct a provider that
    // doesn't exist. Filtered the same way #934 distinguishes a genuine
    // first-party `Events` from an unrelated same-named capability.
    out.retain(|c| {
        c.name != "Events"
            || !(cx.in_bynk_unit()
                || cx
                    .cross_context()
                    .flattened_caps
                    .get(&c.name)
                    .map(String::as_str)
                    == Some("bynk"))
    });
    out
}

fn build_deps_object_ty_with_surface(
    given: &[crate::ir::CapRefIr],
    cx: &LowerCtx<'_>,
    cross_context: &bynk_check::resolver::CrossContextInfo,
    target: BuildTarget,
) -> String {
    let mut parts: Vec<String> = given
        .iter()
        .map(|c| format!("{}: {}", c.name, cap_ref_ty(c, cross_context)))
        .collect();
    match target {
        BuildTarget::Bundle => {
            if cx.cross_context_used() {
                parts.push(format!("surface: {}", surface_ty(cross_context)));
            }
        }
        BuildTarget::Workers => {
            // v0.9.2: in workers mode `env` carries both consumed-context
            // Service Bindings and the local agents' Durable Object namespaces.
            // It is threaded into deps whenever the handler makes a
            // cross-context call or instantiates an agent.
            if cx.cross_context_used() || cx.agents_instantiated() {
                let agents = if cx.agents_instantiated() {
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
        .commons()
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
        // A unit consumed only in the flattened-capability form
        // (`consumes bynk { Clock }` — adapters and the first-party surface)
        // is provided *in-process* by the compose root, and its wrangler.toml
        // declares no Service Binding for it. Including it here made the
        // handlers' `deps.env` type demand a binding (`BYNK: ServiceBinding`)
        // the deployment never has — the emitted Worker failed `tsc --strict`
        // against its own compose module (caught by the examples tsc gate).
        .filter(|q| {
            let flattened_only = cross_context.flattened_caps.values().any(|u| u == *q)
                && cross_context
                    .consumed_services
                    .get(*q)
                    .is_none_or(|svcs| svcs.is_empty());
            !flattened_only
        })
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
    // Events track, slice 0 (spine #936): a context with any handler that
    // emits needs `__eventsDispatch` threaded all the way from `makeSurface`
    // down to that handler's own `deps` param (`emit_service`/`emit_agent`'s
    // matching field) — otherwise `svc.call(args, deps)` inside `makeSurface`
    // fails to typecheck, since `deps`'s static type there is this interface,
    // not whatever compose actually constructs.
    let is_first_party_events = ctx.commons_name == "bynk"
        || ctx
            .cross_context
            .flattened_caps
            .get("Events")
            .map(String::as_str)
            == Some("bynk");
    if is_first_party_events && commons_uses_emit(commons) {
        fields.push(format!(
            "  readonly __eventsDispatch: (events: Array<{}>) => Promise<void>;",
            crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
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

/// v0.54 (#655): whether any of `services` declares an `on call … by c: Caller`
/// handler, whose emitted `deps` carries the calling context's qualified name as
/// its `CallerId` identity (ADR 0092). This is the *single* predicate both
/// bundle caller-identity seams consult — `emit_make_surface` (which adds the
/// `__caller` parameter) and the compose root (which passes the name) — so the
/// two can never disagree on which providers thread a caller. `caller_binder_for`
/// self-guards `HandlerKind::Call`, so no kind filter is needed or wanted here.
pub(crate) fn any_service_binds_caller<'a>(
    services: impl IntoIterator<Item = &'a ServiceDecl>,
    actors: &HashMap<String, ActorDecl>,
) -> bool {
    services.into_iter().any(|s| {
        s.handlers
            .iter()
            .any(|h| bynk_check::actors::caller_binder_for(h, actors).is_some())
    })
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
    // v0.54 (#655): an `on call … by c: Caller` handler reads a live `CallerId`
    // (the calling context's qualified name) threaded through `deps.identity`.
    // In bundle mode the compose root supplies that name to `makeSurface` as a
    // second `__caller` argument — the analogue of the `X-Bynk-Caller` header a
    // Worker reads at its entry (ADR 0092). Only a context with such a handler
    // takes the extra parameter, so a caller-free surface is byte-unchanged. The
    // shared `any_service_binds_caller` predicate is what keeps this seam and the
    // compose root in lockstep on which providers get the extra argument.
    let binds_caller =
        |h: &Handler| bynk_check::actors::caller_binder_for(h, &ctx.actors).is_some();
    let any_caller = any_service_binds_caller(services.iter().copied(), &ctx.actors);
    let params = if any_caller {
        format!("deps: {deps_name}, __caller: string")
    } else {
        format!("deps: {deps_name}")
    };
    writeln!(out, "export function makeSurface({params}) {{").unwrap();
    writeln!(out, "  return {{").unwrap();
    for s in &services {
        // For each handler kind currently only `call`. We bind it as a
        // method on the surface with the deps captured.
        let handler = s
            .handlers
            .iter()
            .find(|h| matches!(lower_handler_kind_ir(&h.kind), IrHandlerKind::Call));
        let Some(h) = handler else { continue };
        let async_kw = if is_effectful_return(&h.return_type) {
            "async "
        } else {
            ""
        };
        let param_decls: Vec<String> = h
            .params
            .iter()
            .map(|p| format!("{}: {}", ts_ident(&p.name.name), ts_type_ref(&p.type_ref)))
            .collect();
        let param_args: Vec<String> = h.params.iter().map(|p| ts_ident(&p.name.name)).collect();
        let ret = ts_type_ref(&h.return_type);
        // A Caller-binding handler's `deps.identity` is the caller name the
        // compose root threaded in; every other handler forwards `deps` verbatim.
        let deps_arg = if binds_caller(h) {
            "{ ...deps, identity: __caller }"
        } else {
            "deps"
        };
        writeln!(
            out,
            "    {async_kw}{sname}({params}): {ret} {{",
            sname = s.name.name,
            params = param_decls.join(", "),
        )
        .unwrap();
        writeln!(
            out,
            "      return {svc}.call({args}{sep}{deps_arg});",
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
    cx: &mut LowerCtx<'_>,
) -> Lowered {
    use crate::emitter::serialisation::{deserialise_ref_via, serialise_expr_via};

    let mut pre = Pre::new();

    let info = cx.cross_context();
    let binding = crate::emitter::wrangler::consumed_binding_name(consumed);

    // Look up the service signature on the consumed context.
    //
    // v0.176 (#642, Decision C): the checker resolved this call before the
    // emitter ran, so an absent signature is the emitter disagreeing with the
    // checker — a compiler bug. It used to lower to `value as JsonValue` and an
    // `((j: any) => ({ tag: "Ok", value: j }))` identity deserialiser, shipping
    // an unvalidated `any` to production to paper over that bug. Fail instead.
    let svc = info
        .consumed_services
        .get(consumed)
        .and_then(|map| map.get(&method.name))
        .unwrap_or_else(|| {
            panic!(
                "bynk.emit.unresolved_cross_context_signature: no signature for \
                 `{consumed}.{}` at emit time, though the checker resolved the call",
                method.name
            )
        });

    // #661 (ADR 0199 Decision G discharged): a context's own handlers generate
    // their *own* codecs for the contracts they participate in
    // (`emit_boundary_helpers`), so the call site reaches them **locally** — no
    // namespace prefix. The consumed context's module is then imported for types
    // only (`import type * as <ns>`), which is what makes each Worker
    // self-contained: `commerce-orders` no longer bundles `commerce-payment`'s
    // provider implementation.
    //
    // The generated **test harness** (`tests/*.test.ts`) is the exception: it
    // imports every participating Worker's `handlers.js` as a real value module
    // (it wires the `env` bindings from them) and does *not* generate its own
    // codecs, so there it still reaches the callee's codecs through that value
    // namespace. `in_test_scaffold()` is exactly that discriminator.
    let ns = if cx.in_test_scaffold() {
        format!("{}.", qualified_to_ns(consumed))
    } else {
        String::new()
    };

    // One invariant for arity, asserted once, in both directions. The checker
    // validated it (`bynk.consumes.service_arity`) before emit ran, so a mismatch
    // either way is the emitter disagreeing with the checker — the same internal
    // fault the signature lookup above asserts on, and it gets the same answer.
    // (Previously the two directions disagreed: a surplus argument panicked, while
    // a missing one silently emitted `null` into the args object — quietly sending
    // a wrong payload for exactly the fault the other direction refused to ship.)
    assert_eq!(
        args.len(),
        svc.params.len(),
        "bynk.emit.unresolved_cross_context_signature: `{consumed}.{}` takes {} argument(s) \
         but the call site has {}, though the checker accepted the call's arity",
        method.name,
        svc.params.len(),
        args.len(),
    );

    let mut args_serialised: Vec<String> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let lowered = pre.lower(a, cx);
        let (_, param_ty) = &svc.params[i];
        args_serialised.push(serialise_expr_via(
            param_ty,
            &lowered,
            &ns,
            cx.runtime_use(),
        ));
    }
    let args_json = if args_serialised.len() == 1 {
        args_serialised.into_iter().next().unwrap()
    } else {
        // Multi-arg: wrap into an object literal keyed by parameter names.
        let pairs: Vec<String> = svc
            .params
            .iter()
            .zip(&args_serialised)
            .map(|((name, _), serialised)| format!("{name}: {serialised}"))
            .collect();
        format!("{{ {} }}", pairs.join(", "))
    };

    let deser_ref = deserialise_ref_via(&svc.return_type, &ns, cx.runtime_use());

    // v0.54: stamp the calling context's qualified name so the callee's
    // `by c: Caller` handler reads a live `CallerId` (Q7). A compile-time
    // constant; the args body is unchanged.
    let caller = escape_ts_string(&cx.commons().commons.name.joined());

    // v0.177 (#643): stamp this context's compiled view of the callee's
    // contract, so the callee can fail closed when the deployed pair disagree.
    //
    // Canonicalised in the **callee's** namespace, from the callee's own type
    // table (`consumed_types[consumed]`) — never this context's. The callee
    // builds the same table for itself via the same `combined_types_for`, so the
    // two hashes agree by construction on a single build and differ only when
    // the deployed sources genuinely differ. Canonicalising here in the caller's
    // namespace would rebrand the same types and 409 every call.
    let contract = match info.consumed_types.get(consumed) {
        Some(types) => bynk_check::contract::service_contract_hash(svc, types),
        // Unreachable alongside the signature assertion above: the same resolver
        // pass populates both maps for a consumed context.
        None => unreachable!(
            "consumed_types missing for `{consumed}`, though its service signature resolved"
        ),
    };

    pre.finish(format!(
        "callService(deps.env.{binding}, \"{}\", {args_json}, {deser_ref}, \"{caller}\", \"{contract}\")",
        method.name
    ))
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
    let info = cx.cross_context();
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
pub(crate) fn type_ref_named_root(r: &TypeRef) -> Option<&str> {
    match r {
        TypeRef::Named(id) => Some(id.name.as_str()),
        _ => None,
    }
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
pub(crate) fn agent_needs_rehydrate(a: &AgentDecl, types: &HashMap<String, Arc<TypeDecl>>) -> bool {
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
        && bynk_check::context_checks::type_ref_is_held(&f.kind.args[1])
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

/// #1187's Agent state-field slice 2c: a held `Map[K, V]`'s own frame type
/// `F`, resolved at the `TyId` level — `V` may be a bare `Connection[F]`, or
/// `F` may sit behind an `Option`/`Effect` wrapper (`Map[K,
/// Option[Connection[F]]]` is checker-legal: `type_ref_is_held`,
/// `bynk-check/src/context_checks.rs`, recurses through both to decide
/// storage admission). Mirrors that recursion at the `TyId` level rather
/// than reusing `Ty::is_held()`/`Ty::held_inner()` (`bynk-check/src/
/// checker.rs`): neither recurses past a bare `Connection` today (both are
/// otherwise dead code — no caller anywhere in this workspace), so using
/// them as-is here would silently reintroduce the same Option-blind gap
/// this slice fixes.
///
/// Tested at the `Types`/`TyId` level directly ([`held_frame_ty_tests`]),
/// not through a full end-to-end fixture: a real `store x: Map[K,
/// Option[Connection[F]]]` program certifies (`bynkc check` exits 0), but
/// `bynkc compile` panics before reaching this function at all —
/// `bynk-check::wire::codec_suffix`'s `TypeRef::Connection(..) =>
/// unreachable!(...)` arm, reached because whatever builds this project's
/// wire/codec instantiation table walks every store field's value type
/// looking for boundary-codec names, `Option`-recursing into a held
/// `Connection` it should have excluded. Confirmed pre-existing and
/// unrelated to this slice (reproduces identically on `main`, before this
/// function existed) — a real `bynk-check` bug, out of scope for this
/// emitter-only cutover; worth its own follow-up issue.
fn held_frame_ty(ty: TyId, tys: &Types) -> Option<TyId> {
    match &*tys.get(ty) {
        Ty::Connection(inner) => Some(*inner),
        Ty::Option(inner) | Ty::Effect(inner) => held_frame_ty(*inner, tys),
        _ => None,
    }
}

/// [`held_frame_ty`]'s `TypeRef`-level mirror, and `held_maps_ts`'s own
/// fallback when the frame's `TyId` doesn't resolve (a held field's own
/// type reference is never checker-validated, only its shape — see that
/// call site's own doc comment). Same `Connection`/`Option`/`Effect`
/// recursion as `type_ref_is_held` (`bynk-check/src/context_checks.rs`),
/// applied here at the AST level instead of just to a bare `Connection` the
/// way this function's own pre-#1187 shape did — so the fallback path
/// renders the same frame type the `TyId` path would have, not the whole
/// `Option<Connection<F>>`/`Effect<Connection<F>>` wrapper.
fn held_frame_ty_ref(t: &TypeRef) -> &TypeRef {
    match t {
        TypeRef::Connection(inner, _) | TypeRef::Option(inner, _) | TypeRef::Effect(inner, _) => {
            held_frame_ty_ref(inner)
        }
        _ => t,
    }
}

#[cfg(test)]
mod held_frame_ty_tests {
    use super::*;

    #[test]
    fn unwraps_a_bare_connection() {
        let tys = Types::new();
        let frame = tys.intern(Ty::Base(BaseType::String));
        let conn = tys.intern(Ty::Connection(frame));
        assert!(matches!(held_frame_ty(conn, &tys), Some(f) if f == frame));
    }

    /// The shape a naive, non-recursive port of `Ty::held_inner()` would
    /// get wrong — the same gap this slice found in the pre-existing
    /// `held_maps_ts` construction (a bare `match v { TypeRef::Connection
    /// (inner, _) => …, _ => ts_type_ref(v) }` that rendered the whole
    /// `Option<Connection<F>>` wrapper instead of unwrapping to `F`).
    #[test]
    fn unwraps_through_option_and_effect_like_type_ref_is_held_does() {
        let tys = Types::new();
        let frame = tys.intern(Ty::Base(BaseType::String));
        let conn = tys.intern(Ty::Connection(frame));
        let opt = tys.intern(Ty::Option(conn));
        assert!(matches!(held_frame_ty(opt, &tys), Some(f) if f == frame));
        let eff = tys.intern(Ty::Effect(conn));
        assert!(matches!(held_frame_ty(eff, &tys), Some(f) if f == frame));
        // `Effect[Option[Connection[F]]]` — nested wrapping, both layers unwrap.
        let opt_eff = tys.intern(Ty::Effect(opt));
        assert!(matches!(held_frame_ty(opt_eff, &tys), Some(f) if f == frame));
    }

    #[test]
    fn a_non_held_type_returns_none_even_wrapped() {
        let tys = Types::new();
        let s = tys.intern(Ty::Base(BaseType::String));
        assert!(held_frame_ty(s, &tys).is_none());
        let opt_s = tys.intern(Ty::Option(s));
        assert!(held_frame_ty(opt_s, &tys).is_none());
    }
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
fn type_base_is_string(t: &TypeRef, types: &HashMap<String, Arc<TypeDecl>>) -> bool {
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

/// True when `t` is `Int` or a refined/opaque type over `Int`.
fn type_base_is_int(t: &TypeRef, types: &HashMap<String, Arc<TypeDecl>>) -> bool {
    match t {
        TypeRef::Base(BaseType::Int, _) => true,
        TypeRef::Named(id) => matches!(
            types.get(&id.name).map(|d| &d.body),
            Some(TypeBody::Refined {
                base: BaseType::Int,
                ..
            }) | Some(TypeBody::Opaque {
                base: BaseType::Int,
                ..
            })
        ),
        _ => false,
    }
}

/// v0.119 (ADR 0155): the TS expression for a driven handler argument. An
/// `Int`-based value (bare or refined/opaque) is coerced to `number` — the
/// property generator yields `bigint`, but handler bodies do `number` arithmetic
/// (the same rule the contract attacker follows; the refinement brand is
/// compile-time only, so `Number(...)` is a no-op at runtime). Everything else —
/// `String`/refined-`String`, `Bool` — passes through.
fn history_arg_ts(p: &Param, i: usize, types: &HashMap<String, Arc<TypeDecl>>) -> String {
    if type_base_is_int(&p.type_ref, types) {
        format!("Number(__st.args[{i}])")
    } else {
        format!("__st.args[{i}]")
    }
}

/// v0.119: the `.call` variant tag for a handler — its name with the first letter
/// upper-cased (`spend` → `Spend`). Must match the checker's synthesised sum
/// variant (`tests_emit::history_variant_name`), which the reader matches with
/// `is` / `match`.
fn history_variant_tag(handler: &str) -> String {
    let mut c = handler.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => handler.to_string(),
    }
}

pub(crate) fn emit_agent(
    out: &mut String,
    a: &AgentDecl,
    state: &[StoreFieldIr],
    commons: &TypedCommons,
    ctx: &EmitProjectCtx,
    source_map: Option<&RefCell<SourceMapBuilder>>,
) {
    let tys = commons.tys();
    // #1187's Agent state-field slice: the checker-resolved TyId for each
    // Cell/Map/Cache/Log field's element type, keyed by field name — read
    // once here so the state-interface block below can render through
    // `ts_ty` (TyId) instead of `ts_type_ref` (raw AST) without repeating
    // this lookup at every one of its four sites.
    let store_field_ty: HashMap<&str, &StoreKindIr> =
        state.iter().map(|f| (f.field.as_str(), &f.kind)).collect();
    emit_doc_block(out, a.documentation.as_deref(), 0);
    let state_ty = format!("{}State", a.name.name);
    // v0.81 (storage track, ADR 0109): an agent's `Cell` fields ARE its state
    // record, so the whole state machinery (interface, zero factory, load/commit,
    // invariant gate) derives the record fields from the cells. Each `Cell[T]`
    // field becomes a `T`-typed record field carrying the cell's initialiser.
    // Handler bodies lower as bare reads / `:=` over `__state`, with an implicit
    // commit at handler end.
    let is_store_agent = true;
    // P6.53 (design/tracks/the-ir.md §6b): membership reads `state`'s own
    // typed `StoreKindIr::Cell` instead of a string comparison against
    // `f.kind.head.name` — a user type genuinely named `Cell` (with a
    // different arity or an unrelated shape) cannot silently pass this
    // check when the field's own already-checked kind says otherwise. The
    // field's own `TypeRef`/`init` stay AST-typed (rendering parameters
    // outside this slice's scope).
    let effective_fields: Vec<RecordField> = a
        .store_fields
        .iter()
        .filter(|f| {
            matches!(
                store_field_ty.get(f.name.name.as_str()),
                Some(StoreKindIr::Cell(_))
            )
        })
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
    // #1187's Agent state-field slice 2c: the field's own membership in
    // `held_maps` still comes from `held_map_fields`'s AST-based
    // `type_ref_is_held` (unchanged — its caller, `write_header`
    // (`emitter.rs`), has no `CheckedProgram` in scope, so this and that
    // predicate stay deliberately parallel, not unified), but the frame
    // type itself is now read off `state`'s already-resolved `TyId` via
    // `held_frame_ty`, through `ts_ty` — fixing a real, previously
    // uncovered gap: the old `match v { TypeRef::Connection(inner, _) =>
    // ts_type_ref(inner), _ => ts_type_ref(v) }` only ever unwrapped a
    // *bare* `Connection`; a `Map[K, Option[Connection[F]]]` value — legal
    // per the checker's own `type_ref_is_held`, which recurses through
    // `Option`/`Effect` (`bynk-check/src/context_checks.rs`) — fell into
    // the `_` arm and rendered the whole `Option<Connection<F>>` wrapper,
    // not `F`. No existing fixture exercised this shape to catch it.
    let held_maps_ts: HashMap<String, String> = held_maps
        .iter()
        .map(|(n, v)| {
            // Review of #1187's slice 2c: no checker pass validates a store
            // field's own type reference (only its shape —
            // `resolve_store_field_ty`'s own doc comment, `ir/lower.rs`,
            // names this explicitly), so `store x: Map[K, Connection[Bogus]]`
            // certifies today with an unresolvable frame type; the whole
            // `Connection[Bogus]` then fails to resolve too
            // (`resolve_type_ref_in`'s `?` propagation, `bynk-check/src/
            // checker.rs`), so `StoreKindIr::Map`'s own value `TyId` falls
            // back to `Ty::Unit` — `held_frame_ty` legitimately returns
            // `None` on a certified program here, not an internal
            // inconsistency. Fall back to the AST-level frame type (still
            // Option/Effect-unwrapped, via `held_frame_ty_ref`) instead of
            // panicking, mirroring `resolve_store_field_ty`'s own posture
            // for the identical reason.
            let ts = store_field_ty
                .get(n.name.as_str())
                .and_then(|kind| match kind {
                    StoreKindIr::Map(_, v_ty) => held_frame_ty(*v_ty, tys),
                    _ => None,
                })
                .map(|frame_ty| ts_ty(frame_ty, tys))
                .unwrap_or_else(|| ts_type_ref(held_frame_ty_ref(v)));
            (n.name.clone(), ts)
        })
        .collect();
    // P6.53 (design/tracks/the-ir.md §6b): typed membership, same reasoning
    // as `effective_fields` above.
    let store_map_fields: Vec<(&Ident, &TypeRef)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| {
                matches!(
                    store_field_ty.get(f.name.name.as_str()),
                    Some(StoreKindIr::Map(..))
                )
            })
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
    // #1187's Agent state-field slice 2b: which `Map` fields have an
    // `@indexed(by: …)` key list, IR-sourced — `StoreFieldIr::indexed` is
    // already the deduped, declaration-order key list
    // `store_field_kind_and_indexed` built from the same annotations this
    // used to re-walk here. Deliberately still keyed and filtered exactly
    // like the AST walk it replaces: no held-map exclusion (a held
    // `Map[K, Connection]` is an ordinary `StoreKindIr::Map` as far as the
    // checker/IR are concerned — "held" is purely this function's own
    // downstream Workers-mode concern, applied separately to `store_map_fields`
    // above), so a held map's own indexes are still emitted here exactly as
    // before.
    let store_map_indexes: HashMap<String, Vec<String>> = if is_store_agent {
        state
            .iter()
            .filter(|f| matches!(f.kind, StoreKindIr::Map(..)) && !f.indexed.is_empty())
            .map(|f| (f.field.clone(), f.indexed.clone()))
            .collect()
    } else {
        HashMap::new()
    };
    // v0.83 (ADR 0110): `store Set[T]` fields are state-record fields too,
    // persisted as a JSON-serialisable `Record<string, boolean>` (a JS `Set`
    // does not serialise). `(name, T)`; the element type is unused in the TS
    // representation but kept for symmetry with maps — needed only for the
    // rehydration check below, which still validates against a raw `TypeRef`
    // (`serialisation.rs`'s own `TypeRef`-driven boundary, out of this
    // slice's scope).
    // P6.53 (design/tracks/the-ir.md §6b): typed membership, same reasoning
    // as `effective_fields` above.
    let store_set_fields: Vec<(&Ident, &TypeRef)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| {
                matches!(
                    store_field_ty.get(f.name.name.as_str()),
                    Some(StoreKindIr::Set(_))
                )
            })
            .map(|f| (&f.name, &f.kind.args[0]))
            .collect()
    } else {
        Vec::new()
    };
    // #1187's Agent state-field slice 2b: which fields are Sets, IR-sourced
    // (declaration order preserved — `state` is built from `a.store_fields`
    // in order) — every consumer below except the rehydration check, which
    // alone needs `store_set_fields`'s own `TypeRef`.
    let set_field_names: Vec<&str> = state
        .iter()
        .filter(|f| matches!(f.kind, StoreKindIr::Set(_)))
        .map(|f| f.field.as_str())
        .collect();
    let set_names: HashSet<String> = set_field_names.iter().map(|s| s.to_string()).collect();
    // v0.87 (ADR 0113): `store Cache[K, V] @ttl(d)` fields — a value record plus
    // a per-entry expiry instant. `(name, V, ttl-millis)`; the ttl is the field's
    // `@ttl` Duration literal (validated by the checker).
    // P6.53 (design/tracks/the-ir.md §6b): `ttl` reads `state`'s own
    // already-computed `StoreKindIr::Cache(_, _, ttl)` (`store_field_ty`,
    // built above from `lower_store_field_shape_ir` →
    // `store_field_kind_and_indexed` → `duration_millis_annotation`)
    // instead of re-walking `f.annotations`/`ExprKind::DurationLit` a
    // second time — the two walks could diverge and emit a wrong TTL with
    // nothing to catch it. `?` on a miss preserves the original's own
    // defensive drop (an internal-consistency signal, not a real-world
    // case: every `Cache`-kind field in `a.store_fields` has a matching
    // `StoreKindIr::Cache` entry by construction).
    let store_cache_fields: Vec<(&Ident, &TypeRef, i64)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Cache" && f.kind.args.len() == 2)
            .filter_map(|f| {
                let StoreKindIr::Cache(_, _, ttl) = store_field_ty.get(f.name.name.as_str())?
                else {
                    return None;
                };
                Some((&f.name, &f.kind.args[1], *ttl))
            })
            .collect()
    } else {
        Vec::new()
    };
    let cache_ttls: HashMap<String, i64> = store_cache_fields
        .iter()
        .map(|(n, _, ttl)| (n.name.clone(), *ttl))
        .collect();
    // v0.95 (ADR 0121): `store Log[T] [@retain(d)]` fields — an ordered array of
    // `{ t, v }` entries. `(name, T, optional retain-millis)`; the retain (from
    // `@retain`) prunes on append.
    // P6.53 (design/tracks/the-ir.md §6b): same dedup as `store_cache_fields`
    // above — `retain` reads `StoreKindIr::Log(_, retain)` instead of
    // re-walking `f.annotations` a second time. Every `Log`-kind field in
    // `a.store_fields` has a matching `StoreKindIr::Log` entry by
    // construction; `.and_then` (not `?`) preserves the original's own
    // per-field presence (every filtered field stays, `retain` is simply
    // `None` when the field has no `@retain`).
    let store_log_fields: Vec<(&Ident, &TypeRef, Option<i64>)> = if is_store_agent {
        a.store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Log" && f.kind.args.len() == 1)
            .map(|f| {
                let retain = store_field_ty
                    .get(f.name.name.as_str())
                    .and_then(|kind| match kind {
                        StoreKindIr::Log(_, retain) => *retain,
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
    // 1) State record type.
    writeln!(out, "export interface {state_ty} {{").unwrap();
    for f in &effective_fields {
        let cell_ty = match store_field_ty.get(f.name.name.as_str()) {
            Some(StoreKindIr::Cell(t)) => *t,
            other => panic!(
                "bynk internal error: state field `{}` is not a resolved Cell in this \
                 function's own state reader, found {other:?}",
                f.name.name
            ),
        };
        writeln!(
            out,
            "  readonly {name}: {ty};",
            name = f.name.name,
            ty = ts_ty(cell_ty, tys),
        )
        .unwrap();
    }
    for (name, _) in &store_map_fields {
        let value_ty = match store_field_ty.get(name.name.as_str()) {
            Some(StoreKindIr::Map(_, v)) => *v,
            other => panic!(
                "bynk internal error: state field `{}` is not a resolved Map in this \
                 function's own state reader, found {other:?}",
                name.name
            ),
        };
        writeln!(
            out,
            "  readonly {name}: Record<string, {v}>;",
            name = name.name,
            v = ts_ty(value_ty, tys),
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
    for name in &set_field_names {
        writeln!(out, "  readonly {name}: Record<string, boolean>;").unwrap();
    }
    // v0.93 (ADR 0118): a sibling posting-list per `@indexed(by: f)` — field
    // value (stringified) → the primary keys whose value has it. Persisted and
    // committed wholesale with the map it indexes.
    for (map, fields) in sorted_index_fields(&store_map_indexes) {
        for f in fields {
            writeln!(out, "  readonly {map}__idx_{f}: Record<string, string[]>;").unwrap();
        }
    }
    for (name, _, _) in &store_cache_fields {
        let value_ty = match store_field_ty.get(name.name.as_str()) {
            Some(StoreKindIr::Cache(_, v, _)) => *v,
            other => panic!(
                "bynk internal error: state field `{}` is not a resolved Cache in this \
                 function's own state reader, found {other:?}",
                name.name
            ),
        };
        writeln!(
            out,
            "  readonly {name}: Record<string, {{ v: {v}; exp: number }}>;",
            name = name.name,
            v = ts_ty(value_ty, tys),
        )
        .unwrap();
    }
    for (name, _, _) in &store_log_fields {
        let elem_ty = match store_field_ty.get(name.name.as_str()) {
            Some(StoreKindIr::Log(t, _)) => *t,
            other => panic!(
                "bynk internal error: state field `{}` is not a resolved Log in this \
                 function's own state reader, found {other:?}",
                name.name
            ),
        };
        writeln!(
            out,
            "  readonly {name}: Array<{{ t: number; v: {v} }}>;",
            name = name.name,
            v = ts_ty(elem_ty, tys),
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
                let mut pre = Pre::new();
                let mut module = ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use);
                module.target = ctx.target;
                module.agent_method_givens = ctx.agent_method_givens.clone();
                module.event_schema_versions = ctx.event_schema_versions.clone();
                module.set_rebrand_info(commons, ctx);
                let mut icx = LowerCtx::new(module, BodyMode::StaticInit);
                icx.local_agents = ctx.local_agents.clone();
                let expr = pre.lower(init, &mut icx);
                // A static initialiser lowers to a pure expression (no setup
                // statements). #1029 review: if any appear, wrap them in an IIFE
                // rather than splicing them into a comma sequence. The comma
                // form only ever worked for expression-shaped hoists — a `const`
                // or an `if` operand does not parse — and T2.1 made a
                // statement-shaped hoist reachable here for the first time, since
                // a value-position `if` that hoists now yields `let …; if (…) {…}`
                // where it used to yield a self-contained arrow. An IIFE is
                // sound for this position specifically: a static initialiser has
                // no enclosing function to `return` out of, so the arrow cannot
                // swallow a control transfer the way it would in a handler body.
                if pre.is_empty() {
                    expr
                } else {
                    format!("(() => {{ {} return {expr}; }})()", pre.stmts().join(" "))
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
        for name in &set_field_names {
            parts.push(format!("{name}: {{}}"));
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
        let d = serialisation::deserialise_expr(ty, &json, path, &ctx.runtime_use);
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
                d = serialisation::deserialise_expr(
                    v,
                    "(__v as unknown as JsonValue)",
                    &name.name,
                    &ctx.runtime_use,
                ),
            ));
        }
        if let Some(k) = store_field_key_type(a, &name.name)
            && type_base_is_string(k, &commons.types)
        {
            rehydrate_checks.push(format!(
                "  for (const __k of Object.keys(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(k, "(__k as unknown as JsonValue)", &name.name, &ctx.runtime_use),
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
                d = serialisation::deserialise_expr(k, "(__k as unknown as JsonValue)", &name.name, &ctx.runtime_use),
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
                d = serialisation::deserialise_expr(t, "(__k as unknown as JsonValue)", &name.name, &ctx.runtime_use),
            ));
        }
    }
    // `Cache[K, V]` — validate each entry's `.v` against `V`.
    for (name, v, _) in &store_cache_fields {
        if is_codecable(v) {
            rehydrate_checks.push(format!(
                "  for (const __e of Object.values(s.{n})) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(v, "(__e.v as unknown as JsonValue)", &name.name, &ctx.runtime_use),
            ));
        }
    }
    // `Log[T]` — validate each entry's `.v` against `T`.
    for (name, t, _) in &store_log_fields {
        if is_codecable(t) {
            rehydrate_checks.push(format!(
                "  for (const __e of s.{n}) {{ const __r = {d}; if (__r.tag === \"Err\") throw rehydrationViolation(\"{agent_name}\", __r.error); }}",
                n = name.name,
                d = serialisation::deserialise_expr(t, "(__e.v as unknown as JsonValue)", &name.name, &ctx.runtime_use),
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
    // #527: an agent whose methods take `given` capabilities rebuilds those
    // deps *inside* the DO (providers cannot cross the JSON wire), and some
    // providers take the Worker `env` — workerd passes it as the DO
    // constructor's second argument. Bundle mode constructs with `state`
    // only and never uses the fetch path, so the parameter stays optional.
    let given_deps_expr = ctx.agent_given_deps.get(&a.name.name).cloned();
    // Events track, slice 0 (spine #936): the same wire problem #527 already
    // solved for capability providers hits `deps.__eventsDispatch` too — it is
    // a function, so it does not survive the DO's JSON wire either. An agent
    // whose own handler body emits directly (`body_emits_directly`'s
    // per-handler test, mirrored here at the whole-agent level) needs its
    // fetch dispatch to rebuild it from `env.EVENTS_FANOUT` exactly as a
    // `given` provider is rebuilt, so it takes the same env-carrying
    // constructor.
    let agent_uses_emit = a
        .handlers
        .iter()
        .any(|h| crate::emitter::block_uses_emit(&h.body, &commons.callees));
    let needs_env_ctor = given_deps_expr.is_some() || agent_uses_emit;
    if needs_env_ctor {
        writeln!(out, "  private __env: unknown;").unwrap();
        writeln!(
            out,
            "  constructor(state: DurableObjectState, env?: unknown) {{"
        )
        .unwrap();
        writeln!(out, "    this.state = state;").unwrap();
        writeln!(out, "    this.__env = env;").unwrap();
        writeln!(out, "  }}").unwrap();
    } else {
        writeln!(out, "  constructor(state: DurableObjectState) {{").unwrap();
        writeln!(out, "    this.state = state;").unwrap();
        writeln!(out, "  }}").unwrap();
    }
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
            let mut cx = LowerCtx::new(
                ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use),
                BodyMode::Invariant {
                    name: "s".to_string(),
                    fields: field_names.clone(),
                },
            );
            let mut pre = Pre::new();
            let pred = pre.lower(&inv.predicate, &mut cx);
            for s in pre.stmts() {
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
    // v0.116 (testing track slice 4): step invariants — evaluate each `transition`
    // against the pre-/post-commit state pair. The old state is still in storage
    // (this method performs the `put`), so reading it here yields the pre-commit
    // snapshot; `undefined` is the genesis commit, which has no prior state to
    // transition from and is skipped (snapshot invariants above still apply).
    // `old`/`new` are lowered to `__old`/`__new` (`new` is a JS reserved word). A
    // violation throws the same `InvariantViolation`-family fault, before the write.
    if !a.transitions.is_empty() {
        writeln!(
            out,
            "    const __prior = await this.state.storage.get<{state_ty}>(\"state\");"
        )
        .unwrap();
        writeln!(out, "    if (__prior !== undefined) {{").unwrap();
        writeln!(out, "      const __old = {{ ...{zero_fn}(), ...__prior }};").unwrap();
        writeln!(out, "      const __new = s;").unwrap();
        for tr in &a.transitions {
            let mut cx = LowerCtx::new(
                ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use),
                BodyMode::Transition {
                    old: "__old".to_string(),
                    new: "__new".to_string(),
                },
            );
            let mut pre = Pre::new();
            let pred = pre.lower(&tr.predicate, &mut cx);
            for s in pre.stmts() {
                writeln!(out, "      {s}").unwrap();
            }
            writeln!(out, "      if (!({pred})) {{").unwrap();
            writeln!(
                out,
                "        console.error(\"InvariantViolation {agent}.{name}\", {{ agent: \"{agent}\", invariant: \"{name}\" }});",
                agent = a.name.name,
                name = tr.name.name
            )
            .unwrap();
            writeln!(
                out,
                "        throw invariantViolation(\"{agent}\", \"{name}\");",
                agent = a.name.name,
                name = tr.name.name
            )
            .unwrap();
            writeln!(out, "      }}").unwrap();
        }
        writeln!(out, "    }}").unwrap();
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
            .map(|p| format!("{}: {}", ts_ident(&p.name.name), ts_type_ref(&p.type_ref)))
            .collect();
        // Lower body into a buffer so we can detect cross-context usage and
        // shape the deps type accordingly.
        let mut body_out = String::new();
        // v0.70: per-statement maps for the spliced handler body (see emit_service).
        let body_smb = RefCell::new(SourceMapBuilder::new());
        // v0.81: a store-agent handler reads/writes cells over a mutable working
        // record `__state`; a state-record handler uses `currentState`/`self.state`.
        // A store handler that performs any `:=` wraps its body in a closure so an
        // implicit commit runs at handler end on every (success) return path.
        // #1196/R6.5: write detection reads the checker's own resolved
        // `Callee::Store` classification (`ir::lower::body_writes_state`)
        // rather than matching a bare-identifier receiver name against this
        // agent's own field-name sets — a locally-shadowed field name (a
        // handler param, say) can no longer false-positive into an
        // unnecessary implicit-commit wrapper the way the deleted
        // `block_writes_state` could (`1196_agent_write_detection_via_
        // resolved_callee`'s own fixture pins the fix). A held `Map[K,
        // Connection]`'s own `put`/`remove` still triggers the commit flush
        // (v0.105, slice 3b-ii) with no extra name-set union needed here —
        // a held map is an ordinary `store Map` field as far as the checker's
        // own `Callee::Store` resolution is concerned, "held" being purely
        // this emitter's own downstream connection-resolution concern.
        let writes_state = is_store_agent && body_writes_state(&h.body, commons);
        let store = is_store_agent.then(|| {
            Box::new(AgentStoreState {
                state: ("__state".to_string(), cell_names.clone()),
                maps: map_names.clone(),
                sets: set_names.clone(),
                caches: cache_ttls.clone(),
                logs: log_retains.clone(),
                indexes: store_map_indexes.clone(),
                held_maps: held_maps_ts.clone(),
            })
        });
        // #934: mirrors the `method` name resolved below (all non-`method_name`
        // agent handler kinds resolve to `"call"`), computed here so it's
        // available before the body lowers.
        let scope_method = h
            .method_name
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "call".to_string());
        let mut module = ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use);
        module.in_bynk_unit = ctx.commons_name == "bynk";
        module.agent_method_givens = ctx.agent_method_givens.clone();
        module.event_schema_versions = ctx.event_schema_versions.clone();
        module.set_rebrand_info(commons, ctx);
        let mut cx = LowerCtx::new(
            module,
            BodyMode::AgentHandler {
                handler: HandlerShared {
                    capabilities: crate::ir::lower::lower_handler_given_ir(h)
                        .into_iter()
                        .map(|c| c.name)
                        .collect::<HashSet<_>>(),
                    handler_scope: Some(format!(
                        "{}.{}.{}",
                        ctx.commons_name, a.name.name, scope_method
                    )),
                    owning_context: ctx.commons_name.clone(),
                    ..HandlerShared::default()
                },
                in_agent_handler: true,
                agent_key_field: Some(a.key_name.name.clone()),
                store,
            },
        )
        .with_source_map(Some(&body_smb));
        cx.local_agents = ctx.local_agents.clone();
        // A handler param is emitted at its natural `ts_ident` name (above), so
        // it must be declared into scope the same way: otherwise a param that
        // shares a name with a `store Map`/`Set`/`Cache`/`Log` field is
        // invisible to `LowerCtx::is_local`, and the store-field dispatch
        // below silently wins over the parameter.
        for p in &h.params {
            cx.declare_binder(&p.name.name);
        }
        // P6.54 (design/tracks/the-ir.md §6b): computed once, not twice —
        // `async_kw` below used to call `is_effectful_return(&h.return_type)`
        // again for the identical value.
        let effectful = is_effectful_return(&h.return_type);
        let async_tail = effectful;
        // A writing store handler's body sits one level deeper, inside the
        // implicit-commit closure.
        let body_indent = if writes_state {
            INDENT_STEP * 3
        } else {
            INDENT_STEP * 2
        };
        emit_block_as_function_body_with_return(
            &mut body_out,
            &h.body,
            &mut cx,
            body_indent,
            async_tail,
            Some(&h.return_type),
        );
        let mut deps_ty = build_deps_object_ty_with_surface(
            &effective_given(&crate::ir::lower::lower_handler_given_ir(h), &cx),
            &cx,
            &ctx.cross_context,
            ctx.target,
        );
        // Events track, slice 0 (spine #936): see the matching field on the
        // service path (`emit_service`) — an agent handler that emits, or
        // that calls another local agent method which itself emits, gets
        // the same compose-supplied `__eventsDispatch` callback.
        let needs_events_dispatch = cx.is_first_party_events()
            && (crate::emitter::block_uses_emit(&h.body, &commons.callees)
                || cx
                    .agent_given_caps_used()
                    .is_some_and(|m| m.contains_key("Events")));
        if needs_events_dispatch {
            let field = format!(
                "__eventsDispatch: (events: Array<{}>) => Promise<void>",
                crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
            );
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
        let async_kw = if effectful { "async " } else { "" };
        let method = h
            .method_name
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| match lower_handler_kind_ir(&h.kind) {
                IrHandlerKind::Call => "call".to_string(),
                // HTTP/cron/queue/open handlers are service-only (rejected in
                // agents by the parser); these arms are defensive and unreachable
                // here.
                IrHandlerKind::Http { .. }
                | IrHandlerKind::Cron { .. }
                | IrHandlerKind::Message
                | IrHandlerKind::Open
                | IrHandlerKind::Close
                | IrHandlerKind::Event => "call".to_string(),
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
        // Events track, slice 0 (spine #936): the same release-at-commit
        // buffer the service path declares (see `emit_service`'s
        // `block_uses_emit` gate) — an agent handler needs the same
        // completion boundary, and a writing store-agent already has one
        // (`commitState`), so `__events` just rides alongside it there,
        // flushing to `deps.__eventsDispatch` (threaded above) once the
        // state commit itself has succeeded. Gated on `body_emits_directly`
        // specifically (not the broader `needs_events_dispatch` above) —
        // a handler that only *forwards* `__eventsDispatch` to another
        // local agent it calls has nothing of its own to buffer or flush;
        // `deps` (typed with the field) simply passes through unchanged.
        let body_emits_directly = crate::emitter::block_uses_emit(&h.body, &commons.callees);
        let events_decl = format!(
            "    const __events: Array<{}> = [];",
            crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
        );
        let flush = "    if (__events.length > 0) { await deps.__eventsDispatch(__events); }";
        if is_store_agent {
            if writes_state {
                writeln!(
                    out,
                    "    const __state = {{ ...(await this.loadState()) }};"
                )
                .unwrap();
                if body_emits_directly {
                    writeln!(out, "{events_decl}").unwrap();
                }
                writeln!(out, "    const __result = await (async () => {{").unwrap();
                splice(out);
                writeln!(out, "    }})();").unwrap();
                writeln!(out, "    await this.commitState(__state);").unwrap();
                if body_emits_directly {
                    writeln!(out, "{flush}").unwrap();
                }
                writeln!(out, "    return __result;").unwrap();
            } else if body_emits_directly {
                writeln!(out, "    const __state = await this.loadState();").unwrap();
                writeln!(out, "{events_decl}").unwrap();
                writeln!(out, "    const __result = await (async () => {{").unwrap();
                splice(out);
                writeln!(out, "    }})();").unwrap();
                writeln!(out, "{flush}").unwrap();
                writeln!(out, "    return __result;").unwrap();
            } else {
                writeln!(out, "    const __state = await this.loadState();").unwrap();
                splice(out);
            }
        } else if body_emits_directly {
            writeln!(out, "    const currentState = await this.loadState();").unwrap();
            writeln!(out, "{events_decl}").unwrap();
            writeln!(out, "    const __result = await (async () => {{").unwrap();
            splice(out);
            writeln!(out, "    }})();").unwrap();
            writeln!(out, "{flush}").unwrap();
            writeln!(out, "    return __result;").unwrap();
        } else {
            writeln!(out, "    const currentState = await this.loadState();").unwrap();
            splice(out);
        }
        writeln!(out, "  }}").unwrap();
        writeln!(out).unwrap();
    }
    // v0.104 (real-time track slice 3b): the `from websocket` `on open` handlers
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
            emit_ws_open_fetch_branch(out, host, tys);
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
        if given_deps_expr.is_some() || agent_uses_emit {
            // #527: the wire deps are JSON — any capability provider in them
            // is a dead plain object (its methods did not survive
            // serialisation). Rebuild this agent's `given` deps in-process,
            // exactly as compose wires them, and let them win the merge.
            //
            // Events track, slice 0: `deps.__eventsDispatch` is a function
            // too, and does not survive the wire any better than a provider
            // — rebuilt from this Worker's own `env.EVENTS_FANOUT` binding
            // (shared by every DO class this script hosts) rather than
            // trusting whatever the caller's JSON carried.
            // P7.2: matches `emitter/workers.rs`'s own established idiom for a
            // multi-binding env whose exact accessed keys vary by call site
            // (`given_deps_expr` may reference other bindings besides
            // `EVENTS_FANOUT`, computed elsewhere and not fully traced here) —
            // a generic string-keyed record, not a precise structural type.
            writeln!(
                out,
                "      const env = this.__env as unknown as Record<string, unknown>;"
            )
            .unwrap();
            let mut rebuilt: Vec<String> = Vec::new();
            if let Some(expr) = &given_deps_expr {
                writeln!(out, "      const __givenDeps = {expr};").unwrap();
                rebuilt.push("...__givenDeps".to_string());
            }
            if agent_uses_emit {
                let bind = crate::emitter::wrangler::agent_binding_name(
                    crate::emitter::wrangler::EVENTS_FANOUT_CLASS_NAME,
                );
                // P7.2: `env` is now `Record<string, unknown>` (above), so this
                // specific binding needs its own cast to what
                // `dispatchToEventsFanout` actually declares.
                writeln!(
                    out,
                    "      const __eventsDeps = {{ __eventsDispatch: (events: Array<{ev_ty}>) => dispatchToEventsFanout(env.{bind} as DurableObjectNamespace, events) }};",
                    ev_ty = crate::emitter::EVENTS_WIRE_EVENT_TS_TYPE
                )
                .unwrap();
                rebuilt.push("...__eventsDeps".to_string());
            }
            // P7.2: `methodName` is read from `url.pathname` at runtime, so no
            // static type can name which handler this is — a callable
            // dictionary is what's actually known, and `result` flows only into
            // a generic `JSON.stringify` below, so `unknown` costs nothing there.
            writeln!(
                out,
                "      const result = await (this as unknown as Record<string, (...bynkArgs: unknown[]) => unknown>)[methodName](...args, {{ ...((deps ?? {{}}) as Record<string, unknown>), {} }});",
                rebuilt.join(", ")
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "      const result = await (this as unknown as Record<string, (...bynkArgs: unknown[]) => unknown>)[methodName](...args, deps);"
            )
            .unwrap();
        }
        // `?? null`: a void method resolves to `undefined`, and
        // `JSON.stringify(undefined)` is the *string* `undefined` — not JSON —
        // which the calling proxy's `response.json()` rejects (#527).
        writeln!(
            out,
            "      return new Response(JSON.stringify(result ?? null), {{ headers: {{ \"content-type\": \"application/json\" }} }});"
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
            emit_ws_dispatch_handlers(out, host, &ctx.runtime_use, &commons.types, tys);
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

    // v0.119 (testing track slice 7, ADR 0155): the history-property driver. Only
    // agents a `for all run: History[Agent]` property targets get this exported
    // test-support function (gated on `history_target_agents`), so every other
    // agent's emission is byte-for-byte unchanged. It drives a generated call
    // sequence through the *real* handlers from a fresh instance, recording each
    // reached step — its call, whether it committed (an invariant/`transition`
    // refusal throws `invariantViolation`, leaving state uncommitted), and the
    // committed `old` → `new` pair — for the runner's predicate. Test-only, so it
    // is stripped from deploy builds with the rest of the test surface.
    if ctx.history_target_agents.contains(&a.name.name) {
        let hs: Vec<&Handler> = a
            .handlers
            .iter()
            .filter(|h| {
                matches!(lower_handler_kind_ir(&h.kind), IrHandlerKind::Call)
                    && h.method_name.is_some()
            })
            .collect();
        let driver = format!("__bynkDriveHistory_{}", a.name.name);
        let factory = agent_factory_name(&a.name.name);
        let key_val = bynk_check::checker::zero_value_ts(&a.key_type, None, &commons.types)
            .unwrap_or_else(|| "undefined as never".to_string());
        // P7.2: `call`'s real shape is a discriminated union across `hs` (one
        // variant per handler, built further down from `history_variant_tag` +
        // each handler's own fields) and `deps` varies per handler too (each
        // handler computes its own `deps_ty` from its own `given` set,
        // `build_deps_object_ty_with_surface`'s own per-handler call elsewhere
        // in this file) — a single driver-wide type for either needs the
        // intersection/union across every handler this agent's history targets,
        // not a same-line text change. `seq`'s own `args` is safe to narrow:
        // it's read back only through `history_arg_ts`-shaped call sites below,
        // never given a fixed shape of its own.
        let step_ty =
            format!("{{ call: any, accepted: boolean, old: {state_ty}, new: {state_ty} }}");
        writeln!(
            out,
            "export async function {driver}(seq: Array<{{ h: number, args: unknown[] }}>, deps: any): Promise<Array<{step_ty}>> {{"
        )
        .unwrap();
        writeln!(out, "  {registry}.reset();").unwrap();
        // P7.2: deferred, not narrowed. A first attempt dropped this cast,
        // reasoning `{factory}` already declares `): {agent}` and `.state`/
        // `.{{method}}` are real declared members — true, but it broke real
        // `tsc --strict` fixtures (248/249): `history_arg_ts` below produces
        // unbranded plain values (`Number(__st.args[0])`), which don't
        // structurally match a branded handler param type (`Amount`) once
        // `__inst` is genuinely `{agent}`-typed instead of `any`. Narrowing
        // `history_arg_ts`'s own output to brand its values correctly is the
        // real fix, not a same-line change here.
        writeln!(out, "  const __inst = {factory}({key_val}) as any;").unwrap();
        writeln!(
            out,
            "  const __load = async (): Promise<{state_ty}> => {{ const __s = await __inst.state.storage.get(\"state\"); return __s === undefined ? {zero_fn}() : {{ ...{zero_fn}(), ...__s }}; }};"
        )
        .unwrap();
        // P7.2: `e` is a caught throw of unknown shape by construction — a
        // marker type stating exactly the one field this checks, not a blanket
        // escape.
        writeln!(
            out,
            "  const __rej = (e: unknown) => !!e && (e as {{ invariantViolation?: unknown }}).invariantViolation !== undefined;"
        )
        .unwrap();
        writeln!(out, "  const __steps: Array<{step_ty}> = [];").unwrap();
        // A driven run deliberately provokes rejected steps (an invariant/
        // `transition` refusal), each of which `console.error`s an
        // `InvariantViolation` line from `commitState` before throwing. Mute just
        // those lines for the duration of the drive so the run isn't drowned in
        // expected noise; every other `console.error` still passes through, and the
        // original is restored in `finally`.
        // P7.2: `console.error`'s own real type, `(...data: unknown[]) => void`
        // — `__ce` is already that type by inference, and the replacement
        // matches it exactly, so neither cast was ever load-bearing.
        writeln!(out, "  const __ce = console.error;").unwrap();
        writeln!(
            out,
            "  console.error = (...__a: unknown[]) => {{ if (typeof __a[0] === \"string\" && __a[0].startsWith(\"InvariantViolation\")) return; __ce(...__a); }};"
        )
        .unwrap();
        writeln!(out, "  try {{").unwrap();
        writeln!(out, "  for (const __st of seq) {{").unwrap();
        writeln!(out, "    const __old = await __load();").unwrap();
        writeln!(out, "    let __accepted = true;").unwrap();
        writeln!(out, "    try {{").unwrap();
        writeln!(out, "      switch (__st.h) {{").unwrap();
        for (i, h) in hs.iter().enumerate() {
            let m = &h.method_name.as_ref().unwrap().name;
            let mut call_args: Vec<String> = h
                .params
                .iter()
                .enumerate()
                .map(|(j, p)| history_arg_ts(p, j, &commons.types))
                .collect();
            call_args.push("deps".to_string());
            writeln!(
                out,
                "        case {i}: await __inst.{m}({}); break;",
                call_args.join(", ")
            )
            .unwrap();
        }
        writeln!(out, "      }}").unwrap();
        writeln!(
            out,
            "    }} catch (__e) {{ if (__rej(__e)) {{ __accepted = false; }} else {{ throw __e; }} }}"
        )
        .unwrap();
        writeln!(
            out,
            "    const __new = __accepted ? await __load() : __old;"
        )
        .unwrap();
        // P7.2: deferred, same reason as `step_ty`'s own `call` field above —
        // a real type here is the same per-handler discriminated union.
        writeln!(out, "    let __call: any;").unwrap();
        writeln!(out, "    switch (__st.h) {{").unwrap();
        for (i, h) in hs.iter().enumerate() {
            let tag = history_variant_tag(&h.method_name.as_ref().unwrap().name);
            let fields: Vec<String> = h
                .params
                .iter()
                .enumerate()
                .map(|(j, p)| {
                    format!(
                        "{}: {}",
                        ts_ident(&p.name.name),
                        history_arg_ts(p, j, &commons.types)
                    )
                })
                .collect();
            let obj = if fields.is_empty() {
                format!("{{ tag: \"{tag}\" }}")
            } else {
                format!("{{ tag: \"{tag}\", {} }}", fields.join(", "))
            };
            writeln!(out, "      case {i}: __call = {obj}; break;").unwrap();
        }
        writeln!(out, "    }}").unwrap();
        writeln!(
            out,
            "    __steps.push({{ call: __call, accepted: __accepted, old: __old, new: __new }});"
        )
        .unwrap();
        writeln!(out, "  }}").unwrap();
        writeln!(out, "  return __steps;").unwrap();
        writeln!(out, "  }} finally {{ console.error = __ce; }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
}

/// v0.104 (real-time track slice 3b): a `from websocket` `on open` handler hosted
/// in a Durable Object (DECISION A) — the service it belongs to, the handler, the
/// protocol's `out` frame type (the `Connection`'s parameter), and the Bearer
/// seam (the edge authenticates with it; the DO method threads the verified
/// identity through `deps`).
struct WsOpenHost<'a> {
    service: &'a str,
    handler: &'a Handler,
    // P6.52 (design/tracks/the-ir.md §6b): `TyId`, not `&'a TypeRef` — read
    // off `ProtocolIr::WebSocket` (`lower_protocol_ir_from_commons`) once at
    // `ws_open_hosts_for`'s own construction site, instead of every renderer
    // below re-resolving the raw `ServiceProtocol::WebSocket` frame types
    // itself.
    out_ty: TyId,
    // v0.106 (slice 3b-iii): the service's inbound `in` frame type and its optional
    // `on message`/`on close` handlers — run in this same DO (`webSocketMessage`/
    // `webSocketClose`), with identity + route args recovered from the socket
    // attachment the `on open` accept wrote.
    in_ty: TyId,
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
        let ProtocolIr::WebSocket { out_ty, in_ty } =
            lower_protocol_ir_from_commons(&s.protocol, commons)
        else {
            continue;
        };
        for h in &s.handlers {
            if !matches!(lower_handler_kind_ir(&h.kind), IrHandlerKind::Open) {
                continue;
            }
            if let crate::emitter::websocket::WsOpenShape::One(t) =
                crate::emitter::websocket::analyse_open_shape(&h.body, local_agents)
                && t.agent == agent
            {
                hosts.push(WsOpenHost {
                    service: &s.name.name,
                    handler: h,
                    out_ty,
                    in_ty,
                    message: s
                        .handlers
                        .iter()
                        .find(|h| matches!(lower_handler_kind_ir(&h.kind), IrHandlerKind::Message)),
                    close: s
                        .handlers
                        .iter()
                        .find(|h| matches!(lower_handler_kind_ir(&h.kind), IrHandlerKind::Close)),
                    seam: bynk_check::actors::bearer_seam_for(h, actors),
                });
            }
        }
    }
    hosts
}

/// Emit the DO method that runs a hosted `from websocket` lifecycle body (`on
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
        ts_ty(host.out_ty, commons.tys())
    )];
    for p in &h.params {
        params.push(format!(
            "{}: {}",
            ts_ident(&p.name.name),
            ts_type_ref(&p.type_ref)
        ));
    }
    let body_smb = RefCell::new(SourceMapBuilder::new());
    let mut module = ModuleCtx::new(commons, &ctx.cross_context, &ctx.runtime_use);
    module.in_bynk_unit = ctx.commons_name == "bynk";
    module.agent_method_givens = ctx.agent_method_givens.clone();
    module.event_schema_versions = ctx.event_schema_versions.clone();
    module.set_rebrand_info(commons, ctx);
    module.target = ctx.target;
    let mut cx = LowerCtx::new(
        module,
        BodyMode::WsDoMethod {
            handler: HandlerShared {
                capabilities: crate::ir::lower::lower_handler_given_ir(h)
                    .into_iter()
                    .map(|c| c.name)
                    .collect::<HashSet<_>>(),
                // #934: the hosting agent + lifecycle method name
                // (`open`/`message`/`close`).
                handler_scope: Some(format!(
                    "{}.{}.{}",
                    ctx.commons_name, agent.name.name, method
                )),
                owning_context: ctx.commons_name.clone(),
                ..HandlerShared::default()
            },
            deps_identity_binder: host.seam.as_ref().and_then(|s| s.binder.clone()),
            ws_self_agent: Some(agent.name.name.clone()),
        },
    )
    .with_source_map(Some(&body_smb));
    cx.local_agents = ctx.local_agents.clone();
    let async_tail = is_effectful_return(&h.return_type);
    let mut body_out = String::new();
    emit_block_as_function_body_with_return(
        &mut body_out,
        &h.body,
        &mut cx,
        INDENT_STEP * 2,
        async_tail,
        Some(&h.return_type),
    );
    let mut deps_ty = build_deps_object_ty_with_surface(
        &effective_given(&crate::ir::lower::lower_handler_given_ir(h), &cx),
        &cx,
        &ctx.cross_context,
        ctx.target,
    );
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
fn emit_ws_open_fetch_branch(out: &mut String, host: &WsOpenHost<'_>, tys: &Arc<Types>) {
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
        ts_ty(host.out_ty, tys)
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
fn emit_ws_dispatch_handlers(
    out: &mut String,
    host: &WsOpenHost<'_>,
    runtime_use: &RuntimeUse,
    types: &HashMap<String, Arc<TypeDecl>>,
    tys: &Arc<Types>,
) {
    if !host.has_inbound() {
        return;
    }
    let out_ts = ts_ty(host.out_ty, tys);
    let att_ty = "{ connId: string; identity: string; args: unknown[] }";
    // The firing socket's minimal structural surface (attachment + send/close), so
    // emitted code stays free of `@cloudflare/workers-types`.
    let ws_ty = "{ deserializeAttachment(): unknown; send(data: string): void; close(code?: number, reason?: string): void }";
    let deps_arg = ws_attachment_deps_arg(&host.seam);

    if let Some(m) = host.message {
        let method = ws_message_do_method_name(host.service);
        // P6.52 (design/tracks/the-ir.md §6b): `host.in_ty` round-trips back
        // to a `TypeRef` for `serialisation::deserialise_expr`, the excluded
        // codec renderer's own boundary (P6.33 ruled it phase 7, `TypeRef`-
        // driven by definition). `ty_to_type_ref` only returns `None` for a
        // function/effect/type-variable `Ty` — none of which a `from
        // websocket` service's own `in:` frame type can ever resolve to,
        // since `check_service_protocols` already constrains it to a
        // codec-eligible type before this function ever runs.
        let in_type = ty_to_type_ref(host.in_ty, tys).unwrap_or_else(|| {
            panic!(
                "bynk internal error: a `from websocket` service's own `in:` frame type did \
                 not round-trip to a TypeRef, but check_service_protocols already certified it \
                 codec-eligible"
            )
        });
        let decode = serialisation::deserialise_expr(&in_type, "__json", "frame", runtime_use);
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
        // Resolved-`Ty` equality (not `type_refs_match`'s surface-syntax
        // comparison, which fell through to `_ => false` for a `List`/`Map`/
        // `Query`/… frame type — a false negative here emits every param as
        // a route value, silently wrong for a frame type that shape covers):
        // must agree with `check_service_protocols`'s identical resolved
        // check, or the checker's "exactly one frame param" could accept a
        // program this picks a different (or no) param's argument for.
        let no_vars = HashSet::new();
        let resolve_ty = |t: &TypeRef| {
            bynk_check::checker::resolve_type_ref_in(t, types, &no_vars, tys)
                .unwrap_or(tys.intern(bynk_check::checker::Ty::Unit))
        };
        for p in &m.params {
            if resolve_ty(&p.type_ref) == host.in_ty {
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

#[cfg(test)]
mod doc_block_tests {
    use super::emit_doc_block;

    /// A `*/` in a doc body must not close the JSDoc comment early — otherwise
    /// the trailing text lands as executable top-level TypeScript (issue #720).
    #[test]
    fn escapes_comment_terminator() {
        let mut out = String::new();
        emit_doc_block(
            &mut out,
            Some("docs */ ; (globalThis as any).PWNED = true; /*"),
            0,
        );
        assert!(
            !out.contains("*/ ;"),
            "unescaped comment terminator leaked into output: {out}"
        );
        assert!(out.contains("*\\/ ;"), "expected escaped terminator: {out}");
        // The single legitimate closer is the one we emit last.
        assert_eq!(
            out.matches("*/").count(),
            1,
            "exactly one real closer: {out}"
        );
        assert!(out.trim_end().ends_with("*/"));
    }

    /// Overlapping/adjacent terminators must not survive or re-form a `*/`
    /// under the non-overlapping left-to-right `str::replace`. Pins the
    /// behaviour against a future refactor away from `str::replace`.
    #[test]
    fn escapes_pathological_terminators() {
        for body in ["*/*/", "**/", "*/*", "*/ */ */"] {
            let mut out = String::new();
            emit_doc_block(&mut out, Some(body), 0);
            // The only surviving `*/` is the emitter's own trailing closer.
            assert_eq!(
                out.matches("*/").count(),
                1,
                "input {body:?} left a stray closer: {out}"
            );
            assert!(out.trim_end().ends_with("*/"), "input {body:?}: {out}");
        }
    }

    /// #1333: the real, converted `emit_doc_block` matches
    /// `137_agent_instantiation_workers`'s own real header comment
    /// byte-for-byte — not a synthetic string, the real fixture content
    /// this function's own conversion is verified against.
    #[test]
    fn matches_the_real_fixtures_own_header_comment_byte_for_byte() {
        let mut out = String::new();
        emit_doc_block(
            &mut out,
            Some(
                "A minimal stateful agent in the bundle target: instantiation lowers through the\ngenerated factory, the method call is a direct call, and state persists per key\nacross calls within a session.",
            ),
            0,
        );
        assert_eq!(
            out,
            "/**\n \
             * A minimal stateful agent in the bundle target: instantiation lowers through the\n \
             * generated factory, the method call is a direct call, and state persists per key\n \
             * across calls within a session.\n \
             */\n"
        );
    }

    /// #1333: `indent` is always an exact multiple of `INDENT_STEP` in
    /// every real call site (`0` or `INDENT_STEP`) — pins that the
    /// `indent / INDENT_STEP` conversion to the printer's own depth unit
    /// produces the real, historic indentation exactly.
    #[test]
    fn indents_at_the_nested_indent_step() {
        let mut out = String::new();
        emit_doc_block(&mut out, Some("a method"), crate::emitter::INDENT_STEP);
        assert_eq!(out, "  /**\n   * a method\n   */\n");
    }
}

#[cfg(test)]
mod ts_type_params_tests {
    use super::ts_type_params;
    use bynk_syntax::ast::{Ident, TypeParam};
    use bynk_syntax::span::Span;

    fn type_param(name: &str) -> TypeParam {
        TypeParam {
            name: Ident {
                name: name.to_string(),
                span: Span::new(0, name.len()),
            },
            span: Span::new(0, name.len()),
        }
    }

    #[test]
    fn empty_params_render_as_the_empty_string() {
        assert_eq!(ts_type_params(&[]), "");
    }

    /// #1333: the real, converted `ts_type_params` matches
    /// `816_locale_negotiation_no_bundle_regression`'s own real
    /// `export function map<A, B>(...)` generic list byte-for-byte.
    #[test]
    fn matches_the_real_fixtures_own_two_param_generic_list() {
        let params = [type_param("A"), type_param("B")];
        assert_eq!(ts_type_params(&params), "<A, B>");
    }

    #[test]
    fn single_param_renders_with_no_separator() {
        assert_eq!(ts_type_params(&[type_param("T")]), "<T>");
    }
}

#[cfg(test)]
mod refined_checks_tests {
    use super::{emit_pred_check, print_numeric_guard_stmt};
    use bynk_syntax::ast::PredKind;

    /// #1335: the real, converted `print_numeric_guard_stmt` matches
    /// `254_multi_file_commons_workers_codec`'s own real `Cents.of`
    /// `Int`-base guard byte-for-byte.
    #[test]
    fn numeric_guard_matches_the_real_fixtures_own_int_guard_byte_for_byte() {
        assert_eq!(
            print_numeric_guard_stmt("Cents", "isInteger", "must be an integer"),
            "    if (!Number.isInteger(value)) {\n      \
             return Err({ field: \"Cents\", message: \"must be an integer\", value });\n    \
             }\n"
        );
    }

    /// #1335: the real, converted `emit_pred_check` matches
    /// `254_multi_file_commons_workers_codec`'s own real `Cents.of`
    /// `NonNegative` predicate check byte-for-byte.
    #[test]
    fn pred_check_matches_the_real_fixtures_own_non_negative_check_byte_for_byte() {
        let mut out = String::new();
        emit_pred_check(&mut out, "Cents", &PredKind::NonNegative);
        assert_eq!(
            out,
            "    if (!(value >= 0)) {\n      \
             return Err({ field: \"Cents\", message: \"must be non-negative\", value });\n    \
             }\n"
        );
    }

    /// #1335's own real deviation from the accepted proposal: `msg` is
    /// carried as opaque, pre-quoted text (not `TsLit::Str`) specifically
    /// because `PredKind::Matches`'s own message embeds
    /// `escape_ts_string`-escaped pattern text directly. A source pattern
    /// with one real backslash (`\d+`) comes back from `escape_ts_string`
    /// already doubled to two backslash *characters* (the escaped form —
    /// what a TS string literal must contain for the runtime string to hold
    /// one literal backslash), and both the condition's own regex-source
    /// string and the message embed that same already-escaped text raw,
    /// matching the pre-conversion `writeln!` code exactly. If `msg` were
    /// instead run through `TsLit::Str`'s own escaper (as the accepted
    /// proposal's own Decision B originally called for), those two already-
    /// doubled backslash characters would each be escaped AGAIN, quadrupling
    /// the original single backslash — this test pins the correct,
    /// once-escaped form and would fail under that double-escaping bug. Not
    /// reachable by any fixture today (no `Matches` predicate in the corpus
    /// uses a backslash), so this is the only proof this real, latent bug
    /// class stays closed.
    #[test]
    fn pred_check_does_not_double_escape_a_matches_patterns_own_backslash() {
        let mut out = String::new();
        emit_pred_check(&mut out, "Code", &PredKind::Matches(r"\d+".to_string()));
        // `escape_ts_string` doubles the pattern's one real backslash to two
        // backslash *characters* — the correctly-escaped form, not a bug.
        // Both the regex-source string and the message embed that same
        // twice-backslash text raw (one escaping pass, not two).
        assert!(
            out.contains("\"^(?:\" + \"\\\\d+\" + \")$\""),
            "condition should carry the once-escaped two-backslash form: {out}"
        );
        assert!(
            out.contains("must match /\\\\d+/"),
            "message should carry the same once-escaped two-backslash form: {out}"
        );
        // A double-escaping bug would quadruple the original single
        // backslash to four backslash characters in the message.
        assert!(
            !out.contains("must match /\\\\\\\\d+/"),
            "message must not be double-escaped to four backslashes: {out}"
        );
    }
}

#[cfg(test)]
mod messages_template_tests {
    use super::{TemplateSegment, placeholder_names, split_template};
    use std::collections::BTreeSet;

    fn kinds(template: &str) -> Vec<(&str, &str)> {
        split_template(template)
            .into_iter()
            .map(|s| match s {
                TemplateSegment::Literal(l) => ("lit", l),
                TemplateSegment::Placeholder { inner, .. } => ("ph", inner),
            })
            .collect()
    }

    #[test]
    fn literal_only_template_is_one_segment() {
        assert_eq!(kinds("Bye"), vec![("lit", "Bye")]);
    }

    #[test]
    fn empty_template_is_one_empty_literal_segment() {
        // emit_message_entry_renderer's `parts.is_empty()` guard exists
        // specifically because split_template never returns zero segments —
        // this pins that invariant.
        assert_eq!(kinds(""), vec![("lit", "")]);
    }

    #[test]
    fn single_placeholder_with_surrounding_literal_text() {
        assert_eq!(
            kinds("Hello, {name}!"),
            vec![("lit", "Hello, "), ("ph", "name"), ("lit", "!")],
        );
    }

    #[test]
    fn placeholder_at_the_very_start_or_end() {
        assert_eq!(kinds("{name}!"), vec![("ph", "name"), ("lit", "!")]);
        assert_eq!(kinds("Hi, {name}"), vec![("lit", "Hi, "), ("ph", "name")]);
    }

    #[test]
    fn back_to_back_placeholders() {
        assert_eq!(kinds("{a}{b}"), vec![("ph", "a"), ("ph", "b")],);
    }

    #[test]
    fn unmatched_brace_and_empty_braces_are_literal_text() {
        // No closing `}` at all.
        assert_eq!(kinds("a {b"), vec![("lit", "a {b")]);
        // Empty `{}` names nothing, so it's not a placeholder either.
        assert_eq!(kinds("a {} b"), vec![("lit", "a {} b")]);
    }

    #[test]
    fn multibyte_literal_text_around_a_placeholder() {
        // A non-ASCII literal must not panic the byte-index scan and must
        // round-trip intact (UTF-8 char-boundary safety, not just byte count).
        assert_eq!(
            kinds("caf\u{e9} {name} \u{1f980}"),
            vec![("lit", "caf\u{e9} "), ("ph", "name"), ("lit", " \u{1f980}")],
        );
    }

    // -- message-bundles slice 3 (#878): ICU-dispatch placeholders --

    #[test]
    fn icu_plural_placeholder_captures_full_inner_text_with_nested_braces() {
        assert_eq!(
            kinds("You have {n, plural, one {# item} other {# items}}."),
            vec![
                ("lit", "You have "),
                ("ph", "n, plural, one {# item} other {# items}"),
                ("lit", "."),
            ],
        );
    }

    #[test]
    fn icu_placeholder_and_plain_placeholder_side_by_side() {
        assert_eq!(
            kinds("{name}: {n, number}"),
            vec![("ph", "name"), ("lit", ": "), ("ph", "n, number")],
        );
    }

    #[test]
    fn icu_placeholder_with_no_true_close_falls_back_to_literal() {
        // A `,` precedes the naive first `}`, so this is ICU-dispatch — but
        // the arm's own `{` is never closed, so `find_icu_close` never
        // reaches depth 0. Same "just literal text" policy as an unmatched
        // plain `{`.
        assert_eq!(
            kinds("{n, plural, one {# item"),
            vec![("lit", "{n, plural, one {# item")],
        );
    }

    #[test]
    fn placeholder_names_trims_icu_names_but_not_plain_ones() {
        assert_eq!(placeholder_names("{ name }"), BTreeSet::from([" name "]));
        assert_eq!(
            placeholder_names("{ n , plural, one {#} other {#}}"),
            BTreeSet::from(["n"])
        );
    }
}

/// Differential coverage for `emit_type`/`emit_record_type`/`emit_sum_type`/
/// `emit_refined_type` reading `bynk-emit::ir::TypeShape` instead of walking
/// `TypeDecl`/`TypeBody` directly (P6.x, #1188). Every expected string below
/// was captured from this slice's own converted output and cross-checked by
/// hand against `ts_type_ref_with`'s formatting (the pre-existing `TypeRef`
/// renderer `ts_ty`, `emitter.rs:4130`, replaces for record/sum field types) —
/// the two produce identical text for every shape exercised here (base types
/// including `Bytes`, a bare generic type variable, a bare named cross-type
/// reference, a `List`/`Option` wrapper, a generic application (`Box[Int]`),
/// and a refined type's brand surviving as a field type rather than
/// collapsing to its base).
#[cfg(test)]
mod type_shape_emission_tests {
    use crate::testkit::{emit_bundle, emit_source};

    const TYPES_FIXTURE: &str = r#"
commons demo {
  type Order = { id: String, total: Int }
  type Box[T] = { value: T }
  type PaymentError = enum { Declined, InsufficientFunds }
  type OrderError =
    | OutOfStock(sku: String, qty: Int)
    | Payment(reason: PaymentError)
    embeds PaymentError as Payment
  type Age = Int where Positive
  type UserId = opaque Int
  type Extras = { tags: List[String], blob: Bytes, note: Option[String], boxed: Box[Int], age: Age }
}
"#;

    /// Pins the `ts_type_ref` → `ts_ty` equivalence claim past the four
    /// shapes `Order`/`Box`/`OrderError` already exercise (review on #1190):
    /// a collection wrapper (`List`), a non-`number` base (`Bytes`), another
    /// wrapper (`Option`), a *generic application* (`Box[Int]` — the
    /// `TypeRef::App`/`Ty::Named{args: non-empty}` arm, distinct from the
    /// bare `Named` arm `OrderError`'s `reason: PaymentError` field already
    /// covers), and a refined field (`Age`) to assert its brand survives
    /// rather than collapsing to its base `number`.
    #[test]
    fn record_field_types_cover_wrappers_generics_and_refined_brands() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains(
                "export interface Extras {\n  \
                 readonly tags: readonly string[];\n  \
                 readonly blob: Uint8Array;\n  \
                 readonly note: Option<string>;\n  \
                 readonly boxed: Box<number>;\n  \
                 readonly age: Age;\n}\n"
            ),
            "{ts}"
        );
    }

    #[test]
    fn record_type_emits_interface_and_namespace_object() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains(
                "export interface Order {\n  readonly id: string;\n  readonly total: number;\n}\n"
            ),
            "{ts}"
        );
        assert!(ts.contains("export const Order = {\n};\n"), "{ts}");
    }

    /// #1339: pins `emit_sum_type`'s own real multi-line, leading-pipe
    /// union shape and its generic payload-constructor arrows byte-for-byte
    /// — the expected strings below are transcribed directly from
    /// `406_generic_sum_envelope`'s own real `expected.ts`
    /// (`bynkc/tests/fixtures/positive/406_generic_sum_envelope`), confirmed
    /// to match it exactly, not independently invented text that merely
    /// looks plausible.
    #[test]
    fn generic_sum_type_matches_the_real_fixtures_own_multiline_union_byte_for_byte() {
        let ts = emit_source(
            r#"
commons envelope {
  type ApiResult[T] =
    | Loaded(value: T)
    | Failed(message: String)
}
"#,
        );
        assert!(
            ts.contains(
                "export type ApiResult<T> =\n    \
                 { readonly tag: \"Loaded\"; readonly value: T }\n  \
                 | { readonly tag: \"Failed\"; readonly message: string };\n"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export const ApiResult = {\n  \
                 Loaded: <T>(value: T): ApiResult<T> => ({ tag: \"Loaded\", value }),\n  \
                 Failed: <T>(message: string): ApiResult<T> => ({ tag: \"Failed\", message }),\n\
                 };\n"
            ),
            "{ts}"
        );
    }

    #[test]
    fn generic_record_type_erases_its_type_parameter() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains("export interface Box<T> {\n  readonly value: T;\n}\n"),
            "{ts}"
        );
    }

    #[test]
    fn sum_type_emits_discriminated_union_and_nullary_constants() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains(
                "export type PaymentError =\n    { readonly tag: \"Declined\" }\n  | \
                 { readonly tag: \"InsufficientFunds\" };\n"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export const PaymentError = {\n  \
                 Declined: { tag: \"Declined\" } as PaymentError,\n  \
                 InsufficientFunds: { tag: \"InsufficientFunds\" } as PaymentError,\n};\n"
            ),
            "{ts}"
        );
    }

    #[test]
    fn sum_type_payload_variants_emit_typed_fields_and_constructors() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains(
                "export type OrderError =\n    \
                 { readonly tag: \"OutOfStock\"; readonly sku: string; readonly qty: number }\n  | \
                 { readonly tag: \"Payment\"; readonly reason: PaymentError };\n"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export const OrderError = {\n  \
                 OutOfStock: (sku: string, qty: number): OrderError => ({ tag: \"OutOfStock\", sku, qty }),\n  \
                 Payment: (reason: PaymentError): OrderError => ({ tag: \"Payment\", reason }),\n};\n"
            ),
            "{ts}"
        );
    }

    #[test]
    fn refined_type_emits_branded_alias_and_of_constructor_without_unsafe() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains("export type Age = number & { readonly __brand: \"Age\" };\n"),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export const Age = {\n  of(value: number): Result<Age, ValidationError> {\n    \
                 if (!Number.isInteger(value)) {\n      \
                 return Err({ field: \"Age\", message: \"must be an integer\", value });\n    }\n    \
                 if (!(value > 0)) {\n      \
                 return Err({ field: \"Age\", message: \"must be positive\", value });\n    }\n    \
                 return Ok(value as Age);\n  },\n};\n"
            ),
            "{ts}"
        );
        assert!(!ts.contains("unsafe(value: number): Age"), "{ts}");
    }

    #[test]
    fn opaque_type_emits_branded_alias_and_unsafe_constructor() {
        let ts = emit_source(TYPES_FIXTURE);
        assert!(
            ts.contains("export type UserId = number & { readonly __brand: \"UserId\" };\n"),
            "{ts}"
        );
        assert!(
            ts.contains("  unsafe(value: number): UserId {\n    return value as UserId;\n  },\n"),
            "{ts}"
        );
    }

    /// Exercises `emit_project`'s own Type/Event loops (Decision A/B, #1188) —
    /// a separate call site from `emit_source`'s single-file `emit()` path
    /// above, going through the full multi-file project pipeline instead.
    #[test]
    fn record_and_sum_types_emit_identically_through_the_project_pipeline() {
        let ts = emit_bundle(
            r#"
type Order = { id: String, total: Int }
type PaymentError = enum { Declined, InsufficientFunds }
"#,
        );
        assert!(
            ts.contains(
                "export interface Order {\n  readonly id: string;\n  readonly total: number;\n}\n"
            ),
            "{ts}"
        );
        assert!(
            ts.contains(
                "export type PaymentError =\n    { readonly tag: \"Declined\" }\n  | \
                 { readonly tag: \"InsufficientFunds\" };\n"
            ),
            "{ts}"
        );
    }

    /// Exercises `emit_project`'s **Event** mirror loop specifically (review
    /// on #1190) — the one call site whose correctness this slice's own
    /// `type_shape_for` leans on without a fallback: that `TypedCommons::types`
    /// already holds an entry keyed `e.name.name` for every `CommonsItem::
    /// Event`, the same table an ordinary declared type resolves through
    /// (Decision B, #1188's own doc comment on `type_shape_for`). The prior
    /// test above only reaches the Type loop; `emit_bundle` can't express an
    /// event at all (it wraps its body in `commons app.bundle`, and `event`
    /// outside a `context` is `bynk.event.outside_context`), so this drives
    /// `project::compile_in_memory` directly with a context source instead.
    #[test]
    fn event_type_emits_through_the_project_pipelines_event_mirror_loop() {
        let out = crate::project::compile_in_memory(
            "context demo {\n  event Notified = { id: String }\n}\n",
            crate::project::BuildTarget::Bundle,
            Default::default(),
        )
        .unwrap_or_else(|_| panic!("event-in-context fixture should compile"));
        let ts = out
            .artefacts
            .docs
            .iter()
            .find(|(path, _)| path.to_string_lossy().contains("demo"))
            .map(|(_, doc)| doc.text())
            .unwrap_or_else(|| panic!("the demo context's own module should be in the output"));
        assert!(
            ts.contains("export interface Notified {\n  readonly id: string;\n}\n"),
            "{ts}"
        );
    }

    /// Review of #1338, finding 1: a type parameter literally named `deps`
    /// (a value-identifier reserved word, but a perfectly legal Bynk type
    /// parameter — `parse_optional_type_params` accepts any identifier) must
    /// render UNESCAPED at an attached method's own generic-declaration
    /// site, matching every other reference to that same parameter
    /// (`self_ty_args`, the method's own param/return types). Before the
    /// fix, `generics` ran the name through `ts_ident` (value-identifier
    /// escaping), producing `get<__id_deps>(self: Box<deps>): deps { ... }`
    /// — `deps` undeclared, `__id_deps` unused, a real `tsc` error; no
    /// fixture in the corpus covered a reserved-word type param, so the
    /// zero-diff check alone could not have caught this.
    #[test]
    fn a_reserved_word_type_parameter_renders_unescaped_on_an_attached_method() {
        let ts = emit_source(
            r#"
commons demo {
  type Box[deps] = { value: deps }

  fn Box.get(self) -> deps {
    self.value
  }
}
"#,
        );
        assert!(
            ts.contains("get<deps>(self: Box<deps>): deps {"),
            "expected the bare, unescaped `deps` type parameter at every \
             reference (declaration, receiver, return type); got:\n{ts}"
        );
        assert!(
            !ts.contains("__id_deps"),
            "the type parameter's own declaration site must not be escaped \
             via ts_ident (that renaming is for value identifiers only); \
             got:\n{ts}"
        );
    }
}

/// Differential coverage for `emit_capability` reading `bynk-emit::ir::OpSig`
/// (built by `lower_capability_item_ir`/`lower_op_sig_ir`) instead of
/// walking `CapabilityOp::params`/`return_type` `TypeRef`s directly (P6.x,
/// #1193, slice 3 of #1187). `STORE_FIXTURE` covers a multi-op capability, a
/// named return/param type (`Order`), and — new relative to #1188's own
/// corpus — a generic op (`get[T]`), whose `Effect[Option[T]]` return type
/// exercises `ts_ty`'s `Ty::Var` arm the way no pre-existing capability
/// fixture's own emitted-TS assertions did. `capability` is a context-only
/// construct (`bynk.capability.outside_context`), so — like the Event mirror
/// test above — this drives `project::compile_in_memory` directly with a
/// context source rather than `emit_source`/`emit_bundle`.
#[cfg(test)]
mod capability_op_sig_emission_tests {
    const STORE_FIXTURE: &str = r#"
context demo {
  type Order = { id: String, total: Int }

  capability Store {
    fn get[T](key: String) -> Effect[Option[T]]
    fn put(key: String, value: Order) -> Effect[()]
    fn count() -> Effect[Int]
  }
}
"#;

    fn emit_store_context(src: &str) -> String {
        let out = crate::project::compile_in_memory(
            src,
            crate::project::BuildTarget::Bundle,
            Default::default(),
        )
        .unwrap_or_else(|_| panic!("capability fixture should compile:\n{src}"));
        out.artefacts
            .docs
            .iter()
            .find(|(path, _)| path.to_string_lossy().contains("demo"))
            .map(|(_, doc)| doc.text())
            .unwrap_or_else(|| panic!("the demo context's own module should be in the output"))
    }

    /// A non-generic op's param/return types (`Order`, a named record type,
    /// and `Effect[()]`/`Effect[Int]`, both base-type-adjacent) render
    /// identically to `ts_type_ref`'s pre-cutover output.
    #[test]
    fn named_and_base_return_types_render_through_ts_ty() {
        let ts = emit_store_context(STORE_FIXTURE);
        assert!(
            ts.contains("put(key: string, value: Order): Promise<void>;\n"),
            "{ts}"
        );
        assert!(ts.contains("count(): Promise<number>;\n"), "{ts}");
    }

    /// The generic op: `get[T]`'s own rigid type variable renders bare
    /// (`ts_ty`'s `Ty::Var` arm) inside the `Effect[Option[T]]` return type's
    /// `Option<…>` wrapper — `<T>` on the interface method itself still
    /// comes from `op.type_params` (`ts_type_params`), not `sig.type_params`
    /// (Decision A/B, #1193).
    #[test]
    fn generic_op_renders_its_rigid_type_var_and_type_param_list() {
        let ts = emit_store_context(STORE_FIXTURE);
        assert!(
            ts.contains("get<T>(key: string): Promise<Option<T>>;\n"),
            "{ts}"
        );
    }

    #[test]
    fn interface_and_injection_token_frame_the_ops() {
        let ts = emit_store_context(STORE_FIXTURE);
        assert!(ts.contains("export interface Store {\n"), "{ts}");
        assert!(
            ts.contains("export const StoreToken: unique symbol = Symbol(\"Store\");\n"),
            "{ts}"
        );
    }
}
