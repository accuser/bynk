# 0120 — Joins and grouping take a **combiner**, not a pair type: `joinOn`/`leftJoin`/`join`/`groupBy` project each result through an `into:` lambda into a user-named type — bynk stays nominal

- **Status:** Accepted (query-algebra track, slice 4 settling; 2026-06-26)
- **Track:** `design/tracks/query-algebra.md` (the joins-&-grouping slice; resolves the result-representation gap left open by the vocabulary ADR). Closes track Q13 (how a join/group pair is represented in the language).
- **Realises:** `design/bynk-design-notes.md` §11 (joins and grouping in the one method-chain vocabulary), reconciled with bynk's nominal type discipline (no anonymous product types).
- **Relates:** ADR 0116 (the query vocabulary — this ADR **revises** its join/`groupBy` signature rows and D7); ADR 0119 (the DO lowering — its `(t, u)` pair-emission becomes the combiner's `V` projection; D5/D6); ADR 0118 (`@indexed` — an equi-`joinOn` key routes through a posting list); ADR 0110 D5 (the value-keyable constraint a `joinOn`/`groupBy` key reuses); ADR 0115 (the `Query[T]` model the builders hang off). The cross-shape `Map × Log` join (ADR 0119 D6) consumes this form when the storage `Log` slice lands.

## Context

ADR 0116 settled the query **vocabulary**, listing the joins and grouping with
the result types `join/joinOn -> (T, U)`, `leftJoin -> (T, Option[U])`, and
`groupBy -> (K, List[T])` (D7). ADR 0119 settled their **lowering** as hash joins
emitting `(t, u)` pairs. Both ADRs wrote `(T, U)` as if it were a type bynk has.

It is not. bynk has **no anonymous product type** — no tuple, no pair. Every type
is **nominal**: records are named, `Option`/`Result` are nominal generics, and the
language deliberately has nothing like `(A, B)`. The `(T, U)` in those ADRs was
notation that was never grounded in the type system; the result-representation was
left unsettled. Slice 4 cannot lower a join until it knows what a join row *is*.

Three ways to ground it were weighed (Alternatives): add a structural **tuple**
type; add a nominal **`Pair[A, B]`**; or **add no type** and have the join project
into a user-named result. This ADR takes the third — the combiner form — because
it is the only one that keeps the language nominal, is the smallest to build, and
is *better* on the merits (it composes multi-way joins flatly), not merely cheaper.

## Decisions

**D1 — No pair/tuple type is introduced; bynk stays nominal.** A join or group
result is **not** an anonymous product. The language gains no `(A, B)` type, no
tuple literal, and no `.0`/`.1` access. Anonymous structural products would be the
first structural type in an otherwise-nominal language — eroding the
"name your data" discipline (`(String, Int, Bool)` is less self-documenting than a
named record) and competing with records for no gain a single slice justifies. A
general n-ary tuple remains a **named deferral**: if broad demand for anonymous
products appears across *several* features later, it is reconsidered then, as its
own deliberate language decision — not one cornered by joins.

**D2 — Joins take an `into:` combiner and yield `Query[V]` (storage) / `List[V]`
(in-memory).** Each join builder carries a final projector that names the result:

| builder | signature |
|---|---|
| `joinOn(other, left: T -> K, right: U -> K, into: (T, U) -> V)` | equi-join → `…[V]` |
| `leftJoin(other, left: T -> K, right: U -> K, into: (T, Option[U]) -> V)` | left outer → `…[V]` |
| `join(other, on: (T, U) -> Bool, into: (T, U) -> V)` | predicate (nested-loop) → `…[V]` |

The builder's **element type is `V`**, inferred from `into`'s return; the join row
`(t, u)` exists only *inside* `into`, never as a value. This **supersedes** ADR
0116's `join/joinOn -> (T, U)` and `leftJoin -> (T, Option[U])` rows. `other` is a
collection / `Query` whose element is `U`; the join key `K` for `joinOn`/`leftJoin`
is **value-keyable** (the Map-key rule, ADR 0110 D5) so it can hash — a
non-keyable key is a diagnostic; `join`'s `on` is any `Bool` predicate (no key, so
nested-loop).

**D3 — `groupBy` takes an `into:` combiner too; it materialises to `List[V]`.**
`groupBy(key: T -> K, into: (K, List[T]) -> V) -> Query[V]` (storage) / `List[V]`
(in-memory), with `K` **value-keyable** (ADR 0110 D5). This **supersedes** ADR
0116 D7: the builder yields `V`, not `(K, List[T])`, and `.collect` yields
`List[V]`, not `Map[K, List[T]]` — the user shapes each group through `into`
(e.g. `into: (oid, rows) => OrderSummary { id: oid, total: rows.sum(r => r.qty) }`),
which is both the common case and uniform with the join builders. Group encounter
order stays **deterministic** (first-seen key order). A grouped query is
materialising, like a join (it must see every row to partition).

**D4 — The combiner composes multi-way joins flatly and runs per emitted row.**
Because each join yields a named `V`, a chain `a.joinOn(b, …, into: f).joinOn(c, …,
into: g)` stays flat and named — no `(( T, U), W)` nesting and no `.0.0`. `into`
runs **once per emitted pair**: for `joinOn`/`join`, per matched `(t, u)`; for
`leftJoin`, per left row with `(t, Some(u))` for each match and `(t, None)` for an
unmatched left row (ADR 0119 D5). The lowering is unchanged from ADR 0119 except
that the hash-join/nested-loop emits `into(t, u)` in place of a bare pair, so an
equi-`joinOn` whose probed key is `@indexed` still routes through the posting list
(ADR 0118).

**D5 — Scope: `Map × Map` / `Query × Query` joins and `groupBy` land in slice 4;
cross-shape `Map × Log` is deferred with the `Log` slice.** The combiner form
applies to every join, but the cross-shape `Map × Log` join (ADR 0119 D6) needs a
`Log` storage kind, which is **not built** (storage track, slice 4 — sequenced
*after* this track). So slice 4 here delivers same-shape joins (`Map`/`Query`
against `Map`/`Query`) and `groupBy`; the `Log` time-window builders and the
cross-shape join land when `Log` does, reusing this exact `into:` form unchanged.

## Implementation note (v0.94)

The signatures above name the arguments (`left:`/`right:`/`into:`) to convey
each lambda's **role**. bynk does not yet have **labelled call arguments** (only
annotations carry labels), and — by the same discipline that rejects a tuple here
(D1) — slice 4 does not add them just for joins. So the v1 surface passes these
**positionally**: `joinOn(other, leftFn, rightFn, intoFn)`,
`leftJoin(other, leftFn, rightFn, intoFn)`, `join(other, onFn, intoFn)`,
`groupBy(keyFn, intoFn)`. The order is type-checked — a `joinOn` whose `left`/
`right` are swapped is a type error (the key functions take `T` vs `U`) except in
a self-join (`T == U`). **Labelled call arguments** are a clean, purely additive
future feature that would realise the named surface above; they are a named
deferral, not a blocker.

## Consequences

- **Surface.** `joinOn`/`leftJoin`/`join`/`groupBy` each gain a trailing combiner
  (`into`, positional in v1 — see the implementation note); there is no pair type
  to name, destructure, or print. The
  §11/ADR 0116 examples that wrote `(T, U)` are rewritten to the combiner form
  (`design/tracks/query-algebra.md` §3 surface, the spec query section).
- **Checker.** The four builders join the `Query`/collection signature tables
  (ADR 0116), generic in `T`/`U`/`K`/`V`, resolving `V` from `into`'s return and
  constraining the join/group key `K` to value-keyable (ADR 0110 D5) for
  `joinOn`/`leftJoin`/`groupBy`. Diagnostics reuse the keyable-key family; an
  `into`/`on`/key arity or type error is the ordinary argument check.
- **Emission.** ADR 0119's hash-join / nested-loop / partition lowerings stand;
  the only change is the emitted element — `into(t, u)` / `into(k, rows)` — so the
  result is `V` directly, with no intermediate pair allocation. An equi-`joinOn`
  on an `@indexed` key probes the posting list (ADR 0118).
- **Deferred (named):** a general n-ary tuple / anonymous products (D1); the
  cross-shape `Map × Log` join and `Log` time-window builders (D5, gated on the
  storage `Log` slice); the correlated-`flatMap`→hash-join rewrite (ADR 0119 D4).

## Alternatives considered

- **A structural tuple type `(A, B, …)`.** Rejected (D1): it matches the ADR 0116
  notation and is broadly reusable (multi-return, zip), but it is a large build
  (literal `(a, b)` syntax colliding with unit `()` and parenthesised expressions,
  pattern destructuring, `.0`/`.1`, tree-sitter, fmt, grammar drift) **and** a
  philosophical shift — the first structural type in a nominal language. Too large
  a commitment to be forced by one slice; left as a deferral if broad demand
  appears.
- **A nominal `Pair[A, B]`.** Rejected (D1): cheap (reuses the `Option`/`Result`
  machinery, no new syntax) but a **narrow, single-purpose** type that essentially
  only serves joins, reads awkwardly (`.left`/`.right`), and **nests** on chained
  joins (`Pair[Pair[A, B], C]` → `.left.left`). The worst of both — a new type with
  one use.
- **Joins materialise straight to a named record the user declares ahead.**
  Rejected: a join result has no name to declare *ahead* of the join; the combiner
  supplies the name *at* the join, which is the same information without a forward
  declaration.
