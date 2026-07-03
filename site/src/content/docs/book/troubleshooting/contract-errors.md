---
title: "`bynk.contract.*` errors"
---
Function contracts — `requires` (preconditions) and `ensures` (postconditions) —
are the invariant predicate attached to a pure function (v0.115). These are their
common errors. See the [Contracts reference](/book/reference/testing/#contracts).

> **Not to be confused with** the capability `@requires` annotation (ADR 0127),
> which names the capabilities a handler needs. A value contract is a bare clause
> over parameters and `result`, not a capability annotation.

## `bynk.contract.result_in_requires`

```text
[bynk.contract.result_in_requires] Error: precondition `bad` references `result`, but the return value is not bound until the function returns
```

**Cause:** a `requires` clause references `result`. A precondition is checked on
entry, before the function runs, so the return value does not yet exist.

**Fix:** move the claim to an `ensures` clause, where `result` is in scope. If you
meant an ordinary value named `result`, rename it — inside an `ensures` predicate
`result` always names the return value.

## `bynk.contract.not_bool`

```text
[bynk.contract.not_bool] Error: contract clause `bad` predicate has type `Int`, but a contract clause must be `Bool`
```

**Cause:** a `requires`/`ensures` predicate does not evaluate to `Bool`.

**Fix:** make it a boolean claim — a comparison (`result <= p`), an `implies`, an
`is` narrowing, or a pure `Bool`-returning method.

## `bynk.contract.impure_predicate`

```text
[bynk.contract.impure_predicate] Error: contract clause `bad` uses an effectful or test-only construct; a contract predicate must be pure
```

**Cause:** a predicate uses an effect, `?` propagation, `expect`, or `Val` — a
contract is the one predicate surface and must be pure.

**Fix:** remove the effectful/test-only construct. A predicate may read the
parameters (and `result`) and call pure value methods only.

## `bynk.contract.duplicate_name`

```text
[bynk.contract.duplicate_name] Error: function `f` declares more than one contract clause named `c1`
```

**Cause:** two clauses — across `requires` and `ensures` — share a name. The name
rides the failure report and the redundant-test flag, so it must be unique per
function.

**Fix:** give each clause a distinct name.

## `bynk.contract.restated_by_test`

```text
[bynk.contract.restated_by_test] Error: this `expect` restates the `ensures never_above` contract of `discount`, which is already checked at every call and by the runner
```

**Cause:** a `case` binds a contracted function's result (`let r = discount(…)`)
and then `expect`s a claim that is α-equivalent to one of the function's `ensures`
clauses. The contract is already checked at every call and generated against by the
runner — the test adds nothing.

**Fix:** delete the restating `expect`. Keep a `case` only when it witnesses a
specific, named value; move a general claim into an `ensures` clause. The check is
conservative — it fires only on a syntactic restatement over the same bound
arguments.
