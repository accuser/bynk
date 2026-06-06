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
| `state { … }` | the agent's persistent fields. Every field must be **zeroable** (see below). |
| `on call <name>(…) -> Effect[T]` | a handler. The return type must be an `Effect` (`karn.agent.return_not_effect`). |

Agents may only be declared inside a context (`karn.agent.outside_context`), and
may not declare `on http` handlers (`karn.parse.http_in_agent`).

## State zeroability

A never-seen key is initialised automatically, so every state field must have a
zero value:

| Field type | Zero |
|---|---|
| `Int` | `0` |
| `Bool` | `false` |
| `String` | `""` |
| `Option[T]` | `None` |
| record of zeroable fields | each field zeroed |

Types with no zero — opaque types, sum types other than `Option`, and refined
types that exclude their zero (e.g. `Int where Positive`) — are rejected with
[`karn.agents.non_zeroable_state_field`](../how-to/troubleshooting/agents-non-zeroable-state-field.md).
Use `Option[T]` to model "not set yet".

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
[emission](emission.md) and [The agent model](../explanation/the-agent-model.md).
