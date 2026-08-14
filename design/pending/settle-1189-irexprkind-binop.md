---
level: patch
changelog: "bynk-emit::ir gains IrExprKind::BinOp/Neg/InterpStr — comparison, arithmetic, unary negation, and string interpolation now lower to real IR nodes instead of panicking (internal only, no language surface change)"
---

## ADR: the-ir-binop-neg-interpstr
title: bynk-emit::ir's IrExprKind grows BinOp/Neg/InterpStr beyond P6.1's own Decision D
summary: Comparison/arithmetic, unary negation, and string interpolation get real IrExprKind nodes, extending IrExprKind past the reference's own Part 6.2 node set

**Context.** P6.1 (#1141, `bynk-emit/src/ir.rs`) decided `IrExprKind`'s whole shape lands in one
piece — every variant the reference's `design/bynk-greenfield-compiler.md` §6.2 node-set listing names
exists as of that slice, and a later P6.x slice only widens `lower`'s match, never `IrExprKind` itself.
`lower_expr_ir` (`bynk-emit/src/ir/lower.rs`) has since carried three `todo!()` arms for constructs
§6.2 never names at all: comparison/arithmetic `BinOp` (`Eq`/`NotEq`/`Lt`/`LtEq`/`Gt`/`GtEq`/`Add`/
`Sub`/`Mul`/`Div`), `UnaryOp::Neg`, and `InterpStr` — P6.2 (#1143) and P6.3 (#1145) each independently
confirmed neither a dedicated `IrExprKind` nor a `Callee` classification exists for any of them, and
left the gap rather than force-fitting it under Decision D's closed-shape framing. #1187's own scoping
pass (slice 4, the `Agent` emitter cutover) found this sitting on the critical path: `lower_invariant_ir`/
`lower_transition_ir` (`ir/lower.rs:981,1005`) already call `lower_expr_ir` on predicate expressions
that are almost always comparisons (`balance >= 0`, a real, already-shipped fixture invariant —
`bynkc/tests/fixtures/positive/248_history_property`), so any real `Agent`/`Service` slice attempt
would panic on an ordinary program before landing.

Re-checking §6.2's own listing directly (`Const, Local, Global, Record, Variant, Field, List, Block,
If, Match, And, Or, Not, Return, Call, Lambda, Await, Send, Pure`) confirms the omission is real, not
misread — none of comparison, arithmetic, negation, or interpolation appears, unlike `Question`/`Is`,
which the reference *does* name and this track has separately, deliberately deferred. The checker
already resolves every one of these expressions' types in full (`check_binop`/`check_unary`,
`bynk-check/src/checker/expressions.rs:570-919`; `InterpStr`'s hole rule, `checker.rs:3400-3421`) —
nothing here was blocked on new checker work, only on `IrExprKind` gaining a place to land the already-
checked result.

**Decision.** `IrExprKind` grows three variants beyond §6.2's listing: `BinOp { op: IrBinOp, lhs, rhs }`
(one shared, `op`-tagged variant covering all ten comparison/arithmetic operators, not ten near-
duplicate variants — none of them are short-circuit the way `And`/`Or` are, so unlike those two, a
shared shape loses no structural information `check_binop`'s own operand-typed dispatch, e.g. `Div`'s
truncating-vs-real-division split and `Eq`/`NotEq`'s `Bytes` content-equality routing, both still
recoverable from `IrExpr.ty` on `lhs`/`rhs`), `Neg { operand }` (mirrors `Not` exactly), and
`InterpStr { parts: Vec<IrInterpPart> }` with `IrInterpPart::{Chunk(String), Hole(Box<IrExpr>)}`
(direct mirror of `bynk_syntax::ast::InterpPart`). Decision D's own "never widens" holds only as long
as the reference's own node set is complete for the codebase's real semantics — it does not extend to
covering up a genuine omission in the reference document itself. `Ok`/`Err`/`Some`/`None`/`Question`
stay `todo!()`, confirmed independent: each blocked by its own unrelated gap (no `Arc<TypeDecl>`
identity for built-in `Option`/`Result`; `?`'s own three-way operand-type desugar), not this one.

**Consequences.** #1187's slice 4 (`Agent`) and slice 5 (`Service`) are unblocked for real invariant/
transition/handler bodies containing ordinary comparisons or arithmetic — previously any such body
would panic. A future consumer of `IrExprKind` (an `Ir → TS` printer, eventually) must handle three
more variants than the reference's own sketch shows; `IrBinOp`'s ten-tag mapping from
`bynk_syntax::ast::BinOp` is the one hand-written, easy-to-transpose piece of this change (pinned by a
table-driven test over all ten tags, `ir/lower.rs`'s
`every_binop_tag_maps_to_its_own_ir_binop_variant`). If a later slice finds §6.2 omits something else,
this ADR is the precedent for extending `IrExprKind` again on the same "real omission, not a deferred
row" grounds — not license to add a variant for convenience.
