---
level: minor
probes: diagnostic_coverage, emit_diagnostics, ts_writes
changelog: A test case delivers an event to a `from Events(E)` service with `svc.event(e)` or `svc.event(e, env)`, honouring its pattern filter and `via schema(N)` guard
---

## ADR: test-event-address
title: A test case delivers an event to a `from Events` service by address
summary: `svc.event(e[, env])` at `unit` and `integration`, through the generated `event` method; refused at `system`

**Context.** A case addresses a service by its natural surface: `svc.call(…)`,
an HTTP verb, `svc.schedule("…")` or `svc.message(m)` (ADR 0205). A `from
Events(E)` service had none. `OnPlaced.event(e, env)` was refused with
`bynk.test.service_bad_address`, so a subscriber's own logic could be tested
only end to end at `system`, by driving the publisher. That logic includes the
events guide's idempotency idiom, a dedup on `env.eventId`. A `system` suite is
not always available either (#2027). Issue #2052.

**Decision.**

- **Shape.** `svc.event(payload)` and `svc.event(payload, envelope)` in a case
  body, on a service whose protocol is `from Events(E)`. The payload is typed
  against the handler's first parameter, which
  `bynk.event.handler_param_type_mismatch` already holds to `E`. The envelope
  is typed against `bynk`'s `EventEnvelope`, whether or not the handler
  declares `env`, because a `via schema(N)` guard reads it either way. The call
  yields the handler's return type (`Effect[()]`), as every address does
  (#1934).
- **Refusals.** An argument count other than one or two is
  `bynk.test.service_call_arity`. A payload that is another declared event is
  `bynk.test.event_not_subscribed` ("`OnPlaced` subscribes to `Placed`, not
  `Cancelled`"). Only the events in the target's scope count, its own and its
  consumed contexts'. Any other wrong payload or envelope is
  `bynk.types.argument_mismatch`. `svc.event` on a service of any other
  protocol stays `bynk.test.service_bad_address`, now with a note naming the
  `from Events` form. A suite whose target doesn't consume `bynk` cannot name
  `EventEnvelope`, so any envelope argument there is a mismatch with a note
  saying so.
- **Delivery reuses the runtime's path.** The pattern filter and the `via
  schema(N)` guard are prologues of the service's generated `event` method
  (`emit_service`). The Worker's `/_bynk/event/` route, the Bundle fan-out and
  the Durable Object fan-out all reach the subscriber through that method. The
  case calls it too, through a per-service helper `__bynkEvent_<svc>` in the
  test module. Like the Worker's `emit_event_wrapper`, the helper forwards the
  envelope only when the method takes one, that is when the handler declares
  `env` or the service has a `via schema(N)` guard. A case's envelope for a
  handler that can observe neither is dropped. Nothing about delivery is
  re-implemented, so a delivered event that the filter or guard rejects is a
  no-op, exactly as in production.
- **Envelope defaults.** An omitted envelope is filled in as an emission would
  mint it, but deterministically:
  - `eventId` is fresh on every call (`test-event-<n>`, counted across the test
    module), so two default deliveries are two emissions, not a redelivery;
  - `publisherId` is the qualified name of the subscriber's publisher. It is
    resolved as the service's header resolves `E`: the target's own
    declaration, else the one context the target consumes that declares `E`.
    A same-named event in a context the target doesn't consume is never the
    publisher. With no unique publisher, a default envelope is refused with
    `bynk.test.event_publisher_ambiguous`, and the case passes its own;
  - `emittedAt` is the test clock's reading, `Instant.fromEpochMillis(0)`.
    This matches the `Clock` double, but the clock is not called, so a stubbed
    `Clock.now` sequence and an observation of `Clock.now` are unaffected;
  - `schemaVersion` is the event's current version: the schema registry's
    reconciled version when the build has one, else its `@schema(N)`, else
    `1`. This is the same value the emitting handler embeds.

  A case that depends on any field passes its own envelope.
- **Tiers.** The address works at `unit` and `integration`, which run the same
  in-process module. At `system` it is refused with
  `bynk.test.event_needs_in_process`. There, an event reaches a subscriber only
  through its publisher's fan-out, which emits `E` to every subscriber rather
  than addressing one, over a per-edge signed request (ADR 0485). A case-built
  delivery to one subscriber has no counterpart in a deployment, and building
  one would mean forging the publisher's signed hop. A `system` case drives the
  emitting handler instead, as fixture `1959_system_kv_and_event_delivery`
  does.

**Consequences.** A subscriber's dedup, filter and version dispatch are
testable without a `system` suite, and the issue's case runs as written. The
test module gains `__bynkEvent_<svc>` helpers only when a body uses the
address, so existing modules are unchanged. Three diagnostics are new:
`bynk.test.event_not_subscribed`, `bynk.test.event_publisher_ambiguous` and
`bynk.test.event_needs_in_process`. The
`system` tier still has no direct event address. Admitting one would need the
harness to sign a `/_bynk/event/` request as the publisher, and that is left
until a case needs it.
