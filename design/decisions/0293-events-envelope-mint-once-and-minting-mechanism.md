# 0293 — The envelope is minted once at emission, using bare `crypto.randomUUID()`/`Date.now()`, not `given Clock`

- **Status:** Accepted (v0.240)

**Context.** An emission fans out to zero or more subscribers
(deliver-and-filter, ADR 0286, unchanged by this slice). If `eventId`/
`emittedAt` were minted per-delivery (inside the fan-out DO's per-subscriber
loop, or at each subscriber's own receiving end) rather than once per
emission, two subscribers of the same emission would observe *different*
envelope values — silently breaking the one property the envelope exists to
provide (a stable identifier for the `Idempotency.dedup(env.eventId)`
idiom, and a shared correlation id across sibling subscribers generally).

**Decision.** The envelope is minted exactly once, at the single
`__events.push` call site (`bynk-emit/src/emitter/lower.rs`, the only place
an `Events.emit[E](event)` call lowers to), before fan-out duplicates that
one buffered entry to every subscriber. `eventId: crypto.randomUUID()` and
`emittedAt: Date.now()` are bare calls, not routed through the `Clock`
capability: requiring `given Clock` alongside `given Events` on every
emitting handler would be new ambient coupling, and would break every
existing slice-0/1 fixture that emits with `given Events` alone. This
mirrors two existing precedents rather than inventing a new one — WebSocket
already mints a connection id the same way (`connId = crypto.randomUUID()`,
`bynk-emit/runtime/src/connection.ts`), and the runtime's own JWT `exp`/
`nbf` checks already bypass the `Clock` capability for internal timestamps
the same way `emittedAt` does here.

**Invariant this decision establishes, for a future substrate change to
preserve.** One `__events.push` call produces exactly one envelope, shared
unchanged by every subscriber that emission fans out to. Verified
empirically, not just by reading the lowering code:
`bynkc/tests/events_envelope_behaviour.rs` runs two sibling subscribers of
one emission and asserts they observe the *identical* `eventId` (catching a
mint-per-delivery regression), and asserts two distinct emissions mint two
*distinct* ids (catching a constant-or-once-only-minted regression — an
equality-only test would pass trivially under that bug).

**`schemaVersion` is present now, computed later.** Per
`design/tracks/events.md` §3.6's explicit obligation ("the envelope must
carry `eventId` and `schemaVersion` from day one" so the future replay
track doesn't need a wire-format change) and §8's done-when bullet listing
all four fields together, `schemaVersion: Int` is on the envelope now,
hardcoded to `1`. It is not yet computed from the event type's structural
shape — that is slice 3's cross-build schema registry (§3.5), a
deliberately separate, larger increment.
