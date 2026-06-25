# 0109 — The handler is the atomic unit for storage: a staged write-set flushed once, an explicit `commit` keyword retired, and a per-effect-class ruling on abort

- **Status:** Accepted (storage track, slice 1; 2026-06-25)
- **Track:** `design/tracks/storage.md` (slice 0 — the gate for the `Cell` slice)
- **Realises:** `design/bynk-design-notes.md` §10 ("Storage Types" — writes commit
  atomically at handler end), §12 ("Consistency Model" — within a handler:
  serialisable, read-your-writes, atomic commit; the DO input/output gate is the
  named source), and §13 ("Failure Model" — a faulting handler commits nothing).
- **Amends:** ADR 0107 D5/D6 — the **eager, per-statement** `commit` model and its
  "revert means non-persistence of the offending commit, not whole-handler
  rollback" reading. Restated here for the `store` model: state becomes
  handler-atomic, so for **state** 0107 D6 is reversed.
- **Relates:** ADR 0106 (`~>` send) — unchanged; ADR 0108 (`store` replaces
  `state`) — settles the write *surface* where this ADR settles the *semantics*;
  the two are inverse dependencies and land together at the `Cell` slice.

## Context

§10/§12 have always specified the handler as a transaction: all of an agent's
storage writes commit together at handler end, or none do, with read-your-writes
inside the handler. The compiler diverged (ADR 0107 context): `commit expr` is an
**explicit eager statement** lowering to `await this.commitState(expr)`, so
`commit good; …; commit bad` persists `good` and then faults — ADR 0107 D6 named
this honestly as "non-persistence of the offending commit, not whole-handler
rollback."

ADR 0108 retires the `state { } / self.state / commit`-spread surface for per-field
`store` writes (`:=`, `.update`, kind ops). Those writes are scattered through the
handler body — there is no longer a single `commit` expression to gate — and the
`:=`/`.update` idempotency story (§10) and the agent-invariant gate (ADR 0107)
both assume the handler is atomic. ADR 0107 D6 explicitly deferred "restoring true
handler atomicity" as a larger semantic change. This ADR makes it, because the
`store` model is not honest without it. It also rules on the harder, adjacent
question §10/§12/§13 leave implicit once writes stage: **what happens to the other
effect classes on abort.**

## Decisions

**D1 — The handler is the atomic unit for storage.** Every `store` write performed
during a handler is staged and **flushed once, at handler end, in a single gated
commit**. A fault at any point before the flush persists **nothing** — true
whole-handler atomicity for state. This supersedes the eager per-statement
`commitState` model: there is no longer a per-statement persistence point, so the
ADR 0107 D6 "earlier commit stands" behaviour does not arise for state.

**D2 — The mechanism is a generated per-handler staged write-set.** The handler
wrapper accumulates writes into an in-memory overlay keyed by `store` field — by
entry for `Map`/`Set`/`Cache`, and as an ordered **pending-append list** for
`Log`/`Queue`. Reads consult **overlay-then-storage**, which is what delivers
read-your-writes (§12): a `Cell` deref or `map.get(k)` after a write in the same
handler sees the written value, and a same-handler `Log` time-window read sees its
pending appends. At handler end the overlay is flushed in one commit. The DO
output gate (the §12 source of atomicity) holds the handler's **own reply to its
caller** — and, once Events lands, its at-commit emissions — until the storage
write confirms; it does **not** hold the `~>` sends or the synchronous cross-agent
`<-` calls of D4, which resolve mid-handler (a held `<-` would deadlock). Choosing
an explicit overlay over leaning on DO's native write-coalescing gives a
**materialised "proposed state"** for D3 and keeps the semantics platform-portable;
native DO atomic-storage remains an available emission simplification later.

**D3 — Invariants are checked against the proposed state (persisted ∪ overlay) at
handler end, before the flush.** This is the direct analogue of today's
`commitState` gate (ADR 0107 D5), restated for the staged model: the runtime
materialises the proposed state, evaluates each invariant purely against it
(consistent with ADR 0108 D5's "pure read of the staged value"), and on violation
throws `InvariantViolation` **before** the flush — so a violating state is never
written. The predicate surface is **ADR 0108 D5 (canonical)**: bare-name resolution
and a **bounded single-element read** — a `Cell` deref, `map.get(k)`, or
`set.contains(x)` — with `Cache` reads and whole-collection scans out. Because that
surface is bounded, the overlay need only materialise the **referenced** fields,
not the whole write-set, so the proposed-state materialisation D3 requires is
cheap.

**D4 — The effect-release ruling is per class, and only partially reverses ADR
0107 D6.** Staging state forces a decision on every other effect a handler can
produce:

- **State** — atomic (D1); a fault commits nothing. *0107 D6 reversed.*
- **Event emission** — staged and released at the flush, so an aborted handler
  emits nothing (design notes §7). This is the **contract**, not a build: Events
  are unimplemented and ADR 0106 deferred the at-commit send tier to the Events
  track, so this ADR binds that future track rather than shipping emission.
  *0107 D6 reversed (when Events lands).*
- **`~>` fire-and-forget sends** — unchanged from ADR 0106: lowered to immediate
  `waitUntil`, fired during the handler, **not** retracted by a later fault.
  *0107 D6 survives.*
- **Cross-agent `<-` calls** — each commits remotely as it returns (§13); a later
  fault does not roll it back. **They stand**, with compensation via sagas (§13).
  *0107 D6 survives.*

So ADR 0107 D6 is reversed for state (and, by contract, events) and survives for
`~>` and cross-agent effects.

**D5 — The explicit `commit` keyword is retired; commit is implicit at handler
end.** With one flush per handler there is no expression to gate, and a surviving
`commit` keyword would be a second write-commit model — the §2 duplication ADR 0108
rejects. `commit` is removed from the grammar at the parity slice; a leftover
`commit` is a migration artefact to chase down (ADR 0108 D2), not a no-op.

**D6 — Atomicity is intra-agent and intra-invocation.** The handler transaction
covers one agent's storage, for one invocation. Cross-agent atomicity is explicitly
**not** provided: distributed atomic commit needs coordination the target platforms
do not expose and that would compromise availability (§13); multi-agent consistency
is the saga's job. Nor is "atomic" the same as "exactly-once": on at-least-once
retry (queue / cron / event triggers) the whole handler re-runs and re-stages —
idempotent for `:=` / `Map.put`, but `Log.append` double-appends (§10's named
exception). The cross-invocation story is the `Idempotency` capability (§12), not
the commit; a reader should not over-read "atomic" as "exactly-once."

## Consequences

- **Amends ADR 0107.** D5 (the `commitState` gate) is restated onto the staged
  write-set; 0107 D6 is reversed for state/events and preserved for
  `~>`/cross-agent. The 0107 amendment rides the parity slice alongside the 0108
  invariant restatement.
- **Read-your-writes is explicit in generated code** (D2 overlay), not an
  incidental property of eager writes.
- **Testing.** Handler atomicity is testable by faulting mid-handler and asserting
  no persisted change; white-box `agent.field` reads (§14) observe committed
  state, i.e. the flushed overlay.
- **Sequencing.** Lands with or before the `Cell` slice (slice 0); ADR 0108's
  `store` surface is not honest without it.
- **Events contract recorded.** When the Events track lands, emission must join the
  atomic set (D4); this ADR is the referent for that requirement.

## Alternatives considered

- **Keep eager per-statement commit (status quo, ADR 0107 D6).** Rejected: it
  breaks the `:=`/`.update` idempotency story and the agent-invariant gate, both of
  which assume the handler is the atomic unit; the `store` model rests on it.
- **Lean solely on DO native atomic storage / the output gate, no explicit
  overlay.** Considered: the platform commits a handler's writes together and rolls
  back on throw. Rejected as the *primary* because D3 needs a materialised proposed
  state at handler end and read-your-writes must be explicit in emission; the DO
  gate still backs outbound-effect holding, and native atomic-storage stays a later
  simplification.
- **Distributed cross-agent atomicity (2PC).** Rejected (D6): unavailable on the
  target platform and availability-hostile (§13); sagas are the answer.
- **Retain explicit `commit` alongside staging.** Rejected (D5): two write-commit
  models, the §2 duplication ADR 0108 exists to remove.
