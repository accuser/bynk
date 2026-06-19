# Build a stateful agent and keep its state zeroable

**Goal:** declare an agent that owns state, reads it, and updates it — with state
that initialises cleanly for a never-seen key.

Agents live inside a `context`.

## Declare the agent

Give it a `key` (its identity), a `state` block, and handlers:

```bynk
context counters

type CounterId = opaque String

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

- Read state with `self.state.<field>`.
- Update it by building a new state value and `commit`-ting it (the spread form
  copies the current state and overrides fields).
- Handlers return `Effect[T]`; returning a plain value in tail position is lifted
  automatically.

## Keep state zeroable

Every state field must have a zero value, because a never-seen key is
initialised automatically. `Int`→`0`, `Bool`→`false`, `String`→`""`,
`Option[T]`→`None`. A field that excludes its zero (for example `Int where
Positive`, which excludes `0`) is rejected with
[`bynk.agents.non_zeroable_state_field`](../../troubleshooting/agents-non-zeroable-state-field.md).

When you need "not set yet", use `Option`:

```bynk
state {
  reading: Option[Int],   -- starts as None — "never set"
}
```

## Address an agent

Construct an agent with its key, then call a handler (binding the effectful
result with `<-`):

```bynk
let c = Counter(CounterId.unsafe("a"))
let n <- c.increment()
```

## Related

- Tutorial: [Add a stateful agent](../../tutorials/05-stateful-agent.md).
- Reference: [agents](../../reference/agents.md).
- Troubleshooting: [`bynk.agents.non_zeroable_state_field`](../../troubleshooting/agents-non-zeroable-state-field.md).
