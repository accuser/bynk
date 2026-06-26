# Build a stateful agent and keep its state zeroable

**Goal:** declare an agent that owns state, reads it, and updates it — with state
that initialises cleanly for a never-seen key.

Agents live inside a `context`.

## Declare the agent

Give it a `key` (its identity), one or more `store` fields, and handlers:

```bynk
context counters

type CounterId = opaque String

agent Counter {
  key id: CounterId

  store count: Cell[Int]

  on call current() -> Effect[Int] {
    count
  }

  on call increment() -> Effect[Int] {
    let next = count + 1
    count := next
    next
  }
}
```

- Read a `store` field by its bare name (`count`).
- Update it by assigning with `:=`. When the new value depends on the old one,
  read the old value into a `let` first — a `:=` whose right-hand side names its
  own field is rejected.
- Every `store` write is committed atomically when the handler returns; there is
  no `commit` step, and a faulting handler persists nothing.
- Handlers return `Effect[T]`; returning a plain value in tail position is lifted
  automatically.

## Keep state zeroable

Every `store` field needs a starting value for the never-seen key that Bynk
initialises automatically. Either the type has a zero (`Int`→`0`, `Bool`→`false`,
`String`→`""`, `Option[T]`→`None`), or you supply an explicit initialiser with
`=`. A field whose type excludes its zero (for example `Int where Positive`, which
excludes `0`) and which has no initialiser is rejected with
[`bynk.agents.non_zeroable_state_field`](../../troubleshooting/agents-non-zeroable-state-field.md).

When you need "not set yet", use `Option`:

```bynk
store reading: Cell[Option[Int]]   -- starts as None — "never set"
```

When the type has no zero but you have a sensible default, give an initialiser:

```bynk
store limit: Cell[Int where Positive] = 1
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
