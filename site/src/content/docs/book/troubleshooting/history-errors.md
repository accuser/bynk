---
title: "`bynk.history.*` errors"
---
A **history property** — `for all run: History[Agent]` — generates a driven
call-history of an agent and asserts a predicate over the reached run (v0.119).
These are its common errors. See the
[History properties reference](/book/reference/testing/#history-properties).

## `bynk.history.not_an_agent`

```text
[bynk.history.not_an_agent] Error: `for all` cannot generate `History[Amount]` — only an agent has handlers to sequence
```

**Cause:** the type argument to `History[...]` is not an agent — for example a value
type, a service, or a collection. Only an agent has handlers to sequence and
reachable states to observe.

**Fix:** name an agent. To generate values, use a plain `for all x: T`; to generate a
behaviour, drive an agent with `for all run: History[Agent]`.

## `bynk.history.not_generable`

```text
[bynk.history.not_generable] Error: `History[Wallet]` cannot be driven — handler `setLabel`'s parameter `code: Code` is not generable (e.g. a `Matches` refinement)
```

**Cause:** a handler of the agent has a parameter whose type cannot be generated
(such as a `String where Matches(...)`), so the runner cannot synthesise a call to
drive it.

**Fix:** make every handler parameter refinement-generable, or exercise that handler
with a concrete `case` (supplying a pinned `Val[T]`) instead of a history.

## `bynk.history.outside_property`

```text
[bynk.history.outside_property] Error: `History[…]` is only valid as a `for all` generator inside a `property`
```

**Cause:** `History[Agent]` appears where a value type is expected — a field, a
parameter, a return type, or a local annotation. `History` is a test-only generator,
not a value type.

**Fix:** use `History[Agent]` only as the type of a `for all` binding inside a
`property`. A driven history is bound as an ordinary `List[Step]`; store or pass that
instead if you need a value.

## `bynk.history.restates_invariant`

```text
[bynk.history.restates_invariant] Error: history property `p` merely re-checks a guarantee agent `Wallet`'s `invariant`/`transition` already enforces on every reached state
```

**Cause:** the history predicate re-checks a per-state guarantee a declared
`invariant` or `transition` already enforces (e.g. `run.all((s) => s.new.balance >=
0)` when `Wallet` carries `invariant nonneg: balance >= 0`). The driver only commits
states the invariants admit, so the check can never fail.

**Fix:** assert a **cross-step** protocol a per-state invariant cannot express — for
example "no accepted spend without a prior accepted top-up" via `run.upTo(s)` — or
delete the redundant property. (The check is conservative and syntactic, so a
differently-written near-duplicate may slip through.)
