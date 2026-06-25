# 0110 — `Map` is one type, two op sets: a `store` field is a storage map (effectful, mutating, direct methods); a value is the immutable collection

- **Status:** Accepted (storage track, slice 2; 2026-06-25)
- **Track:** `design/tracks/storage.md` (slice 2 — storage `Map`; the open question Q2 / the named "`Map`: storage kind vs collection value" ADR)
- **Realises:** `design/bynk-design-notes.md` §10 ("Storage Types" — the `Map` storage kind and its `put`/`get`/`update`/`upsert`/`remove` op set), building on the `Cell` slice (ADRs 0108/0109).
- **Relates:** ADR 0108 (`store` replaces `state`; D5 — the invariant predicate surface admits a keyed `map.get(k)`); ADR 0109 (handler-atomic commit — the overlay/flush this reuses). The lazy *query* surface over a `Map` (iteration/filter) is the **query-algebra** sibling track (§11) and is **out of scope** here — only the keyed entry ops land.

## Context

`Map[K, V]` is, today, an **immutable value type** (`TypeRef::Map`) with pure
methods (`get`, and value-returning combinators). Design-notes §10 *also* names
`Map` as a **storage kind**: `store reservations: Map[ReservationId, Reservation]`
— a keyed, durable collection mutated in place through effectful operations. The
storage track flagged the collision as the one Map-specific call to settle before
implementation (`storage.md` §7 Q2): does the *same* `Map[K, V]` spelling serve
both, disambiguated somehow, or do the two get split names?

The `Cell` slice already established the shape this builds on: a `store`-agent's
fields are its state record, mutated over a working copy and committed atomically
at handler end (ADR 0109). `Map` extends that from a single value to a keyed
collection. The checker already tracks which receivers are `store` fields
(`store_cells` from the checker slice).

## Decisions

**D1 — One spelling; receiver *provenance* disambiguates the op set.** `Map[K, V]`
stays a single type name. The op set is decided by what the receiver **is**:

- a **`store` field** of type `Map[K, V]` is a *storage map* — effectful,
  mutating, entry-level methods (D2);
- a **value** (`let`/param/field of a record) of type `Map[K, V]` is the
  *immutable collection* — pure methods returning new maps, unchanged.

The checker already knows a receiver's provenance, so dispatch is unambiguous. We
**reject split names** (`StoreMap` vs `Map`): two names for one concept is the
dialectal duplication §2 forbids, and the design notes write `Map[…]` for both. We
also reject a structural/keyword marker on the type — provenance is enough.

**D2 — Storage-map operations are `Effect`-typed direct methods; there is no
`:=`.** Per §10, a storage map exposes:

| op | type | idempotent | notes |
|---|---|---|---|
| `put(k, v)` | `Effect[()]` | yes (last-write-wins) | unconditional write |
| `get(k)` | `Effect[Option[V]]` | — (read) | absent ⇒ `None` |
| `update(k, fn: V -> V)` | `Effect[()]` | no | **faults if `k` absent** |
| `upsert(k, default: V, fn: V -> V)` | `Effect[()]` | no | RMW with default-if-absent |
| `remove(k)` | `Effect[()]` | yes (no-op if absent) | |
| `contains(k)` | `Effect[Bool]` | — (read) | |
| `size()` | `Effect[Int]` | — (read) | |

Like every storage op (§10) these are awaited with `<-`. A storage map has **no
`:=`** (the `:=` write form is `Cell`-only); the superseded value-pattern
`map := map.insert(k, v)` is gone — direct methods only. `update` on an absent key
is a **runtime fault** (the handler faults; nothing commits — ADR 0109), which is
exactly what `upsert` exists to avoid.

**D3 — Persistence: a storage map is a field of the agent's state record
(wholesale), reusing the `Cell` commit. Per-entry storage keys are deferred.** A
`store m: Map[K, V]` field becomes a `Record<K, V>`-typed field of the agent's
state record. Operations mutate/read the working copy that ADR 0109 already
stages; the whole record (cells **and** maps) flushes once at handler end through
the same `commitState` (with the invariant gate). This reuses the proven `Cell`
machinery verbatim and keeps a materialised proposed state. The **per-entry
storage-key** layout (ADR 0109 D2's "overlay keyed by entry", so a large map need
not load/save wholesale) is a **scalability follow-on**, not this slice — it is an
emission optimisation behind the same surface, and an agent with a small map is
correct under wholesale persistence today.

**D4 — A keyed `map.get(k)` is admissible in an invariant predicate; whole-map
scans are not.** ADR 0108 D5 already admits a *bounded single-element read* —
including a keyed `map.get(k)` — as a **pure read of the staged value**. So an
invariant may read `reservations.get(rid)`, evaluated against the working record
(not a live storage op), exactly as a `Cell` read in a predicate is. A
whole-collection scan (`size`, iteration, `contains` over unknown keys) stays out
of predicates (unbounded / not the gate's job). `map.get` therefore has the same
dual nature as a `Cell`: `Effect`-typed in handler position, a pure staged read in
predicate position.

**D5 — `K` is a value-keyable type; `V` is unconstrained.** Map keys reuse the
existing value-keyable constraint (`bynk.types.unkeyable_map_key`) — the same rule
the value `Map` already enforces. Refined `V` element types ride the same
follow-on as refined `Cell` elements.

## Consequences

- **Checker.** Un-gate `Map` (drop `bynk.store.kind_unsupported` for it); add a
  `store_maps` scope (field → `(K, V)`) beside `store_cells`; resolve the D2 op
  set kind-awarely on a store-map receiver, effect-typed; admit `map.get(k)` in
  predicates (D4). New diagnostics as needed (e.g. an unknown storage-map op).
- **Emission.** A storage map lowers to a `Record<K, V>` state field; `put`/
  `remove`/`update`/`upsert` mutate the working record, `get`/`contains`/`size`
  read it; `update`-on-absent throws. Commit is unchanged (the existing flush).
- **Scope held.** No `@indexed` (needs the annotation grammar, track Q3 — still
  unsettled); no query/iteration surface (query-algebra track, §11); no per-entry
  storage keys (D3 follow-on). These are named, not silently dropped.

## Alternatives considered

- **Split names (`StoreMap` vs `Map`).** Rejected (D1): two names for one concept;
  the notes use `Map` for both.
- **Per-entry storage keys now.** Rejected for this slice (D3): more emission
  complexity for a scalability win that is invisible behind the surface and
  separable; wholesale persistence reuses the proven `Cell` commit and is correct.
- **A storage map with `:=` (`map := …`).** Rejected (D2): §10 removes the
  value-pattern in favour of direct methods; `:=` stays `Cell`-only.
- **Admitting whole-map reads in invariants.** Rejected (D4): unbounded to check
  and not the commit gate's job; only the bounded keyed `get` is in.
