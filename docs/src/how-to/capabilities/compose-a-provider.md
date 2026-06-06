# Compose a provider from other capabilities

**Goal:** build one adapter out of others — a `Payments` provider that uses
`Http` and `Logger` — instead of duplicating that wiring in every handler.

## Declare the provider's dependencies with `given`

A `provides` block may carry a `given` clause, exactly like a handler. The
provider's bodies can then call the listed capabilities:

```karn
context payment

capability Logger { fn info(message: String) -> Effect[()] }
capability Http   { fn post(path: String) -> Effect[()] }
capability Charge { fn run() -> Effect[()] }

provides Logger = ConsoleLogger {
  fn info(message: String) -> Effect[()] { Effect.pure(()) }
}

provides Http = FetchHttp {
  fn post(path: String) -> Effect[()] { Effect.pure(()) }
}

provides Charge = HttpCharge given Http, Logger {
  fn run() -> Effect[()] {
    let _ <- Logger.info("charging")
    let _ <- Http.post("/charge")
    Effect.pure(())
  }
}
```

`Charge` depends on `Http` and `Logger`. A handler that uses `Charge` never sees
that — it just declares `given Charge`, and the composition root supplies the
rest.

## The rules

- A capability you call in a provider body must be in that provider's `given`
  (`karn.given.undeclared_capability`).
- Each `given` name must be a declared capability
  (`karn.given.unknown_capability`).
- A capability can't depend on itself, directly or transitively
  ([`karn.provider.dependency_cycle`](../troubleshooting/provider-dependency-cycle.md)).

## How it's wired

The providers form a dependency graph. The generated `compose` instantiates them
in dependency order and injects each one's dependencies — `Logger` and `Http`
first, then `new HttpCharge({ Http, Logger })`. You write the graph; the compiler
orders it.

## Related

- Reference: [Capabilities & providers](../../reference/capabilities.md).
