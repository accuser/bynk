# Query-algebra track — reading and transforming agent-local data

Persistent design doc for the **query algebra** of `design/bynk-design-notes.md`
§11, the feature-track artefact of
[ADR 0076](../decisions/0076-feature-track-posture.md). It realises and sharpens
§11; the design notes stay the north star. It is the **sibling track** the
storage track (`design/tracks/storage.md`) sequences *before* its `Set`/`Log`
slices — `Log`'s time-window reads and `Map`'s deferred `@indexed` both live
here, and storage's lazy read surfaces produce the `Query[T]` this track defines.

**Trigger (ADR 0076):** multi-increment ✔ (the `Query[T]` type, the builder and
terminal vocabularies, the `@indexed` indexing model, joins/grouping, the
in-memory effectful iterators, the `Log` time-window builders — ~5–7 slices) and
surface-not-yet-settled ✔ (the lazy/eager dispatch rule, the indexing model and
its compiler/runtime split, the `Ordering` story for `sortBy`/`min`/`max`, the
Durable-Object lowering of scans-vs-indexes, and the cross-shape join model are
all open). **Not a security boundary** — agent-locality is an architectural
correctness property, not an authn/authz gate — so no `/security-review` gate;
but query→index routing soundness and the atomicity of index maintenance are a
**correctness boundary**, so each slice runs `/code-review`.

## 1. Conceptual model (sharpened from §11)

The query algebra is **one combinator vocabulary** for reading and transforming
data, shared by agent-local storage and in-memory collections. The same names
(`filter`, `map`, `sortBy`, `take`, …) appear on both; **the receiver's type
decides evaluation timing**:

- a chain against a **`store` field** (`Map`/`Set`/`Log`) is **lazy** — it builds
  a `Query[T]`, a computational description; nothing touches storage until a
  **terminal** (`collect`/`first`/`count`/…) executes it, with the storage-read
  effect (`-> Effect[T]`);
- a chain against an **in-memory value** (`List`, and value `Map`/`Set`) is
  **eager** — each method returns its result immediately (`-> T`, no effect).

This lines up two splits the language already keeps: **pure-build / effectful-
execute** matches **lazy / eager**. Building a query is a pure function returning
`Query[T]`; terminating one against storage is effectful. The reader sees from the
receiver's type which world they are in — exactly the receiver-provenance
discipline the storage track settled for `Map` (ADR 0110), generalised from
*op set* to *evaluation strategy*.

**`Query[T]` is a first-class, nameable, by-reference type.** A pure helper may
return one for composition (§11); two queries are not value-comparable (they are
descriptions, not values). It is **non-storable** and **non-boundary** (like
`Effect`/`Fn`, ADRs 0031/0030) — a query is built, passed, and executed, never
persisted or sent across a context boundary.

**Queries are agent-local.** A query reaches only the owning agent's storage;
cross-agent data flow stays message-passing (a typed call returning data, not a
query that reaches across the boundary). This preserves state privacy and
structurally rules out distributed-join failure modes — a hard scoping rule, not
a default.

## 2. The divergence this track closes

| Concern | Today | §11 target |
|---|---|---|
| In-memory transform | partial, as **free functions** in `bynk.list` (`bynk.list.map`/`filter`/`find`/`any`/`all`/`traverse`) + a thin List **kernel** (`length`/`get`/`prepend`/`fold`/`foldEff`) | one **method-chain** vocabulary (`xs.filter(p).map(f).sortBy(k)`), uniform with storage queries |
| Storage reads | entry-level only — `Map`/`Set`/`Cache` expose `get`/`contains`/`size`, no iteration or scan | lazy `Query[T]` builders + terminals over the whole collection |
| `Query[T]` | does not exist | a first-class, by-reference, non-storable type |
| Laziness | n/a (no storage iteration) | storage chains lazy; in-memory chains eager; one vocabulary |
| Indexing | `@indexed` **parses and gates** (ADR 0111; deferred by the `Map` slice) | secondary indexes maintained by the runtime, queries routed by the compiler, **index hygiene as build-time warnings** |
| `Log` reads | `Log` not yet built (storage slice 4, gated on this track) | time-window builders (`since`/`before`/`between`/`recent`/`reversed`) composing with the general vocabulary |
| Ordering | no `Ordering`/orderable concept | `sortBy`/`min`/`max`/`sum`/`average` over an ordering on a key |

The first row is the in-memory half: the eager combinators exist piecemeal as
`bynk.list` free functions (ADR 0034's hybrid posture), not the uniform
method-chain surface §11 specifies. The rest is the storage half — entirely new —
plus indexing, which the `Map` slice explicitly deferred here.

## 3. Concrete surface

```
-- pure construction (returns a Query, no effect)
fn pendingExpiringBefore(t: Duration)
    -> Query[Reservation] given Reservations: Map[ReservationId, Reservation] {
  Reservations
    .filter(r => r.status == Pending)
    .filter(r => r.expiresAt < t)
}

-- effectful execution, inside a handler (terminal returns Effect[T])
on call sweep() -> Effect[Int] {
  let stale <- pendingExpiringBefore(deadline).collect
  Effect.pure(stale.length)
}

-- time-window on a Log, composing with the general builders
events.since(start).filter(e => e.kind == Order).map(e => e.payload).collect

-- secondary index declaration drives compiler routing
store reservations: Map[ReservationId, Reservation] @indexed(by: orderId, by: expiresAt)
```

**Builders** (return `Query[T]` on storage, the same collection eagerly
in-memory): `filter`, `map`, `flatMap`, `sortBy`, `take`, `skip`, `distinct`,
`distinctBy`, plus joining (`join`, `joinOn`, `leftJoin`) and grouping
(`groupBy`). **Terminals** (return `Effect[T]` on storage, `T` in-memory):
`collect`, `first`, `firstOrElse`, `count`, `fold`, `sum`/`min`/`max`/`average`,
`any`, `all`, `forEach`. **`Log` time-window builders:** `since`/`before`/
`between`/`recent`/`reversed`. **In-memory effectful iteration** on `List[A]`
(eager, not on `Query`): `traverse`/`traverseAll`/`parTraverse`/`parTraverseAll`
(short-circuit-by-default, dispatched on the function's `Result`-ness).

The only new surface a reader sees against today is the **method-chain** form
replacing scattered `bynk.list.*` calls, the **`Query[T]`** type in pure-helper
signatures, the `@indexed` annotation becoming meaningful, and the `Log`
time-window builders — the handler/`store`/`given` forms are untouched.

## 4. Internal architecture (the seams)

- **`bynk-syntax`:** likely **no new grammar** — the builders/terminals are
  ordinary method calls (`recv.filter(p)`) and `Query[T]` is an ordinary generic
  type ref. The work is a `Query` built-in type name (reserved, like `List`/`Map`)
  and possibly `@indexed`'s already-parsed annotation (ADR 0111). To confirm in
  the settling phase.
- **`bynk-check`:** a `Ty::Query(T)`; **receiver-provenance dispatch** — a chain
  whose root is a `store_maps`/`store_sets`/`store_logs` field types lazy
  (builders → `Query[T]`, terminals → `Effect[T]`); a chain whose root is an
  in-memory `List`/`Map`/`Set` value types eager (builders → the collection,
  terminals → `T`). The builder/terminal signature tables (generic in the element
  and key types); the `Ordering`/orderable-key rule for `sortBy`/`min`/`max`; the
  storage-read effect folding into the agent's storage capability (no new
  `given`); `Query[T]` non-storable/non-boundary enforcement.
- **`bynk-emit`:** lower a **lazy storage query** to a Durable-Object read — a
  **scan** by default, an **index lookup** when the predicate matches a declared
  `@indexed` key; lower **eager in-memory** chains to TS array/object operations.
  Maintain secondary indexes **inside the atomic commit** (ADR 0109) so an indexed
  map is no less atomic than an unindexed one. The cross-shape join lowering.
- **Tooling (per-slice, part of "done"):** the `@indexed` hygiene diagnostics
  (missing/unused/ambiguous index) surfaced in the LSP and build report;
  completion/hover/signature for the builder/terminal vocabulary on both
  receivers; the book/spec §11 pages.

## 5. Dependencies & the ADR slate

This track depends on the **storage track**: `Map`/`Set`/`Cache` shipped
(ADRs 0110/0113); the `@indexed` annotation surface parses and gates (ADR 0111);
the atomic-commit machinery (ADR 0109) is the seam index maintenance hooks into.
`Log` (storage slice 4) depends on **this** track for its read surface, and the
`Map` `@indexed` follow-on is realised **here**.

Front-loaded, hard-to-reverse ADRs to write in the settling phase (roughly in
slice order):

- **The `Query[T]` model & lazy/eager dispatch** (to write). `Query[T]` as a
  first-class, by-reference, non-storable type; receiver-provenance dispatch
  generalising ADR 0110 from op-set to evaluation strategy; the storage-read
  effect folding into the storage capability (no new `given`). Constrains every
  later slice.
- **The builder/terminal vocabulary & `Ordering`** (to write). The closed
  combinator set and signatures; how `sortBy`/`min`/`max` order a key — an
  `Ordering` instance vs a closed orderable-base set (`Int`/`Float`/`String`/
  `Duration`); `groupBy` materialisation; numeric-terminal result types
  (`average -> Float`).
- **The `@indexed` indexing model** (to write). The runtime/compiler split:
  runtime maintains secondary indexes in the commit; the compiler routes queries
  and emits **hygiene diagnostics** (missing/unused/ambiguous index) — and whether
  those are warnings or errors. The selectivity heuristic and ambiguity tie-break.
- **The Durable-Object query lowering** (to write). How a scan and an index
  lookup lower; cross-shape joins; the `Log` time index.

External (not in this track): the storage `Log`/`Queue` slices (consumers); the
`Idempotency` capability (§12) that `Log.append` leans on; the events/reactive
systems (§11 defers reactive queries to them).

## 6. Ordered slice decomposition

> **Track status: not started** (drafted 2026-06-25, pending review). Sequenced
> after storage slice 3c (`Cache`, shipped v0.87); unblocks storage slice 4
> (`Log`) and the `Map` `@indexed` follow-on.

| # | Slice | Depends on | Status |
|---|---|---|---|
| 0 | Settling — `Query[T]` model + dispatch ADR; vocabulary + `Ordering` ADR; indexing-model ADR | — | not started |
| 1 | **Eager in-memory vocabulary** on `List` (method-chain `filter`/`map`/`flatMap`/`sortBy`/`take`/`skip`/`distinct`/`distinctBy` + terminals `fold`/`count`/`any`/`all`/`first`/`sum`/`min`/`max`/`average`) — no storage, no laziness; reconcile with the `bynk.list` free functions | 0 | not started |
| 2 | **Lazy `Query[T]` over storage `Map`** — the builder/terminal split, `Query[T]` type, **scan** execution (no index yet); pure-build/effectful-terminate | 1, storage `Map` | not started |
| 3 | **`@indexed`** — secondary indexes maintained in the commit; compiler routing + the missing/unused/ambiguous **hygiene diagnostics** | 2 | not started |
| 4 | **Joins & grouping** — `joinOn`/`leftJoin`/`join`, `groupBy`; **cross-shape** (Map×Log) | 3 | not started |
| 5 | **In-memory effectful iteration** — `traverse`/`traverseAll`/`parTraverse`/`parTraverseAll` as the uniform method surface (if not already covered by `bynk.list`) | 1 | not started |
| — | *`Log` time-window builders land with **storage slice 4** (`Log`), consuming this track's `Query[T]` + `since`/`before`/`between`/`recent`* | 2 | external |

Slice 1 (eager in-memory) is the cheapest foundation and unblocks slice 2's
shared vocabulary; slices 2–4 are the storage half in increasing power. Slice 5
may collapse into slice 1 depending on the `bynk.list` reconciliation.

## 7. Open design questions (settle before the relevant slice)

1. **Scope of v1** (slice 0). §11 already defers cost-based optimisation,
   materialised views, reactive queries, async streaming iterators, time-travel,
   and SQL-like syntax. Confirm the v1 surface is exactly the builder + terminal
   vocabulary + `@indexed` + joins/grouping, and lock the slice order (in-memory
   first, vs storage-`Map`-first).
2. **Lazy/eager dispatch** (slice 0). Receiver provenance — a `store`-field root
   builds `Query`, a value root is eager — generalising ADR 0110. Confirm this
   covers the mixed case (a query terminal returning a `List`, then chained
   eagerly) and how the checker tracks "query-rooted" through a chain.
3. **`Query[T]` storability/boundary** (slice 0). Non-storable, non-boundary,
   not value-comparable — like `Effect`/`Fn` (ADRs 0031/0030); returnable from a
   pure helper and passable as an argument (§11). Confirm the exact rule set and
   its diagnostics.
4. **`Ordering` for `sortBy`/`min`/`max`/`sum`/`average`** (slice 1). The
   language has no `Ordering` concept. Options: a closed **orderable base set**
   (`Int`/`Float`/`String`/`Duration`, with refined types widening) keyed by
   `sortBy`'s `T -> K`; or a first `Ordering`/typeclass mechanism. The smaller
   choice (orderable base set) likely suffices for v1.
5. **The `bynk.list` reconciliation** (slice 1). The eager combinators exist as
   free functions (`bynk.list.map`/`filter`/…). Do they become **methods**
   (migration + a `bynk-fmt` codemod, deprecating the free functions), or do
   method and free-function forms coexist (ADR 0037's call-surface question)?
6. **`@indexed` hygiene: warnings vs errors** (slice 3). §11 says the compiler
   *warns* on a missing/unused/ambiguous index. Bynk's diagnostic model can do
   warning-category — confirm these are warnings (not hard errors), and settle the
   selectivity heuristic and the ambiguity tie-break (most-selective + note).
7. **The storage-read effect surface** (slice 2). §11 says queries fold into the
   storage capability that "comes with" the agent's `store` fields — no new
   `given`. Confirm a storage terminal is `Effect`-typed (awaited with `<-`) like
   the existing entry ops, needing no extra capability (contrast `Cache`'s
   `given Clock`, which is eviction-specific, ADR 0113).
8. **`flatMap` returning `Query[U]` on storage** (slice 2/4). Nested storage
   queries — feasibility of the bind and its lowering (a correlated scan vs a
   join rewrite).
9. **Cross-shape joins** (slice 4). `Map × Log` joins using each side's index
   (the Log time index + the Map key); the lowering and the index-routing across
   shapes.
10. **Numeric/aggregate terminals** (slice 1/2). `average -> Float` (so `Int`
    averages don't truncate); `sum`/`min`/`max` result types; empty-collection
    behaviour (`min`/`max`/`average` of nothing → `Option`? fault? default?).
