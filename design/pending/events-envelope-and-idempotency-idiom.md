---
level: minor
changelog: An `on event(e: E, env: EventEnvelope)` handler's optional second parameter carries runtime metadata about the emission — `eventId`, `publisherId`, `emittedAt`, and a reserved `schemaVersion` — enabling the `Idempotency.dedup`/`remember` idiom keyed on `env.eventId` for effectful subscribers
---

## ADR: events-envelope-publisher-id-amendment
title: The envelope's `publisherId` is the emitting context, not the emitting agent, amending design/bynk-design-notes.md §7
summary: Events track slice 2 (spine #936) — `Events.emit` is legal from a plain, keyless service handler with no agent identity to report, so §7's per-agent framing cannot hold uniformly at every legal emission site

**Context.** `design/bynk-design-notes.md` §7 (lines 229–234) asserts "the
publisher is the emitting agent, not the bounded context — agents are the
unit of state and effect, so they are the unit of ordering," and shows the
envelope example as `env.publisherId` alongside that claim. This does not
hold uniformly against the shipped surface: `Events.emit[E](event)` is also
legal from a plain, keyless `service` handler with no agent instance at all
— confirmed by the existing owner-only negative fixture
`bynkc/tests/fixtures/negative/503_events_emit_outside_owner` (a bare
`service leak { on call() -> Effect[()] given Events { Events.emit[...] } }`,
negative only because of the *owner* check, not the handler shape) and its
agent-handler analogue at `953_agent_handler_event_emit_outside_owner`. A
plain service handler has no `self`, no key field, and no DO instance — at
that call site there is no agent to be "the publisher."

Agent self-identity does exist as a real, narrow, already-shipped mechanism
(`self.<keyField>` lowers to `this.state.id.toString()` inside an agent
handler, `bynk-emit/src/emitter/lower.rs`'s agent-handler `self` rewrite),
but it is unavailable in a plain service handler, and a per-agent-instance
`publisherId` would additionally invite a subscriber to correlate an
ordering guarantee across an agent's own emissions that §3.4's own ADR
(`events-per-publisher-fifo-scope`) already measured as **not** holding
across concurrent invocations of one agent — advertising an identity that
implies a false guarantee is worse than not advertising one.

**Decision.** `publisherId: String` is the emitting *context's* fully
qualified name (e.g. `"commerce.order"`) — a compile-time-known string,
identical for every invocation regardless of whether the emission happened
inside an agent handler, a plain service handler, or a composed provider
body, since `HandlerShared.owning_context` (a new field threaded alongside
the existing `handler_scope`, `bynk-emit/src/emitter.rs`) is populated
identically at every one of those construction sites. This amends §7's
"the publisher is the emitting agent" framing to "the publisher is the
emitting context" — a smaller, honest guarantee that matches what's
actually knowable uniformly, rather than one that only works for the agent
case and silently omits the service case.

**Consequences.** A subscriber that needs per-agent-instance correlation
(e.g. detecting that two emissions came from the *same* `Ledger` instance,
not just the same context) has no support from `publisherId` alone — it
needs its own identifier carried in the event payload itself, the same
constraint §6's ordering-guarantee note already places on cross-publisher/
concurrent-invocation ordering. Not a regression: no shipped surface before
this slice exposed publisher identity at all.

## ADR: events-envelope-mint-once-and-minting-mechanism
title: The envelope is minted once at emission, using bare `crypto.randomUUID()`/`Date.now()`, not `given Clock`
summary: Events track slice 2 (spine #936) — one emission must produce one `eventId` shared by every subscriber it fans out to, or the `Idempotency` dedup idiom provides no real cross-subscriber correlation

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

## ADR: events-idempotency-idiom-is-documented-convention
title: The `env.eventId`-keyed dedup idiom ships as documented convention, not new capability syntax, and corrects a design-notes API mismatch
summary: Events track slice 2 (spine #936) — `design/bynk-design-notes.md` §12's worked example uses an `Idempotency.dedup(on:, expiresAfter:)?` call shape that was never built; the real, shipped API is the two-call `dedup`/`remember` pair

**Context.** §12 (lines 656–666) shows `<- Idempotency.dedup(on: e.eventId,
expiresAfter: 7d)?` — labelled arguments, a `?`-postfix short-circuit, and
`eventId` read directly off the event *payload* rather than an envelope.
None of this matches the shipped capability
(`bynk-check/src/firstparty/bynk.bynk`):

```
capability Idempotency {
    fn dedup[T](key: String) -> Effect[Option[T]]
    fn remember[T](key: String, value: T, expiresAfter: Duration) -> Effect[()]
}
```

Two positional operations, no `on:`/`expiresAfter:` labels on `dedup`, no
`?`-shortcut sugar, `key: String` only (settled narrower than a generic key
type in `idempotency-capability.md` §3.4). This is the same class of gap
`design/tracks/events.md` §3.3 found in §7's pattern-filtering example
before slice 1, and #939's own DECISION B found in §7's original
`Events.emit` call shape — a design-notes worked example describing an API
that was never actually built.

**Decision.** No new capability syntax is added for this idiom. The already
-shipped `match`-based `dedup`/`remember` pair (proven end to end for the
first time combined with Events by
`bynkc/tests/fixtures/positive/961_events_envelope_idempotency_dedup`) is
the idiom, keyed on `env.eventId` — a plain `String`, so it type-checks
directly against `dedup[T](key: String)` with no coercion. This also
settles `design/tracks/events.md` §4's own open question ("settle here
whether it is bespoke syntax or documented convention") in favour of
convention: `scope_idempotency_key` (`bynk-emit/src/emitter/lower.rs`)
already prefixes every `dedup`/`remember` call's key with the calling
handler's own qualified name, so two different subscriber services deduping
the same emission's `eventId` can never collide — inherited unchanged from
#934, nothing new needed to make the idiom safe.

**A narrower, pre-existing checker limitation found while proving the
"envelope with no capability" case (named here, not fixed — out of
scope).** `bynkc/tests/fixtures/positive/962_events_envelope_bare_consumes_bynk`
set out to prove a subscriber can access `EventEnvelope` via a bare
`consumes bynk` (no braces, no capability at all) — confirmed true for
*naming* the type in a parameter position (`env: EventEnvelope` resolves
and type-checks). Plain field access on it via a bare `let` binding
(`let _ = env.eventId`), however, fails with `bynk.types.method_not_found`
— reproduced identically on an ordinary cross-context record field
(`e.orderId`, no adapter involved), so this is a general, pre-existing gap
in bidirectional field-access resolution unrelated to Events or
`EventEnvelope` specifically, simply never previously exercised because
every shipped fixture only ever accesses a cross-unit record field from an
*expected-type* position (a capability call argument, e.g.
`Idempotency.dedup[T](env.eventId)`, which does resolve correctly) rather
than a bare synthesised `let`. Out of scope for this track to fix; named as
a candidate for its own future increment.
