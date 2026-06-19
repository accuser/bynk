# 0059 — The refactor track is behaviour-preserving, patch-versioned, trunk-based, and golden-verified

- **Status:** Accepted (v0.29.1)
- **Backlog:** `design/bynk-engineering-roadmap.md` Part B (formerly `bynk-refactor-proposal-queue.md`, now in `archive/`)

## Context
A June 2026 code-quality review of `bynkc` produced a backlog of
internal-quality refactors (four files hold ~26k of the crate's ~37k
lines). These are structural and maintainability changes only — **no
observable behaviour change, no language-surface change**. They land as
a dedicated effort with **feature work paused** for its duration.

The hot files the track restructures — `project.rs`, `checker.rs`,
`emitter.rs` — are the most-churned in the crate (touched in 34/25/36 of
the 40 commits preceding the track). A long-lived integration branch
accumulating the whole track would therefore conflict catastrophically
the moment the freeze lifts and feature work resumes in those files.

## Decision
The track runs as a sequence of **short-lived branches, each merged to
`main` on its own PR**, under a **feature freeze** enforced as policy
(the `bynk-tooling` queue is not scheduled while the track is live) —
**not** a single integration branch. The freeze removes the only strong
argument against incremental landing (conflict avoidance) without
creating one for a mega-branch; per-step landing keeps `main`
bisectable, releasable, and reviewable in tractable diffs.

Four properties hold for every increment:

1. **Behaviour-preserving.** The Bynk language and the compiler's
   observable output are unchanged. The acceptance gate is the existing
   golden fixtures passing **byte-identical and unedited**. (The track
   may change *internal Rust* signatures — e.g. the `CompileOptions`
   collapse — which is within its "internal quality" remit; the crate is
   pre-1.0 with only in-repo consumers.)
2. **Patch-versioned on the `0.29` line.** Increments bump `0.29.1`,
   `0.29.2`, … — signalling behaviour-preservation in the version and
   reserving the next *minor* (`0.30`) for the next *feature* when the
   freeze lifts.
3. **Tests precede moves.** Characterisation tests for the pure helpers a
   structural split will relocate land **ahead of** that split, as their
   own PR, green on current `main`. The before/after identity of those
   tests is the proof a move is faithful; co-shipping them with the move
   forfeits the "before". (This increment, v0.29.1, is that first pin for
   the section-1 movers; `checker.rs`'s helpers are pinned ahead of their
   own split.)
4. **No new per-increment ADRs.** Refactors are not language-defining, so
   they do not each earn a decision record; this single ADR anchors the
   whole track. A contested within-track design call is recorded as a PR
   note, not a new ADR.

## Consequences
The track is safe to interleave with a frozen feature queue and easy to
review one increment at a time; a faulty step is bisectable to one PR and
revertable in isolation. The cost is many small PRs and a patch-release
cadence on the `0.29` line — release timing stays a separate, deliberate
tag/dispatch step, so bumping a version in an increment PR does not by
itself ship anything. The proposal queue's items 12 (resolver cloning)
and 13 (version-marker convention) remain deliberately unscheduled —
12 awaits a scale signal, 13 is applied opportunistically during other
increments.
