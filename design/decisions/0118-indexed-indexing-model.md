# 0118 — the `@indexed` indexing model: runtime-maintained secondary indexes in the atomic commit; the compiler routes equality filters and emits index-hygiene **warnings**

- **Status:** Accepted (query-algebra track, slice 0 settling — second batch; 2026-06-25)
- **Track:** `design/tracks/query-algebra.md` (the indexing-model ADR; gates slice 3 `@indexed`). Settles track Q7.
- **Realises:** `design/bynk-design-notes.md` §11 ("Indexing" — secondary indexes maintained by the runtime, queries routed by the compiler, **index hygiene as build-time warnings**).
- **Relates:** ADR 0111 (the `@indexed(by: …)` annotation surface — parses and gates, deferred *here* for the routing/maintenance semantics); ADR 0110 (the storage `Map` as a wholesale `Record` state field — the representation indexes extend, and D3's per-entry-key deferral that bounds the I/O story); ADR 0109 (handler-atomic commit — the staged write-set index maintenance hooks into); ADR 0117 (the non-failing warning channel — what makes hygiene *warnings* possible); ADR 0116 (the query vocabulary — `filter`/`joinOn` are what route). The **DO lowering** of a scan vs an index lookup is [ADR 0119](0119-durable-object-query-lowering.md).

## Context

ADR 0111 settled that `@indexed(by: f, …)` parses in field-declaration position
and gates as not-yet-functional, deferring its meaning to this track. §11
specifies the meaning: the runtime maintains secondary indexes, the compiler
routes queries through them, and **index hygiene is a set of build-time
warnings** (missing / unused / ambiguous). The warning channel (ADR 0117) now
exists, so those warnings can surface without failing the build — the
prerequisite §11 assumed.

One reality must frame everything: a storage `Map` persists **wholesale** — the
whole `Record<string, V>` loads and commits with the agent's state record (ADR
0110 D3; per-entry storage keys deferred). So the entire map is already in
memory in a handler. An index therefore changes the **CPU** profile of a query
(O(1) routing vs an O(n) scan), not yet its **I/O** profile (the map loads
regardless). This ADR settles the surface and semantics so they are identical
under both the wholesale model and the future per-entry-key one.

## Decisions

**D1 — `@indexed(by: k, …)` declares secondary indexes the runtime maintains
inside the atomic commit.** Each `by: k` label (a field of the element type `V`,
or the primary key) declares a secondary index. The runtime updates it on every
mutating map op (`put`/`update`/`upsert`/`remove`) as part of the **same staged
write-set** that ADR 0109 flushes once at handler end — so an indexed map is **no
less atomic** than an unindexed one (§11), and a fault before the commit leaves
neither the map nor its indexes changed. Indexes are derived, never user-visible,
and never separately readable.

**D2 — Representation: a secondary index is a sibling `Record` in the agent's
state record — a posting list from the indexed value to the primary keys.**
`@indexed(by: orderId)` on `store reservations: Map[Rid, Reservation]` adds a
derived state field shaped `Record<orderId, Rid[]>` (a posting list: each indexed
value maps to the set of primary keys whose entry holds it). It persists and
commits **wholesale** alongside the map (ADR 0110 D3), maintained in lockstep in
the staged write-set (D1). The indexed key MUST be **value-keyable** (the Map-key
rule, ADR 0110 D5) — a non-keyable `by:` target is a compile error at the
annotation.

**D3 — The compiler routes an equality filter on an indexed key to an index
lookup; everything else scans.** A lazy `Query` whose `filter(r => r.k == v)`
(or a primary-key equality) matches a declared `@indexed(by: k)` lowers to an
**index lookup** (consult the posting list for `v`, fetch those entries) rather
than a **scan** (iterate all entries) — ADR 0119. **Equality** is the v1 routing
trigger; **range/inequality** routing over an ordered index (`r.expiresAt < t`)
is a named deferral. A `joinOn` equi-join side is index-eligible (§11) when its
join key is indexed.

**D4 — Index hygiene is build-time WARNINGS, not errors (Q7), via ADR 0117.**
Analysing query expressions against declared indexes, the compiler emits
warning-category diagnostics (surfaced, non-failing):

- **`bynk.index.missing`** — a query filters by equality on a non-indexed key
  that *could* be indexed; carries a machine-applicable suggestion to add
  `@indexed(by: k)`. (The scan still compiles and runs — it is a perf hint.)
- **`bynk.index.unused`** — a declared `@indexed(by: k)` no query routes through;
  suggests removing it (it costs maintenance on every write).
- **`bynk.index.ambiguous`** — a query whose predicate could use more than one
  index; the compiler picks the most selective (D5) and notes the choice.

They are **warnings**, never hard errors — index hygiene is a build-review
concern, not a compile gate (§11). This is "the cost of index management moved
from runtime debugging to build-time review" (§11), now realisable because a
warning no longer fails the build (ADR 0117).

**D5 — Selectivity heuristic & ambiguity tie-break: structural, not cost-based.**
With no runtime cardinality statistics at compile time, when several indexes
match a query the compiler picks the **most selective by a static structural
estimate** — prefer an equality over a higher-cardinality key; break ties by
**declaration order** — and records the choice in the build report (D4's
`ambiguous` note). A **cost-based optimiser** with real statistics is a named
deferral (§11 already defers cost-based optimisation). The heuristic is
intentionally simple and predictable so a developer can reason about routing.

**D6 — Scope under wholesale persistence: indexes are a CPU optimisation now, an
I/O one later — same surface either way.** Because the map persists wholesale
(ADR 0110 D3), an index makes routing **O(1) instead of O(n) in CPU** but does
**not** reduce the Durable-Object read (the whole `Record` loads regardless). The
**I/O** payoff — touching only the matched entries — arrives with the
**per-entry DO storage-key** layout (the ADR 0110 D3 follow-on). The `@indexed`
surface, the routing, and the hygiene warnings are **identical** under both; only
the lowering's I/O profile changes (ADR 0119 D3). Stated, not hidden — a
developer who indexes for scale gets the CPU win today and the I/O win for free
when per-entry keys land.

## Consequences

- **Checker.** Un-gate `@indexed` (ADR 0111); the `by:`-key value-keyable check
  (D2); a query-analysis pass that matches `filter`/`joinOn` equality predicates
  to declared indexes (D3), chooses by the selectivity heuristic (D5), and emits
  the three hygiene warnings with suggestions (D4). New warning-category
  diagnostics `bynk.index.missing` / `unused` / `ambiguous` (registered with the
  slice).
- **Emission.** Maintain each index `Record` in the staged write-set on every map
  mutation, inside the ADR 0109 flush (D1/D2); the routed-lookup vs scan lowering
  is ADR 0119.
- **Deferred (named).** Range/inequality index routing (D3); cost-based
  selectivity (D5); the per-entry-key I/O scaling (D6 / ADR 0110 D3); indexes on
  `Set`/`Log` beyond `Map` (ADR 0111 sequences them with their kinds).

## Alternatives considered

- **`@indexed` hygiene as hard errors.** Rejected (D4): §11 specifies warnings,
  and a missing/unused index is a perf hint, not a malformed program; the warning
  channel (ADR 0117) exists precisely so these inform without gating.
- **A cost-based optimiser now.** Rejected (D5): no compile-time statistics exist,
  and §11 defers cost-based optimisation; a predictable structural heuristic
  serves the target workloads and keeps routing legible.
- **Per-entry DO storage keys in this slice.** Rejected (D6): a larger emission
  change (ADR 0110 D3 deferred it); the wholesale model is correct and the
  `@indexed` surface is forward-compatible with it.
- **A user-readable index handle.** Rejected (D1): an index is a derived
  maintenance structure; exposing it would invite it drifting from the map and
  duplicate the query surface that already reads it.
