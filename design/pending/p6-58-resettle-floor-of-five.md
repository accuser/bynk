---
level: patch
changelog: "P6.58: re-settles design/tracks/the-ir.md §5 at ast_importers = 5 (not 0), with a per-file structural argument for each of the five surviving files. Breaks the §5/§7 circularity between this track's own completion criterion and phase 7's bynk-ts entry condition. Names §12's retirement condition. Doc-only, no AST_IMPORTER_EXCEPTIONS growth."
---

## ADR: p6-58-resettle-floor-of-five

title: `design/tracks/the-ir.md` re-settles §5 at `ast_importers` = 5, breaks the §5/§7 deadlock, names §12's retirement condition

summary: Doc-only — the evidence for this floor is the whole of §6b (P6.42–P6.57), not asserted here for the first time

**Context.** §5's own completion criterion named `ast_importers` = 0, with an explicit "confirm,
don't assume" charge on itself: *"unlike R3.5's own 4/4 floor…, no analogous carve-out is known for
this probe today. That is a claim to verify at completion, not assume now."* §6a's own closing
paragraph (Fifty-second slice-history entry) found the criterion unmet at its own hand-off point —
`ast_importers` read 7, not 0 — and named two ways forward it had not itself taken: a fresh
slice-decomposition sweep of `emitter.rs`/`emitter/emit.rs`'s own remaining surface, or a second
re-settling naming the true floor. §6b (P6.42–P6.57) took both, deliberately in that order: Phase G
cleared `project.rs` entirely (7 → 5, real movement, no exclusion growth); Phase H converted every
genuinely convertible `emitter.rs`/`emit.rs`/`lower.rs` site the research found, closing several real
defects along the way, while confirming through direct tracing (not estimation) that the remainder is
structurally blocked. This slice is the re-settling §6a's own paragraph said had not yet happened —
its evidence is everything that came before it, not new investigation of its own.

**Decision.** §5 amended in place, not restated elsewhere: the opening paragraph's claim ("reads 0 …
no analogous carve-out is known") becomes "reads 5 … a carve-out is now known, named, and per-file
argued" — directly answering the charge the original paragraph issued against itself. The closing "not
a new floor" paragraph is replaced with the actual argument: one entry per surviving file
(`emitter.rs`, `emitter/emit.rs`, `emitter/lower.rs`, `emitter/workers.rs`,
`emitter/workers_entry.rs`), each citing its own structural blocker — traced during §6b, not asserted
now — rather than a bare count. The framing sentence that makes this a *floor*, not five unrelated
leftovers: the residue is exactly `bynk-emit/src/emitter{,/**}`, the TypeScript-rendering subtree
phase 7's own printer inherits.

`AST_IMPORTER_EXCEPTIONS` does **not** grow, with the reasoning recorded alongside the floor argument
itself: excluding all five would make the probe read 0 while measuring nothing (every existing probe
test would pass vacuously); excluding only `emitter/lower.rs` (its own strongest single candidate,
since #1210's stated rejection ground — a live `cap_op_param_names` walk — no longer exists after
P6.29) would buy a floor of 4 at the cost of the clean subtree statement above; file-granularity
exclusion is the same harm the #1176 exclusion's own "named not prefixed" discipline, and this
retirement plan's own decision 2, both already rule out.

**The §5/§7 deadlock, broken.** §7 (a heading this slice also adds — it was missing, leaving seven
`§7` cross-references dangling against an unheaded table) named phase 7's `bynk-ts` printer's own
entry condition as "this track's probe reads 0." But ~52 references in `emitter.rs` alone
(`ts_type_ref*`/`ts_base`/`ty_to_type_ref`/`pred_condition_and_message`) are `bynk-ts`'s own work by
P6.33's ruling — `pred_condition_and_message`'s own doc comment says it is the *one shared mapping*
for `emit::emit_pred_check` and `serialisation::emit_inline_pred_check`, already half-consumed by a
file this track excludes on exactly that phase-7 ground. The renderer family cannot leave `bynk-emit`
before `bynk-ts` exists to receive it; `bynk-ts` could not start under the old wording until the
renderer family left. Amended: `bynk-ts` enters when the probe reads its named floor (5) *and* every
file in the residue is either Q7-settled body rendering or `TypeRef`-driven codec/type rendering — the
surface `bynk-ts` is for. The probe target gave; the phase boundary did not need to.

§12 (Retirement) now names the condition as this floor, and records that the retirement PR must carry
the floor argument and the amended `bynk-ts` condition forward into `retired-tracks.md`, since both
live in tables this doc's own deletion will otherwise take with it.

**Consequences.** `ast_importers`: **unaffected (5)** — doc-only, no source changes. This is the last
slice before retirement (P6.59); §12's own condition is now met.
