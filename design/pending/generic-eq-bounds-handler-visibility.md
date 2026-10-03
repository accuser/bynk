---
level: minor
changelog: A call to a generic function that compares its type parameter with `==` must instantiate it with an equality-supporting type, directly or through other generics (`same(f, h)` on two functions is now rejected, #1688). The cross-context visibility rules now hold in service and agent handlers and provider operations, not only in free functions, and a consumer may build a transparent export with record syntax, as the spec says (#1700)
---

## ADR: equality-bounds-at-instantiation
title: A generic function's comparisons bound its callers' type arguments
summary: Compared type parameters are inferred per generic function, transitively, and checked at each call; closes ADR 0421's first known limit

**Context.** ADR 0421 made `==` require an equality-supporting type,
recursively. It left one known limit: inside a generic function a type
variable is equality-supporting, so `fn same[T](a: T, b: T) -> Bool { a == b }`
accepted `same(f, h)` on two functions, and the runtime compared them by
identity. Bynk generics are not monomorphised, so the call site is the only
place the concrete type is known.

**Decision.**
1. **Inferred bounds, not declared ones.** A generic function *compares* a type
   parameter when an `==`/`!=` operand's type mentions it, or when it passes the
   parameter to a compared parameter of another generic function. That makes
   the bound transitive across calls (and across units, since a caller sees the
   callee's declaration). Explicit `T: Eq` bounds were the alternative, and a
   larger language change.
2. **Computed by checking the callee again.** The checker re-checks a generic
   callee's body into throwaway sinks, with a recording frame that `==` and
   compared call arguments add type variables to. The result is cached per
   declaration for one unit's check. A recursive cycle reads an in-progress
   scan as comparing nothing, which can only accept, never wrongly reject.
3. **Checked at the call.** Once the type arguments are inferred, each compared
   parameter's argument is checked with the same walk `==` uses. A hit reports
   `bynk.types.not_comparable` (or the `Stream` or held-value code) at the call,
   naming the function and the parameter, and labelling the parameter's
   declaration.

**Consequences.**
- Calls that instantiate a compared parameter with a function, `Effect`,
  `Query`, `Stream` or held `Connection` (or a type containing one) are newly
  rejected.
- Generics that never compare their parameter, such as a `map`-style helper,
  still accept functions.
- ADR 0421's other known limit (a record imported from another unit is not
  walked through its fields) is unchanged.

Proved by:
- negatives `1688_generic_eq_function_arg` (direct),
  `1688_generic_eq_transitive` and `1688_generic_eq_across_units`;
- the behavioural fixture `1688_generic_eq_accepted`, which shows comparable
  instantiations, a recursive comparing generic, and non-comparing generics
  given functions all still compile and run.
