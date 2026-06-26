# 0116 — the closed query/collection vocabulary (builders + terminals); ordering via a closed orderable-base set; empty-aggregate results are `Option`; `bynk.list` free functions migrate to methods

- **Status:** Accepted (query-algebra track, slice 0 settling; 2026-06-25)
- **Provenance:** the query-algebra track (slice 0 — the builder/terminal vocabulary + `Ordering` ADR). Settles track Q5 (`Ordering`), Q6 (`bynk.list` reconciliation), and Q11 (numeric/aggregate terminals); records the `flatMap` signature duality (Q9's checker half).
- **Realises:** `design/bynk-design-notes.md` §11 (the builder/terminal vocabulary, `sortBy`/`min`/`max` over an ordering, the one method-chain surface replacing scattered `bynk.list.*` calls).
- **Relates:** ADR 0115 (the `Query[T]` model and lazy/eager dispatch this vocabulary's signatures hang off); ADR 0114 (`Instant` joins the orderable base set); ADR 0048 (combinators as a *closed* kernel-method set — no user-defined query methods); ADR 0034/0036/0037 (the `bynk.list` hybrid posture and the call-surface question Q6 closes); ADR 0110 D5 (the value-keyable constraint `distinctBy`/`groupBy` keys reuse).

## Context

ADR 0115 fixed *what* `Query[T]` is and *how* lazy/eager dispatch routes a chain.
This ADR fixes the **vocabulary** that rides on it: the closed set of builders and
terminals, their signatures (generic in the element/key types, uniform across the
lazy storage and eager in-memory receivers), and the three sub-questions §11 left
open — how `sortBy`/`min`/`max` order a key (the language has no `Ordering`
concept), what the numeric/aggregate terminals return (and on an empty
collection), and what becomes of the existing `bynk.list` free functions now that
§11 wants one method-chain surface.

## Decisions

**D1 — A closed builder + terminal vocabulary, shared by both receivers.** Like
the kernel-method posture (ADR 0048), the set is **closed** — no user-defined query
combinators in v1. The same names appear on a lazy storage chain (builders ->
`Query[T]`, terminals -> `Effect[T]`; ADR 0115 D3) and an eager in-memory chain
(builders -> the collection, terminals -> `T`).

*Builders* (return `Query[T]` lazily / the collection eagerly):

| builder | shape |
|---|---|
| `filter(p: T -> Bool)` | select |
| `map(f: T -> U)` | transform |
| `flatMap(f: T -> Query[U] \| List[U])` | bind (D5) |
| `sortBy(f: T -> K)` | order by an orderable key `K` (D2) |
| `take(n: Int)` / `skip(n: Int)` | limit / offset |
| `distinct` | dedupe (structural-equality `T`) |
| `distinctBy(f: T -> K)` | dedupe by a value-keyable key |
| `join(other, on: (T, U) -> Bool)` | `-> (T, U)` predicate join |
| `joinOn(other, left: T -> K, right: U -> K)` | `-> (T, U)` equi-join (index-eligible) |
| `leftJoin(other, on: …)` | `-> (T, Option[U])` |
| `groupBy(f: T -> K)` | `-> (K, List[T])` partition (D7) |

*Terminals* (return `Effect[T]` on storage / `T` in-memory):

| terminal | result |
|---|---|
| `collect` | `List[T]` |
| `first` | `Option[T]` (D4) |
| `firstOrElse(default: T)` | `T` |
| `count` | `Int` |
| `fold(init: U, f: (U, T) -> U)` | `U` |
| `sum` / `min` / `max` / `average` | numeric/ordered aggregate (D3/D4) |
| `any(p: T -> Bool)` / `all(p: T -> Bool)` | `Bool` (short-circuit) |
| `forEach(f: T -> Effect[Unit])` | `Unit` |

**D2 — `Ordering` via a closed orderable-base set, not a typeclass (Q5).**
`sortBy(f: T -> K)`, `min(f: T -> K)`, `max(f: T -> K)` order by the **key** `K`
the projection yields, where `K` is drawn from the closed **orderable base set**:
`Int`, `Float`, `String` (lexicographic), `Duration`, `Instant` (chronological;
ADR 0114 D8). A refined type widens to its base for ordering. A non-orderable key
(a record, a tuple, `Bool`, `Option`) is `bynk.types.key_not_orderable`. `sortBy`
is **ascending**; descending is `sortBy(…)` then a reverse (`Log.reversed`, or a
deferred `sortByDescending`). We **reject** a general `Ordering`/typeclass
mechanism for v1 — the language has no typeclasses, the base set covers the target
workloads, and a typeclass is a far larger commitment than this slice needs. (A
multi-key sort and a custom comparator are named deferrals.)

**D3 — Numeric/aggregate result types (Q11).** `sum` requires a **numeric** key
(`Int`/`Float`/`Duration`) and returns that type (`Int -> Int`, `Float -> Float`,
`Duration -> Duration`). `min`/`max` return the projected **orderable** key type
`K` (D2), wrapped per D4. `average` is defined on a numeric key and returns
**`Float`** for `Int`/`Float` keys (so integer averages do **not** truncate) and
`Duration` for a `Duration` key (millis averaged, integer-rounded). All three take
the projection `f: T -> K` (e.g. `reservations.sum(r => r.qty)`).

**D4 — Empty-collection results are `Option`; `sum`/`count`/`fold` have an
identity (Q11). This must be settled now, not at the storage slice.** Because a
storage query learns emptiness only by *executing*, the terminal's result type
cannot differ between the in-memory and storage receivers — so the empty case is
fixed at the type:

- `first -> Option[T]`, `min -> Option[K]`, `max -> Option[K]`,
  `average -> Option[Float]` (or `Option[Duration]`) — **total**, `None` on empty;
- `sum -> 0 / 0.0 / 0.milliseconds`, `count -> 0`, `fold -> init` — the identity,
  no `Option` (an empty sum is unambiguously the zero);
- `firstOrElse(default) -> default`; `any -> false`, `all -> true` (vacuous).

**D5 — `flatMap` signature duality (Q9's checker half).** `flatMap`'s lambda
return type flips with the receiver provenance (ADR 0115 D3): storage-rooted,
`flatMap(f: T -> Query[U]) -> Query[U]`; in-memory, `flatMap(f: T -> List[U]) ->
List[U]`. The checker dispatches the lambda's **expected return type** by
provenance, not just the builder's result. The storage bind's *lowering* (a
correlated scan vs a join rewrite) is a separate concern, deferred to the lowering
ADR / slices 2–4 (track Q9).

**D6 — `bynk.list` free functions migrate to methods, with a `bynk-fmt` codemod
(Q6).** The combinators become **kernel methods** on `List` (and value
`Map`/`Set`), delivering §11's one method-chain vocabulary. The existing
`bynk.list.*` free functions (`map`/`filter`/`find`/`any`/`all`/`traverse`) are
**deprecated** — they emit a deprecation diagnostic during a transition window and
are then removed — and a `bynk-fmt` codemod rewrites call sites (the `state→store`
codemod precedent, ADR 0108). `find(p)` is expressed as `filter(p).first` (no
separate `find` terminal; the codemod rewrites it). The **effectful iteration**
methods `traverse`/`traverseAll`/`parTraverse`/`parTraverseAll` (track slice 5) are
**eager `List` methods, not query builders** — they operate on already-collected
in-memory lists, dispatched on the function's `Result`-ness (§11), and never appear
on `Query[T]` (a collected result has left the lazy domain, ADR 0115 D4).

**D7 — `groupBy` materialisation.** `groupBy(f: T -> K) -> Query[(K, List[T])]`
(storage) / `List[(K, List[T])]` (in-memory), with `K` **value-keyable** (the
Map-key constraint, ADR 0110 D5). A `.collect` on a grouped query materialises to
`Map[K, List[T]]`. Group encounter order is deterministic (first-seen key order).

## Consequences

- **Checker.** The builder/terminal signature tables, generic in the element and
  key types, resolved on both an in-memory collection receiver and a `Ty::Query`
  receiver (ADR 0115 D3); the orderable-base-set rule for `sortBy`/`min`/`max`
  (D2) keying off the projection type and including `Instant` (ADR 0114 D8); the
  numeric/aggregate result and empty-case typing (D3/D4); the `flatMap` provenance
  dispatch (D5); the `groupBy` key constraint (D7). New diagnostics:
  `bynk.types.key_not_orderable`, `bynk.query.sum_needs_numeric`, and the
  `bynk.list` deprecation warning (D6).
- **Stdlib + tooling.** The combinators land as kernel methods (the slice-1
  surface), with the `bynk.list` free functions deprecated and the `bynk-fmt`
  codemod (D6); LSP completion/hover/signature for the vocabulary on both
  receivers; the book/spec §11 pages.
- **Emission.** Eager in-memory chains lower to TS array/object ops; lazy storage
  chains defer to the lowering ADR (this ADR fixes only the surface and result
  types). `average`'s `Float`/`Duration` result and the empty-`Option` cases are
  the same shape in-memory and over storage (D4), so the lowering inherits them.
- **Scope held / named.** Deferred (named, not dropped): `sortByDescending` /
  multi-key sort / custom comparators (D2); a typeclass `Ordering` (D2); the
  storage `flatMap` lowering and cross-shape joins (D5; a later ADR); `@indexed`
  routing (a later ADR).

## Alternatives considered

- **A first-class `Ordering`/typeclass.** Rejected (D2): no typeclass machinery
  exists; the closed orderable base set covers v1 at a fraction of the cost. A
  forward-compatible future refinement, not this slice.
- **`min`/`max`/`average` of an empty collection faulting or returning a default.**
  Rejected (D4): a fault makes a total query partial; an arbitrary default lies.
  `Option` is the honest total result, and identical in-memory and over storage.
- **`average -> Int` for an `Int` key.** Rejected (D3): truncation is a silent
  precision loss; `Float` is the correct mean type.
- **Coexisting `bynk.list` free functions and methods.** Rejected (D6): two ways
  to do the same thing dilutes the one uniform vocabulary §11 specifies; the
  codemod makes migration mechanical.
- **A `find(p)` terminal.** Rejected (D6): `filter(p).first` is the same thing in
  the combinator vocabulary; a dedicated `find` is redundant surface.
