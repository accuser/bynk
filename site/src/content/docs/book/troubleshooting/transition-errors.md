---
title: "`bynk.transition.*` errors"
---
Step invariants — `transition <name>: <pred over old/new>` — are the invariant
predicate widened from a single committed state to the *move* between two (v0.116).
These are their common errors. See the
[Step invariants reference](/book/reference/agent-invariants/#step-invariants).

## `bynk.transition.not_bool`

```text
[bynk.transition.not_bool] Error: transition `bad` predicate has type `Int`, but a transition must be `Bool`
```

**Cause:** a `transition` predicate does not evaluate to `Bool`.

**Fix:** make it a boolean claim over `old`/`new` — a comparison
(`new.count >= old.count`), an `implies`, an `is` narrowing, or a pure
`Bool`-returning method.

## `bynk.transition.impure_predicate`

```text
[bynk.transition.impure_predicate] Error: transition `bad` uses an effectful or test-only construct; a step invariant predicate must be pure
```

**Cause:** a predicate uses an effect, `?` propagation, `expect`, or `Val` — a
transition is the one predicate surface and must be pure.

**Fix:** remove the effectful/test-only construct. A predicate may read the
`old`/`new` state and call pure value methods only.

## `bynk.transition.no_step_reference`

```text
[bynk.transition.no_step_reference] Error: transition `bad` references neither `old` nor `new`, so it constrains a single state, not a step
```

**Cause:** a `transition` predicate mentions neither `old` nor `new`, so it is a
claim about one committed state, not a move.

**Fix:** write it as an `invariant` (which constrains a single committed state), or
reference `old`/`new` to make it a genuine step claim.

## `bynk.transition.duplicate_name`

```text
[bynk.transition.duplicate_name] Error: agent `Order` declares more than one transition named `t`
```

**Cause:** two transitions share a name. The name rides the `InvariantViolation`
failure report, so it must be unique per agent.

**Fix:** give each transition a distinct name.

## `bynk.transition.cross_agent_reference`

```text
[bynk.transition.cross_agent_reference] Error: transition `bad` references another agent; a step invariant constrains a single agent's own state move
```

**Cause:** a predicate references another agent. A transition, like an invariant,
constrains one agent's own reachable states.

**Fix:** a property that genuinely spans agents belongs in a saga or a scenario,
not a transition.

## `bynk.parse.transition_after_handler`

```text
[bynk.parse.transition_after_handler] Error: a `transition` must be declared before the agent's handlers
```

**Cause:** a `transition` appears after an `on` handler. Step invariants form a
phase between the store fields and the handlers, beside the snapshot invariants.

**Fix:** move the transition above the first handler.
