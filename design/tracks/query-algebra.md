# Query-algebra track — reading and transforming agent-local data

Persistent design doc for the **query algebra** of `design/bynk-design-notes.md`
§11, the feature-track artefact of
[ADR 0076](../decisions/0076-feature-track-posture.md). It realises and sharpens
§11; the design notes stay the north star. It is the **sibling track** the
storage track (`design/tracks/storage.md`) sequences *before* its `Log` slice
(`Set` shipped at v0.84 with only entry ops — `add`/`remove`/`contains`/`size` —
and needed no query surface). `Log`'s time-window reads and `Map`'s deferred
`@indexed` both live here, and storage's lazy read surfaces produce the
`Query[T]` this track defines.

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
-- pure construction (returns a Query, no effect). `now` and `expiresAt` are
-- `Instant`s — absolute time, a distinct base type (ADR 0114, settling Q4).
fn pendingExpiredAt(now: Instant)
    -> Query[Reservation] given Reservations: Map[ReservationId, Reservation] {
  Reservations
    .filter(r => r.status == Pending)
    .filter(r => r.expiresAt < now)     -- expiresAt: Instant
}

-- effectful execution, inside a handler (terminal returns Effect[T]). The instant
-- comes from the Clock, exactly as a timestamp is minted everywhere else.
on call sweep() -> Effect[Int] given Clock {
  let now <- Clock.now()                -- Clock.now() -> Effect[Instant] (ADR 0114 D4)
  let stale <- pendingExpiredAt(now).collect
  Effect.pure(stale.length)
}

-- time-window on a Log, composing with the general builders. `since` takes an
-- `Instant` (ADR 0114).
events.since(dayStart).filter(e => e.kind == Order).map(e => e.payload).collect

-- secondary index declaration drives compiler routing
store reservations: Map[ReservationId, Reservation] @indexed(by: orderId, by: expiresAt)
```

(The instant arithmetic uses ADR 0114 D3's `Instant + Duration -> Instant` — e.g.
a caller computes a horizon `now + 1.hours`. Q4 settled to **introduce `Instant`**
(ADR 0114), a distinct base type that re-types `Clock.now()`, `now`, `expiresAt`,
and the `Log` time-window builders' arguments, and lets ADR 0112 D4's `Int`↔
`Duration` coercion be withdrawn — so this example is now fully typed.)

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
  `given`); `Query[T]` non-storable/non-boundary enforcement. **`flatMap` is the
  one builder whose lambda return type flips with the root** — `T -> Query[U]`
  storage-rooted, `T -> List[U]` in-memory — so the checker must dispatch its
  argument type, not just its result, by provenance (Q9 covers the lowering; this
  is the signature-level duality).
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

It also had a **language-primitive dependency the storage track did not need: an
absolute-instant type** — **settled by [ADR 0114](../decisions/0114-instant-primitive.md)**
(Q4). The `Log` time-window builders (`since`/`before`/`between`) take an absolute
time, and the common `Map` query compares a stored instant field against one (the
§3 example). Rather than leave instants as bare `Int` millis (ADR 0112 D4's
posture), Q4 introduced **`Instant`** as a distinct base type (epoch millis, no
literal, minted by `Clock.now()`), re-typing `Clock` and withdrawing 0112 D4's
`Int`↔`Duration` coercion. Like `Duration` (a prerequisite slice before `Cache`),
`Instant` is sequenced as a prerequisite slice (1b) before the storage-`Map` query
slice and the `Log` slice.

Front-loaded, hard-to-reverse ADRs (roughly in slice order); the settling-phase
batch (0114–0116) has landed, the indexing/lowering pair is next:

- **[ADR 0115](../decisions/0115-query-model-lazy-eager-dispatch.md) — the
  `Query[T]` model & lazy/eager dispatch** (accepted). `Query[T]` as a
  first-class, by-reference, non-storable/non-boundary type; receiver-provenance
  dispatch generalising ADR 0110 from op-set to evaluation strategy; the
  storage-read effect folding into the storage capability (no new `given`).
  Constrains every later slice. Settles Q2/Q3/Q8.
- **[ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md) — the
  builder/terminal vocabulary & `Ordering`** (accepted). The closed combinator
  set and signatures; `sortBy`/`min`/`max` over a closed orderable-base set
  (`Int`/`Float`/`String`/`Duration`/`Instant`), not a typeclass; `groupBy`
  materialisation; numeric-terminal result types (`average -> Float`);
  empty-aggregate results as `Option`; the `bynk.list`→methods migration. Settles
  Q5/Q6/Q11 (and Q9's checker half).
- **[ADR 0118](../decisions/0118-indexed-indexing-model.md) — the `@indexed`
  indexing model** (accepted). Runtime-maintained secondary indexes (a posting-list
  `Record`) updated in the atomic commit; the compiler routes equality
  `filter`/`joinOn` to an index, else scans; **index hygiene is build-time
  warnings** (via ADR 0117) with most-selective structural tie-break. Honest
  scope: a CPU win under wholesale persistence, an I/O win when per-entry DO keys
  land (ADR 0110 D3). Settles Q7.
- **[ADR 0119](../decisions/0119-durable-object-query-lowering.md) — the
  Durable-Object query lowering** (accepted). A lazy `Query` lowers to a pipeline
  over the in-memory state `Record`s (reusing the slice-1 TS shapes; source =
  `Object.values`/index posting list, terminal = `Effect` over staged state);
  scan, index lookup, correlated `flatMap`, hash joins, and the cross-shape
  `Map × Log` join via the Log time index. Strictly intra-agent. Settles Q9/Q10.

External (not in this track): the storage `Log`/`Queue` slices (consumers); the
`Idempotency` capability (§12) that `Log.append` leans on; the events/reactive
systems (§11 defers reactive queries to them).

## 6. Ordered slice decomposition

> **Track status: settling complete; slices 1 + 1b shipped (v0.88–v0.90)**
> (2026-06-25). All settling ADRs have landed: the foundational batch —
> [0114](../decisions/0114-instant-primitive.md) (`Instant`, Q4),
> [0115](../decisions/0115-query-model-lazy-eager-dispatch.md) (`Query[T]`
> model + dispatch, Q2/Q3/Q8), [0116](../decisions/0116-query-vocabulary-and-ordering.md)
> (vocabulary + `Ordering`, Q5/Q6/Q11) — and the second batch:
> [0118](../decisions/0118-indexed-indexing-model.md) (the `@indexed` model, Q7),
> [0119](../decisions/0119-durable-object-query-lowering.md) (the DO lowering,
> Q9/Q10). **Shipped:** slice 1's eager `List` vocabulary (v0.88), the
> non-failing warning channel ([ADR 0117](../decisions/0117-non-failing-warning-channel.md),
> v0.89) that unblocks slice 1c and `@indexed` hygiene, and the `Instant`
> primitive (slice 1b, v0.90). **Remaining:** slice 1c (`bynk.list` deprecation,
> now unblocked), then the storage half — slice 2 (lazy `Query` over `Map`) → 3
> (`@indexed`) → 4 (joins/grouping). Sequenced after storage slice 3c
> (`Cache`, shipped v0.87); unblocks storage slice 4 (`Log`) and the `Map`
> `@indexed` follow-on.

| # | Slice | Depends on | Status |
|---|---|---|---|
| 0 | Settling — `Query[T]` model + dispatch (ADR 0115); vocabulary + `Ordering` (ADR 0116); `@indexed` model (ADR 0118); DO-lowering (ADR 0119) | — | **complete (0114–0119)** |
| 1 | **Eager in-memory vocabulary** on `List` (method-chain `map`/`filter`/`flatMap`/`sortBy`/`take`/`skip`/`distinct`/`distinctBy` + terminals `count`/`any`/`all`/`first`/`firstOrElse`/`sum`/`min`/`max`/`average`) as kernel methods — no storage, no laziness | 0 | **shipped (v0.88)** |
| 1c | **`bynk.list`→methods migration** (ADR 0116 D6) — deprecate `map`/`filter`/`find`/`any`/`all` (warning + machine-applicable auto-fix to the method form); `reverse`/`traverse` keep their free form | 1 | **shipped (v0.91)** |
| 1b | **`Instant` primitive** (ADR 0114) — sixth base type, `Clock.now() -> Effect[Instant]`, `Instant`/`Duration` arithmetic, orderable; prerequisite for slice 2's instant-field queries and the `Log` slice | — | **shipped (v0.90)** |
| 2 | **Lazy `Query[T]` over storage `Map`** — the builder/terminal split, `Query[T]` type, **scan** execution (no index yet); pure-build/effectful-terminate | 1, 1b, storage `Map` | not started |
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
   and SQL-like syntax. **Confirmed** — the v1 surface is exactly the builder and
   terminal vocabulary ([ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md)),
   plus `@indexed` and joins/grouping, and the slice order is **in-memory first**
   (slice 1 before the storage half).
2. ~~**Lazy/eager dispatch** (slice 0).~~ **Settled —
   [ADR 0115](../decisions/0115-query-model-lazy-eager-dispatch.md) D3/D4:**
   receiver provenance generalises ADR 0110 from op-set to evaluation strategy;
   the checker tracks query-rootedness by the receiver *type* (`Ty::Query`); a
   terminal's result leaves the lazy domain (no re-lazification), so the mixed
   case is just two ordinary phases.
3. ~~**`Query[T]` storability/boundary** (slice 0).~~ **Settled — [ADR 0115](../decisions/0115-query-model-lazy-eager-dispatch.md)
   D1/D2:** first-class, by-reference, non-storable / non-boundary /
   not-comparable, reusing the `Effect`/`Fn` diagnostic machinery (ADRs
   0031/0030); returnable from a pure helper and passable as an argument.
4. ~~**An absolute-instant type** (slice 0; prerequisite for `Log`).~~ **Settled —
   [ADR 0114](../decisions/0114-instant-primitive.md):** introduce **`Instant`**,
   a distinct base type (epoch millis, no literal, minted by `Clock.now()`),
   re-typing `Clock.now() -> Effect[Instant]` and withdrawing ADR 0112 D4's
   `Int`↔`Duration` coercion. Sequenced as prerequisite slice 1b.
5. ~~**`Ordering` for `sortBy`/`min`/`max`/`sum`/`average`** (slice 1).~~ **Settled
   — [ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md) D2:** a closed
   **orderable base set** (`Int`/`Float`/`String`/`Duration`/`Instant`, refined
   types widening) keyed by the projection `T -> K`; no typeclass in v1.
6. ~~**The `bynk.list` reconciliation** (slice 1).~~ **Settled —
   [ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md) D6:** the
   combinators become **methods**; the `bynk.list.*` free functions are deprecated
   and rewritten by a `bynk-fmt` codemod (the `state→store` precedent).
7. ~~**`@indexed` hygiene: warnings vs errors** (slice 3).~~ **Settled —
   [ADR 0118](../decisions/0118-indexed-indexing-model.md) D4/D5:** the
   missing/unused/ambiguous diagnostics are **warnings** (via the now-shipped
   warning channel, ADR 0117), each with an add/remove auto-fix; ambiguity breaks
   to the **most selective by a static structural heuristic** (cost-based stats
   deferred). Indexes are runtime-maintained in the atomic commit.
8. ~~**The storage-read effect surface** (slice 2).~~ **Settled —
   [ADR 0115](../decisions/0115-query-model-lazy-eager-dispatch.md) D5:** a storage
   terminal is `Effect`-typed (awaited with `<-`) and folds into the storage
   capability the `store` fields carry — no new `given`; building a query is pure.
9. ~~**`flatMap` returning `Query[U]` on storage** (slice 2/4).~~ **Settled** —
   the checker half by [ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md)
   D5 (lambda return type dispatched by provenance), the **lowering** by
   [ADR 0119](../decisions/0119-durable-object-query-lowering.md) D4 (a correlated
   scan; the join-rewrite optimisation is a named deferral).
10. ~~**Cross-shape joins** (slice 4).~~ **Settled —
    [ADR 0119](../decisions/0119-durable-object-query-lowering.md) D5/D6:** hash
    joins over the in-memory `Record`s; a `Map × Log` join narrows the `Log` by its
    implicit time index first, then probes the `Map`'s key/secondary index.
11. ~~**Numeric/aggregate terminals** (slice 1/2).~~ **Settled —
    [ADR 0116](../decisions/0116-query-vocabulary-and-ordering.md) D3/D4:**
    `average -> Float` (no truncation); `sum`/`min`/`max` result types fixed;
    empty-collection results are **`Option`** (`first`/`min`/`max`/`average`)
    while `sum`/`count`/`fold` use the identity — fixed at the type because
    storage learns emptiness only by executing.
12. ~~**A non-failing warning channel** (slice 1c; surfaced building slice 1).~~
    **Settled & shipped — [ADR 0117](../decisions/0117-non-failing-warning-channel.md)
    (v0.89):** the warning channel was built as its own increment (a severity-aware
    collection sink; warnings surface but compile/check succeed), and slice 1c
    (v0.91) then landed the `bynk.list` deprecation as a real warning +
    machine-applicable auto-fix on top of it — not the build-breaking removal the
    gap would otherwise have forced.
