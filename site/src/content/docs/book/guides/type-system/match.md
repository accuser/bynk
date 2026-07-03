---
title: "Pattern-match with `match`"
---
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

## Match on a primitive value

`match` also dispatches a primitive `Int`, `String`, or `Bool` against literal
patterns — the idiomatic way to map a raw value (an external reference code, a
flag) to a domain type, instead of an `if`/`else if ==` chain:

```bynk
fn classify(code: Int) -> String {
  match code {
    31 => "english"
    32 => "irish"
    _  => "other"
  }
}
```

`Int` and `String` are unbounded, so a literal `match` needs a wildcard `_` arm
to be exhaustive. `Bool` is complete once both `true` and `false` appear:

```bynk
fn label(flag: Bool) -> String {
  match flag {
    true  => "on"
    false => "off"
  }
}
```

Literal patterns match by value — an integer (which may be negative, `-1 => …`),
a string, or a boolean. They are `match`-only: to test a single value elsewhere,
use `==`. A refined type (`type Code = Int where …`) is matched against the same
literals as its base.

## Related

- For a one-branch test that yields a `Bool`, see
  [Narrow and bind with `is`](/book/guides/type-system/narrow-with-is/).
- Reference: [type system](/book/reference/types/).
