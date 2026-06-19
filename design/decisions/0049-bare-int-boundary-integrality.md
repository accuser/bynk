# 0049 — Bare-`Int` boundary fields validate integrality

- **Status:** Accepted (v0.22b)
- **Spec:** §7.2

## Context
A bare `Int` at the boundary validated `typeof === "number"` only — the
`Number.isInteger` check lived solely in refined types' `.of`. So
`deserialise_Order({qty: 1.5})` succeeded, and the new typed JSON codec
(0045) would have handed programs an "`Int`" holding `1.5`. The v0.21
finite-`Float` rule (0040) is the template: codec and `.of` must agree.

## Decision
Bare `Int` gains `Number.isInteger` validation at **all three**
deserialisation sites: record/nested fields (`emitter/serialisation.rs`),
workers handler params (`workers_entry.rs` `deserialise_call`), and the
codec's inline base-type arm. The failure is a `StructuralMismatch`
expecting an `integer`, with the value rendered via `String(...)` (the
`typeof` of `1.5` would unhelpfully say `number`).

Consequences accepted deliberately:
- a **behaviour change to every emitted codec and handler** with a
  bare-`Int` field — 28 fixture snapshots re-blessed, diff reviewed
  (every hunk is an added `isInteger` guard; nothing else moved);
- a **wire-contract tightening**: a previously-accepted `{qty: 1.5}` now
  `Err`s / 400s. Pre-1.0, correct over compatible.

*Alternative — leave bare `Int` lax:* rejected; a latent correctness hole
made user-visible by `Json.decode` is a hole, not a compatibility
guarantee.

## Consequences
`Int` means integer everywhere a value enters Bynk, not just through
`.of`. The re-bless precedent: a deliberate wire-contract change rides
its own increment, isolated from additive surface.
