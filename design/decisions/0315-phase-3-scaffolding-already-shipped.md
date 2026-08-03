# 0315 — ADR 0313 is superseded — no standalone ExprKey(Span) slice

- **Status:** Accepted (v0.247.6)

**Context.** [ADR 0313](0313-phase-3-scaffolding-before-retrofit.md) decided that
`design/tracks/identity-and-totality.md`'s phase 3 (spine #1046) should land the 2026-07-27 pipeline
review's `ExprKey(Span)` scaffolding — a newtype over `Span`, block-local maps, a debug-only
uniqueness check, and a loud internal error replacing the emitter's `_ => "unknown".to_string()`
fallback — as its own slice (T3.1/T3.2), ahead of a real `ExprId` retrofit.

Implementation work on that slice found this scaffolding had already shipped. Commit `43abc242`
("Wave 8: leisure batch for the compiler pipeline review", #960, 28 July 2026 — six days before this
track's spine opened) closed the review's batch 8.5 (findings #28/#46):

- Fixed the else-less-`if` span-aliasing bug at its root cause (a zero-width synthetic span,
  `bynk-syntax/src/parser/expressions.rs:1582-1589`), not merely worked around it.
- Added the debug-only uniqueness check the review proposed, scoped to `check_record`'s top-level
  `CommonsItem::Fn` loop.
- Replaced 9 of 10 `_ => "unknown".to_string()` emitter fallbacks with a loud internal error — the
  tenth (`bynk-emit/src/emitter/lower.rs:3025`) is a documented, deliberate soft-fallback for an
  already-diagnosed program, not residue.
- **Explicitly rejected the `ExprKey(Span)` newtype**, "confirmed with the user mid-implementation":
  it "changes no behavior — it's migration scaffolding for a future `NodeId` retrofit, not a bug fix,"
  against a real cost (22 files across 6 crates read `expr_types` directly).

ADR 0313 was written without checking for this — a `git log -S` on the fallback-count discrepancy
`design/tracks/identity-and-totality.md`'s own risk register had already flagged would have found it.

**Decision.** ADR 0313 is superseded. No standalone `ExprKey(Span)` slice is built. The one gap
`43abc242` left genuinely open — its uniqueness check never reaching `check_handler_body`/`check_body`
(`bynk-emit`'s entry points for service/agent handler bodies and test-case bodies, which bypass
`check_record`'s loop entirely) — is real and is closed directly, in `bynk-check/src/checker.rs`, by
extending the existing check rather than building the newtype first. What remains of phase 3's
`R2.4`/`R2.5`/`R4.9` invariant needs real `ExprId` allocation at parse time, not scaffolding toward it;
that stays unbuilt and unsliced, named as a forward reference (T3.4) in the track doc.

**Consequences.** ADR 0313 is not edited — decisions are immutable once accepted, per
`design/decisions/README.md`. Its file and its `design/decisions/README.md` index row get a
superseded pointer to this ADR's assigned number in a follow-up edit once the stamp assigns it. The
track doc's T3.1/T3.2 slice numbers retire unused; T3.3–T3.7 keep their original meaning unchanged.
GitHub issues #1053 (T3.1) and #1054 (T3.2) are closed with a pointer here rather than implemented as
originally scoped.
