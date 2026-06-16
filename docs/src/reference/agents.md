# Agents

An agent is a keyed, stateful entity declared inside a `context`.

## Declaration

```karn
agent Counter {
  key id: CounterId

  state {
    count: Int,
  }

  on call current() -> Effect[Int] {
    self.state.count
  }

  on call increment() -> Effect[Int] {
    let next = self.state.count + 1
    commit { ...self.state, count: next }
    next
  }
}
```

| Part | Rule |
|---|---|
| `key <name>: <Type>` | the agent's identity; one key field. |
| `state { … }` | the agent's persistent fields. Every field needs an **initial value** — an explicit initialiser or an implicit zero (see below). |
| `on call <name>(…) -> Effect[T]` | a handler. The return type must be an `Effect` (`karn.agent.return_not_effect`). |

Agents may only be declared inside a context (`karn.agent.outside_context`), and
may not declare HTTP handlers (`karn.parse.http_in_agent`).

## State initialisation

A never-seen key is initialised automatically, so every state field needs a
defined initial value. A field gets one in one of two ways:

**1. An explicit initialiser** — `field: T = <value>`. The value is a
compile-time constant: a literal, a sum variant, `Some`/`None`/`Ok`/`Err`, a
record, or `T.unsafe(lit)`. It may not reference `self`, parameters, or
capabilities (`karn.agents.bad_state_initialiser` otherwise). An initialiser
makes any type admissible — including the ones that have no implicit zero.

```karn
state {
  status:  OrderStatus = Pending,   -- a sum: the initial state
  level:   Level       = 1,          -- a refined Int (Positive)
  retries: Int         = 3,          -- a non-zero default
}
```

**2. An implicit zero** — a field with no initialiser must have a defined zero:

| Field type | Zero |
|---|---|
| `Int` | `0` |
| `Bool` | `false` |
| `String` | `""` |
| `Option[T]` | `None` |
| record of zeroable fields | each field zeroed |

A field that has neither an initialiser nor an implicit zero — an opaque type, a
non-`Option` sum, or a refined type that excludes its zero (`Int where
Positive`) — is rejected with
[`karn.agents.non_zeroable_state_field`](../troubleshooting/agents-non-zeroable-state-field.md).
Add an initialiser (or, to model "not set yet", use `Option[T]`).

## State machines

Because a sum-typed field can carry an initial variant, an agent's state can be
a **state machine**: the sum's variants are the states, the initialiser names the
start state, `match self.state.<field>` reads the current state (exhaustively),
and a transition is a `commit`:

```karn
agent Order {
  key id: OrderId
  state {
    status: OrderStatus = Pending,
    items:  Int,
  }

  on call place() -> Effect[Result[(), OrderError]] {
    match self.state.status {
      Pending => {
        commit { ...self.state, status: Placed }
        Ok(())
      }
      Placed    => Err(AlreadyPlaced)
      Cancelled => Err(AlreadyCancelled)
    }
  }
}
```

v0.11 does not restrict which transitions are legal — any `commit` to any state
type-checks. (Legal-transition tables and invariants are a later increment.)

## Reading and committing state

- **Read** with `self.state.<field>`.
- **Commit** a replacement state with `commit <record>`, usually the spread form
  `commit { ...self.state, <field>: <value> }`. `commit` is valid only in an
  agent handler (`karn.commit.outside_agent`); the value must match the state
  type (`karn.commit.wrong_state_type`); and at most one `commit` may be
  reachable per execution path (`karn.commit.two_reachable_commits`).

## Addressing and calling

Construct an agent with its key, then call a handler, binding the effect:

```karn
let c = Counter(CounterId.unsafe("a"))
let n <- c.increment()
```

## Lifecycle and emission

A fresh key's state falls back to the compiled zero value on first access. On the
`bundle` target an agent uses an in-process state registry; on `workers` it
compiles to a Cloudflare Durable Object keyed by the agent key. See
[emission](emission.md) and [The agent model](../guides/agents-and-state/the-agent-model.md).
