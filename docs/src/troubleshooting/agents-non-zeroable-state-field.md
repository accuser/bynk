# `bynk.agents.non_zeroable_state_field`

```text
[bynk.agents.non_zeroable_state_field] agent `Gauge` state field `level` has no
defined zero value, so a fresh key cannot be initialised
```

## What it means

An agent's state field has a type with no zero value. Bynk initialises a
never-seen key's state automatically, so every field needs a well-defined
starting value. Types that have one include `Int` (`0`), `Bool` (`false`),
`String` (`""`), `Option[T]` (`None`), and records of zeroable fields. Types that
do **not** include opaque types, sum types (other than `Option`), and refined
types that exclude their zero.

```bynk
agent Gauge {
  key id: String
  state {
    level: Int where Positive,   -- Positive excludes 0 — no zero value
  }
}
```

## Fix

- **Add an initialiser** (v0.11). Give the field an explicit starting value with
  `= <value>`; any type becomes admissible, including sums (a state machine's
  initial state) and refined types:

  ```bynk
  state {
    level: Int where Positive = 1,
  }
  ```

  The initialiser must be a compile-time value; see
  [`bynk.agents.bad_state_initialiser`](agents-bad-state-initialiser.md).

- **Use `Option` for "not set yet".** `None` is a valid zero and means "never
  set" (`level: Option[Int]`).
- **Relax the refinement** so the zero is admitted (e.g. `NonNegative` instead of
  `Positive`, since `0` satisfies `NonNegative`).

## Related

- [Build a stateful agent](../guides/agents-and-state/stateful-agent.md)
- Reference: [agents](../reference/agents.md)
- Explanation: [The agent model](../guides/agents-and-state/the-agent-model.md)
