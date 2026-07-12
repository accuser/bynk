---
title: Type system
---
## Built-in base types

| Type | Values | Emits |
|---|---|---|
| `Int` | integer literals (`0`, `-42`) | `number` |
| `Float` | float literals (`1.5`, `0.0`, `-3.14`) | `number` |
| `String` | string literals (`"…"`) | `string` |
| `Bool` | `true`, `false` | `boolean` |
| `Duration` | unit literals (`5.minutes`, `30.seconds`) | `number` (millis) |
| `Instant` | no literal — `Clock.now()` / `Instant.fromEpochMillis(n)` | `number` (epoch millis) |
| `Bytes` | no literal — `Bytes.fromUtf8(s)` / `Bytes.fromBase64(s)` / `Bytes.empty()` | `Uint8Array` |

The unit type is written `()`. `Int` and `Float` are **distinct and
incompatible** — there is no implicit coercion (`bynk.types.no_numeric_coercion`).
Convert explicitly: `i.toFloat()` (Int → Float, total) or `f.round()` /
`f.floor()` / `f.ceil()` / `f.truncate()` (Float → Int); parse a string with
`Int.parse(s)` / `Float.parse(s)`, each returning `Option`.

### Duration

`Duration` (v0.86) is a **span of time**, erased to a `number` of milliseconds. A
`Duration` literal is `<int>.<unit>` over a closed unit set — `5.minutes`,
`30.seconds`, `1.hours`, `2.days`, `100.milliseconds`. The operator surface is
`Duration ± Duration`, `Duration * Int` / `Int * Duration` (scalar scaling), and
`Duration` comparison (subtraction is unclamped — may go negative). Convert
explicitly: `d.toMillis() -> Int` and the static `Duration.millis(n: Int) ->
Duration`. It round-trips through the JSON codec as an integer. See
[Operators](/book/reference/operators/#duration--instant-arithmetic).

### Instant

`Instant` (v0.90) is an **absolute point in time**, erased to a `number` of Unix
epoch milliseconds. It has **no literal**: an `Instant` is minted by `Clock.now()`
(typed `Effect[Instant]`) or built from an `Int` via `Instant.fromEpochMillis(n)`.
Arithmetic composes with `Duration`: `Instant ± Duration -> Instant`
(advance/retreat) and `Instant - Instant -> Duration` (the span between).
Comparison is chronological and `Instant` is **orderable** (so `sortBy`/`min`/`max`
key on it) but **not numeric** (`sum`/`average` reject it). The escape to raw
millis is `t.toEpochMillis() -> Int`; the zero is the epoch. Timestamp math goes
**through `Instant`** — `now + 5.minutes` is `Instant + Duration`; the former
`Int + Duration -> Int` clock-math coercion was withdrawn at v0.90, so every
`Instant`↔`Int` mix is a `no_numeric_coercion` error. See
[Operators](/book/reference/operators/#duration--instant-arithmetic).

### Bytes

`Bytes` (v0.110) is an **immutable octet sequence** — the type for arbitrary
binary data that `String` (UTF-8 text) cannot hold without corruption. It is the
one base type that does **not** emit to `number`: a `Bytes` erases to a host
`Uint8Array`. There is **no literal**; construct a `Bytes` with:

- `Bytes.fromUtf8(s: String) -> Bytes` — the UTF-8 encoding of a string (total).
- `Bytes.fromBase64(s: String) -> Option[Bytes]` — decode base64; `None` on an
  invalid string.
- `Bytes.empty() -> Bytes` — the zero value (the empty sequence).

The usable surface is `b.length() -> Int` (the octet count), `b.toBase64() ->
String` (total), and `b.decodeUtf8() -> Option[String]` (`None` on an invalid
UTF-8 sequence). Encoding (text → bytes) is total; decoding (bytes → text) is
partial and surfaced as `Option`, never hidden.

`==`/`!=` compare **by content**, byte for byte — so two independently-built
`Bytes` with the same octets are equal (unlike the number-erased base types, a
`Bytes` is not compared by host reference). A record carrying a `Bytes` field
gets correct equality by comparing that field with `==` in a hand-written
comparator. `Bytes` is **not orderable** (no `<` / `sortBy` key) and **not
`Map`-keyable** — key on `b.toBase64()` (a `String`) instead. It has no
arithmetic, concatenation, or slicing in v1.

On the wire a `Bytes` **serialises as a base64 JSON string** (and deserialising
requires a valid base64 string), so it round-trips through any record or `store`
field and crosses a `bundle` context boundary — a fully ordinary serialisable
value, the opposite of a `Stream`. (One current limit: a bare `Bytes` directly in
a `workers` cross-context signature is diagnosed as not-yet-supported — put it
inside a record, whose typed codec base64-encodes it, or build with `--target
bundle`.)

## Built-in generic types

| Type | Variants | Purpose |
|---|---|---|
| `Result[T, E]` | `Ok(T)`, `Err(E)` | success or error |
| `Option[T]` | `Some(T)`, `None` | a value or nothing |
| `Effect[T]` | — | an effectful computation yielding `T` |
| `HttpResult[T]` | see [HTTP](/book/reference/http/) | an HTTP response |
| `Stream[T]` | — | a value-over-time source (see [Stream](#stream)) |
| `Query[T]` | — | a lazy read over `store` storage (see [Query](#query)) |

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

A stream's first end-to-end use is a [**streamed HTTP response**](/book/reference/http/#streamed-responses)
— `Streaming(stream)` returns an SSE body consuming a `Stream[String]`. A richer
combinator vocabulary and live runtime sources are later increments of the
real-time track.

## Query

`Query[T]` (v0.92; ADRs 0115/0119) is a **lazy read over a `store`'s storage** —
the lazy receiver of the same combinator vocabulary the eager [`List`
methods](#list-methods) carry, dispatched by **receiver provenance**: a chain
rooted in a `store reservations: Map[K, V]` field is a `Query`, while the same
names on an in-memory `List` are eager. Like `Effect`/`Fn`/`Stream` it is
**non-storable and non-boundary** — rejected in any storable or boundary position
(`bynk.types.query_at_boundary`) — but is otherwise first-class: nameable,
returnable from a pure helper, passable. A query is **agent-local** and reads
**staged** state (read-your-writes).

**Builders** are pure and return a further `Query` — `filter`, `map`, `flatMap`,
`sortBy`, `take`, `skip`, `distinct`, plus the joins and `groupBy` below.

**Terminals** execute the query and are `Effect`-typed (awaited with `<-`), folding
into the storage capability the `store` fields already carry (no new `given`):

| Terminal | Result |
|---|---|
| `.collect()` | `Effect[List[T]]` |
| `.first()` | `Effect[Option[T]]` |
| `.count()` | `Effect[Int]` |
| `.sum(key)` / `.min(key)` / `.max(key)` / `.average(key)` | `Effect[…]` (empty-total: `Option`, or the zero for `sum`) |
| `.any(p)` / `.all(p)` | `Effect[Bool]` |
| `.fold(init, f)` | `Effect[acc]` |
| `.forEach(f)` | `Effect[()]` |

### Joins and grouping

Joins and grouping (v0.92+; ADR 0120) take an **`into:` combiner** that projects
each result through a lambda into a **user-named type** — bynk has no anonymous
pair/tuple, so a join row is always a named record. The arguments are positional
(`left:`/`right:`/`into:` name them for readability):

| Form | Yields |
|---|---|
| `joinOn(other, left: T -> K, right: U -> K, into: (T, U) -> V)` | equi-join → `…[V]` |
| `leftJoin(other, left: T -> K, right: U -> K, into: (T, Option[U]) -> V)` | left outer → `…[V]` |
| `join(other, on: (T, U) -> Bool, into: (T, U) -> V)` | predicate (nested-loop) → `…[V]` |
| `groupBy(key: T -> K, into: (K, List[T]) -> V)` | grouping → `…[V]` |

Each yields a `Query[V]` over storage and a `List[V]` eagerly. Because every result
is a named `V`, chained joins stay flat and named — no nested pairs. An equi-`joinOn`
whose probed key is [`@indexed`](/book/reference/agents/) routes through the index.

## List methods

`List[T]` (v0.88; ADR 0116) carries the query algebra's **eager, in-memory**
combinator vocabulary as kernel methods, so a chain reads
`xs.filter((x) => x > 2).map((x) => x * 2)` (the same names the lazy
[`Query`](#query) carries over storage; the receiver decides eager vs lazy).

**Builders** (return a `List`): `map`, `filter`, `flatMap`, `sortBy`, `take`,
`skip`, `distinct`, `distinctBy`.

**Terminals**: `count`, `any`, `all`, `first`, `firstOrElse`, `sum`, `min`, `max`,
`average`.

Ordering keys (`sortBy`/`min`/`max`) come from the closed orderable base set —
`Int`/`Float`/`String`/`Duration`/`Instant`, refined types widening, opaque keys
rejected (`bynk.types.key_not_orderable`). Numeric keys (`sum`/`average`) are
`Int`/`Float`/`Duration` (`bynk.query.sum_needs_numeric`), with `average -> Float`.
**Empty aggregates are total** — `first`/`min`/`max`/`average` return `Option`,
`sum` the zero. The first-party `bynk.list` free functions are the deprecated
predecessors of these methods (see [Operators & built-ins](/book/reference/operators/) and
[First-party `bynk` capabilities](/book/reference/bynk-capabilities/)).

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

### WebSocket services

> The full protocol surface — the `on open` / `on message` / `on close` handlers,
> edge authentication, broadcast over a held `Map`, the `TestConnection` model, and
> the platform mapping — is on the [WebSocket reference page](/book/reference/websocket/); the
> worked chat-room is the guide [Handle a WebSocket connection](/book/guides/entry-points/websocket/).
> This section summarises how a connection is produced.

A `service … from WebSocket(in:, out:)` produces connections. The upgrade
**authenticates at the edge** — like an HTTP route, `on open` must name its actor
with `by` (there is no anonymous upgrade; a browser `WebSocket` carries a Bearer
token in the `Sec-WebSocket-Protocol` subprotocol, since it cannot set an
`Authorization` header) — and the handler receives a fresh, owned `Connection[out]`
it must dispose, the canonical disposal being transfer into an agent:

```bynk
service ChatGateway from WebSocket(in: ClientFrame, out: ServerFrame) {
  on open (roomId: RoomId) -> Effect[()] by user: Participant {
    let _ <- connection.send(ServerFrame { text: "welcome" })
    let _ <- Room(roomId).join(user.identity, connection)
    ()
  }
}
```

The service holds **exactly one** `on open`; inbound frames then arrive at the
agent that owns the connection through the explicit `on message` / `on close`
handlers, and the agent fans frames out to many connections by holding them in a
`Map` and broadcasting over it. On the **bundle** target the connection is a
`TestConnection` — a capture-and-inspect channel that records every frame sent — so
a WebSocket service is fully developable and testable with no Durable Object. On
the **Workers** target the connection maps onto a Durable Object using the
hibernatable-WebSocket API: a `Connection` stored in agent state survives
hibernation and is restored on rehydration.

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
[Decode untrusted JSON into a typed value](/book/guides/type-system/decode-json/).

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
- **Consume** with [`match`](#matching) or [`is`](/book/reference/operators/).

Sum types emit a discriminated union keyed on a `tag` field.

### Error embeddings (v0.154)

A sum used as an error type may declare **embeddings** — a trailing
`embeds <type> as <Variant>` clause naming a single-payload variant that wraps a
sub-error. The [`?` operator](/book/reference/operators/) then converts that
sub-error automatically, replacing the boilerplate `.mapErr(Wrap)` on every
cross-context chain:

```bynk,ignore
type OrderError =
  | OutOfStock(sku: Sku)
  | Payment(reason: PaymentError)
  | Fulfilment(reason: ScheduleError)
  embeds PaymentError as Payment, ScheduleError as Fulfilment
```

Each `embeds E as V` requires `V` to be a variant of the same sum with exactly
one payload field of type `E`, and a given `E` may be embedded by only one
variant (so the conversion is unambiguous). The conversion is **one level**: `?`
converts a `Result[T, E]` into `Result[_, F]` only when `F` declares `E`
directly. Mapping a domain error to an *HTTP* status stays an explicit `match`
in the handler — by design, not an embedding.

## Opaque types

An opaque type is backed by another type but is nominally distinct:

```bynk
type OrderId = opaque String
```

- Construct only via `OrderId.of(...)` (checked, returns `Result`) or
  `OrderId.unsafe(...)` (unchecked); record syntax is rejected
  (`bynk.resolve.opaque_record_construction`).
- Construction and inspection are confined to the defining module/context.
- Opaque types are **excluded** from [literal admission](/book/reference/refined-types/).

## Refined types

A base type plus a predicate. See the [refined-type reference](/book/reference/refined-types/).

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
