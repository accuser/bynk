# 0121 — `Log[T]` is an append-only, time-indexed sequence: `append` stamps a `Clock.now()` `Instant` (the one non-idempotent write) and is the only clock-consuming op; reads are lazy `Query[T]` with time-window roots; `@retain` prunes on append

- **Status:** Accepted (storage track, slice 4; 2026-06-26)
- **Provenance:** the storage track (slice 4 — `Log`, the first kind unblocked after the query-algebra sibling track completed). Makes `@retain` the second functional storage annotation.
- **Realises:** `design/bynk-design-notes.md` §10 (`Log[T]` — append-only, ordered, time-indexed; `log.append` as the one non-idempotent write) and §11 (the `Log` time-window builders `since`/`before`/`between`/`recent`/`reversed`, deferred from the query track to land with this slice).
- **Relates:** ADR 0114 (`Instant` — the per-entry timestamp type; `Clock.now() -> Effect[Instant]`); ADR 0115 (the `Query[T]` model — a `Log` is a lazy query root); ADR 0116 (the builder/terminal vocabulary that composes over a `Log` query); ADR 0119 (DO query lowering — D6 already specs the `Log` time index and the cross-shape `Map × Log` join; this slice consumes it); ADR 0120 (joins take an `into:` combiner — the `Map × Log` join uses it); ADR 0110 (wholesale `Record` persistence — a `Log` adapts it to an ordered array) and ADR 0109 (handler-atomic commit); ADR 0113 (`Cache` — the `given Clock` precedent, applied more narrowly here); ADR 0111 (the `@retain` annotation, registered-and-gated to this slice). Forward dep: §12 `Idempotency` (the safe-use story for the non-idempotent append).

## Context

`Log[T]` is the fifth storage kind and the first the storage track reaches after
its query-algebra sibling completed. Design-notes §10 defines it as **append-only,
ordered, time-indexed**, with `append` the *one non-idempotent storage write*; §11
gives it **time-window builders** (`since`/`before`/`between`/`recent`/`reversed`)
that produce lazy `Query[T]` and compose with the general read vocabulary. Both
prerequisites are now in place — `Instant` (ADR 0114) types the timestamps and the
window arguments, and `Query[T]` + the DO lowering (ADRs 0115/0119) mean a `Log`
only has to provide a *lazy root and a time index*; the query track explicitly
deferred the `Log` time-window builders and the `Map × Log` join to **land here**.

What remains is `Log`-specific: its representation, the append's timestamp source
and non-idempotency, the time-window read surface, and what `@retain` means.

## Decisions

**D1 — A `Log[T]` is an ordered, append-only array of timestamped entries.** It
persists as a JSON **array** `Array<{ t: number, v: V }>` — the entry value plus
its append-time instant as epoch milliseconds (`Instant`'s wire form, ADR 0114).
An **array, not a `Record`** (the shape `Map`/`Set`/`Cache` use, ADR 0110):
order is *intrinsic* to a `Log`, and an array carries it for free. The whole array
is a state-record field, committed wholesale at handler end (ADRs 0109/0110); a
refined element type validates on append and on rehydration, as for every kind.

**D2 — `append(e)` stamps `Clock.now()`, requires `given Clock`, and is the one
non-idempotent write.** `log.append(e) -> Effect[()]` appends `{ t: <now>, v: e }`
where `<now>` is the handler's injected clock (`Clock.now() -> Effect[Instant]`,
ADR 0114). A handler that appends therefore declares **`given Clock`** — the same
explicit-time discipline as `Cache` (ADR 0113 D4), and the **only** clock-consuming
`Log` op (D3 makes reads clock-free). `append` is **non-idempotent** by design
(§10): an at-least-once retry appends twice. The compiler cannot prevent that — it
is a runtime concern — so v1 **documents the safe-use story rather than enforcing
it**: the entry type carries a deduplication key (event/request/operation id) and
consumers dedupe, or the appending handler uses the **`Idempotency` capability**
(§12, a future track) keyed on that id. `append` is a mutating op and drives the
implicit commit (ADR 0109).

**D3 — Reads are lazy `Query[T]` over the entry values, with `Log`-specific
time-window roots; reads need no clock.** A read chain on a `Log` field builds a
`Query[T]` over the entry **values** (`T`, not `{ t, v }`) — the per-entry
timestamp is implicit, consumed by the window builders but not exposed in the
element (exposing it to `map`/`filter` is a named follow-on). The `Log`-specific
query roots:

- `since(t: Instant) -> Query[T]` — entries with timestamp ≥ `t`;
- `before(t: Instant) -> Query[T]` — timestamp < `t`;
- `between(start: Instant, end: Instant) -> Query[T]` — closed range `[start, end]`;
- `recent(n: Int) -> Query[T]` — the last `n` entries, newest first;
- `reversed -> Query[T]` — reverse iteration order;

and the general builder/terminal vocabulary (ADR 0116) composes onto them
(`log.since(t).filter(…).map(…).collect`). Crucially, the window roots take
**explicit `Instant` arguments** (a caller passes `clock.now() - 1.hours`, the
`Instant`/`Duration` arithmetic of ADR 0114), so **a reading handler needs no
`given Clock`** — narrower than `Cache`, whose eviction reads consult the clock.
`recent`/`reversed` use append order, which is canonical.

**D4 — `@retain(d: Duration)` is optional and prunes on append.** Unlike `@ttl` on
`Cache` (required — a keyed store with no expiry is a `Map`), `@retain` is
**optional**: a `Log` without it keeps every entry. When present, retention
**prunes at append time** — each `append`, having the clock already (D2), drops
leading entries with `t < now() - d`. Pruning at the write point (rather than
lazily on read, as `Cache` evicts) keeps **reads clock-free** (D3) and bounds the
array without a separate sweep; an entry just past the horizon survives until the
next append (eventual, like every lazy reap). `@retain` flips its annotation gate
functional (ADR 0111), the second annotation to do so after `@ttl`.

**D5 — Lowering extends ADR 0119; the `Map × Log` join and time index land here.**
A `Log` query lowers per ADR 0119: the **scan** is the array itself (already
ordered — no `Object.values`); `since`/`before`/`between` filter on `t`;
`recent(n)`/`reversed` slice and reverse; the implicit **time index** is the append
order (monotonic in the common case), so a range window may binary-search it, with
scan-and-filter as the correct floor. The cross-shape **`Map × Log` join** that
ADR 0119 D6 specs and ADR 0120 left to this slice is realised here: a `Log` query
root plugs into the existing combiner-form join machinery (`into:`, no pair type),
the join using the `Log` time index for the windowed side and the `Map` key for the
lookup side. Everything stays intra-agent, over the staged write-set, inside the
atomic commit (ADR 0119 D7).

## Consequences

- **`@retain` ships functional** — the second annotation after `@ttl`; only
  `@indexed` (on `Map`) and `@bounded` remain registered-and-gated.
- **`Log` reads are clock-free**; only `append` needs `given Clock` (D2/D3) — a
  cleaner split than `Cache`, because the window arguments are explicit `Instant`s.
- **The query track's two `Log` carryovers are discharged** — the time-window
  builders (D3) and the `Map × Log` join (D5).
- **The non-idempotent append is the documented exception** (D2); compiler
  enforcement is out of scope, and `Idempotency` (§12) is the future safe-use
  story the guidance references.
- **Rehydration** validates refined entry types on load (as every kind); the
  failure-mode *shape* (storage track Q6) and refinement-tightening migration
  (Q7) stay deferred — not `Log`-specific.
- **Deferred follow-ons:** exposing the per-entry timestamp to `map`/`filter`;
  secondary `@indexed` on `Log` value fields (only the time index is v1); active
  / alarm-based retention; and per-entry DO storage keys (the query track's
  deferred I/O win, which would also bound an append's read-modify-write of the
  array).
- **Rejected alternatives.** (a) A `Record<string, …>` representation — loses the
  intrinsic order a `Log` is defined by (D1). (b) **Required** `@retain` — a `Log`
  legitimately keeps everything (audit trails); optional is honest (D4). (c)
  Retention enforced lazily on **read** (the `Cache` model) — would make every
  windowed read consult the clock; prune-on-append keeps reads clock-free (D4).
  (d) Exposing `{ t, v }` as the query element — §11's worked queries read the
  value (`e.kind`, `e.payload`), so the value is the element and the timestamp
  stays implicit (D3).
