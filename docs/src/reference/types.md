# Type system

## Built-in base types

| Type | Values | Emits |
|---|---|---|
| `Int` | integer literals (`0`, `-42`) | `number` |
| `Float` | float literals (`1.5`, `0.0`, `-3.14`) | `number` |
| `String` | string literals (`"…"`) | `string` |
| `Bool` | `true`, `false` | `boolean` |

The unit type is written `()`. `Int` and `Float` are **distinct and
incompatible** — there is no implicit coercion (`karn.types.no_numeric_coercion`).
Convert explicitly: `i.toFloat()` (Int → Float, total) or `f.round()` /
`f.floor()` / `f.ceil()` / `f.truncate()` (Float → Int); parse a string with
`Int.parse(s)` / `Float.parse(s)`, each returning `Option`.

## Built-in generic types

| Type | Variants | Purpose |
|---|---|---|
| `Result[T, E]` | `Ok(T)`, `Err(E)` | success or error |
| `Option[T]` | `Some(T)`, `None` | a value or nothing |
| `Effect[T]` | — | an effectful computation yielding `T` |
| `HttpResult[T]` | see [HTTP](http.md) | an HTTP response |

`ValidationError` is the error type returned by refined-type `.of` constructors.

## The JSON codec

Two compiler-backed statics decode and encode JSON at a typed boundary:

| Form | Type | Purpose |
|---|---|---|
| `Json.encode(v)` | `String` | serialise a checked value to a JSON string |
| `Json.decode[T](s)` | `Result[T, JsonError]` | parse a JSON string into `T`, validating structure (and any refinements) |

`Json.decode[T]` takes an explicit type argument and validates the decoded value
against `T` — including refined-type predicates — so untrusted JSON enters the
program only as a fully-checked value. `JsonError` is the error it returns
(malformed JSON, or a structural/refinement mismatch). See the guide
[Decode untrusted JSON into a typed value](../guides/type-system/decode-json.md).

## Type aliases

```karn
type Id = Int
```

An alias introduces a distinct named type. Even a plain alias is branded in the
emitted TypeScript and carries `.of`/`.unsafe` constructors.

## Record types

A record groups named, immutable fields:

```karn
type Order = {
  id: String,
  item: String,
}
```

- **Construct** by naming every field: `Order { id: "1", item: "book" }`.
- **Access** with dot notation: `o.id`.
- **Update** with the spread form, which copies and overrides:
  `Order { ...o, item: "pen" }`.

Records emit a TypeScript `interface` with `readonly` fields. A record field may
not directly be of the record's own type (`karn.resolve.recursive_record_field`).

## Sum types

A sum type is one of several variants; a variant may carry a payload:

```karn
type Status =
  | Pending
  | Shipped(tracking: String)
  | Cancelled(reason: String)
```

An all-payloadless sum may also be written `enum { A, B, C }`.

- **Construct** by naming a variant: `Pending`, `Shipped("1Z…")`.
- **Consume** with [`match`](#matching) or [`is`](operators.md).

Sum types emit a discriminated union keyed on a `tag` field.

## Opaque types

An opaque type is backed by another type but is nominally distinct:

```karn
type OrderId = opaque String
```

- Construct only via `OrderId.of(...)` (checked, returns `Result`) or
  `OrderId.unsafe(...)` (unchecked); record syntax is rejected
  (`karn.resolve.opaque_record_construction`).
- Construction and inspection are confined to the defining module/context.
- Opaque types are **excluded** from [literal admission](refined-types.md).

## Refined types

A base type plus a predicate. See the [refined-type reference](refined-types.md).

## Matching

`match` branches on every variant of a sum/`Result`/`Option`, binding payloads:

```karn
match s {
  Pending => "…"
  Shipped(tracking: t) => t
  Cancelled(reason: r) => r
}
```

A `match` must be exhaustive (`karn.types.non_exhaustive_match`); a `match` is an
expression whose arms must share a type (`karn.types.match_arm_mismatch`).
