# Pattern-match with `match`

**Goal:** branch on the variants of a sum type (or a `Result`/`Option`), binding
each variant's payload.

## Match on a sum type

`match` requires an arm for **every** variant. Name a variant to match it; bind
its payload by naming the fields:

```bynk
commons shop {
  type Status =
    | Pending
    | Shipped(tracking: String)
    | Cancelled(reason: String)

  fn describe(s: Status) -> String {
    match s {
      Pending => "awaiting shipment"
      Shipped(tracking: t) => t
      Cancelled(reason: r) => r
    }
  }
}
```

Omit a variant and the program does not compile — there is no accidental
fall-through. A `match` is an expression: its value is the value of the matched
arm.

## Match on `Result` and `Option`

The same form works for the built-in sum types:

```bynk
fn label(o: Option[Int]) -> String {
  match o {
    Some(n) => "present"
    None => "absent"
  }
}
```

## Related

- For a one-branch test that yields a `Bool`, see
  [Narrow and bind with `is`](narrow-with-is.md).
- Reference: [type system](../../reference/types.md).
