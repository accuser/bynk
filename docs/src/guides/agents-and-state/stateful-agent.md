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
    let _ <- count.update((c) => c + 1)
    count
  }
}
```

- Read a `store` field by its bare name (`count`).
- Write it unconditionally with `:=` (the new value does **not** depend on the old
  one).
- When the new value *does* depend on the old one, use `count.update(fn)` — a
  read-modify-write — rather than `:=`. A `:=` whose right-hand side names its own
  field is rejected (see [below](#modify-a-cell-in-place)).
- Every `store` write is committed atomically when the handler returns; there is
  no `commit` step, and a faulting handler persists nothing.
- Handlers return `Effect[T]`; returning a plain value in tail position is lifted
  automatically.

## Modify a cell in place

A `:=` write replaces a cell's value with an expression that stands on its own:

```bynk,ignore
count := 0          -- reset
limit := limit      -- rejected: the right-hand side reads the cell being written
```

When the new value is computed *from the old one*, reach for `update(fn)` instead.
It takes a pure combiner `(T) -> T` and applies it to the current value:

```bynk,ignore
let _ <- count.update((c) => c + 1)   -- increment
let _ <- count.update((c) => c * 2)   -- double
```

Why a separate operation rather than `count := count + 1`? Because the latter
hides a *read* of the prior value inside what looks like a plain write. Making it
`update` keeps that prior-value dependency visible (and the combiner retry-safe).
A self-referencing `:=` is therefore rejected with
[`bynk.cell.self_reference`](../../reference/diagnostics.md), steering you to
`update`.

`update` mutates the cell; it does not return the new value. To read-modify-write
**and** return — as `increment` above does — await the `update`, then read the
bare name back (the read sees the staged write):

```bynk,ignore
let _ <- count.update((c) => c + 1)
count                                  -- the committed new value
```

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
