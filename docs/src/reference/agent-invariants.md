# Agent invariants

An **invariant** is a property an agent guarantees of *every* committed state. It
is a universally-quantified claim — *for all reachable states, this holds* — and
the runtime enforces it at each commit boundary. Invariants are the contract half
of validation; tests are the behaviour half (see
[Invariants as contracts](../guides/agents-and-state/understand-invariants.md)).

## Declaration

Invariants form a phase **between the `state { }` block and the handlers**:

```bynk
type OrderStatus = enum { Pending, Placed, Paid }

agent Order {
  key id: OrderId

  state {
    status:     OrderStatus       = Pending,
    user:       Option[UserId],
    cart:       Option[Cart],
    paymentRef: Option[AuthId],
  }

  invariant placed_has_user_and_cart:
    status == Placed implies (user.isSome() && cart.isSome())
  invariant paid_has_payment_ref:
    status == Paid implies paymentRef.isSome()

  on call place(u: UserId, c: Cart) -> Effect[()] {
    commit { ...self.state, status: Placed, user: Some(u), cart: Some(c) }
    ()
  }
}
```

Each invariant is `invariant <name>: <predicate>`. The predicate references the
agent's **state fields by bare name** — `status`, `paymentRef` — not
`self.state.status` (an invariant is a claim about the *proposed* committed
state, not the currently-persisted one).

## The predicate surface

A predicate is an ordinary **pure, `Bool`-typed** expression over the state
fields, with two additions:

- **`implies`** — logical implication. `P implies Q` reads as P → Q and is
  equivalent to `!P || Q`, but directional and prose-readable. It is the
  lowest-precedence operator (below `||`).
- **`is`** — pattern-matching as a `Bool` expression (`order is Placed(o)`),
  with optional bindings that remain in scope across the predicate.

Predicates may call **pure value methods** (`Option.isSome`/`isNone`, sum
`is`-checks). They may **not**:

| Not allowed | Diagnostic |
|---|---|
| A non-`Bool` predicate | `bynk.invariant.not_bool` |
| Effects, capabilities, or test-only constructs | `bynk.invariant.impure_predicate` |
| Referencing another agent | `bynk.invariant.cross_agent_reference` |
| Two invariants with the same name | `bynk.invariant.duplicate_name` |
| An `invariant` after a handler | `bynk.parse.invariant_after_handler` |

Invariants are **per-agent**: they constrain one agent's reachable states. A
property that genuinely spans agents is eventually-consistent — express it with a
saga or a scenario, not an invariant.

## When they fire, and what a violation looks like

All invariants are **runtime-checked at the commit boundary**. A handler runs to
completion and builds the value it `commit`s; the runtime evaluates each
invariant against that value *before* it is persisted. If any fails:

- the commit **faults** with an `InvariantViolation` — the offending state is
  never written;
- the failure is a **fault, not an outcome** — the handler's `Result` is never
  produced, consistent with Bynk's failure model.

Intermediate states *within* a handler are not constrained — a handler may
briefly hold an inconsistent state while transitioning, as long as the committed
state satisfies every invariant (the same deferral transactional databases use
for constraints).

> **"Revert" means non-persistence, not rollback.** A fault guarantees only that
> the *offending committed value is never written*. It does **not** undo effects
> the handler already performed (a `~>`/`<-` send) or an *earlier* `commit` in
> the same handler. The handler is not transactional. (ADR 0107.)

### Observability limit (MVP)

In v0.80, a violation surfaces to a programmatic caller as a bare **500-class
fault** — observationally identical to any other internal fault. The compiler
*does* `console.error` the agent type and invariant name (never the key value, to
keep domain identifiers out of logs) at the commit site, so a refusal is
distinguishable from a crash **in the logs**. Making the refusal
caller-distinguishable is a [named follow-on](../about/versioning-and-roadmap.md)
(a general typed-agent-fault channel), as is the compile-time
provable-violation pass.
