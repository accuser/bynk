# Work with `Result` and optional values

**Goal:** produce and consume `Result` (success or error) and `Option` (a value
or nothing).

Bynk has no exceptions and no `null`. Fallible operations return `Result[T, E]`;
possibly-absent values are `Option[T]`.

## Construct values

```bynk
commons demo {
  fn ok(n: Int) -> Result[Int, String] {
    Ok(n)
  }
  fn fail() -> Result[Int, String] {
    Err("nope")
  }
  fn wrap(n: Int) -> Option[Int] {
    Some(n)
  }
  fn empty() -> Option[Int] {
    None
  }
}
```

## Consume with `match`

```bynk
fn extract(o: Option[Int]) -> Int {
  match o {
    Some(n) => n
    None => 0
  }
}
```

The same form works for `Result`, with `Ok` and `Err` arms.

## Propagate errors with `?`

Inside a function that itself returns a `Result`, `?` unwraps an `Ok` or returns
early on an `Err`:

```bynk
commons demo {
  type Reps = Int where InRange(1, 100)

  fn doubled(n: Int) -> Result[Int, ValidationError] {
    let r = Reps.of(n)?
    Ok(r * 2)
  }
}
```

If `Reps.of(n)` is `Err(e)`, `doubled` returns `Err(e)` immediately; otherwise
`r` is the unwrapped value.

## Related

- [Pattern-match with `match`](match.md) ·
  [Narrow and bind with `is`](narrow-with-is.md)
- [Define a refined type and validate untrusted input](define-and-validate.md)
