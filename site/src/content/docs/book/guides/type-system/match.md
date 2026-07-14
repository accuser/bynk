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

## Discriminate a nested payload

A payload binding is itself a pattern, so you can look *inside* a variant's
payload instead of only naming it. This is how you discriminate an error cause,
or unwrap an `Option` of a `Result` in one flat `match` — no second, nested
`match`:

```bynk
commons monitor {
  type FetchError =
    | PollClosed
    | UnknownChoice

  fn code(res: Result[Int, FetchError]) -> Int {
    match res {
      Ok(n)              => n
      Err(PollClosed)    => 0
      Err(UnknownChoice) => 400
    }
  }

  fn inner(opt: Option[Result[Int, FetchError]]) -> Int {
    match opt {
      Some(Ok(n))  => n
      Some(Err(_)) => -1
      None         => -2
    }
  }
}
```

Exhaustiveness sees through the nesting: `Some(Ok(_))` / `Some(Err(_))` / `None`
covers every case **without** a wildcard, and omitting `Err(UnknownChoice)`
above is a compile error naming the uncovered shape. Capitalisation
disambiguates a **binding** from a **variant**: a lowercase name (`n`) binds the
payload; an uppercase name (`PollClosed`) matches that nested variant.

## Guard an arm with `if`

An arm may carry a trailing `if` **guard** — an arbitrary `Bool` expression over
the arm's bindings. The arm matches only when the pattern matches *and* the guard
holds:

```bynk
commons routing {
  type Req =
    | Get(path: String)
    | Post(body: String)

  fn route(r: Req) -> String {
    match r {
      Get(path) if path == "/api" => "api"
      Get(path)                   => path
      Post(body)                  => body
    }
  }
}
```

A guarded arm never counts toward exhaustiveness — the guard can fail at runtime
— so the following unguarded `Get(path)` arm stays reachable. (For a guard drawn
from the closed refinement vocabulary over a primitive, `where` is planned as the
complementary form.)

## Related

- For a one-branch test that yields a `Bool`, see
  [Narrow and bind with `is`](/book/guides/type-system/narrow-with-is/).
- Reference: [type system](/book/reference/types/).
