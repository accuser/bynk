# Type system

## Built-in base types

| Type | Values | Emits |
|---|---|---|
| `Int` | integer literals (`0`, `-42`) | `number` |
| `Float` | float literals (`1.5`, `0.0`, `-3.14`) | `number` |
| `String` | string literals (`"…"`) | `string` |
| `Bool` | `true`, `false` | `boolean` |

The unit type is written `()`. `Int` and `Float` are **distinct and
incompatible** — there is no implicit coercion (`bynk.types.no_numeric_coercion`).
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
| `Stream[T]` | — | a value-over-time source (see [Stream](#stream)) |

`ValidationError` is the error type returned by refined-type `.of` constructors.

## Stream

`Stream[T]` (v0.100) is a **lazy, pull-shaped sequence of values produced over
time** — the primitive for incremental output, distinct from `Effect[T]` (which
resolves exactly once) and `Query[T]` (a snapshot read over storage). Like those
neighbours it is **non-serialisable, non-storable, non-boundary, and not
value-comparable**: a live source is built and consumed in place, never persisted,
sent across a context boundary, or compared with `==`.

The v1 vocabulary is deliberately minimal:

| Form | Type | Purpose |
|---|---|---|
| `Stream.of(xs)` | `List[T] -> Stream[T]` | build a stream from a list (the deterministic source) |
| `s.map(f)` | `(T -> U) -> Stream[U]` | lazily transform each element |
| `s.take(n)` | `Int -> Stream[T]` | bound the stream to the first `n` elements |
| `s.collect()` | `Effect[List[T]]` | drain the stream to a list (the terminal) |

Errors ride **in-band** as `Result` elements (`Stream[Result[T, E]]`); a fault in
the producer aborts the stream as faults abort handlers.

A stream's first end-to-end use is a [**streamed HTTP response**](http.md#streamed-responses)
— `Streaming(stream)` returns an SSE body consuming a `Stream[String]`. A richer
combinator vocabulary and live runtime sources are
later slices of the [real-time track](https://github.com/accuser/bynk/blob/main/design/tracks/websocket.md).

## Connection

`Connection[F]` (v0.102) is a **held resource** — a typed handle to a long-lived
WebSocket connection, where `F` is the type of frames the server can send. It is
the one concrete instance of the closed **`Held`** kind. Held values are
**runtime-produced** (there is no constructor — they arrive from a capability
operation or a handler parameter the framework supplies) and governed by an
**ownership discipline** (the *linearity* rules, §2.9): a held value has at most
one owner, and must be **disposed** — stored, closed, or transferred — before its
scope exits.

| Operation | Type | Notes |
|---|---|---|
| `c.send(f)` | `F -> Effect[()]` | write a frame; **non-consuming** (the binding stays owned) |
| `c.close()` | `Effect[()]` | end the connection; **consuming** (the binding is spent) |

Held values are **non-serialisable, non-boundary, and not value-comparable** —
they may not cross a context boundary, be compared with `==`, or be stored except
in `Cell[Option[Connection]]` / `Map[K, Connection]` (a `Set`/`Log`/`Cache`
rejects them). Storing one (`conns.put(u, c)`) or closing it (`c.close()`) disposes
it; using it afterward, or letting it escape a handler undisposed, is a compile
error. The compiler reports an undisposed connection (`bynk.held.leak`), a use after
disposal (`bynk.held.use_after_consume`), and branches that dispose inconsistently
(`bynk.held.branch_divergence`).

The `from WebSocket` protocol that produces real connections — and the runtime
`Connection` implementations — arrive in a later slice of the
[real-time track](https://github.com/accuser/bynk/blob/main/design/tracks/websocket.md);
v0.102 builds the type and the discipline against hand-written held sources.

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

```bynk
type Id = Int
```

An alias introduces a distinct named type. Even a plain alias is branded in the
emitted TypeScript and carries `.of`/`.unsafe` constructors.

## Record types

A record groups named, immutable fields:

```bynk
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
not directly be of the record's own type (`bynk.resolve.recursive_record_field`).

## Sum types

A sum type is one of several variants; a variant may carry a payload:

```bynk
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

```bynk
type OrderId = opaque String
```

- Construct only via `OrderId.of(...)` (checked, returns `Result`) or
  `OrderId.unsafe(...)` (unchecked); record syntax is rejected
  (`bynk.resolve.opaque_record_construction`).
- Construction and inspection are confined to the defining module/context.
- Opaque types are **excluded** from [literal admission](refined-types.md).

## Refined types

A base type plus a predicate. See the [refined-type reference](refined-types.md).

## Matching

`match` branches on every variant of a sum/`Result`/`Option`, binding payloads:

```bynk
match s {
  Pending => "…"
  Shipped(tracking: t) => t
  Cancelled(reason: r) => r
}
```

A `match` must be exhaustive (`bynk.types.non_exhaustive_match`); a `match` is an
expression whose arms must share a type (`bynk.types.match_arm_mismatch`).
