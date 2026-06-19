# Decode untrusted JSON into a typed value

**Goal:** turn a raw JSON string from outside the program into a fully-typed —
and validated — Bynk value, with a typed error when it does not fit.

`Json.decode[T](s)` parses a JSON string and checks it against `T`, returning a
`Result[T, JsonError]`. The check covers both **shape** (the right fields and
base types) and any **refinements** on `T`, so untrusted JSON only ever enters
your program as a value that already satisfies its type's invariants — the
"make illegal states unrepresentable" idea, applied at the boundary.

## Decode into a record

```bynk
commons orders {
  type Order = {
    id: String,
    qty: Int,
  }

  fn parse(raw: String) -> Result[Order, JsonError] {
    Json.decode[Order](raw)
  }
}
```

The type argument is required (`Json.decode[Order](raw)`) — it tells the
compiler what shape to validate against. A missing field, a wrong base type, or
malformed JSON all produce an `Err(jsonError)`; a well-formed payload produces
`Ok(order)`.

## Decode straight into a refined type

Point `decode` at a type whose fields are refined, and the predicates are
enforced as part of decoding — no separate `.of` step:

```bynk
commons orders {
  type OrderId = String where NonEmpty
  type Quantity = Int where InRange(1, 1000)

  type Order = {
    id: OrderId,
    qty: Quantity,
  }

  fn parse(raw: String) -> Result[Order, JsonError] {
    Json.decode[Order](raw)
  }
}
```

`{"id": "", "qty": 5000}` is rejected — the empty `id` violates `NonEmpty` and
`5000` is outside `Quantity`'s range — so a decoded `Order` is guaranteed valid
everywhere downstream.

## Handle the result

`JsonError` is an ordinary error type, so handle it like any other `Result` —
propagate with `?`, or branch with `match`:

```bynk
commons orders {
  type Order = { id: String, qty: Int }

  fn quantityOf(raw: String) -> Result[Int, JsonError] {
    let order = Json.decode[Order](raw)?
    Ok(order.qty)
  }
}
```

## Encode the other way

`Json.encode(v)` serialises a checked value to a JSON `String`:

```bynk
commons orders {
  type Order = { id: String, qty: Int }

  fn render(o: Order) -> String {
    Json.encode(o)
  }
}
```

**See also:** [Define a refined type and validate untrusted
input](define-and-validate.md) (the `.of` path, for values you already have in
hand), and the [type-system reference](../../reference/types.md#the-json-codec).
