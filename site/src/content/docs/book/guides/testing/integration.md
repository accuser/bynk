---
title: Test tiers
---
A Bynk test declares *how much of the real world is present when it runs* with an
**`as <tier>` clause** on its `case` (or `suite`) header. The three tiers are the
testing pyramid — `unit`, `integration`, `system` — and they are **not three kinds
of test**. There is **one body**; a tier only changes how much of it is real.

| Tier | What is real | Wire crossed? |
|------|--------------|---------------|
| `unit` (default) | the unit under test, in process; collaborators may be stubbed with `provides` | no |
| `integration` | real collaborators **within one context** | no |
| `system` | contexts stood up as the Workers they deploy as | **yes** — the real serialise → JSON → deserialise edge |

`unit` is the default and is **never written**. You reach for a higher tier by
adding one word to the header — the body does not change.

## One body, three tiers

Promotion changes only the header. The body is byte-for-byte identical at every
tier:

```bynk
case "a small order authorises end to end"                { … }  -- as unit (default)
case "a small order authorises end to end" as integration { … }  -- real Payment, no wire
case "a small order authorises end to end" as system      { … }  -- deployed Workers, real wire
```

At `unit` a collaborator's provision is under your control (you may stub a seam
with [`provides`](/book/reference/testing/#provides)). At `integration` and
`system` the real collaborators run — so their own invariants and contracts run
too. A green `unit` case that *fails* when promoted means a collaborator's
invariant has caught a defect a stub was hiding, with **no new test code**.

## `as` on the `suite` header

`as` also sits on the `suite` header, setting a default every `case` inherits and
may override — the case always wins:

```bynk
suite checkout as integration {          -- every case defaults to integration…
  case "small order authorises"       { … }  -- as integration (inherited)
  case "a unit-level edge"    as unit { … }  -- case overrides the suite default
}
```

A case's effective tier is `case.tier ?? suite.tier ?? unit`.

Tiers are a **`case`-only** affordance. A `property` *generates* and does not
promote, so a suite-level `as` binds its `case` members only; an `as` on a
`property` header is [`bynk.tier.property_has_tier`](/book/troubleshooting/integration-errors/#bynktierproperty_has_tier).
To check a generated input end to end, promote *that witness* as a concrete
`case … as integration`.

## Participants are inferred, not listed

For `integration` and `system` the compiler already knows which collaborators are
real: it walks the unit under test's transitive **`consumes` graph**. There is **no
participant list to maintain** — an explicit list could only drift from the real
dependency graph. (The old `suite integration "…" { wires … }` form and its
`wires` clause are retired.)

A `system` case whose inferred set is fewer than two contexts — the target and at
least one consumed context — is
[`bynk.tier.system_needs_wire`](/book/troubleshooting/integration-errors/#bynktiersystem_needs_wire):
`system` describes the *cross-context, wired* tier, so it needs a wire to cross.
To exercise one context's real collaborators without a wire, use `as integration`.

## When to promote

- **Stay at `unit`** for the common case — a scenario about one unit's logic, with
  collaborators you control seam by seam.
- **Promote to `integration`** when the point is a unit *with its real
  collaborators in one process* — no stub standing in for behaviour you actually
  want to exercise. This is the missing middle: real collaborators, one context,
  no wire.
- **Promote to `system`** when the point is a flow **across contexts** — the real
  serialise → JSON → deserialise → projection path that only the Workers target
  emits, which no in-process tier touches.

The pyramid's proportion advice rides along for free: **many `unit`, some
`integration`, few `system`**.

## A worked example

Two contexts: `shop.payment` authorises a charge; `shop.orders` consumes it.

```bynk
context shop.payment

exports transparent { PayError }

type PayError = enum { Declined }

capability Bank {
  fn charge(cents: Int) -> Effect[Result[Int, PayError]]
}

provides Bank = StubBank {
  fn charge(cents: Int) -> Effect[Result[Int, PayError]] {
    if cents > 10000 { Err(Declined) } else { Ok(cents) }
  }
}

service authorise {
  on call(cents: Int) -> Effect[Result[Int, PayError]] given Bank {
    let r <- Bank.charge(cents)
    r
  }
}
```

```bynk,ignore
context shop.orders

consumes shop.payment as Pay

exports transparent { OrderError }

type OrderError = enum { Rejected }

service place {
  on call(cents: Int) -> Effect[Result[Int, OrderError]] {
    let a <- Pay.authorise(cents)
    match a {
      Ok(n)  => Ok(n)
      Err(_) => Err(Rejected)
    }
  }
}
```

A `system` suite exercises the flow end to end. Its participants — `shop.orders`
and `shop.payment` — are **inferred** from `shop.orders`'s `consumes`, so there is
nothing to wire by hand:

```bynk
suite checkout as system {
  case "small order authorises across the wire" {
    let r <- shop.orders.place(100)
    expect r is Ok(_)
  }

  case "large order is rejected end to end" {
    let r <- shop.orders.place(50000)
    expect r is Err(_)
  }
}
```

`shop.orders.place(100)` enters the orders Worker; inside, `place` calls
`shop.payment.authorise(100)`, which crosses a simulated Service Binding into the
payment Worker — serialising the argument and deserialising the result for real.
Both hops are exercised.

## The state lifecycle is fixed across tiers

Promotion is honest because the **unit under test is always a real in-memory
instance, keyed normally, fresh per case** — at every tier. Only the realness of
its *collaborators* and whether sends cross a serialisation boundary change with
the tier. Snapshot and step invariants are checked at the commit boundary, so they
fire at **every** tier for free — which is exactly why promoting a `unit` case can
surface a collaborator invariant a stub was hiding, with no new test code.

**Agents** (Durable Objects) work across the `system` wire: a participant's agents
are backed by in-memory instances — same key, same instance **within a case**,
with state starting empty and resetting **per case**. So a service that drives an
agent can be exercised end to end, and you can assert on accumulated state.

## Run it

```sh
bynkc test .
```

```text
Running tests...

checkout:
  ✓ small order authorises across the wire
  ✓ large order is rejected end to end

2 passed, 0 failed.
```

For a `system` suite, `bynkc test` compiles the inferred participants in workers
mode under `out/workers/`, stands each one up as an in-process Worker, wires the
bindings, type-checks everything with `tsc --strict`, and runs it on Node. No
`wrangler` or `miniflare` is needed.

See the [testing reference](/book/reference/testing/#tiers-the-as-tier-clause), the
[`provides`](/book/reference/testing/#provides) seam override, and
[`bynk.tier.*` / `bynk.provides.*` errors](/book/troubleshooting/integration-errors/).
