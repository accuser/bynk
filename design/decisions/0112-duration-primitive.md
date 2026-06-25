# 0112 — `Duration` is a distinct base type erased to `Int` milliseconds; literal `5.minutes`; arithmetic, scalar scaling, and comparison; one sanctioned `Int`↔`Duration` mix for clock math

- **Status:** Accepted (storage track, slice 3b; 2026-06-25)
- **Track:** `design/tracks/storage.md` (slice 3b — the `Duration` prerequisite that ADR 0111 D5 sequenced before `Cache`). Unblocks `@ttl`/`@retain` (and, later, timers and `Clock` arithmetic).
- **Realises:** `design/bynk-design-notes.md` §10 (the `@ttl(30.minutes)` / `@retain(30.days)` annotation arguments) and ADR 0111 D5 (`@ttl`/`@retain` take a `Duration`, a distinct primitive, not bare `Int`).
- **Relates:** ADR 0040 (`Float` is a distinct base type erased to `number` — `Duration` follows the same playbook); ADR 0041 (no implicit `Int`↔`Float` coercion; conversions are value methods — `Duration` extends this with one deliberate exception, D4); the `Clock` capability (`now() -> Effect[Int]`, Unix milliseconds — the unit `Duration` erases to and the consumer of D4).

## Context

ADR 0111 settled the storage-annotation surface and decided that `@ttl`/`@retain`
take a **`Duration`**, not a bare `Int` — a TTL and a count must not be
interchangeable (ADR 0041's posture). It sequenced `Duration` as a prerequisite
slice (3b) before `Cache` (3c), leaving the primitive's own design to this ADR.

A `Duration` is conceptually a span of time. The language already has the right
precedent: `Float` (ADR 0040) is a **distinct base type** the checker keeps apart
from `Int`, both erased to TS `number`. `Duration` is the same shape — a span is
"a number the type system refuses to confuse with a plain count" — with one extra
need: real arithmetic (`5.minutes + 30.seconds`, `timeout * 2`, `elapsed < limit`)
and the ability to advance a `Clock` timestamp (`clock.now() + 5.minutes`). The
scope was chosen as the **full usable primitive** (literal + type + arithmetic +
comparison), not a literal-only stub.

## Decisions

**D1 — `Duration` is a fifth base type, erased to `Int` milliseconds.** It joins
`Int`/`String`/`Bool`/`Float` as a `BaseType`, lowering to TS `number` carrying a
**whole number of milliseconds** (the `Clock.now()` unit, so all time shares one
base). The distinction from `Int` is Bynk-side only, erased at runtime — exactly
`Float`'s arrangement (ADR 0040). `Duration` is a usable type name anywhere a
type is written (`store`/record field, `let`, parameter, return): `Cell[Duration]`,
`{ timeout: Duration }`.

**D2 — Literal form `<int-literal>.<unit>`, a closed unit set.** `5.minutes`,
`30.days`, `500.milliseconds`. Units are closed: `milliseconds`, `seconds`,
`minutes`, `hours`, `days` (each the obvious millisecond factor). The literal is a
**parser recognition** over existing tokens — `5.minutes` already lexes as
`IntLit` `.` `Ident` (the lexer excludes `5.` from being a float, which needs a
digit on both sides) — promoted to a dedicated `DurationLit` AST node when the
receiver is an integer literal and the field is a unit name. No new lexer token.
Only an **integer literal** receiver forms a duration (`5.minutes`); a field
access on a variable (`x.minutes`) is unaffected, and the parenthesised/method
forms (`5.round()`) are untouched. The tree-sitter grammar keeps parsing the
surface as `field_access` — the two parsers must agree on *what is valid*, not on
tree shape — so no grammar/highlighting churn.

**D3 — Operator surface (v1): `Duration`-closed arithmetic, scalar scaling, and
comparison.**

- `Duration + Duration -> Duration`, `Duration - Duration -> Duration`.
- `Duration * Int -> Duration`, `Int * Duration -> Duration` (scalar scaling).
- `Duration < | <= | > | >= Duration -> Bool`; `==`/`!=` between `Duration`s.

Subtraction is **not clamped** — `Duration` is `Int`-backed and may go negative
(a `30.seconds - 1.minute` is `-30.seconds`); clamping would hide arithmetic the
same way silent widening would. **Deferred** (named follow-ons, not v1):
`Duration / Int` (scalar division), `Duration / Duration -> Int` (ratio), and
unit-typed display. These need no new design, only later demand.

**D4 — Exactly one sanctioned `Int`↔`Duration` mix: timestamp math.**
`Int + Duration -> Int` and `Int - Duration -> Int` are admitted, interpreting the
`Int` as a millisecond instant — so `clock.now() + 5.minutes` type-checks and
yields the advanced instant (an `Int`, millis). This is the **deliberate
exception** to ADR 0041's no-coercion rule, justified narrowly: `Clock` is the
canonical time source and already speaks `Int` milliseconds, `Duration` *is*
milliseconds, and advancing a timestamp by a span is the operation the storage
kinds (`@ttl` eviction) and timers are built on. Every *other* `Int`/`Duration`
mix stays a `bynk.types.no_numeric_coercion` error (e.g. `Duration + Int`,
`5.minutes + 3`). The alternative — a distinct `Instant` type so
`Clock.now() -> Instant` and `Instant + Duration -> Instant` — is **rejected for
now**: it would re-type the shipped `Clock` capability (a breaking change beyond
this slice) for a cleanliness this one exception buys without. `Instant` remains
a possible future refinement; D4 is forward-compatible with it.

**D5 — Conversions are explicit, mirroring the numeric kernel (ADR 0041).** Two
directions, no implicit bridge:

- `d.toMillis() -> Int` — a value method (like `i.toFloat()`), the escape to a
  raw millisecond count.
- `Duration.millis(n: Int) -> Duration` — a static constructor (like
  `Float.parse`), the way to build a `Duration` from a runtime `Int`. Unit-named
  statics (`Duration.seconds(n)`, …) are a thin **deferred** convenience; the
  literal covers the constant case and `millis` covers the dynamic one.

**D6 — Codec and zero.** A `Duration` **serialises as a JSON number** (its
milliseconds) and **deserialises requiring an integer** (`Number.isInteger`, as a
refined `Int` does; a non-integer or non-finite value from the wire is rejected) —
so a `Duration` in a record or `store` field round-trips. Its implicit zero is
`0` (`0.milliseconds`).

## Consequences

- **`@ttl`/`@retain` get their argument type.** With `Duration` a real type, ADR
  0111's `@ttl(d)`/`@retain(d)` arguments are `Duration` literals; slice 3c
  (`Cache`) can make `@ttl` functional. The annotation-argument checker (ADR 0111
  D4) restricts the value to a `Duration` literal.
- **`Clock` math reads naturally.** `clock.now() + 5.minutes` is the blessed
  idiom for a deadline (D4); `deadline - clock.now()` would be `Int - Int` (a raw
  millis delta), lifted back with `Duration.millis(…)` when a span is wanted.
- **One new diagnostic surface, reused codes.** The unit set and the literal are
  enforced at parse/resolve; operator misuse rides the existing
  `bynk.types.no_numeric_coercion` and the standard binary-operator type errors —
  no new diagnostic codes are required for D3/D4 beyond what `Float` already uses.
- **Implementation seams (slice 3b):** `BaseType::Duration` (+ `name()`); the
  `DurationLit` AST node and its parser recognition; `Ty::Base(Duration)` in
  operator type rules (D3/D4) and the kernel-method tables (D5); `zero_value_ts`
  and the codec (D6); emission — a `DurationLit` lowers to its constant
  milliseconds, operators to the corresponding `number` arithmetic, `toMillis`/
  `Duration.millis` to identities. Tooling (tree-sitter unaffected per D2; LSP
  hover/completion for units and the kernel) lands with the slice.
- **Rejected alternatives.** (a) `Duration` as bare `Int` — rejected by ADR 0111
  D5 / ADR 0041 (a span is not a count). (b) An `Instant` type for clock math —
  deferred (D4), avoids re-typing `Clock` now. (c) Clamped subtraction —
  rejected (D3), hides arithmetic. (d) Implicit `Int`→`Duration` widening —
  rejected except the single D4 timestamp case.
