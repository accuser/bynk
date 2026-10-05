---
level: patch
changelog: `bynk test` now checks that the `bynkc` it runs matches `bynk`. A different minor version prints a warning; a different major version refuses to run unless you pass `--allow-skew` or set `BYNK_ALLOW_SKEW=1`. `bynk dev` and `bynk deploy` apply the same check when `BYNK_BYNKC` points them at a separate compiler (#1675)
---

## ADR: act-on-compiler-skew
title: Where `bynk` runs a second compiler, it acts on that compiler's skew
summary: `bynk test` and the `BYNK_BYNKC` override path warn on minor skew and refuse major skew unless allowed, using `doctor`'s classification

**Context.** `bynk` links the compiler in-process for `check`, `fmt`, `dev`
and `deploy` (ADR 0101). A second, separately versioned `bynkc` runs on only
two paths: `bynk test`, which always delegates (it orchestrates `tsc`/`node`),
and the `BYNK_BYNKC` override. `compiler::resolve` already classifies that
compiler's skew from the driver (patch ignored, minor, major), but only
`bynk doctor` rendered it (ADR 0084), and nothing acted on it. A `cargo install
bynkc` on `PATH` beside a tree-built `bynk` could check code with one compiler
and test it with another, many increments apart, silently. The original
"doctor reports it" decision was taken when releases were a day apart.

**Decision.** Recommended option of #1675 (Decision A):
- **Minor skew warns.** The command prints which `bynkc` it is running and that
  it differs from `bynk`, then runs.
- **Major skew refuses.** The command prints why, and exits non-zero without
  running the compiler.
- **One override.** `bynk test --allow-skew`, or `BYNK_ALLOW_SKEW=1` (any
  non-empty value) for any command. `dev`/`deploy` take only the variable.
  Allowed, a major skew warns and runs.
- **One classification.** The verdict reuses `doctor`'s `Skew` exactly, so
  `doctor` and the commands never disagree about what counts as skew.

**Consequences.**
- `bynk test` and an overridden `dev`/`deploy` can now fail before running, on
  a major skew. `doctor`'s own exit contract (ADR 0084) is unchanged: it still
  reports, and only `--strict` turns a minor skew into a failure there.
- While Bynk is on 0.x, every release changes the minor version, so two
  different releases are a minor skew (a warning), and a major refusal can't
  trigger until 1.0. That follows the existing classification; deciding
  whether 0.x minor skew should count as major is a separate question.

Proved by `compiler.rs`'s unit tests for each verdict, and by
`bynk/tests/test_skew.rs`. That runs a real `bynk test` against a fake `bynkc`
reporting a chosen version: a minor skew warns and delegates, a major skew
refuses without delegating, and either override runs it.
