---
level: patch
changelog: "bynk-emit::ir's IrExprKind::Variant no longer carries a sum: Arc<TypeDecl> identity field; Ok/Err/Some/None now lower to it directly (closing the todo!() blocking their construction, #1225) since the constructed sum's own identity is already the wrapping IrExpr's own TyId, the same way IrPat::Variant's own scrutinee_ty already covers both a user-declared sum and a built-in one (internal only, no shipped emitter consumer of this dormant IR construction path exists yet — byte-identical output, confirmed by a zero-diff bless run)"
---

## ADR: option-result-variant-identity
title: `IrExprKind::Variant` carries no declaration identity — a constructed sum's own `TyId` is the wrapping node's own `ty`
summary: Resolves #1225 by dropping `Variant::sum: Arc<TypeDecl>` rather than widening it, so `Ok`/`Err`/`Some`/`None` (which have no `TypeDecl`) construct the same node a user-declared sum does

**Context.** `IrExprKind::Variant` (`bynk-emit/src/ir.rs`), P6.2's own sum-variant-construction node, has
carried `sum: Arc<TypeDecl>` since it was first sketched — a real user-declared sum's own constructor
call (`Circle(n)`, `Shape.Circle(n)`) is driven by `Callee::Ctor { sum: Arc<TypeDecl>, tag: String }`
(`bynk-check/src/checker.rs`), and `lower_call_ir`'s own `Callee::Ctor` arm cloned `sum` straight through.
This mirrors the module's own general convention for a *declaration* identity field — `Record::def`,
`GlobalRef::sum` (`ir.rs`) do the same for their own construction shapes.

`Ok`/`Err`/`Some`/`None` construct `Option`/`Result` — closed, built-in sums, never a `TypeBody::Sum`
declaration. No code path anywhere in `bynk-check` ever produces an `Arc<TypeDecl>` for either: they are
dedicated `Ty` variants (`Ty::Option(TyId)`, `Ty::Result(TyId, TyId)`), never `Ty::Named` and never backed
by an entry in `TypedCommons::types`. `Callee::Ctor`'s own two minting sites
(`bynk-check/src/checker/calls.rs`) can only ever resolve a real declared sum by name — and, separately,
`Ok`/`Err`/`Some`/`None` are not calls at all: they are dedicated `ExprKind::Ok`/`Err`/`Some`/`None` AST
variants, checked through `check_ok`/`check_err`/`check_some`/`check_none`
(`bynk-check/src/checker/expressions.rs`), which never record a `Callee` for them at all. Both facts
together are why `ir::lower`'s own `todo!()` for these four constructors (`bynk-emit/src/ir/lower.rs`,
open since P6.2/P6.3, #1143/#1145) could not simply reuse the existing `Variant` construction path — there
was no `sum` value it could ever legitimately hold.

This is not a new problem for this module. `IrPat::Variant` (the *pattern*-matching side, same file)
already had to solve it, for the identical reason (a `match` scrutinee can be `Result`/`Option` just as
easily as a user sum), and already chose `scrutinee_ty: TyId` over `Arc<TypeDecl>` — resolved downstream
via `bynk_check::checker::variants_of` (`pub` specifically for this, per its own doc comment), which
matches on the interned `Ty` behind the `TyId` and uniformly produces tag/payload information for
`Ty::Named { kind: Sum, .. }` (looking up the real `TypeDecl`), `Ty::Result`, `Ty::Option`, `Ty::ActorSum`,
and `Ty::HttpResult` alike — no special-casing gap for any built-in case, and no `TypeDecl` needed at all
for the built-in arms.

**Decision.** `IrExprKind::Variant` drops `sum` entirely rather than widening it to cover the built-in
case — `Variant { tag: String, payload: Vec<IrExpr> }`. The wrapping `IrExpr::ty` (already present on
every node, R6.1) already carries the exact identity `sum` used to duplicate: a constructor call's own
checked type *is* the sum it constructs (confirmed directly — `lower_call_ir` already computes
`ty = cx.expr_ty(e.id)` as its first line, before ever branching into the `Callee::Ctor` arm), so no
second, redundant field is needed inside `Variant` itself. `Ok`/`Err`/`Some`/`None` lower to this same
node — `tag: "Ok"/"Err"/"Some"/"None"`, `payload` the lowered inner expression (empty for `None`) — with
no `Callee` classification needed either, matching the checker's own posture toward them. A consumer
needing the constructed sum's own tag/payload shape (a future printer) calls `variants_of` against the
enclosing `IrExpr::ty`, the same function `IrPat::Variant`'s own lowering already calls for the identical
purpose on the pattern side — one shared resolution path for both directions, not two.

Considered and rejected: (1) inventing a synthetic `TypeDecl` for `Option`/`Result` so `sum` could keep
its `Arc<TypeDecl>` shape uniformly — no established path for a "declaration" that isn't one, and a new
kind of value this module would need to invent solely to satisfy a field that turns out not to need
declaration identity at all; (2) widening `sum` to an enum over `Arc<TypeDecl>` and a built-in tag — adds
a permanent branch to the common (user-sum) case for the sake of the rarer built-in one, and still
duplicates information `ty` already carries losslessly.

**Consequences.** `IrExprKind::Variant`'s only real consumer today (a unit test) now reads the constructed
sum's identity off `tail.ty` via the checker's own `Ty`/`Types` intern table instead of `sum.name.name` —
updated as part of this change, not left broken. `ir::lower` has no shipped emitter consumer yet (the
whole `IrItem`/`IrExpr` construction path remains dormant, exercised only by its own test suite), so this
lands with zero behavioural change to any emitted TypeScript — confirmed by a zero-diff `bless` run across
the full positive fixture corpus. `Question`'s own separate `todo!()` (`ir/lower.rs`) is unaffected and
stays open — its own three-way desugar fork (`Option[T]?` vs. `Result[T,E]?` vs. an `embeds` conversion)
is a distinct design question this ADR does not settle, named again in #1225's own tracking issue.
