# Use a literal where a refined type is expected

**Goal:** write a literal value directly where a refined type is expected,
without calling `.of` or handling a `Result`.

When you write a literal in a position whose expected type is a refined type,
Karn checks the literal against the predicate **at compile time** and admits it
directly. A valid literal compiles; an invalid one is a compile error
([`karn.refine.literal_violates`](../troubleshooting/refine-literal-violates.md)).

## Where admission applies

```karn
commons demo {
  type Quantity = Int where InRange(1, 100)

  -- return position
  fn defaultQty() -> Quantity {
    5
  }

  -- let with a type annotation
  fn sample() -> Quantity {
    let q: Quantity = 10
    q
  }

  -- Ok / Some / Err payloads
  fn checked() -> Result[Quantity, ValidationError] {
    Ok(50)
  }

  -- a refined-typed call argument
  fn clamp(q: Quantity) -> Quantity {
    q
  }
  fn useClamp() -> Quantity {
    clamp(10)
  }
}
```

Each admitted literal lowers to a `.unsafe` call (e.g. `Quantity.unsafe(5)`) —
the check happened in the compiler, so none is needed at runtime.

## When to reach for `.of` or `.unsafe` instead

- The value is **not** a literal you write yourself (it comes from a request, a
  database, a variable): use [`.of`](define-and-validate.md), which validates at
  runtime and returns a `Result`.
- The value is a non-literal you can **prove** is already valid: use `.unsafe`,
  which constructs without checking. Reach for it sparingly.
- **Opaque types are excluded** from literal admission — construct them with
  `.of` or `.unsafe`.

## Related

- Reference: [refined-type API](../../reference/refined-types.md).
- Rationale: [The refined-literal admission model](../../explanation/refined-literal-admission.md).
