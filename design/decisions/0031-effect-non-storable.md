# 0031 — `Effect[T]` stays non-storable; effectful calls keep `<-` confinement

- **Status:** Accepted (v0.20a)
- **Spec:** §5.5

## Context
`Effect` is an eager `Promise`: an un-bound effectful call is already
running. Function values must not open a back door that capability calls
don't have.

## Decision
Calling an effectful function value is an **effect operation**: legal only in
an effectful context (`karn.effect.fn_value_in_pure_context`), exactly like a
capability call. The confinement is *emergent*, not a new storability
checker: `Effect`'s universal incompatibility, tail auto-lift, and the
pure-context gates already confine it — v0.20a adds only the
value-application gate and pins the storage case with a fixture.

## Consequences
No semantic surprises from Promise eagerness in pure code; effectful bodies
match the pre-existing capability-call semantics exactly.
