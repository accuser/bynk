# 0384 — Phase 6 (`the-ir.md`, spine #1137) retires — `ast_importers` at its named floor of 5

- **Status:** Accepted (v0.249.36)

summary: The retirement PR itself — deletes the track doc, archives its closing summary, re-points every doc that named it, closes the spine issue

**Context.** P6.58 re-settled `design/tracks/the-ir.md` §5's completion criterion at `ast_importers` =
5, named §12's retirement condition as that floor, and amended phase 7's own `bynk-ts` entry condition
to match. Every slice named to reach it — §6a's P6.25–P6.41, §6b's P6.42–P6.58 — has landed. §12's own
condition is met; this slice is what carries that out.

**Decision.** `design/tracks/the-ir.md` deleted. Its closing summary appended to
`design/archive/retired-tracks.md`, carrying forward — since both live in tables the doc's own
deletion would otherwise take with it — the five-file floor's own per-file argument and the amended
`bynk-ts` entry condition, plus a survey of the track's own three arcs (IR construction, P6.0–P6.24;
the completion plan, §6a, P6.25–P6.41; the retirement plan, §6b, P6.42–P6.58) and the fifty-one ADRs
(0332–0382) that carry its decisions.

`design/bynk-compiler-trajectory.md`'s own phase-6 row marked retired, matching the format every
earlier phase's own retirement used; its §7 ("Tracks") gains a clarifying note that phase 6 is the one
case on record where a later phase's own track opened against a re-settled, argued floor rather than a
literal zero — argued in the open, before phase 7 opens, not after.

`xtask/src/greenfield_status.rs`'s own `ast_importers` doc block — the probe's own living
documentation, since the probe itself stays in the tree — re-points every `the-ir.md §N` citation at
the archived closing summary (section numbers don't survive archival, so citations become "phase 6's
own closing summary" rather than a specific, now-nonexistent section) and fixes a genuine staleness
found along the way: two sentences still read "`ast_importers` = 0 proves…", unchanged since #1184,
never updated for the re-settled floor. A new closing paragraph on `AST_IMPORTER_EXCEPTIONS` itself
records the retirement plainly, so a future reader of this file alone — without ever having read the
deleted track doc — has the floor, its own file list, and where the full argument lives.

`design/tracks/README.md`'s "Active tracks" table drops its `the-ir.md` row (removed, not marked
retired in-table — matching how every earlier retirement in this table was handled); its own prose
paragraph on the track updates to past tense and gains the floor's own headline fact.

**In-source doc comments citing `the-ir.md`** (fifteen files across `bynk-emit`/`bynk-check`) are
**left as historical references, not swept** — the same explicit decision every earlier track's own
retirement made for its own doc (`project-model.md`'s citations still live in `bynk-check/
project_model.rs` today; `semantics-in-the-checker.md`'s still live in several files under
`bynk-check/src/`). A citation naming the doc that was true when the comment was written is a
historical fact, not a broken link — matching how a comment citing a merged PR or a specific commit
stays accurate after that PR's own branch is deleted. The auto-generated `CHANGELOG.md`/`changelog.md`
entries this track produced are immutable for the same reason: each is what was true at that version,
not a live pointer.

**Consequences.** `ast_importers`: **unaffected (5)** — gated, not deleted; a regression ratchet phase
7 inherits and drives down as it builds the printer this floor's own residue names. Closes
[#1137](https://github.com/accuser/bynk/issues/1137). Opens phase 7 (the printer, per the trajectory),
entry-gated on this retirement's own amended `bynk-ts` condition rather than a probe reading that this
track's own scope could never have reached.
