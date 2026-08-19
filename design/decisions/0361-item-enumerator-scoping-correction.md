# 0361 — P6.36's own `lower_unit_items_ir` enumerator is not the right shape for either of its two intended consumers

- **Status:** Accepted (v0.249.13)

summary: Phase E of the #1137 completion plan (`design/tracks/the-ir.md` §6a) — a scoping correction, per the P6.20 precedent, for the row the plan itself flagged as its own highest re-scope risk

**Context.** This row proposed one shared `lower_unit_items_ir` enumerator to serve both
`collect_external_references` and `write_header`, already naming the totality risk as its own open
question: `IrItem` has no `Actor`/`Messages`/`Event`/`Const` variant, and silently skipping an
unrepresentable item would be wrong specifically for `collect_external_references`, whose whole job is
deciding what a file's header needs to import.

**`collect_external_references`: the totality risk proved out, and there is no partial fix.** Traced
directly: the function's own `match item { CommonsItem::... }` has nine arms, covering every declared
kind by design — missing one is a real correctness bug (a silently missing runtime import in the
emitted file), not a style gap. Three of those nine arms are variants `IrItem` cannot represent at
all: `Actor`, `Messages`, and `Event` (which contributes via `EventDecl::as_type_decl()`, per this
row's own original text). A "partial" enumerator covering only `IrItem`'s six representable variants
would not simplify anything — the function would need to keep its own direct `CommonsItem::
{Actor,Messages,Event}` arms *alongside* the enumerator, two dispatches doing the same job side by
side, worse than the single `match` it has today. Not pursued; the function's own per-variant
`CommonsItem` match stays as-is.

**`write_header`: re-examined match by match, most of what's left is already at its real floor.**
`has_agent` and `hosts_ws_open`'s own outer `CommonsItem::Service`/`Agent` arms are bare kind
classification with no declaration content read — the same "outer match picks the body-rendering path,
not itself a re-derivable decision" shape Q7 (§3.7) already settled elsewhere in this track.
`has_agent_uses_emit`'s own inner check already reads `commons.callees` (via `block_uses_emit`), and
`hosts_ws_open`'s own inner check already reads `lower_handler_kind_ir` — both already `Callee`/IR-
driven; only the outer classification is raw AST, for the reason above, and converting that
classification would gain nothing. What *does* read genuine declaration content —
`has_agent_invariants`'s `!a.invariants.is_empty() || !a.transitions.is_empty()`,
`has_rehydration_gate`'s `agent_needs_rehydrate(a, ...)`, and the held-storage check's
`agent_has_held_storage(a)` — has two possible fixes, neither of them "one enumerator": three small,
narrowly-scoped `TypedCommons`-only helpers (the `lower_op_sig_ir_from_commons`/
`capability_op_sig_from_commons` shape P6.29 already established), or the full `CheckedProgram`-gated
`lower_agent_item_ir` (verified reachable here — both of `write_header`'s own call sites, `emit` and
`emit_project`, hold a real `&CheckedProgram`, so this is not P6.20's own compose-time blocker). The
latter would lower a whole agent's state, handlers, invariants, and transitions — including full body
lowering per handler — just to answer three boolean presence questions, the identical "expensive pass
run just to discard almost all of it" shape P6.23's own `EventSubscriberShape` precedent deliberately
guarded against with a cheap protocol pre-filter before ever reaching a real lowering pass. Building
three separate helpers is real, buildable, lower-risk work — but it is three small decisions, not the
one unifying enumerator this row proposed. Neither shape landed here; named as unscoped future work
(§7) instead.

**Decision.** Not implementable as described. No enumerator built; no source changes. The three
small-helper candidates for `write_header`'s own remaining checks are named in §7 as forward
references, entry condition: a slice proposal arguing each is worth its own helper — these carry no
shadowing hazard (unlike the `Callee`-class defects P6.21 closed), so they are cheap-but-low-priority,
not urgent.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — no code change. Matches the P6.20 precedent
exactly: a slice's own premise investigated directly against the tree, found not to hold in the shape
proposed, recorded so a future reader does not re-attempt the same design.
