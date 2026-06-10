# 0033 — Closures over capabilities; bottom-up lambda effectfulness

- **Status:** Accepted (v0.20a)
- **Spec:** §5.2, §5.5

## Context
Effectful iteration (v0.20b's `traverse` shapes) needs lambdas that call
`given` capabilities. But a lambda's effectfulness is read off its body,
while a capability call is only legal in an effectful body — circular as
stated.

## Decision
A lambda may close over and call a `given` capability (the enclosing
handler's capability map and used-tracking stay shared). Effectfulness is
judged **bottom-up by the presence of effect operations** — an `<-` bind, a
capability call, a call returning `Effect` — via a syntactic pre-scan run
before typing; a firing scan makes the lambda effectful and wraps its result
in `Effect`. Nested lambdas are scanned separately — an inner lambda's
effects are its own. `commit` inside a lambda is **forbidden** (the lambda
frame drops the agent state type, so the existing
`karn.commit.outside_agent` fires).

## Consequences
The map-vs-traverse distinction falls out structurally; capability usage
inside lambdas counts toward `given` tracking; agent state transitions stay
visibly top-level in handler bodies.
