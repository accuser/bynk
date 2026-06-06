# Refined-type API

A refined type is a base type plus one or more predicates:

```karn
type Age = Int where InRange(0, 150)
type Username = String where MinLength(3) and MaxLength(20)
```

Predicates are combined with `and`. A refined type emits a branded type plus a
constructor object with `.of` and `.unsafe`.

## Predicates

### Int

| Predicate | Holds when |
|---|---|
| `NonNegative` | value ≥ 0 |
| `Positive` | value > 0 |
| `InRange(lo, hi)` | lo ≤ value ≤ hi (inclusive) |

### String

| Predicate | Holds when |
|---|---|
| `NonEmpty` | length ≥ 1 |
| `MinLength(n)` | length ≥ n |
| `MaxLength(n)` | length ≤ n |
| `Length(n)` | length = n |
| `Matches(regex)` | the whole string matches `regex` (anchored) |

A predicate must apply to the base type (`karn.types.predicate_base_mismatch`).
An `InRange` with `lo > hi` is rejected (`karn.types.inverted_range`), as is a set
of predicates that admit no value (`karn.types.empty_refinement`) or a negative
length (`karn.types.negative_length`). An invalid regex is
`karn.types.invalid_regex`.

## `.of` — checked construction

```karn
Age.of(value)   // Result[Age, ValidationError]
```

`.of` **always** returns a `Result`. Use it for values not known at compile time
(input, variables). See
[Define a refined type and validate untrusted input](../how-to/refined-types/define-and-validate.md).

## `.unsafe` — unchecked construction

```karn
Age.unsafe(value)   // Age
```

Constructs without checking. Use only when the value is already known valid.

## Literal admission

A literal written where a refined type is expected is checked **at compile time**
and admitted directly (lowering to `.unsafe`), with no `Result`. Admission applies
in these positions:

- return position (block tail);
- a `let` with a type annotation;
- an `Ok`/`Some`/`Err` payload;
- a refined-typed call argument.

A literal that violates the predicate is a compile error
([`karn.refine.literal_violates`](../how-to/troubleshooting/refine-literal-violates.md)).
**Opaque types are excluded** from admission. Admitted literals are compile-time
literals only — integers, strings, booleans, and `()` — not arithmetic
expressions or identifiers.

See [The refined-literal admission model](../explanation/refined-literal-admission.md)
for the rationale.
