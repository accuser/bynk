# 0119 — Durable-Object query lowering: a lazy `Query[T]` lowers to a pipeline over the agent's in-memory state `Record`s — scan, index lookup, correlated `flatMap`, hash joins, and the `Log` time window

- **Status:** Accepted (query-algebra track, slice 0 settling — second batch; 2026-06-25)
- **Track:** `design/tracks/query-algebra.md` (the DO-lowering ADR; gates the storage half of slices 2–4). Settles track Q9 (the `flatMap` lowering) and Q10 (cross-shape joins).
- **Realises:** `design/bynk-design-notes.md` §11 (lazy storage queries; cross-shape `Map × Log` joins; the implicit `Log` time index).
- **Relates:** ADR 0115 (the lazy storage `Query` lowers to a DO read, distinct from an eager in-memory chain — this settles *how*); ADR 0118 (the indexing model — scan vs index lookup, the posting-list representation); ADR 0110 (the wholesale `Record` storage model the pipeline runs over, and D3's per-entry-key deferral that bounds the I/O); ADR 0109 (staged state — a query reads the staged write-set, read-your-writes); ADR 0116 (the builder/terminal vocabulary whose TS shapes the storage lowering reuses, and `groupBy` materialisation); the storage `Log` slice (slice 4), which consumes D6.

## Context

ADR 0115 fixed that a lazy storage `Query[T]` lowers to a Durable-Object read —
a scan by default, an index lookup under `@indexed` — distinct from the eager
in-memory chain (slice 1). ADR 0118 settled the indexing model. This ADR settles
the lowering itself: the TS a storage query becomes.

The grounding fact (again): a storage `Map` persists **wholesale** as a
`Record<string, V>` in the agent's state record (ADR 0110 D3). So a query's "DO
read" is, today, an iteration over an **already-loaded** in-memory `Record` —
there is no extra round-trip per query. The lowering is therefore a TS pipeline
over the Record's values, sharing the slice-1 combinator shapes; the index just
narrows the source. The shape is chosen so that when per-entry DO storage keys
land (the ADR 0110 D3 follow-on), the *same* lowering becomes a true partial read.

## Decisions

**D1 — A lazy storage `Query[T]` lowers to a pipeline over the agent's in-memory
state `Record`(s); the terminal carries the effect.** Because the map is loaded
wholesale (ADR 0110 D3), the builder chain lowers to the **same TS array/object
shapes as the eager in-memory vocabulary** (slice 1, ADR 0116) — the only
differences are the **source** (`Object.values(this.state.<map>)`, narrowed by an
index per D3) and that the **terminal** is `Effect`-typed (ADR 0115 D5), lowering
to an `async` expression that reads the **staged** state (ADR 0109 — read-your-
writes within the handler). Building a `Query` emits nothing; the terminal emits
the whole pipeline.

**D2 — Scan lowering (default): `Object.values(record)` → the builder pipeline.**
With no matching index, `reservations.filter(p).map(f).collect` lowers to
`Object.values(staged.reservations).filter(__x => p(__x)).map(__x => f(__x))`
(the slice-1 shapes), wrapped in the effectful read. Iteration follows the map's
**insertion order** (ADR 0110's Map ordering guarantee). `first` short-circuits;
`count` reads `.length`; aggregates reuse the slice-1 terminal lowerings.

**D3 — Index-lookup lowering: consult the posting list, then map the matched
primary keys back through the map.** When ADR 0118 D3 routes
`filter(r => r.k == v)` to an index, the lowering reads the sibling index Record's
posting list (`(staged.<map>__by<K>[v] ?? [])`), maps those primary keys back
through the map Record (`ids.map(id => staged.<map>[id])`), and runs the remaining
pipeline on that **narrowed array**. Under wholesale persistence both still touch
the loaded Record, so the win is skipping the O(n) filter (ADR 0118 D6); the
lowering shape is exactly the one a per-entry-key store turns into a partial read
(fetch only `ids`). The routed equality predicate is dropped from the residual
pipeline (the index already enforced it).

**D4 — `flatMap` returning `Query[U]` lowers as a correlated scan (Q9).** A
storage `flatMap(f: T -> Query[U])` lowers to a nested pipeline: for each element
of the outer source, execute the inner query (over its own `Record`, routed
through its own index per D3 when eligible) and concatenate the results
(`(...).flatMap(__x => <inner-pipeline(__x)>)`). A **join-rewrite** that
recognises a correlated equi-`flatMap` and lowers it as a hash join (D5) is a
named deferral — the correlated scan is correct, and v1's small working sets make
it adequate.

**D5 — Joins lower as hash joins over the in-memory Records; `leftJoin` keeps
unmatched left rows.** `joinOn(other, left, right)` builds a hash map keyed by the
**smaller** side's join key — the side's index `Record` when that key is
`@indexed` (ADR 0118), else a map built on the fly — and probes it from the other
side, emitting `(t, u)` pairs. `leftJoin` emits `(t, None)` for an unprobed left
row; `join(other, on)` with a general predicate falls back to a nested-loop
filter (no key to hash). A join **materialises** its result (as `groupBy` does,
ADR 0116 D7).

**D6 — Cross-shape `Map × Log` joins use each side's index; the `Log` time index
is implicit and always present (Q10).** A `Log[T]` carries an implicit timestamp
per entry (§11) and persists in time order, so a time-window builder
(`since`/`before`/`between`) lowers to a **bounded slice** of the Log (a bound on
the timestamp; a filter under wholesale persistence, a range read under per-entry
keys); `recent(n)` takes the last `n`, `reversed` flips order. A **cross-shape
join** narrows the `Log` side by its time window first, then hash-joins (D5)
against the `Map` side's key/secondary index — `events.since(t).joinOn(
reservations.filter(…), left: e => e.rid, right: r => r.id)` probes the Map's
index with each windowed Log entry's join key. The `Log` storage representation
itself lands with **storage slice 4**, consuming this lowering.

**D7 — Everything is intra-agent, over staged state, inside the atomic commit.**
A query reads the **staged** write-set (ADR 0109) — read-your-writes within the
handler — and **never escapes the agent** (ADR 0115 D6): no query performs a
cross-agent or cross-DO read. Index maintenance on writes happens in the same
staged write-set (ADR 0118 D1), so a query in the same handler sees a consistent
map-and-indexes view.

## Consequences

- **Emission.** The query → TS-pipeline lowering: scan (D2), index lookup (D3),
  correlated-`flatMap` (D4), hash joins incl. cross-shape and the `Log` time
  window (D5/D6), all reusing the slice-1 combinator shapes and reading staged
  state (D1/D7). No new runtime beyond the index `Record`s (ADR 0118).
- **The wholesale-vs-per-entry seam.** Every lowering is written as
  "source → pipeline" so the per-entry-key follow-on (ADR 0110 D3) replaces only
  the **source** (a partial DO read) without touching the pipeline — the I/O win
  ADR 0118 D6 names, for free.
- **Deferred (named).** The `flatMap` join-rewrite (D4); cost-based join ordering
  (§11 defers cost-based optimisation); true partial DO reads (per-entry keys);
  streaming terminals beyond `forEach` (§11 deferred).

## Alternatives considered

- **A per-query DO round-trip (treat each query as its own storage read).**
  Rejected (D1): the map is already loaded wholesale (ADR 0110 D3); a separate
  read would be redundant I/O and break read-your-writes against staged state.
- **A join-rewrite for `flatMap` now.** Rejected (D4): more emission complexity
  for a win that is invisible at v1's working-set sizes; the correlated scan is
  correct and the rewrite is a clean later optimisation.
- **Lowering storage queries differently from the in-memory vocabulary.**
  Rejected (D1/D2): they share the combinator semantics (ADR 0116); reusing the
  TS shapes keeps one code path and one mental model — the receiver and the effect
  typing are the only difference (ADR 0115).
- **Cross-agent / distributed joins.** Rejected (D7): §11 and ADR 0115 D6 make
  queries strictly agent-local; cross-agent data flow stays message-passing.
