# `karn.refine.literal_violates`

```text
[karn.refine.literal_violates] Error: literal 0 does not satisfy `InRange` required by type `Reps`
```

## What it means

You wrote a literal in a position whose expected type is a refined type, and the
literal does not satisfy that type's predicate. Because Karn checks admitted
literals at compile time, this is a build error rather than a runtime failure.

```karn,fail
commons demo {
  type Reps = Int where InRange(1, 100)

  fn bad() -> Reps {
    0          -- 0 is outside InRange(1, 100)
  }
}
```

## Fix

Pick the option that matches where the value comes from:

- **It should be a different literal.** Use one that satisfies the predicate
  (e.g. `1`).
- **It comes from runtime input.** Don't use a bare literal — validate with
  [`.of`](../refined-types/define-and-validate.md), which returns a `Result` you
  handle:

  ```karn
  fn parse(n: Int) -> Result[Reps, ValidationError] {
    Reps.of(n)
  }
  ```

- **The predicate is wrong.** If `0` *should* be allowed, widen the type
  (e.g. `InRange(0, 100)`).

## Related

- [Use a literal where a refined type is expected](../refined-types/literal-admission.md)
- Reference: [refined-type API](../../reference/refined-types.md)
