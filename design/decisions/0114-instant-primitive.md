# 0114 — `Instant` is a distinct base type for absolute time, erased to `Int` epoch milliseconds; `Clock.now() -> Effect[Instant]`; `Instant`/`Duration` arithmetic; supersedes ADR 0112 D4

- **Status:** Accepted (query-algebra track, slice 0 settling; 2026-06-25)
- **Track:** `design/tracks/query-algebra.md` (slice 1b — the `Instant` prerequisite the track's Q4 raised, sequenced before the storage-`Map` query slice and the storage `Log` slice).
- **Realises:** `design/bynk-design-notes.md` §11 (the `Log` time-window builders `since`/`before`/`between` and instant-valued `Map` fields, which compare against an absolute time) and the track's Q4.
- **Relates / supersedes:** ADR 0112 (`Duration` — same base-type playbook; this ADR **supersedes 0112 D4**, removing the `Int`↔`Duration` timestamp-math coercion now that an `Instant` type carries it); ADR 0040 (`Float` is a distinct base type erased to `number` — `Instant` follows the same shape); ADR 0041 (no implicit numeric coercion — `Instant` *restores* the rule that 0112 D4 broke); ADR 0113 (`Cache` absolute-expiry millis — now an internal `Instant` comparison); the `Clock` capability (re-typed by D4).

## Context

ADR 0112 D4 admitted exactly one exception to ADR 0041's no-coercion rule:
`Int + Duration -> Int`, interpreting the `Int` as a millisecond instant, so
`clock.now() + 5.minutes` type-checked. It named the alternative — a distinct
`Instant` type — and **deferred** it as "a possible future refinement; D4 is
forward-compatible with it," because re-typing the shipped `Clock` capability was
a breaking change the `Cache` slice did not need.

The query-algebra track makes instants pervasive: the `Log` time-window builders
(`since`/`before`/`between`) take an absolute time, and the common `Map` query
compares a stored instant field against one (`r.expiresAt < now`). Under the bare
`Int` posture there is no type distinction between a plain count and an instant —
the very confusion `Duration` was introduced to remove, now reappearing for
timestamps. The track's Q4 chose to settle this by **introducing `Instant`** as a
prerequisite, before the query surface ossifies around bare `Int`.

A `Instant` is conceptually an absolute point on the timeline; a `Duration` is a
span. The two compose: `instant + span = instant`, `instant − instant = span`.
`Float` (ADR 0040) and `Duration` (ADR 0112) already set the precedent — a
distinct base type the checker refuses to confuse with `Int`, erased to TS
`number`.

## Decisions

**D1 — `Instant` is a sixth base type, erased to `Int` epoch milliseconds.** It
joins `Int`/`String`/`Bool`/`Float`/`Duration` as a `BaseType`, lowering to TS
`number` carrying **Unix epoch milliseconds** (the same wall-clock base as
`Clock` and `Duration`, so all time shares one unit). The distinction from `Int`
and from `Duration` is Bynk-side only, erased at runtime — exactly `Float`'s and
`Duration`'s arrangement. `Instant` is a usable type name anywhere a type is
written (`store`/record field, `let`, parameter, return): `Cell[Instant]`,
`Map[OrderId, Instant]`, `{ expiresAt: Instant }`.

**D2 — No literal form.** Unlike `Duration` (`5.minutes`), an `Instant` has **no
source literal**. A wall-clock instant written in source would be a non-portable
magic epoch number, and the only meaningful absolute "now" comes from the
runtime. Instants are **minted** from `Clock.now()` (D4) and **derived** by
arithmetic (D3) or constructed from a runtime `Int` (D5). This keeps the surface
minimal and rules out epoch-literal foot-guns.

**D3 — Operator surface: `Instant`/`Duration` arithmetic and `Instant`
comparison.**

- `Instant + Duration -> Instant`, `Duration + Instant -> Instant`,
  `Instant - Duration -> Instant` — advance / retreat an instant by a span.
- `Instant - Instant -> Duration` — the (signed, unclamped) span between two
  instants (`deadline - now`).
- `Instant < | <= | > | >= Instant -> Bool` (chronological order); `==`/`!=`
  between `Instant`s.

Rejected as meaningless and kept as `bynk.types.no_numeric_coercion` errors:
`Instant + Instant`, `Instant * Int`, and every `Instant`↔`Int` mix
(`Instant + Int`, `Instant - Int`). Time arithmetic goes through `Duration`.

**D4 — `Clock.now() -> Effect[Instant]` (the breaking re-type).** The `Clock`
capability's `now` returns `Effect[Instant]`, not `Effect[Int]`. `Clock` is the
canonical source of absolute time and minting an instant is the only way to
obtain "now." This is the breaking change Q4 accepted and 0112 D4 deferred; its
migration surface is in Consequences.

**D5 — Supersede ADR 0112 D4: no `Int`↔`Duration` timestamp-math exception.**
With a real `Instant` type carrying timestamp math (D3), the `Int + Duration ->
Int` / `Int - Duration -> Int` coercion 0112 D4 admitted is **no longer needed
and is withdrawn**. `clock.now() + 5.minutes` still type-checks — but now as
`Instant + Duration -> Instant` (D3), fully typed, with **no coercion**. Every
`Int`↔`Duration` mix reverts to a `bynk.types.no_numeric_coercion` error,
restoring ADR 0041's rule with no exception. `Instant` thus pays for itself: it
removes the one wart 0112 had to tolerate.

**D6 — Conversions are explicit, mirroring `Duration` (ADR 0112 D5 / ADR 0041).**
Two directions, no implicit bridge:

- `t.toEpochMillis() -> Int` — a value method, the escape to a raw Unix-millis
  count (for the wire, external APIs, logging).
- `Instant.fromEpochMillis(n: Int) -> Instant` — a static constructor, the way to
  build an `Instant` from a runtime `Int` (a timestamp arriving over JSON, a
  stored millis value).

**D7 — Codec and zero.** An `Instant` **serialises as a JSON number** (its epoch
millis) and **deserialises requiring an integer** (`Number.isInteger`, as a
refined `Int` / a `Duration` does — a non-integer or non-finite wire value is
rejected), so an `Instant` in a record or `store` field round-trips. Its implicit
zero is the **Unix epoch**, `Instant.fromEpochMillis(0)`.

**D8 — `Instant` is orderable.** It joins the closed orderable base set
(`Int`/`Float`/`String`/`Duration`) that the query vocabulary's
`sortBy`/`min`/`max` key on (ADR 0116), ordered chronologically. This is what
makes `events.sortBy(e => e.at)` and `reservations.min(r => r.expiresAt)` work.

## Consequences

- **Migration — the `Clock` re-type (D4) is the breaking surface.** Code that
  bound `Clock.now()` and used the result as a bare `Int` must adopt `Instant`:
  an instant-valued `let`/`store`/record field annotated `Int` becomes `Instant`;
  raw `Int` math on a former timestamp becomes `Duration` arithmetic or an
  explicit `toEpochMillis()`. `clock.now() + 5.minutes` is unchanged at the
  call-site (now typed `Instant`). A `bynk-fmt` codemod re-annotates the common
  cases (instant-typed `let`/fields fed by `Clock.now()`); residual `Int`-on-
  instant arithmetic surfaces as a type error the developer resolves. Fixtures and
  first-party stdlib using `Clock.now() -> Int` migrate with the slice.
- **`Cache` eviction is internally an `Instant` comparison (ADR 0113).** A
  `Cache`'s stored absolute expiry (`exp`, computed `clock.now() + ttl`) is now
  an `Instant` at the Bynk level; the check-on-read `now >= exp` is an `Instant`
  comparison (D3). The persisted representation (a `number` in `Record<string, {
  v, exp }>`) and the surface are **unchanged** — `exp` was never user-visible.
- **Checker.** A `BaseType::Instant` (+ `name()`); `Ty::Base(Instant)` in the
  operator type rules (D3/D5 — withdraw the 0112 D4 `Int`±`Duration` arm, add the
  `Instant`±`Duration` / `Instant`−`Instant` / comparison arms); the kernel-method
  table (`toEpochMillis`, and `Instant.fromEpochMillis` static, D6); `Clock.now`'s
  return re-typed (D4); the orderable base set extended (D8); `zero_value_ts` and
  the codec (D7). No literal path (D2) — no parser/grammar/tree-sitter change.
- **Emission.** `Instant` lowers to `number` (epoch millis); `Instant`/`Duration`
  operators to the corresponding `number` arithmetic; `toEpochMillis`/
  `fromEpochMillis` to identities; `Clock.now()` to the platform `Date.now()`
  wrapper (unchanged at runtime — only the Bynk type moved).
- **Tooling.** LSP hover/completion for the `Instant` kernel and the
  `Instant.fromEpochMillis` static; no highlighting change (no new token).

## Alternatives considered

- **Stay bare `Int` (keep 0112 D4).** Rejected by Q4: no type distinction between
  a count and an instant; the confusion `Duration` was introduced to remove
  persists for timestamps, and the query/Log surface would ossify around it.
- **An `Instant` literal.** Rejected (D2): an absolute instant in source is a
  non-portable magic number; `Clock.now()` + `fromEpochMillis` cover minting.
- **Keep 0112 D4's coercion *and* add `Instant`.** Rejected (D5): two ways to do
  timestamp math, one of them an ADR 0041 violation; `Instant` lets us delete the
  exception instead of compounding it.
- **A refined `Int` (`Int where …`) rather than a base type.** Rejected: a
  refinement constrains values, not the *operator algebra*; `Instant` needs its
  own arithmetic (`Instant − Instant -> Duration`), which is a type distinction,
  exactly as `Duration` is over `Int` (ADR 0112 D1).
