# Capabilities & providers

A **capability** is a typed interface to the outside world; a **provider**
implements one. Handlers receive capabilities through a `given` clause. All three
live inside a `context`.

## Declaring a capability

```karn
capability Logger {
  fn info(message: String) -> Effect[()]
}
```

A capability is a set of operation *signatures* (no bodies). Each operation
returns `Effect[T]` (capabilities are how effectful work reaches the outside).

## Providing a capability

```karn
provides Logger = ConsoleLogger {
  fn info(message: String) -> Effect[()] {
    Effect.pure(())
  }
}
```

`provides Cap = Impl { … }` implements every operation of `Cap`. The signatures
must match exactly (`karn.provider.signature_mismatch`,
`karn.provider.missing_operation`, `karn.provider.extra_operation`). There is one
provider per capability in a context.

## Using a capability

A handler lists the capabilities it needs with `given`, then calls them:

```karn
service hello {
  on call() -> Effect[String] given Logger {
    let _ <- Logger.info("hi")
    "ok"
  }
}
```

A `given` name must be a declared capability (`karn.given.unknown_capability`); a
call to a capability not in `given` is an error (`karn.given.undeclared_capability`);
a declared-but-unused capability is a warning (`karn.given.unused_capability`).

## Provider composition (`provides … given`)

A provider may itself depend on other capabilities — declare them with `given`
after the provider name, and call them in the bodies:

```karn
context demo

capability Logger  { fn info(message: String) -> Effect[()] }
capability Greeter { fn greet() -> Effect[()] }

provides Logger = ConsoleLogger {
  fn info(message: String) -> Effect[()] {
    Effect.pure(())
  }
}

provides Greeter = PoliteGreeter given Logger {
  fn greet() -> Effect[()] {
    let _ <- Logger.info("hello")
    Effect.pure(())
  }
}
```

The same `given` discipline applies (unknown / undeclared-use are errors). The
providers form a **dependency graph** over capabilities; the composition root
instantiates them in dependency order, injecting each provider's dependencies.

A capability may not depend on itself, directly or transitively
(`karn.provider.dependency_cycle`) — including the trivial `provides X = … given
X`.

## Emission

Providers compile to classes implementing the capability interface; a composed
provider gains a constructor that receives its dependencies, and the generated
`compose` instantiates providers in topological order. See [emission](emission.md).
