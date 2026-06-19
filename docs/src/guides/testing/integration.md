# Test a flow across Workers

A unit test (`test <context> { … }`) checks one context with its collaborators
replaced by `mocks`. That never runs the real cross-context wire — the
serialise → JSON → deserialise → projection path that only the workers target
emits. A **`test integration`** block fills that gap: it stands several contexts
up as the Workers they deploy as, wires their Service Bindings together, and runs
a flow end to end through the real wire — no mocks.

## The contexts

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

## The integration test

Put it under the project's `tests/` tree. `wires` lists every participating
context; cases call in by qualified name.

```bynk
test integration "checkout" {
  wires shop.orders, shop.payment

  test "small order authorises across the wire" {
    let r <- shop.orders.place(100)
    assert r is Ok(_)
  }

  test "large order is rejected end to end" {
    let r <- shop.orders.place(50000)
    assert r is Err(_)
  }
}
```

`shop.orders.place(100)` enters the orders Worker; inside, `place` calls
`shop.payment.authorise(100)`, which crosses a simulated Service Binding into the
payment Worker — serialising the argument and deserialising the result for real.
Both hops are exercised.

## Run it

```sh
bynkc test .
```

```text
Running tests...

integration · checkout:
  ✓ small order authorises across the wire
  ✓ large order is rejected end to end

2 passed, 0 failed.
```

`bynkc test` compiles the participants in workers mode under `out/workers/`,
stands each one up as an in-process Worker, wires the bindings, type-checks
everything with `tsc --strict`, and runs it on Node. No `wrangler` or `miniflare`
is needed.

## Rules to know

- **At least two participants** — a single context is a unit test's job.
- **Closure**: every context a participant `consumes` must itself be wired. The
  compiler names the missing one (`bynk.integration.unwired_dependency`).
- **No `mocks`** — integration tests wire real implementations. Mock in unit
  tests instead.
- Cross-context **capabilities** (`given B.Cap`) work unchanged: the provider is
  instantiated locally in the consumer Worker, so wiring the providing context is
  enough.
- **Agents** (Durable Objects) work across the wire: a participant's agents are
  backed by in-memory instances — same key, same instance **within a case**, with
  state starting empty and resetting **per case**. So a service that drives an
  agent can be exercised end to end, and you can assert on accumulated state.

See the [testing reference](../../reference/testing.md#test-integration--multi-worker-integration-tests)
and [`bynk.integration.*` errors](../../troubleshooting/integration-errors.md).
