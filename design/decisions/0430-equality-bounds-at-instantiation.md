# 0430 — A generic function's comparisons bound its callers' type arguments

- **Status:** Accepted (v0.300)

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
2. **Computed in the declaring unit.** While a unit is checked, the checker
   re-checks each of its generic functions and methods into throwaway sinks,
   with a recording frame that `==` and compared call arguments add type
   variables to. It does this in that unit's own environment, so a call made
   through the unit's own `uses`, or a name shadowed in an importer, resolves
   as written. Units are checked `uses`-first, and the result is cached per
   declaration for the whole project check, so an importer only looks it up.
   A miss, or a recursive cycle read mid-scan, counts as comparing nothing,
   which can only accept, never wrongly reject.
3. **Checked at the call.** Once the type arguments are inferred, each compared
   parameter's argument is checked with the same walk `==` uses. A hit reports
   `bynk.types.not_comparable` (or the `Stream` or held-value code) at the call,
   naming the function and the parameter, and labelling the parameter's
   declaration when it is in the caller's file.

**Consequences.**
- Calls that instantiate a compared parameter with a function, `Effect`,
  `Query`, `Stream` or held `Connection` (or a type containing one) are newly
  rejected.
- Generics that never compare their parameter, such as a `map`-style helper,
  still accept functions.
- Methods are bound the same way: a method's own type parameters, and its
  generic receiver type's (`fn Box.has(self, x: A)`), so ADR 0421's first
  known limit is closed for methods too.
- ADR 0421's other known limit (a record imported from another unit is not
  walked through its fields) is unchanged.

Proved by:
- negatives `1688_generic_eq_function_arg` (direct),
  `1688_generic_eq_transitive`, `1688_generic_eq_across_units`,
  `1688_generic_eq_two_hop` (transitive through another unit's `uses`) and
  `1688_generic_eq_method_receiver`;
- the behavioural fixture `1688_generic_eq_accepted`, which shows comparable
  instantiations, a recursive comparing generic, and non-comparing generics
  given functions all still compile and run;
- the compile-only fixtures `1688_generic_eq_shadowed_helper` (an importer's
  own comparing `helper` does not change what an imported generic compares)
  and `1688_generic_eq_methods_accepted`.
