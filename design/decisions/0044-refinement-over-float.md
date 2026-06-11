# 0044 — Refinement over `Float`: float bounds, numeric predicates extend, bounds match the base

- **Status:** Accepted (v0.21)
- **Spec:** §5.3, §6.2

## Context
`type Probability = Float where InRange(0.0, 1.0)` and
`type Price = Float where Positive` are the decimal types v0.22's data
modelling needs. The refinement machinery was hard-coded `Int`-only.

## Decision
- `InRange` accepts **float bounds** (`InRange(0.0, 1.0)`) when the base
  is `Float`. Internally a separate predicate representation
  (`InRangeF`) keeps every `Int` refinement path untouched.
- **`NonNegative` and `Positive` extend to `Float`.**
- **Bound literals must match the base type**: `Float where
  InRange(0, 1)` (and the mixed `InRange(0, 1.0)`) are
  `karn.types.no_numeric_coercion` — the no-coercion rule (0041) applied
  to refinement bounds.
- Literal admission (v0.9.4), the `.of` constructor, emptiness
  consistency checks, and `is`-narrowing checks all extend to float
  bounds; the `.of` runtime check swaps `Number.isInteger` for
  `Number.isFinite` (0040).

## Consequences
Refined floats behave like refined ints everywhere: `let r: Ratio =
0.5` admits at compile time (lowering through `unsafe`), an
out-of-range literal is `karn.refine.literal_violates`, and
`Positive` correctly excludes `0.0` in the emptiness check
(`InRange(-1.0, 0.0) and Positive` is an empty refinement).
