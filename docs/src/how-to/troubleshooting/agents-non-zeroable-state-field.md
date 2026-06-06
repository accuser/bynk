# `karn.agents.non_zeroable_state_field`

```text
[karn.agents.non_zeroable_state_field] agent `Gauge` state field `level` has no
defined zero value, so a fresh key cannot be initialised
```

## What it means

An agent's state field has a type with no zero value. Karn initialises a
never-seen key's state automatically, so every field needs a well-defined
starting value. Types that have one include `Int` (`0`), `Bool` (`false`),
`String` (`""`), `Option[T]` (`None`), and records of zeroable fields. Types that
do **not** include opaque types, sum types (other than `Option`), and refined
types that exclude their zero.

```karn
agent Gauge {
  key id: String
  state {
    level: Int where Positive,   -- Positive excludes 0 — no zero value
  }
}
```

## Fix

- **Use `Option` for "not set yet".** This is the usual answer: `None` is a valid
  zero and means "never set".

  ```karn
  state {
    level: Option[Int],
  }
  ```

- **Relax the refinement** so the zero is admitted (e.g. `NonNegative` instead of
  `Positive`, since `0` satisfies `NonNegative`).
- **Choose a zeroable field type** (a plain `Int`, a record of zeroables, etc.).

## Related

- [Build a stateful agent](../agents/stateful-agent.md)
- Reference: [agents](../../reference/agents.md)
- Explanation: [The agent model](../../explanation/the-agent-model.md)
