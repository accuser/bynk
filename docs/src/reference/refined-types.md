# Refined-type API

A refined type is a base type plus one or more predicates:

```bynk
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

A predicate must apply to the base type (`bynk.types.predicate_base_mismatch`).
An `InRange` with `lo > hi` is rejected (`bynk.types.inverted_range`), as is a set
of predicates that admit no value (`bynk.types.empty_refinement`) or a negative
length (`bynk.types.negative_length`). An invalid regex is
`bynk.types.invalid_regex`.

## `.of` — checked construction

```bynk
Age.of(value)   -- Result[Age, ValidationError]
```

`.of` **always** returns a `Result`. Use it for values not known at compile time
(input, variables). See
[Define a refined type and validate untrusted input](../guides/type-system/define-and-validate.md).

## `.unsafe` — unchecked construction

```bynk
Age.unsafe(value)   -- Age
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
([`bynk.refine.literal_violates`](../troubleshooting/refine-literal-violates.md)).
**Opaque types are excluded** from admission. Admitted literals are compile-time
literals only — integers, strings, booleans, and `()` — not arithmetic
expressions or identifiers.

See [The refined-literal admission model](../guides/type-system/refined-literal-admission.md)
for the rationale.

## Narrowing with `is`

A runtime value can be narrowed to a refined type with `is`. `value is Refined`
runs the type's predicates at runtime and yields a `Bool`; where that truth gates
the branch (an `if` body, the right of `&&`), the value is narrowed to the refined
type — so it can be passed where the refined type is expected, without going
through `.of`:

```bynk
commons demo

type Quantity = Int where InRange(1, 100)

fn double(q: Quantity) -> Int {
  2
}

fn classify(n: Int) -> Int {
  if n is Quantity {
    double(n)        -- n : Quantity here
  } else {
    0
  }
}
```

- The value must be an **identifier** to be narrowed (a `let` binding or a
  parameter); `f(x) is Quantity` is a valid check but narrows nothing.
- The refined type's base must match the value's
  ([`bynk.types.is_base_mismatch`](../troubleshooting/is-base-mismatch.md)).
- This is the flow-sensitive counterpart to `.of`: `.of(v)` returns a `Result`
  for the untrusted case; `is` narrows in a guard. Refinements are **not**
  preserved through arithmetic (`a + b` of two refined `Int`s is a plain `Int`).

See [Narrow and bind with `is`](../guides/type-system/narrow-with-is.md).
