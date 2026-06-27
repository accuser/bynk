# 0128 — `Stream[T]`, the value-over-time primitive: a new built-in parametric *value* type (not an `Effect`), classified non-serialisable/non-storable/non-boundary/non-comparable like `Query`/`Effect`/`Fn`, with a minimal `Stream.of`/`map`/`take`/`collect` vocabulary lowering to a host async iterable

- **Status:** Accepted (real-time track, slice 0; 2026-06-27).
- **Provenance:** the first slice of the real-time / WebSocket feature track (`design/tracks/websocket.md`), authored with its slice per the track's steer. Consumes proposal `design/proposals/v0.100-stream-primitive.md`; settles that track's **Q1** (form + how much to commit), **Q3** (error/termination), and **Q4** (vocabulary breadth).
- **Realises:** the gap the design notes left implicit — the language could express a single deferred value (`Effect[T]`) and a snapshot read (`Query[T]`) but had no word for a value produced **incrementally over time**. `Stream[T]` is that word, and the seam the streaming-HTTP terminal (slice 1) and a future streaming `Ai` capability plug into.
- **Relates:** [ADR 0115](0115-query-model-lazy-eager-dispatch.md) (`Query[T]` — the by-reference, non-storable/non-boundary value type this is modelled on, almost line for line); [ADR 0031](0031-effect-non-storable.md) (`Effect` non-storable — the parallel "a live thing is not a value" precedent); [ADR 0030](0030-function-types-non-boundary.md) (function types non-boundary — the other parallel); [ADR 0116](0116-query-vocabulary-and-ordering.md) (the query combinator vocabulary whose minimal-then-grow discipline this follows, and whose D6 deprecation of `bynk.list` free functions steered the constructor to a static — see D7).

## Context

`Effect[T]` resolves exactly once; it cannot express a token feed from a model, a
server-sent progress stream, or any incremental response. `Query[T]` is a
snapshot-consistent read over a materialised storage source — bounded, with no
temporal dimension. Neither is a value *produced over time*. The real-time
track's streaming leg (a streaming-HTTP response body) and its eventual capability
leg (a streaming `Ai.generateText`) both need that primitive, and it is the
track's most hard-to-reverse commitment — a new core type constrains every later
consumer. So it is front-loaded as slice 0, settled once, behind a conformance
fixture, before any consumer is built.

The type system already classifies a family of **non-serialisable** values —
function types, `Effect`, `Query`, held resources — that are built and consumed in
place and may not be persisted, sent across a context boundary, or captured by a
serialised closure. A live IO source belongs in exactly that family. So the
slice is largely an *extension of existing passes*, not a new concept.

## Decisions

- **D1 — `Stream[T]` is a new built-in parametric *value* type, modelled on
  `Query[T]`.** A `TypeRef::Stream(Box<TypeRef>, Span)` (parser/AST) and a
  `Ty::Stream(Box<Ty>)` (checker), resolved/displayed/walked exactly as the other
  single-parameter builtins are. It is **not** a variant of `Effect` (it resolves
  many times, breaking `Effect`'s once-semantics) and **not** a `Query` (a stream
  is an unbounded temporal source with no snapshot). It is covariant in its
  element (`compatible`), so it is nameable, returnable from a pure helper, and
  passable within a context — like `Query`.

- **D2 — classified non-serialisable / non-storable / non-boundary /
  non-comparable, by one arm per existing site.** `Stream` joins
  `Fn`/`Effect`/`Query` in the `json_codable` rejection set, the `store`-field
  admission check, and the boundary-type validation — each an existing exhaustive
  `match` gaining a `Stream` arm. Two new diagnostics name the rejections in the
  type's own vocabulary: **`bynk.types.stream_at_boundary`** (a `Stream` in a
  storable or boundary-crossing position) and **`bynk.types.stream_not_comparable`**
  (a `Stream` compared with `==`/`!=`). The latter is needed precisely *because*
  `Stream` is assignable (`compatible`), which the `==` arm would otherwise accept
  — a deliberate guard kept narrow to `Stream` so `Effect`/`Fn` equality behaviour
  is untouched.

- **D3 — minimal vocabulary: `Stream.of` / `map` / `take` / `collect`.** The
  builders `map` (`(T -> U) -> Stream[U]`) and the bounded `take`
  (`Int -> Stream[T]`) stay lazy; the terminal **`collect` drains to
  `Effect[List[T]]`**. `collect` goes **one step beyond the track's stated
  "construction / `map` / `take`"** (proposal DECISION B): a builder-only slice is
  untestable — you cannot assert on a stream you cannot drain — so the minimal
  observation terminal ships now, stated openly. A fuller algebra
  (`filter`/`scan`/fan-in/merge) earns its own slice + ADR, as the query algebra
  did (Q4).

- **D4 — errors ride in-band as `Result` elements (Q3).** A stream that can yield
  failures is `Stream[Result[T, E]]` — a use of the type parameter, needing zero
  new machinery. An `Err` element is an **outcome** (matched like any `Result`); a
  **fault** in the producer aborts the stream as faults abort handlers. No separate
  completion/error channel.

- **D5 — keep the type parameter now, defer the element semantics (Q1).** The
  irreversible surface is the *parametric type*; retrofitting a parameter later is
  the disruptive move, so it is committed now. But the **element semantics** that
  only a non-chunk, boundary-crossing consumer pins — whether a stream element may
  itself cross a context boundary, and whether such an element must be serialisable
  — are **deferred** to the first consumer that needs `T ≠ Chunk` (a streaming `Ai`
  capability, or in-memory effectful iteration). Slice 0 never crosses a boundary
  with a stream element, and a `Stream` as a whole is non-boundary anyway, so the
  question is not forced here. This shrinks the genuinely irreversible commitment to
  what is exercised.

- **D6 — lowering: `Stream[T]` → a host async iterable, emitted inline.** A
  `Stream[T]` lowers to `AsyncIterable<T>`; `Stream.of(xs)` to an async generator
  over the list, `map`/`take` to async-generator wrappers, and `collect` to an
  async drain returning `Promise<T[]>` (an `Effect`, awaited with `<-`). Emission
  is **inline** (async-generator IIFEs), like the `Query`/`List` collection
  kernels — **no runtime-library import** — so non-stream files emit
  byte-identically and the strip-only invariant is preserved. (This refines the
  proposal's `runtime.ts`-helper sketch toward the established inline-kernel
  precedent.) On the test target the in-memory `of`→`collect` path is fully
  deterministic, so a handler's streamed output is assertable with no IO.

- **D7 — the constructor is the static `Stream.of(xs)`, not a `bynk.stream.*`
  free function.** The proposal's provisional spelling was `bynk.stream.of`; but
  ADR 0116 D6 deprecated the `bynk.list` free functions in favour of method/static
  forms, so a *new* `bynk.*` free function would introduce a deprecated-style
  surface. `Stream.of(xs)` mirrors the existing static constructors
  `Duration.millis(n)` / `Instant.fromEpochMillis(n)` — gated in the resolver and
  checker by the same `id.name == STREAM && not-in-scope && not-a-user-type`
  guard — and reads consistently with them. The element type is inferred from the
  `List[T]` argument.

## Consequences

- The type, its classification, and its vocabulary are settled and conformance-
  tested (positive fixture `233_stream_vocabulary` exercising `of`/`map`/`take`/
  `collect` and a `Stream[Result[T, E]]`; negatives `246_stream_at_boundary` and
  `247_stream_not_comparable`). Emitted output type-checks under `tsc --strict`.
- The streaming-HTTP response terminal (slice 1) and any later streaming capability
  cite this ADR and consume `Stream[T]` without re-deciding its form. The first
  consumer needing `T ≠ Chunk` settles the deferred element semantics (D5) in its
  own ADR.
- `Stream` is the first non-serialisable payload that is also **assignable**
  (`compatible`); the `==` guard (D2) is the seam that keeps "assignable" from
  leaking into "comparable". Should a future need arise to make `Effect`/`Query`/
  `Fn` uniformly non-comparable, a shared `value_comparable` predicate would
  generalise this guard — explicitly out of scope here to avoid changing their
  behaviour.
- The minimal vocabulary is a standing constraint: `filter`/`scan`/merge/fan-in and
  live runtime sources are deferred, named here so a later slice adds them behind
  its own ADR rather than rediscovering the boundary.
