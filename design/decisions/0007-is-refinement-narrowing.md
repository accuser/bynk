# 0007 — Refinement narrowing reuses `is` with check-time disambiguation

- **Status:** Accepted (v0.13)
- **Spec:** §6.3 (narrowing), §5.6

## Context
`value is RefinedType` (flow-sensitive counterpart to `.of`) shares surface
syntax with sum-variant checks. The parser cannot know which is meant.

## Decision
**Check-time disambiguation, no grammar change**: if the value is not a sum (or
the name not one of its variants) and the pattern is a bare nullary name
resolving to a refined type over a compatible base, the `is` is a refinement
check, narrowing identifiers in the established narrowing positions.

## Consequences
No new syntax or pattern kind. Lowering reuses the `.of` predicate logic as a
boolean expression with a branch-entry rebinding for the brand.
