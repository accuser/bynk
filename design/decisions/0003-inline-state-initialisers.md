# 0003 — Agent state fields take inline initialisers, not an `init` block

- **Status:** Accepted (v0.11)
- **Spec:** §4.5, §5.4

## Context
Sum-typed agent state (`status: OrderStatus`) has no implicit zero; fields need
declared initial values. Two surfaces were considered: inline `field: T = expr`
or a separate `init { … }` block.

## Decision
**Inline** — `field: T = expr` on the state field. Locality (a field's full
story in one place), consistency with the type-system storage syntax, and zero
new keywords.

## Consequences
Sum-typed state machines work without `Option`-wrapping. The `init`-block shape
remains possible later but has no driver.
