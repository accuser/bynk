---
level: patch
changelog: "P6.15: `bynk-emit::ir::lower`'s `ExprKind::Question` (`?`) now lowers to a real `IrExprKind::Match`, generalising the reference document's own `Match{Ok,Err}` sketch to bynk's actual two-scrutinee-shape desugar — closing the load-bearing gap that blocked `IrItem::Provider`/`IrItem::Service` from ever getting a real emitter call site (internal only, no shipped emitter consumer of this dormant construction path exists yet — byte-identical output, confirmed by a zero-diff bless run)"
---

## ADR: question-ir-lowering
title: `?`'s real IR desugar generalises the reference's own `Match{Ok,Err}` sketch to bynk's actual two-scrutinee-shape semantics
summary: Resolves P6.15 by lowering `ExprKind::Question` to a real `IrExprKind::Match`, reusing P6.4/P6.5's pattern/match machinery rather than a bespoke opaque node, and adds `IrExprKind::HttpResultNotFound` plus `LowerIrCtx::return_ty` threading as the two pieces of new infrastructure this needed

**Context.** `bynk-greenfield-compiler.md`'s own desugaring table (§6.4) sketches `Question(e)` as
`Match { scrutinee: e, arms: [Ok(v) => v, Err(e) => Return(Err(convert(e)))] }` — a single, uniform
two-arm shape. The shipped string emitter's real desugar (`emitter/lower.rs:1014-1061`, ADR 0177/ADR
0178) is not that: it branches on the operand's own checked type into two genuinely different
scrutinee shapes, not one. An `Option[T]?` matches `Some`/`None`, not `Ok`/`Err` — `None` early-returns
the synthesized `HttpResult.NotFound` sentinel, never an `Err` construction at all. A `Result[T,E]?`
does match the reference's own `Ok`/`Err` shape, with `convert` either the identity (the error type
already matches the enclosing function's own declared error type) or a real declared `embeds`
conversion. The reference's sketch is right for the second shape and silently wrong for the first —
confirmed by tracing the real emitter code, not assumed from the reference text.

**Decision.** Generalise rather than follow the reference literally: `lower_question_ir` branches once
on the operand's own resolved type (`Ty::Option` vs. `Ty::Result`) and constructs the matching
`IrExprKind::Match`, reusing the already-shipped `IrArm`/`IrPat::Variant`/`Exhaustive::Total`/
`MatchForm::Flat` machinery (P6.4/P6.5) rather than inventing a bespoke opaque `Question` node — R6.7's
own "desugaring happens exactly once, in phase 6" mandate reads as a decomposition requirement, and a
`Match` genuinely decomposes further (unlike, say, `IrExprKind::HttpResultNotFound` below, which has no
further structure to decompose). Both arms' own scrutinee/payload types resolve via
`bynk_check::checker::variants_of` — the same function `IrPat::Variant`'s own real-pattern lowering
already calls, so a synthesized `Ok(v)`/`Some(v)` pattern here is constructed through the identical
path a real parsed one would be, not a hand-rolled shortcut.

Two new pieces of infrastructure, both narrowly scoped to what this decision needs:

1. **`IrExprKind::HttpResultNotFound`** — a zero-payload sentinel for `Option[T]?`'s own early-return
   value. Not routed through `GlobalRef` (the existing "resolve a source identifier naming a nullary
   sum variant" mechanism): `GlobalRef`'s own doc comment already names `HttpResult`/`QueueResult` as
   out of its scope ("dropped during implementation") because `HttpResult` is a checker built-in with
   no `TypeDecl` to resolve against — and this value is never sourced from user syntax at all (no bynk
   text spells `HttpResult.NotFound`; it is compiler-synthesized boilerplate the string emitter already
   hand-writes today). A dedicated marker is the narrower, more honest shape than stretching `GlobalRef`
   to cover a provenance it was never meant to.
2. **`LowerIrCtx::return_ty: Option<TyId>`**, set once by each of the four real body-lowering entry
   points (`lower_fn_body_ir`, `lower_handler_body_ir`, `lower_service_handler_body_ir`,
   `lower_provider_op_ir`) right after resolving their own `return_type`. Mirrors
   `emitter/lower.rs`'s own `LowerCtx::return_ty` field-for-field, for the identical reason: deciding
   whether a `Result[T,E]?`'s error type needs a declared `embeds` conversion requires knowing the
   *enclosing* function's own declared error type, which `Question`'s own operand alone never carries.
   `embed_conversion_ir` is the IR-native sibling of the string emitter's own `embed_conversion` —
   same two checker primitives (`checker::compatible`/`checker::embedding_for`), ported rather than
   duplicated.

Considered and rejected: keeping `Question` as a single opaque `IrExprKind` node (the shape a first,
narrower reading of "not this row's own commission" might suggest) — would leave the real desugaring
work (which of two scrutinee shapes, which conversion) undone until some future printer, in direct
tension with R6.7's own normative-desugar mandate; a `Match`-based decomposition is available now and
costs nothing to build today.

**Consequences.** No shipped emitter path calls into `lower_expr_ir`'s `Question` arm yet — P6.2's own
emitter-side cutover (`lower_method_call`/`lower_call` reading `Callee`-driven dispatch) has not landed,
the same dormant-path posture #1225's own `Ok`/`Err`/`Some`/`None` construction landed under. Verified
by three unit tests (Option-lift, bare Result propagation, and a declared `embeds` conversion), not a
bless run — there is nothing yet for a bless run to exercise. Confirmed via a zero-diff bless run anyway
that no *other* emitted output changed. `Is`'s own separate `todo!()` is unaffected and stays open — its
own R5.9/R5.10 deferral (#1157's own Decision D) is a distinct design question this ADR does not settle.
