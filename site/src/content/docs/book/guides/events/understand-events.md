---
title: Understand events
---
A context sometimes needs to tell other contexts that something happened,
without knowing — or caring — who is listening. Bynk's `event`/`Events`/`from
Events(E)` trio is in-system pub-sub: one context declares a typed fact and
emits it, any number of others subscribe to it, and the compiler — not your
code — wires the delivery.

This page is the mental model for the emit/subscribe core (slice 0),
subscription pattern filtering (slice 1), and the runtime envelope (slice
2). For the full track's remaining scope (schema versioning, replay), see
[Versioning & roadmap](/book/about/versioning-and-roadmap/).

## The three pieces

```mermaid
flowchart LR
  A["commerce.order<br/>event PaymentConfirmed"] -->|"Events.emit[E]"| B["fan-out"]
  B -->|"delivered"| C["commerce.notifications<br/>from Events(E)<br/>on event(e: E)"]
```

- An **`event`** is a typed fact, declared inside a context — a record shape,
  nothing more:

  ```bynk,ignore
  context commerce.order

  exports transparent { PaymentConfirmed }

  event PaymentConfirmed = {
    orderId: String,
  }
  ```

- **`Events`** is the first-party capability that emits one: `Events.emit[E](value)`,
  `given Events` on the handler. Like every generic capability operation
  (`Idempotency.dedup[T]`, for the same reason — see
  [First-party `bynk` capabilities](/book/reference/bynk-capabilities/)), the
  type argument is always explicit, never inferred from `value`'s type.

  ```bynk,ignore
  service markPaid {
    on call(orderId: String) -> Effect[()] given Events {
      Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: orderId })
    }
  }
  ```

- **`from Events(E)`** is a service protocol — the sixth member of the closed
  protocol set alongside `call`/`http`/`cron`/`queue`/`websocket`. A service
  with this protocol has exactly one handler, `on event`, and is not called
  directly; it runs whenever a matching event arrives.

  ```bynk,ignore
  context commerce.notifications

  consumes commerce.order
  consumes bynk { Logger }

  service OnPayment from Events(PaymentConfirmed) {
    on event(e: PaymentConfirmed) -> Effect[()] given Logger {
      Logger.info(e.orderId)
    }
  }
  ```

  With no pattern on the header, every emission of `PaymentConfirmed` reaches
  every subscriber of `PaymentConfirmed`. A subscriber that wants only some
  of them can filter — see the next section.

## Filtering delivery with a pattern

A subscription may narrow which emissions it receives with a structural
pattern on the payload:

```bynk,ignore
context commerce.notifications

consumes commerce.order
consumes bynk { Logger }

service OnDomesticPayment from Events(PaymentConfirmed { region: Region.Domestic, .. }) {
  on event(e: PaymentConfirmed) -> Effect[()] given Logger {
    -- e.region is NOT statically Region.Domestic here — e is
    -- PaymentConfirmed as always. Only *delivery* is filtered: this
    -- subscriber only runs for emissions where region == Domestic.
    Logger.info(e.orderId)
  }
}
```

A pattern lists a subset of the event's declared fields; listing any field
requires a trailing `..` (`from Events(E { })` is rejected — use the bare
`from Events(E)` form for an unfiltered subscription). A field's value is a
literal (`Int`/`String`/`Bool`) or a nullary sum-type variant, written bare
(`Domestic`) or qualified (`Region.Domestic`); a variant that carries a
payload is rejected — matching only the tag while ignoring a payload would
silently admit emissions the pattern looks like it should exclude. Multiple
listed fields compose with AND. There is no nested-field sub-pattern yet.

**Deliver-and-filter, not narrower routing.** Every emission of `E` still
reaches the fan-out mechanism; each subscriber's own generated handler
evaluates the pattern as a guard and no-ops if it doesn't match. This is why
`e`'s type is unaffected — the filter is a runtime check at the top of the
handler body, not a language-level type refinement. A future increment may
add static narrowing once the design's own open refinement-propagation
question is settled; today, a handler that needs to act on the fact that its
pattern matched still reads `e.region` at its full declared type.

## The envelope, and idempotent handling

An `on event` handler may declare a second, optional parameter —
`env: EventEnvelope` — carrying runtime metadata about the emission:

```bynk,ignore
context commerce.notifications

consumes commerce.order
consumes bynk { Idempotency }

service OnPayment from Events(PaymentConfirmed) {
  on event(e: PaymentConfirmed, env: EventEnvelope) -> Effect[()] given Idempotency {
    let seen <- Idempotency.dedup[()](env.eventId)
    match seen {
      Some(_) => (),
      None => {
        -- react to e here, exactly once per eventId
        let _ <- Idempotency.remember[()](env.eventId, (), 7.days)
      }
    }
  }
}
```

`EventEnvelope` is `{ eventId: String, publisherId: String, emittedAt:
Instant, schemaVersion: Int }`. Two things worth knowing before you reach
for it:

- **`eventId` is minted once per emission, not once per delivery.** If two
  sibling subscribers both declare `env`, they observe the *same*
  `eventId` for the same emission — it identifies the emission, not the
  delivery.
- **`publisherId` names the emitting *context*, not the emitting agent.**
  `Events.emit` is legal from a plain `service` handler with no agent
  instance at all, so there is no per-agent identity to report uniformly;
  `publisherId` is the emitting context's qualified name (e.g.
  `"commerce.order"`) every time.

**The idempotency idiom.** Delivery is at-least-once (see below) — an
effectful subscriber that must not double-apply an emission dedups on
`env.eventId` using the already-documented
[`Idempotency`](/book/reference/bynk-capabilities/) capability, exactly as
shown above. This is the ordinary `dedup`/`remember` pair, not new syntax —
`env.eventId` is just a `String` key like any other, and two different
subscriber services deduping the same `eventId` never collide (each
handler's key is scoped to its own qualified name automatically). A
subscriber that only reads or transforms state, taking no effect, is
trivially idempotent already and needs no `env` parameter at all.

`schemaVersion` is reserved for a future slice — it is always `1` today,
not yet computed from the event type's shape.

## Only the declaring context may emit

`Events.emit[E]` compiles only when `E` is an event declared **in the emitting
context itself** — even though `E` is visible cross-context for subscription
via the ordinary `consumes` you'd expect (`commerce.notifications` above
`consumes commerce.order` precisely to name `PaymentConfirmed` in its `from
Events(...)` header). A foreign context attempting to emit it fails closed:

```
context commerce.notifications
consumes commerce.order
consumes bynk { Events }

service leak {
  on call() -> Effect[()] given Events {
    Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: "x" })
  }
}
→ [bynk.event.emit_outside_owner] `PaymentConfirmed` is not declared in this
  context — only the context that declares an event may emit it
```

This is deliberate, and new: every other cross-context boundary in Bynk
governs what a context may *name* (`uses`/`consumes`) — this is the first that
restricts what a context may *do* with something it can already see. A type
being subscribable does not make it forgeable.

## Emission is fire-and-forget, and release-at-commit

`Events.emit[E]` returns `Effect[()]` — the emitting handler never learns
whether a subscriber ran, or how it went. What it *does* guarantee: an
emission only ever reaches a subscriber if the emitting handler itself
**committed**. An agent handler that emits and then goes on to violate one of
its own invariants emits nothing — the invariant violation throws before the
event is released, exactly as if `Events.emit` had never been called:

```bynk,ignore
agent Ledger {
  key id: String
  store total: Cell[Int] = 0

  invariant total_stays_small:
    total < 10

  on call bump(amount: Int) -> Effect[()] given Events {
    let _ <- total.update((n) => n + amount)
    do Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: "ledger-event" })
  }
}
```

Calling `bump` with an amount that pushes `total` past `10` throws
`InvariantViolation` — the state write never commits, and the emission above
it never reaches `OnPayment` either. This is not a special case for `Events`:
it is the same all-or-nothing handler-commit boundary agents already have
(store writes vs. invariants), extended to cover an emission raised anywhere
in the same handler body, including one raised inside a plain `service` that
calls into the agent.

## Delivery, concretely, per platform

The fan-out substrate differs by target, but the emit/subscribe surface above
does not — the same source compiles and runs the same way on every platform.
A subscription pattern's guard runs as the first line of the generated
handler on every target too — deliver-and-filter, not a routing difference:

| Target | Mechanism |
|---|---|
| **Cloudflare Workers** | Each publishing context gets its own compiler-synthesised fan-out Durable Object. One subscriber's delivery failure is caught and logged without blocking delivery to its siblings. Ordering is preserved *within* one emission (everything a single handler invocation emits) and *across* successive, non-overlapping calls to one agent — but **not** across concurrent invocations of the same agent, since the Durable Object delivers by making an outbound call per subscriber and does not serialise two overlapping deliveries against each other. |
| **Bundle** (node, browser) | Dispatch is in-process — no Durable Object, no wire. The composed program calls directly into each subscriber's handler. |

What the track does **not** yet give you: a delivery retry, a durable log to
replay from, or ordering across *concurrent* invocations of the same
publishing agent — measured, not assumed, and found not to hold. A subscriber
that needs a total order across concurrent publishes must carry its own
sequence number in the event payload. A subscriber that must not double-apply
a redelivered emission dedups on `env.eventId` via `Idempotency` (see "The
envelope, and idempotent handling", above) — though only up to that
capability's current, in-memory limit, since a duplicate delivered across an
isolate restart is not deduped until a durable provider exists.

**See also:**
[First-party `bynk` capabilities](/book/reference/bynk-capabilities/),
[Understand the capability model](/book/guides/effects-and-capabilities/understand-the-capability-model/),
[Understand: invariants as contracts](/book/guides/agents-and-state/understand-invariants/),
[Diagnostics](/book/reference/diagnostics/).
