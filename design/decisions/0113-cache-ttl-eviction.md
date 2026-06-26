# 0113 — `Cache` is a `Map` with per-entry TTL expiry; eviction is lazy (check-on-read); time comes from `given Clock`, not an ambient clock

- **Status:** Accepted (storage track, slice 3c; 2026-06-25)
- **Provenance:** the storage track (slice 3c — `Cache`, the finale of the slice-3 group: annotation surface → `Duration` → `Cache`). Makes `@ttl` the first functional annotation.
- **Realises:** `design/bynk-design-notes.md` §10 (`Cache[K, V]` — "`Map` ops + TTL eviction", TTL-bounded). **Finalises** ADR 0111 D6 (the lazy-eviction *sketch*) and flips its `@ttl` registry gate functional.
- **Relates:** ADR 0110 (storage `Map` — the op set and `Record<string, V>` persistence this extends); ADR 0111 (the `@ttl` annotation surface and the D6 sketch this supersedes); ADR 0112 (`Duration` — the `@ttl` argument type); ADR 0109 (handler-atomic commit — the overlay/flush reused); the `Clock` capability (`now() -> Effect[Int]`, Unix milliseconds — the eviction time source, D4).

## Context

`Cache[K, V]` is the last of the slice-3 kinds. Design-notes §10 defines it as
"`Map` ops + TTL eviction": a keyed collection whose entries **expire** after a
time-to-live. The two prerequisites are now in place — the `@ttl` annotation
(ADR 0111) and the `Duration` type (ADR 0112) — so the only design left is how
expiry is represented, when entries are evicted, and **where the current time
comes from**.

ADR 0111 D6 *sketched* the answer: lazy check-on-read, with the time read from
the agent runtime's ambient clock so `cache.get(k)` needs no `given Clock`. This
ADR revisits that last point and decides the other way.

## Decisions

**D1 — `Cache[K, V]` is a `Map[K, V]` with a per-entry expiry instant.** It
persists as `Record<string, { v: V, exp: number }>` — the value plus the absolute
expiry instant in **milliseconds** (the `Clock` unit). This extends the `Map`
representation (ADR 0110's `Record<string, V>`) with the `exp` field; the whole
record commits atomically at handler end like every other storage field
(ADR 0109). Provenance dispatch is unchanged (ADR 0110 D1): a `store` field of
`Cache[K, V]` is the storage cache.

**D2 — `@ttl(d: Duration)` is required on a `Cache` field; it sets the entry
lifetime.** A `Cache` *is* a TTL-bounded map, so the TTL is part of the type's
configuration, not optional. A `Cache` field without `@ttl` is a diagnostic
(`bynk.store.cache_ttl_required`) steering the author to a `Map` if they want no
expiry. `@ttl` becomes **functional** here (its ADR 0111 registry gate flips);
its argument is a `Duration` literal (ADR 0112), validated by the
annotation-argument checker. A per-`put` TTL override (`put(k, v, ttl)`) is a
named follow-on, not v1.

**D3 — Operations are the `Map` op set, with expiry applied on read.** Effect-
typed, awaited with `<-`, exactly as `Map` (ADR 0110):

- `put(k, v)` — store `{ v, exp: now() + ttl }`. Resets the entry's lifetime.
- `get(k) -> Option[V]` — an entry **past `exp` reads as `None`**; a live entry
  is `Some(v)`.
- `update(k, fn)` / `upsert(k, default, fn)` — read-modify-write; an expired
  entry is treated as **absent** (so `update` faults, `upsert` inserts), and the
  rewrite stamps a fresh `exp`.
- `remove(k)` — idempotent delete (no clock needed).
- `contains(k) -> Bool` — `false` for an expired entry.
- `size() -> Int` — counts **live** entries only (expired entries are not
  counted, even before they are reaped).

**D4 — Eviction is lazy (check-on-read), and the time comes from `given Clock`,
not an ambient clock.** No alarms: an expired entry is simply skipped on read and
**reaped at the next commit** (overwritten, or pruned when the working record
flushes). The current time is read from the handler's **`Clock` capability** —
the same `given Clock` the rest of the language uses — *superseding ADR 0111 D6's
ambient-clock sketch*. A handler that performs a time-consulting `Cache` op
(`put`/`get`/`update`/`upsert`/`contains`/`size`) must therefore declare
`given Clock`; the checker enforces it (`bynk.store.cache_needs_clock`) with the
same machinery as any other capability use. `remove` alone needs no clock.

Rationale: this is the **Bynk-idiomatic** choice. Time is an effect; routing it
through `Clock` makes the dependency **visible at the handler signature** (like
capabilities and cross-context calls) and makes eviction **testable** — a mocked
`Clock` makes expiry deterministic, where ambient `Date.now()` in generated code
would be untestable and would smuggle a hidden time dependency past the `given`
surface. The ergonomic cost — writing `given Clock` on Cache handlers — is small
and exactly the kind of honesty the language trades for elsewhere. (`Cache` ops
are already `Effect`-typed and `<-`-awaited, so the cost was already visible at
the call site; D4 makes it visible at the *signature* too.)

**D5 — Emission reuses the `Map` lowering plus the expiry envelope.** A `Cache`
field is a state-record field of TS type `Record<string, { v: V, exp: number }>`;
ops lower to entry operations over the working record `__state.<cache>`, reading
`deps`'s `Clock` for `now()`. `put` writes `{ v, exp: <clock.now()> + <ttlMs> }`
where `<ttlMs>` is the field's `@ttl` lowered through `Duration` (a constant);
`get`/`contains`/`update`/`upsert`/`size` compare `exp` against `now()`. Mutating
ops (`put`/`update`/`upsert`/`remove`) drive the implicit commit (ADR 0109), as
with `Map`.

## Consequences

- **`@ttl` ships functional** — the first annotation to do so, closing the loop
  the slice-3a registry opened. The remaining annotations (`@retain`/`@indexed`/
  `@bounded`) stay gated to their slices.
- **Cache handlers carry `given Clock`.** This is a visible, intended cost
  (D4). A read-only `cache.get` handler is effectful and clock-dependent —
  honestly so.
- **Supersedes ADR 0111 D6.** The eviction *mechanism* (lazy, check-on-read,
  `{value, expiresAt}`) is kept; the *time source* is changed from ambient to
  `given Clock`.
- **New diagnostics:** `bynk.store.cache_ttl_required` (D2) and
  `bynk.store.cache_needs_clock` (D4).
- **Deferred (named follow-ons):** a per-`put` TTL override; active/alarm-based
  reaping; `@bounded` size caps on a `Cache`; surfacing eviction as an event.
- **Rejected alternative:** the ADR 0111 D6 ambient clock — sugar-free Cache
  reads, but an untestable hidden time dependency outside the `given` surface;
  rejected as un-Bynk.
