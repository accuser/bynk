# 0125 — `Cell[T].update(f)`: the method-shaped read-modify-write — the one callable `Cell` operation (`update(f: (T) -> T) : Effect[()]`), dispatched by receiver provenance and lowered to a staged read-modify-write committed by the existing end-of-handler flush; `read`/`write` stay sugar (bare name / `:=`), not methods

- **Status:** Accepted (post-storage-track language increment; 2026-06-26).
- **Provenance:** a single-increment language proposal closing a half-built gap — the type system (`bynk-type-system.md` §2.7.3) listed `Cell[T].update(f) : Effect[Unit]` and the self-referencing-`:=` diagnostic steered users to it, but no code resolved it. This ADR records the wiring and the decisions it settled; it also moves the `Cell` operations in §2.7.3 from *Open* to **normative**.
- **Realises:** the design notes' "apprentice-level" cell vocabulary (§10) — `update` named alongside `:=` as core read-modify-write — and the §2.7.3 signature, previously written down but unimplemented.
- **Relates:** [ADR 0109](0109-handler-atomic-commit.md) (the end-of-handler atomic commit + invariant gate this reuses unchanged); [ADR 0108](0108-state-record-to-store-fields.md) (the `store`-fields-are-the-state-record model, with bare-name read and `:=` write); [ADR 0110](0110-storage-map-vs-value-map.md) (the receiver-provenance dispatch and the `Map.update` lowering this mirrors); [ADR 0107](0107-agent-invariant-predicate-surface.md) (the commit-time invariant gate the flush runs before the durable write).

## Context

A `store n: Cell[T]` field is read by its **bare name** (implicit-deref sugar) and
written with **`:=`** (ADR 0108). A `:=` whose right-hand side reads its own
target — `n := n + 1` — is rejected with `bynk.cell.self_reference`, whose note
tells the author to *"use `<cell>.update(fn)` for a read-modify-write so the
dependency on the prior value is explicit."*

That advice pointed at a method that did not exist:

- The `MethodCall` dispatcher in the checker routed `store`-field receivers by
  provenance — branches for `Map`/`Set`/`Cache`/`Log`, each a `check_store_*_op`
  helper — but had **no `Cell` branch**, so `n.update(fn)` fell through to the
  generic method-call path and reported a misleading "no such method".
- The emitter's `lower_method_call` had the matching `Map`/`Set`/`Cache`/`Log`
  branches but **no cell branch**, so even a well-typed `update` would not emit.

So the diagnostic steered to `update`, §2.7.3 defined `update`, and nothing
implemented it — the only way through a read-modify-write was the
`let cur = n; n := cur + 1` workaround the rule was meant to retire.

## Decisions

- **D1 — `update` is the one method-shaped `Cell` operation.** Surface vocabulary
  gains exactly `n.update(f)` with `f: (T) -> T`. §2.7.3 also lists `read()` and
  `write(v)`, but those stay **sugar** — the bare name reads, `:=` writes — and are
  **not** exposed as callable methods. Two ways to do the identical thing invites
  style drift and undercuts the "cells are not first-class" framing; `read`/`write`
  remain only as the desugaring targets §2.7.3 names.

- **D2 — `update` returns `Effect[()]`, not `Effect[T]`.** It mutates the cell; it
  does **not** thread the new value out. This keeps parity with `Map.update` /
  `Cache.update` and preserves the rule that *reading* a cell is always the
  bare-name sugar — a value-returning `update` would create a second,
  method-shaped read path. Read-modify-write-**and-return** is therefore the
  explicit two steps: await the `update`, then read the bare name back
  (read-your-writes) — e.g. `increment()` returning the incremented `n`.

- **D3 — dispatch by receiver provenance; the checker helper is a sibling of the
  others.** A `store_cells` branch in the `MethodCall` dispatcher resolves a bare
  ident naming a store cell to `check_store_cell_op`, structurally a copy of
  `check_store_set_op`: the same arity (`bynk.types.call_arity`) and
  argument-mismatch machinery, with an unknown op raising `bynk.store.unknown_op`
  ("a `Cell` store field has no operation `<x>` — expected `update`").

- **D4 — lower to a staged read-modify-write; reuse the existing commit.** The
  emitter lowers `n.update(f)` to `(() => { __state.n = (f)(__state.n); return
  undefined; })()` over the in-memory working state — `Map.update`'s lowering
  **minus the key-absent guard** (a cell is always present, so there is no fault
  path). The mutation is synchronous (read-your-writes within the handler); the
  single end-of-handler `commitState` flush runs the **invariant gate before the
  durable write**, exactly as `:=` does (ADR 0109). A handler whose only mutation
  is an `update` now triggers that flush — the write-detection that wraps a body
  in the committing closure recognises `cell.update` alongside `:=`. **No new
  runtime helper.**

- **D5 — the combiner is pure for free.** `f: (T) -> T` is a non-effectful
  `Ty::Fn`, so an effectful body fails the existing function-type check —
  identically to `Map.update`'s combiner. A bare read of *another* cell is itself
  effectful sugar, so a combiner that touches another cell is effectful and is
  rejected at the same gate. No new rule.

- **D6 — the `:=` self-reference diagnostic stays; its suggested fix now exists.**
  `n := n + 1` continues to raise `bynk.cell.self_reference` pointing at `update`;
  the only change is that following the advice now compiles. The rule (make the
  prior-value dependency visible and retry-safe) is unchanged.

- **D7 — settle §2.7.3 for `Cell`.** The `Cell` operations move from the section's
  *"Operations and their types — Open"* framing to **normative**; the remaining
  operations in that block stay illustrative.

## Consequences

- A read-modify-write is now first-class and the steering diagnostic is honest:
  `n := n + 1` → `let _ <- n.update((c) => c + 1)`.
- No capability gate: unlike `Cache`, a cell update reads no clock — it is a pure
  staged mutation.
- Atomicity is inherited, not re-derived: an `update` mutates the same staged
  `__state` that `:=` writes, so a fault between an `update` and commit persists
  nothing.
- Surfacing `read`/`write` as methods is explicitly **not** a follow-on (D1) —
  they remain sugar.
