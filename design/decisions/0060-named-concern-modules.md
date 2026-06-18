# 0060 — Split sprawling files into named single-concern modules (flat is fine)

- **Status:** Accepted (v0.29 — follows the refactor track, ADR 0059)
- **Relates to:** `design/karn-engineering-roadmap.md` Part B (formerly `karn-refactor-proposal-queue.md`, now in `archive/`)

## Context
The v0.29 refactor track split the two largest files — `project.rs`
(8.3k → a parent + 8 submodules) and `checker.rs` (7k → a parent + 4
submodules) — into named per-concern modules. Both used the same
**flat-resolution** recipe: each submodule does `use super::*` and the
parent glob-re-exports each submodule, so name resolution stays flat.

That recipe makes the split **mechanical and byte-identical**, but it
also means the split buys **legibility, not decoupling** — every
submodule still sees everything, so coupling is unchanged. The track
ran the recipe ~10 times without a single behaviour regression; the
open question it left was a policy one: when, and how, should a file be
split, and is the flat (non-decoupling) split worth doing on its own?

## Decision
**Avoid sprawling source files. When a file sprawls or mixes distinct
concerns, split it into named, single-concern submodules — and a flat
split is the accepted default.** Sprawling files are cognitively hard to
read; named concern-modules make the structure legible at a glance. The
goal is legibility and bounded files, **not** decoupling, so:

- **The flat `use super::*` + parent-glob-re-export pattern is the
  default.** Do not hold out for an interface-narrowing "decoupling"
  split; it is higher-cost, often reveals the concerns are not cleanly
  separable, and is not required to get the legibility win. (A genuine
  decoupling split — a narrow `pub(crate)` interface, no glob — remains
  available where a real boundary exists, but it is a deliberate,
  separate decision, not the baseline.)
- We do **not oversell** a flat split: "`checker.rs` is now five files"
  does not mean the checker is decoupled. The flat re-export means new
  cross-dependencies can still grow freely; the win is navigation.

**Trigger (soft):** ~2,000 lines is "eye it"; ~5,000 is "split it"; and
*any* file mixing clearly distinct concerns is a candidate regardless of
size. A submodule directory already existing does **not** mean the
parent is small — check the parent's own line count (`emitter.rs` keeps
an `emitter/` dir yet the parent is still ~5.8k).

**Recipe (settled, from the track):** characterization pins on the pure
helpers about to move → land them first → move the free functions
**verbatim** → content-preservation check (every top-level item present
exactly once across the new tree) → golden/diagnostic fixtures as the
byte-identical gate → compiler-driven `pub(crate)` (only where a
cross-module call demands it). The parent keeps the type definitions +
`impl` blocks, the entry points, the shared core, and the facade; only
the free functions distribute. Behaviour-preserving throughout.

## Consequences
Future large files are split by default, cheaply and safely — the recipe
is proven and the acceptance gate is the existing fixtures. Two files
remained sprawling after the v0.29 track and are the immediate
candidates: **`emitter.rs` (~5.8k)** and **`parser.rs` (~5.3k)**; a
couple of the track's own submodules (`tests_emit.rs`,
`checker/expressions.rs`) are a secondary tier if they keep growing. The
accepted cost is that flat splits do not reduce coupling — that is a
known, deliberate trade in favour of legibility, revisited only where a
real interface boundary is worth the extra cost.
