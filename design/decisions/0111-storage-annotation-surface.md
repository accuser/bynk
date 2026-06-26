# 0111 — Storage annotations: a closed `@name(args)` registry gated per slice; `@ttl`/`@retain` take a `Duration` literal that lands first

- **Status:** Accepted (storage track, slice 3 settling; 2026-06-25)
- **Provenance:** the storage track (the open question Q3 — "annotation grammar and the closed annotation set; which are v1"). Unblocks the `Cache` half of slice 3.
- **Realises:** `design/bynk-design-notes.md` §10 ("Storage Types" — the refinement annotations `@indexed`/`@ttl`/`@retain`/`@bounded` and the `Cache[K, V]` TTL-bounded kind).
- **Relates:** ADR 0108 (`store` replaces `state`; the `StoreField` whose grammar §10 leaves an annotation slot in — `ast.rs` deferred the field to this ADR); ADR 0110 (the `Map` slice named `@indexed` a follow-on); ADR 0041 (no numeric coercion — named conversions) which forces `Duration` to be a distinct type, not bare `Int`; the `Clock` capability (`now() -> Effect[Int]`, Unix **milliseconds**) which fixes the unit a `Duration` lowers to.

## Context

The storage-field grammar has carried an annotation slot since slice 1 —
`store <name>: <Kind>[…] [@annotations] [= init]` (§10) — but the slot is empty:
`ast.rs` records "access-pattern annotations are deferred until their grammar
settles (storage track Q3), so there is no annotations field yet." There is no
`@`/`At` token in the lexer; the surface is greenfield.

Design-notes §10 names four annotations — `@indexed(by: …)` (Map access pattern),
`@ttl(…)` (Cache eviction), `@retain(…)` (Log retention window), `@bounded(…)`
(size cap) — and calls them "refinement annotations [that] add information about
access pattern and constraints **without dictating implementation**." They are
metadata on a field, not operations.

Settling Q3 is the gate on the `Cache` half of slice 3: a `Cache[K, V]` is "`Map`
ops + TTL eviction" (§10), so a *useful* Cache is exactly a Map plus a `@ttl`
default — without the annotation surface a Cache is indistinguishable from a Map.
And `@ttl` needs a **time value**, which surfaces the real fork: Bynk has no
`Duration` type (time is plain `Int` Unix-millis via `Clock.now()`). The choice
recorded here (over an `Int`-millis literal or a grammar-only deferral) is to
introduce a `Duration` literal **first**, so `@ttl(5.minutes)` reads as §10
writes it and `@retain(30.days)` reuses the same value.

## Decisions

**D1 — Grammar: a leading `@`, an annotation name, and an optional parenthesised
argument list, in field-declaration position.** An annotation is `@name` or
`@name(arg, …)`. Annotations sit **after the storage kind and before the `=`
initialiser** (`store reservations: Map[K, V] @indexed(by: orderId) = {}`), and a
field may carry **more than one**, whitespace-separated (`@ttl(5.minutes)
@bounded(10000)`). The lexer gains a single `@` (`At`) token; the parser attaches
a `Vec<Annotation>` to `StoreField` (filling the slot `ast.rs` reserved). `@`
appears **only** in this position — it is not an expression operator and not a
general attribute surface (no `@`-on-handlers/agents/types); keeping it local to
`store` fields is what makes it cheap and unambiguous.

**D2 — A closed registry of four names; unknown names are a diagnostic.** The
annotation vocabulary is exactly `@indexed`, `@ttl`, `@retain`, `@bounded` —
**not user-extensible**. An unrecognised name is rejected
(`bynk.store.unknown_annotation`), the same closed-catalogue discipline the
storage *kinds* already follow (`bynk.store.unknown_kind`). This is the §2 stance
against an open dialect: annotations are language features with checker meaning,
not free-form decorators.

**D3 — Each annotation is valid on specific kinds, and is gated to the slice that
implements it.** The registry pins each name to the kind(s) it attaches to and to
a support state, so the *grammar* lands once and the *meanings* arrive with their
kind slices — the same parse-everything / gate-the-unsupported pattern the kinds
use (`bynk.store.kind_unsupported`):

| Annotation | Valid on | Argument | Functional in |
|---|---|---|---|
| `@ttl(d)` | `Cache` | a `Duration` | the Cache slice (slice 3) |
| `@retain(d)` | `Log` | a `Duration` | the Log slice (slice 4) |
| `@indexed(by: f, …)` | `Map` (later `Set`/`Log`) | one or more field-name labels | with the query-algebra track |
| `@bounded(n)` | `Queue`, `Log` | an `Int` literal | the Queue/Log slices |

An annotation used on the wrong kind is `bynk.store.annotation_kind_mismatch`; an
annotation whose slice has not landed is `bynk.store.annotation_unsupported`. The
v1 deliverable of the **annotation slice** is D1+D2+D3 — the token, the AST, the
closed registry, and the gating — with **`@ttl` the first to become functional**
(in the Cache slice, once `Duration` exists per D5).

**D4 — Annotation arguments are compile-time constant literals, not runtime
expressions.** An annotation is metadata fixed at the declaration; its arguments
are restricted to literals (and the `by:` field-name labels of `@indexed`), never
arbitrary expressions, capabilities, or `store`-field reads. This keeps
annotations evaluable at compile time and out of the effect/dataflow surface.
`@ttl` and `@retain` therefore take a **`Duration` literal**; `@bounded` an `Int`
literal; `@indexed` bare field-name identifiers.

**D5 — `Duration` is a distinct primitive, introduced as a prerequisite slice
ahead of Cache, spelled as a postfix-unit literal.** `@ttl(5.minutes)` requires a
`Duration` value, and per ADR 0041 (no numeric coercion) it must **not** be bare
`Int` — a TTL and a count should not be interchangeable. So:

- **Literal form:** `<int-literal>.<unit>`, e.g. `5.minutes`, `30.days`,
  `500.milliseconds`. Units are a closed set: `milliseconds`, `seconds`,
  `minutes`, `hours`, `days`. This reuses existing tokens — `5.minutes` lexes as
  `IntLit` `.` `Ident` (the lexer already excludes `5.` from being a float, which
  needs a digit on both sides) — so the literal is a **parser** recognition in
  duration position, not a new lexer token.
- **Type:** `Duration`, a foundation type that **lowers to `Int` milliseconds**
  (the `Clock.now()` unit), so all time arithmetic shares one base. It is a
  newtype, not an alias — `Duration` and `Int` are not assignable to each other.
- **Scope of the prerequisite slice:** just the literal and the type, enough for
  `@ttl`/`@retain` to name it. `Duration` **arithmetic and comparison**
  (`d1 + d2`, `clock.now() + d`, `d < timeout`), conversions, and codec ride
  `Duration`'s own ADR/slice; they are not needed to settle Q3.

**D6 — `@ttl` eviction is lazy, checked on read, against the agent's clock
(sketch; settled in the Cache slice).** Making `@ttl` *functional* implies an
eviction model. The intended lowering, recorded here so D5's `Duration` is the
right shape: a `Cache[K, V]` persists each entry as `{ value: V, expiresAt: Int }`
(insertion-time `now() + ttl`, in millis); a `get`/`contains` treats an entry past
`expiresAt` as **absent** (returns `None`/`false`) and lets the next commit reap
it. Eviction is **lazy** (check-on-read) — no alarms, DO-friendly — and reads the
**agent runtime's** millisecond clock at the read site, not the handler's `Clock`
capability, so Cache reads stay sugar-free of a `given Clock`. The precise
representation, the per-`put` TTL override, and active reaping are the Cache
slice's to finalise; Q3 only commits to "`@ttl` carries a `Duration` default."

## Consequences

- **Q3 is closed.** The annotation grammar (D1), the closed set (D2), the
  per-kind/per-slice gating (D3), and the argument model (D4) are settled; "which
  are v1" is answered by D3's table — the grammar+registry are v1, `@ttl` is the
  first functional annotation.
- **A new prerequisite enters the track.** `Duration` (D5) is a language-wide
  primitive (it will also serve `@retain`, timers, and `Clock` arithmetic), so it
  is sequenced as its own slice **before** Cache, with its own ADR for the full
  arithmetic/comparison surface. The storage track's slice table and Q-list are
  updated accordingly: the `Cache` half of slice 3 now depends on an
  **annotation-surface** slice and a **`Duration`** slice.
- **Re-sequencing of slice 3.** `Set` shipped (v0.84) as the first half of the
  original "Set + Cache" slice 3. The remainder becomes: (3a) annotation surface —
  token, AST, closed registry, gating; (3b) `Duration` literal + type; (3c)
  `Cache` (+ `@ttl` functional, the lazy-eviction lowering of D6).
- **Three new diagnostics** are reserved for the annotation slice:
  `bynk.store.unknown_annotation` (D2), `bynk.store.annotation_kind_mismatch` and
  `bynk.store.annotation_unsupported` (D3).
- **Rejected alternatives.** (a) `@ttl(<Int millis>)` — fastest to ship, but ADR
  0041 forbids treating a duration as a bare count, and it would print a magic
  number at every Cache declaration. (b) Settle the grammar but defer `@ttl`'s
  value type — leaves Cache non-functional after the slice, deferring the real
  question rather than answering it. (c) An open/extensible annotation surface —
  rejected by §2 (no open dialect); annotations carry checker meaning.
