# Feature tracks

Persistent design docs for **far-reaching, multi-increment language features** —
the artefact introduced by [ADR 0076](../decisions/0076-feature-track-posture.md).

A feature track applies when a feature has two or more of: it spans several
increments, its surface is not yet settled, or it is a security/safety boundary.
For everything else, the standard single-increment
[proposal](../proposals/README.md) still applies.

## What a track doc is — and isn't

- **Persistent, not transient.** Unlike a proposal (deleted by the PR that
  implements it), a track doc lives for the whole feature: it is the living map
  the per-slice proposals are cut from, updated as each slice lands, retired only
  when the theme completes.
- **A realisation of the design notes, not a replacement.** It sharpens the
  conceptual commitment in `../bynk-design-notes.md` into a concrete surface,
  an internal architecture, a security/threat model, and an ordered slice
  decomposition. The design notes stay the north star.
- **Not a build authorisation.** Merging a track doc settles *direction*. Each
  slice is still an ordinary `vX.Y-<slug>.md` proposal under `../proposals/`,
  citing this doc and the foundational ADRs; *merging that proposal* is the
  approval to build, per `../proposals/README.md`.

## Lifecycle

1. **Settle.** Draft the doc; close its open design questions (investigation +
   prior art); land the load-bearing, hard-to-reverse **ADRs up front**.
2. **Slice.** Cut each increment as an ordinary proposal that cites the doc and
   the ADRs; build / land / delete as usual. Mark the slice done here.
3. **Retire.** When the last slice lands, the doc is removed (or archived); its
   decisions live on in the ADRs and the spec-in-place.

## Active tracks

- [`actors.md`](actors.md) — actor declarations as boundary contracts: the
  `actor` declaration, the `by` handler clause, authentication schemes, and
  identity. **Phase: ✅ COMPLETE — Q1–Q7 shipped (v0.45–v0.54).** Inaugural
  feature track. (Q8 replay/ordering deferred to a future Events track.)
