# 0295 — An event's payload and envelope are validated at the receiving Workers route; there is no dead-letter path

- **Status:** Accepted (v0.240.1)

**Context.** Compiling a minimal two-context project to `--target workers` and
tracing an event payload end to end (mint in `lower.rs` → the fan-out DO's
`FanoutEvent` → `deliverEvent` → the receiving `/_bynk/event/` route →
`compose.ts`'s generated wrapper) showed it typed `unknown` at every hop,
terminating in `handlers.OnPayment.event(payload as any, deps)`. No
`deserialise_<PayloadType>` was ever invoked. This falsified §6's "refined
fields in the payload are validated on receipt, malformed events routed to
the platform dead-letter policy" for the Events track specifically (a
plausibly-accurate sentence for `on queue` consumers, which run on real
Cloudflare Queues with a native dead-letter queue — but Events chose the
fanout-DO substrate, not queue-per-topic, per ADR 0284, so it has no such
substrate to inherit a dead-letter policy from).

Tracing the root cause found it deeper than "the route forgot to call an
existing codec": the subscriber's generated module had **no codec at all**
for the event payload type. `emit_consumed_context_helpers`
(`bynk-emit/src/emitter.rs`) narrows codec generation to services a
consumed context's own methods are actually **called** on
(`called_consumed_services`, walking only `MethodCall` expressions — ADR
0200 Decision E's narrowing, so a caller doesn't pay for a callee surface it
never reaches). A pure `from Events(E)` subscriber calls no method on the
publisher at all, so this narrowing bailed before the event's payload type
was ever considered a root — the gap was in codec *emission*, not just in
the route wiring it up.

**Decision.** Three changes, landing together:

1. **Widen the codec-emission root set, not the narrowing rule.**
   `emit_consumed_context_helpers` now also collects, per consumed context,
   the payload type of every `from Events(E)` this context subscribes to
   where the consumed context owns `E` (via `consumed_event_names`) — in
   addition to, not instead of, the existing called-service roots. The
   justification is that a subscriber **participates** in the event's
   contract as its receiving half; an event payload is not an uncalled
   surface the way an unreached method is, so it does not belong under ADR
   0200 Decision E's original rationale. A context subscribing to its own
   locally-declared event needed no change — the existing `local_boundary`
   path already covers it.
2. **The envelope gets a hand-written runtime validator, not a generated
   codec.** `EventEnvelope` is declared in `adapter bynk`, not a consumed
   context — widening the consumed-context codec machinery to also cover
   every adapter-declared type would touch every context importing any
   `bynk` type, for the sake of one record. `deserialiseEventEnvelope`
   (`bynk-emit/runtime/src/boundary.ts`) is a small, self-contained
   validator matching the same `Result<T, BoundaryError>` /
   `StructuralMismatch` shape every generated codec already uses.
3. **Validate at the receiving route, not in the compose wrapper.** The
   `/_bynk/event/` block in `workers_entry.rs`'s `emit_worker_entry` gets the
   same validate-then-`400` shape the pre-existing `/_bynk/call/` block has
   always used: the envelope validated once, unconditionally, before the
   `switch` (it is always on the wire regardless of whether a given
   subscriber declared the optional `env` parameter); the payload validated
   per-`case`, against that subscriber's own now-generated codec. The
   compose-surface wrapper (`emit_event_wrapper`, `workers.rs`) is
   unchanged — it already accepted `payload: unknown` and cast `as any`, so
   the now-validated value passes through it unmodified.

Bundle target is out of scope by construction: event dispatch there
(`project.rs`'s `__eventsDispatch`) is an in-process direct call, never
serialised, so TypeScript's own static typing is the boundary's real check.

**Consequences.** A malformed payload or envelope now gets `400` at the
route instead of silently reaching the handler body. `deliverEvent` already
throws on a non-`ok` response, and the fan-out DO's per-subscriber
`try`/`catch` already logs and continues to the next subscriber — so the
observable effect is a logged delivery failure to **that one subscriber**,
siblings unaffected. This is not a dead-letter policy, and none is
introduced: `design/tracks/events.md` §6 is corrected to say so plainly
rather than continue to imply one exists.

A previously-compiling project can now fail to compile if an event's
payload closure reaches a type the subscriber has no exported view of (the
pre-existing `owned(n)` filter, which already applied to ordinary
cross-context calls, is now reachable via events too) — the same category
of edge the call path already had, not a new one.

This closes the prerequisite Events slice 3 (#972, field-default
expressions) named as depending on it: a default has no validation
boundary to attach a fallback to until a real deserialise step exists on
this path.
