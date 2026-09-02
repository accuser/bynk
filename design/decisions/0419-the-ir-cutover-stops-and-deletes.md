# 0419 — The IR cutover stops at Slice 3.1 and deletes the unconsumed expression lowering — a second code generator is not a retype

- **Status:** Accepted (v0.289.64)

**Context.** `design/tracks/the-ir.md` (phase 6, spine #1137) settled at Q7/#1175 how the emitter would
eventually consume the IR it built: `emitter/lower.rs` keeps writing strings, only its dispatch *reads*
move from `bynk_syntax::ast` to `bynk_lower`'s output. Phase 6 retired without slicing that cutover.
The 30 August 2026 post-restructuring review found the consequence — fifteen `bynk-lower` entry points
and twenty-one `bynk-ir` types with no consumer outside their own tests — and asked for a decision:
adopt or delete. #1536 decided adopt, and #1542 (`the-ir-cutover.md`) opened to execute Q7.

Its Slice 3.2 was accepted on two premises: that the cutover is a *mechanical, byte-identical retype*,
and that it lands as *one slice* because a half-converted state "would mean … a temporary duplicate
copy of the whole mutually recursive machinery … Neither buys real safety." The branch that tested it
(`archive/slice3-2-expr-stmt-core`, 19 commits) falsified both. Against `main`: `emitter/lower.rs`
6,321 → 10,117 lines; 56 `_v2` sibling functions duplicating AST-typed ones, none retyped, none
deleted; a per-body static gate choosing AST path or IR path at each of 3 of 7 flipped entry points; 6
production `todo!()`s; `ts_writes` 809 → 1,079; two goldens accepted as unrecoverable diffs; seven
follow-on issues for parity gaps. Every flip found a real behavioural difference, each a bug in the IR
path fixed there; the residual diffs needed `bynk-ir` widened (redundant parens, spread syntax) and the
gate needed R5.9's is-binding scopes (ADR 0338's own deferral) before it could come out. That is a
second expression code generator being brought to parity with the first, gated per body — the second
reachable path `bynk-compiler-trajectory.md` §6's question 3 and `bynk-greenfield-compiler.md`'s P5
exist to forbid, reproduced by the track opened to close them.

Pricing the remainder (`the-ir-cutover.md` §10.3, now in `design/archive/retired-tracks.md`): the
four handler-bodied entry points were flagged as higher risk than the three done; `Callee::Cross`, the
indexed-filter fast path, R5.9, spread and parens all remained; and only after all of that could the
26 AST-typed functions and the gate be deleted — for an end state that, per Q7, still emits strings.
Not less than the ~4,400 lines already paid, open-ended, to close R6.13 for `emitter/lower.rs`'s
*reads* only. The trajectory's own §8 names the response: "A phase's estimate is wrong by a large
factor … the phase boundary is the stopping point, and the trajectory's value is what has already
landed, not what remains."

One finding the review had missed changed what "delete" meant: the expression lowerer *was* reachable
in production, through `lower_event_subscriber_shapes_ir → lower_service_item_ir →
lower_service_handler_ir → lower_block_ir → lower_expr_ir` for every `from Events(E)` service —
every handler body lowered and discarded for two booleans, with `lower_ident_ir`'s terminal
`unreachable!()` live on that path under a safety argument written for a different caller. The
review's "zero callers" had counted direct callers only.

**Decision.** The cutover stops at Slice 3.1. Slice 3.2 is not merged; its branch is retained as a
tag for the evidence. The one production detour is repointed at the two shape-only helpers it needed
(D0, zero-diff by construction). Everything `rustc` then reports unreachable is deleted (D1: 48
`bynk-lower` functions, four `LowerIrCtx` fields and their methods, every `todo!()` the crate carried;
D2: 23 `bynk-ir` items, then D3's probe caught `EmbedIr`, `IndexIr` and the four mutating-op tables), with the tests that pinned a *kept* helper
only through a deleted constructor re-created directly against the helper (21 in D1), and every
surviving doc comment that narrated a deleted item rewritten rather than unlinked. The refusal is
recorded in `bynk-greenfield-compiler.md` Part 15.1 in R15.1's four fields — claim, cost avoided,
trigger, evidence — and gated by a new `unconsumed_ir_items` probe: for each `pub` item in
`bynk-ir`/`bynk-lower`, a non-comment, non-test reader in another crate must exist. It reads 0 and can
only fall.

This supersedes `the-ir.md`'s Q7/#1175 (there is no cutover to execute) and ADR 0338's R5.9 deferral
(there is no IR lowering context to thread bindings into). It does not reopen phase 6's own retirement
floor (`ast_importers` = 5), the trajectory's §1 endpoint, ADR 0381's six declined conversions, or ADR
0366's `TypeShape::Refined` embedding. Phase 8's twin decision (#1537) is the same question on different
evidence and is not decided here.

**Consequences.** `bynk-lower/src/lib.rs` 10,195 → 2,123 lines and `bynk-ir/src/lib.rs` 1,923 →
634, with zero emitted-output change across the whole e2e corpus at every step. `bynk-lower` is the
AST-analysis-helper crate its own description always claimed (17 public entry points, each with a
production caller); `bynk-ir` is the declaration-level vocabulary those helpers return (19 items, each with a reader
outside both crates — the probe's own review tightened it so the two IR crates cannot vouch for each
other, which caught and resolved five more). The "available but unwired" state cannot land silently again in either crate: a new
`pub` item with no reader fails `greenfield_status_table_is_current` by name. The hygiene track
(#1533) should re-cut its `bynk-lower` split target, which no longer exists at the size it planned
for. What was lost is real and named: the IR path's own test corpus (121 tests) and the design work
in the deleted types, both retrievable from history and from the tag if the trigger ever fires. The
trigger is specific — a checker-resolved expression-level fact the string lowerer cannot get without
re-deriving it, that tree-native emission would not serve better — so the refusal is a decision, not
a preference.
