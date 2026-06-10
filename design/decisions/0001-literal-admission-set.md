# 0001 — Compile-time literals admitted against refined types are a narrow, closed set

- **Status:** Accepted (v0.9.4)
- **Spec:** §5.3, §6.4

## Context
Writing a literal where a refined type is expected (`let q: Quantity = 5`)
should not force a runtime `.of` round-trip; but a static evaluator over
arbitrary expressions is a correctness liability.

## Decision
Admission applies to a **closed literal set**: integer, string, and boolean
literals, `()`, and unary minus applied directly to an integer literal. Not
arithmetic on literals, not identifiers or consts. The predicate is evaluated at
compile time; an admitted literal lowers to the existing `.unsafe` constructor
(validation discharged statically).

## Consequences
The static evaluator stays trivially correct. The set can widen in a later
increment without breaking programs; nothing in the language depends on
admission of computed expressions.
