# Agents

An agent is a keyed, stateful entity declared inside a `context`.

## Declaration

```bynk
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

| Part | Rule |
|---|---|
| `key <name>: <Type>` | the agent's identity; one key field. |
| `store <name>: <Kind>[…]` | a persistent field of a storage kind (`Cell`, `Map`, `Set`, `Cache`, `Log`). Every field needs an **initial value** — an explicit initialiser or an implicit zero (see below). |
| `on call <name>(…) -> Effect[T]` | a handler. The return type must be an `Effect` (`bynk.agent.return_not_effect`). |

A `Cell[T]` is a single stored value, read by its bare name and written with `:=`;
the other kinds (`Map`, `Set`, `Cache`, `Log`) expose effectful methods. See the
[storage kinds in the grammar reference](grammar.md#rule-store_field) for the full
catalogue.

Agents may only be declared inside a context (`bynk.agent.outside_context`), and
may not declare HTTP handlers (`bynk.parse.http_in_agent`).

## State initialisation

A never-seen key is initialised automatically, so every `store` field needs a
defined initial value. A field gets one in one of two ways:

**1. An explicit initialiser** — `store field: Cell[T] = <value>`. The value is a
compile-time constant: a literal, a sum variant, `Some`/`None`/`Ok`/`Err`, a
record, or `T.unsafe(lit)`. It may not reference `self`, parameters, or
capabilities (`bynk.agents.bad_state_initialiser` otherwise). An initialiser
makes any type admissible — including the ones that have no implicit zero.

```bynk
store status:  Cell[OrderStatus] = Pending   -- a sum: the initial state
store level:   Cell[Level]       = 1         -- a refined Int (Positive)
store retries: Cell[Int]         = 3         -- a non-zero default
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
[`bynk.agents.non_zeroable_state_field`](../troubleshooting/agents-non-zeroable-state-field.md).
Add an initialiser (or, to model "not set yet", use `Option[T]`). Collection kinds
with no implicit zero (such as `Cell[List[T]]`) likewise need an initialiser —
typically `= []`.

## State machines

Because a sum-typed `Cell` can carry an initial variant, an agent's state can be
a **state machine**: the sum's variants are the states, the initialiser names the
start state, `match <field>` reads the current state (exhaustively), and a
transition is an assignment:

```bynk
agent Order {
  key id: OrderId

  store status: Cell[OrderStatus] = Pending
  store items:  Cell[Int]

  on call place() -> Effect[Result[(), OrderError]] {
    match status {
      Pending => {
        status := Placed
        Ok(())
      }
      Placed    => Err(AlreadyPlaced)
      Cancelled => Err(AlreadyCancelled)
    }
  }
}
```

Bynk does not restrict which transitions are legal — any `:=` to any value of the
field's type type-checks. (Legal-transition tables are a later increment;
[invariants](agent-invariants.md) constrain reachable states today.)

## Reading and writing state

- **Read** a `store` field by its bare name (`count`, `status`).
- **Write** a `Cell` with `name := <value>`. A `:=` is valid only against a
  `store Cell` field (`bynk.cell.invalid_target`); the value must match the cell's
  type; and a `:=` whose right-hand side names its own field is rejected
  (`bynk.cell.self_reference`) — a read-modify-write uses `update` instead.
- **Read-modify-write** a `Cell` with `update`, the one method-shaped cell
  operation:

  | Operation | Type | Notes |
  |---|---|---|
  | `cell.update(f)` | `Effect[()]` | `f: (T) -> T`, a pure combiner applied to the current value. Awaited with `<-`. Mutates the cell; does not return the new value (read the bare name back to observe it). |

  ```bynk,ignore
  let _ <- count.update((c) => c + 1)
  ```

  `read`/`write` are not callable methods — the bare name reads and `:=` writes.
- **Commit** is implicit: every `store` write a handler makes is collected and
  persisted **atomically when the handler returns**, after invariants are checked.
  A handler that faults partway through persists nothing.

## Invariants

An agent may declare **invariants** — predicates that must hold of every
committed state — in a phase between the `store` fields and the handlers:

```bynk
invariant available_non_negative:
  available >= 0
```

They are runtime-checked at each commit boundary; a violation faults before the
state is written. See [Agent invariants](agent-invariants.md) for the predicate
surface (`implies`, `is`, pure value methods), the diagnostics, and what a caller
observes.

## Addressing and calling

Construct an agent with its key, then call a handler, binding the effect:

```bynk
let c = Counter(CounterId.unsafe("a"))
let n <- c.increment()
```

## Lifecycle and emission

A fresh key's state falls back to the compiled zero value on first access. On the
`bundle` target an agent uses an in-process state registry; on `workers` it
compiles to a Cloudflare Durable Object keyed by the agent key. See
[emission](emission.md) and [The agent model](../guides/agents-and-state/the-agent-model.md).
