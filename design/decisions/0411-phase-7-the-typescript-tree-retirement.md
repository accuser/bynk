# 0411 — Phase 7 (`the-typescript-tree.md`, spine #1293) retires — all four gated probes at their own argued floor

- **Status:** Accepted (v0.289.48)

**Context.** §12's own retirement condition — `ts_writes`, `verbatim_origins` and
`verbatim_sites` each reading their own argued floor — was met once #1501 (`ts_writes`, ADR
0409) and #1502 (`verbatim_origins`, ADR 0410) landed; `verbatim_sites` (ADR 0399, confirmed
unchanged by the #1486 capstone) and `ts_any` (ADR 0404) had already settled earlier. Every
slice named to reach these floors — Arc A through Arc F, #1462's and Arc F's own residual
grounding, the verbatim_sites capstone — has landed. Nothing left is a design question; this
slice carries out §12's own already-written retirement procedure, the same shape ADR 0384 did
for phase 6.

**Decision.** `design/tracks/the-typescript-tree.md` deleted. Its closing summary appended to
`design/archive/retired-tracks.md`, carrying forward the four probes' own final floors and their
full arguments (`ts_any` 26/ADR 0404, `verbatim_sites` 2/ADR 0399+0407, `ts_writes` 809/ADR
0409, `verbatim_origins` 1/ADR 0410), a survey of the track's own six arcs (A: independent
probes/TOML/wrangler; B: the `bynk-ts` crate and node algebra; C: the bulk per-file conversion;
D: the R8 rule closures and IR crate carve; E: `serialisation.rs`; F plus the closing
floor-arguing arc: every named residual resolved rather than left open-ended), and the
twenty-one ADRs (0385–0410, minus five numbers belonging to a concurrent, unrelated
property-generator track) that carry its decisions.

`design/bynk-compiler-trajectory.md`'s own phase-7 row marked retired, matching the format every
earlier phase's own retirement used, with phase 8's row marked openable; its §7 ("Tracks") gains
a second exception note — phase 7, like phase 6 before it, opened its own successor against
argued, non-zero floors rather than a literal zero, the discipline applied a second time rather
than assumed to transfer automatically.

`design/tracks/README.md`'s "Active tracks" table drops its `the-typescript-tree.md` row
(removed, not marked retired in-table, matching how every earlier retirement in this table was
handled); its own prose paragraph on the track updates to past tense and gains the four floors'
own headline facts. The "Retired tracks" list below it gains concise entries for both
`the-typescript-tree.md` and `the-ir.md` — the latter's own entry had never been added when
phase 6 retired, the same documentation gap this table's own prose already named as a precedent
failure mode (`compiler-architecture.md`'s retirement left an identical gap, caught only when
phase 3 retired in turn) — closed now rather than left for a future retirement to catch again.

`xtask/src/greenfield_status.rs`'s own four phase-7 probe doc comments (`ts_writes`, `ts_any`,
`verbatim_origins`, `verbatim_sites`) — the probes' own living documentation, since all four stay
in the tree — re-point every `the-typescript-tree.md §N` citation at the archived closing summary
(section numbers don't survive archival) and update each probe's own "reading is currently X,
converging toward a floor" language to record the actual settled floor and its owning ADR,
the same correction ADR 0384 made for `ast_importers`'s own doc block.

**In-source doc comments citing `the-typescript-tree.md`** (dozens of files across `bynk-emit`/
`bynk-ts`) are **left as historical references, not swept** — the same explicit decision every
earlier track's own retirement made for its own doc (`the-ir.md`'s citations still live in
`bynk-emit/src/emitter.rs` and others today). A citation naming the doc that was true when the
comment was written is a historical fact, not a broken link. The ADR files under
`design/decisions/` and the auto-generated `changelog.md` entries this track produced are
immutable for the same reason: each is what was true at that version, not a live pointer.

**Consequences.** All four probes: **unaffected** — gated, not deleted; regression ratchets a
future phase inherits, the same precedent `ast_importers` set for phase 6. Closes
[#1293](https://github.com/accuser/bynk/issues/1293). Makes phase 8 openable per the
trajectory's own "a phase's track opens when the previous phase's probe reads zero" rule — not
opened by this PR itself; a fresh settling review still needs to ground phase 8's own spine
against the current tree, the same discipline this track's own opening applied to phase 6's.
