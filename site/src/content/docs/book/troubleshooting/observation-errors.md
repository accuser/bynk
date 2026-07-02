---
title: "`bynk.observe.*` errors"
---
Observation — `expect Cap.op called …` and the `trace(Cap.op)` escape hatch — asserts
over a unit's *interaction* with its capabilities (v0.117). These are their common
errors. See the [Observation reference](/book/reference/testing/#observation).

## `bynk.observe.not_a_seam`

```text
[bynk.observe.not_a_seam] Error: `AuthId` is not a capability the unit under test consumes; only a consumed capability's calls can be observed
```

**Cause:** the observation subject is not a capability the unit `consumes` / has in
scope via `given` — for example a type, a service, or a consumed-context alias.

**Fix:** observe a capability the unit actually depends on. Only injected capability
seams record calls; a cross-context service or an agent handler is not (yet)
observable.

## `bynk.observe.unknown_op`

```text
[bynk.observe.unknown_op] Error: capability `Logger` has no operation named `nope`
```

**Cause:** the `Cap.op` subject names an operation the capability does not declare.

**Fix:** use one of the capability's declared operations (check the `capability`
block).

## `bynk.observe.with_not_bool`

```text
[bynk.observe.with_not_bool] Error: a `with` predicate has type `String`, but a `Bool` is required
```

**Cause:** a `with` predicate does not evaluate to `Bool`.

**Fix:** make it a boolean claim over the operation's parameters — a comparison
(`with amount > 1000`), an `is` narrowing (`with msg is "…"`), or a pure
`Bool`-returning method.

## `bynk.observe.impure_with`

```text
[bynk.observe.impure_with] Error: a `with` predicate uses an effectful or test-only construct; it must be pure
```

**Cause:** a `with` predicate uses an effect, `?` propagation, `expect`, `Val`, or
`trace` — a `with` predicate is the one predicate surface and must be pure.

**Fix:** remove the effectful/test-only construct. A `with` predicate may read the
operation's arguments and call pure value methods only.

## `bynk.observe.outside_case`

```text
[bynk.observe.outside_case] Error: an observation is only valid inside a `case` body
```

**Cause:** an `expect Cap.op called …` observation appears outside a `case` — calls
are recorded per case, so there is nothing to observe elsewhere.

**Fix:** move the observation into a `case`.

## `bynk.observe.trace_outside_test`

```text
[bynk.observe.trace_outside_test] Error: `trace` is only valid inside a `case` body
```

**Cause:** `trace(Cap.op)` appears outside a `case`. `trace` is a test-only builtin;
elsewhere it is an ordinary identifier.

**Fix:** use `trace(Cap.op)` only inside a `case`. In production code, `trace` names
whatever value you bind it to.

## `bynk.observe.bad_count`

```text
[bynk.observe.bad_count] Error: a call count must be a non-negative integer literal (`called once` or `called <n> times`)
```

**Cause:** a call count is not a non-negative integer literal.

**Fix:** write `called once`, or `called <n> times` with a literal `<n>` (e.g.
`called 3 times`).
