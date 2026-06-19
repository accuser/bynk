# 0028 — Generics are Open-narrow: functions only, no bounds

- **Status:** Accepted (v0.20a)
- **Spec:** §4.3.1, §5.2, §6.4a

## Context
The combinator stdlib (v0.20b) needs generic functions; full generics (user
generic types, bounds/typeclasses) is a far larger commitment.

## Decision
Type parameters on **functions** only (`fn name[A, B](…)`). Generic *type*
declarations stay rejected (`bynk.generics.no_generic_types`) — `List`/`Map`
(built-in, v0.20b) remain the only generic types — and bounds are rejected
(`bynk.generics.no_bounds`). The `TypeParam` representation is a struct, so a
bound is a later field addition, not a rework. Within its own body a type
parameter is a rigid variable, equal only to itself. Emission is erased TS
generics. Generic functions are free functions; generic methods can follow
additively.

## Consequences
A strict, additively-extensible subset. Constrained generics stay reachable;
nothing ships that would have to be unshipped.
