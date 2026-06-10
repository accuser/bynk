# 0032 — Named functions are values where a function type is expected

- **Status:** Accepted (v0.20a)
- **Spec:** §5.1

## Context
`map(xs, double)` must work; mandated eta-expansion (`(x) => double(x)`)
is needless friction. But `karn.resolve.fn_without_call` (a bare function
reference is an error) protects against accidental references everywhere
else — and the resolver that owned it has no type information.

## Decision
Relax contextually: a bare named-function reference is a value of its
signature's function type **where a function type is expected**, and an
error elsewhere. The judgment (with `param_as_function`) relocates to the
checker; the resolver branches become silent passes. Emission is the
function's TS identifier, unchanged.

## Consequences
The relaxations change no currently-passing program's diagnostics (pinned by
the negative corpus). Call resolution stays declared-functions-first — the
pre-existing ident/call precedence asymmetry is documented, not changed.
