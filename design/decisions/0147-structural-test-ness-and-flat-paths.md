# 0147 — test-ness is structural: a `suite` is legal in any file, stripped from the build; `[paths]` is a flat `include`/`exclude`

- **Status:** Accepted (testing track, slice 1b; v0.113).
- **Provenance:** the testing feature track's file/build slice (DECISION S). Slice 1a
  (ADRs 0145/0146) made `expect`/`suite`/`case` the one predicate surface; this slice
  settles *where* those declarations may live and *how* the toolchain finds and builds
  them.
- **Realises:** test-ness as a property of the **declaration** (a `suite`), not of a
  file's name or directory — so a `suite` is legal in any `.bynk` file (including an
  *atomic* file beside `commons`/`context`), is discovered across the whole source
  tree, and is stripped from the production build; and a flat `[paths]
  include`/`exclude` layout replacing the role-named `src`/`tests` split.
- **Relates:** ADR 0144 (one predicate surface — the language already gates `expect`
  at the *block*, so making test-ness structural is the honest extension); ADR 0146
  (`suite`/`case` — the declaration whose placement this frees); ADR 0136 (strip-only
  emission — the build's "what is emitted" discipline, here narrowed to drop test
  declarations before checking); ADR 0098 (`--format json` — the runner's
  `path:line:col` locations, now uniformly root-relative). Supersedes the v0.9.1
  split-paths model and the `#47` `.test.bynk` self-identifying suffix.

## Context

Since v0.9.1, a Bynk project separated **source** units (under `[paths] src`) from
**test** units (under `[paths] tests`), and a test file's path had to mirror its
target's qualified name under the tests root (`bynk.project.inconsistent_test_path`),
optionally with a self-identifying `.test.bynk` suffix. Test-ness was thus tied to a
file's *location* and *name*, even though the language already gates `expect` at the
`case` block and `Mock[T]` as test-only — test-ness was already a property of the
*code*, restated redundantly by the file system.

The testing track (ADR 0144) reframes every checked claim as the one invariant
predicate, and a `suite` (ADR 0146) as an ordinary top-level declaration. Once a
`suite` is just a declaration, the role-named directory split is redundant: the
compiler can tell a test from source by the keyword, wherever the declaration sits.
Two things then become possible that the split forbade — a single *atomic* file
holding `commons`/`context` **and** its `suite` (the shareable / single-file /
in-browser-playground case), and dropping the role config entirely.

## Decision

**D1 — Test-ness is a property of the declaration.** A `suite` (and `suite
integration`) is a *test-only declaration kind*. The compiler classifies each
top-level declaration by keyword; a file's name and directory carry no test/source
role.

**D2 — A `suite` is legal in any file, beside source declarations.** A single `.bynk`
file may hold more than one top-level unit — a `commons`/`context` **and** a `suite`
together (the atomic file). The parser returns all top-level units in a file, and the
project model partitions *declarations*, not files, by kind: the source units flow to
the build, the suites to `bynkc test` only. Conventionally source and tests are
separate files, but that is convention, not a rule.

**D3 — The build strips test-only declarations.** `bynkc compile`/`build` skips every
`suite` wherever it sits — never type-checked for the build, never emitted into the
deployable — while `bynkc test` compiles and runs them. So an atomic file ships its
`commons`/`context` and drops its `suite`; a pure-suite file contributes nothing to
the deployable. (Test *modules* still emit to a `tests/` subtree of the **output** for
`bynkc test`; that is an emitter convention, independent of source placement.)

**D4 — Discovery scans the whole source tree.** `.bynk` files are found by walking the
`include` roots, not a designated tests folder; test declarations are discovered
wherever they are.

**D5 — There is no `.test.bynk` filename marker.** The suffix only restated what the
`suite` keyword says; it is no longer required or meaningful (a file so named is an
ordinary `.bynk` file). A `suite` carries **no** path-identity requirement — it names
its target and is found anywhere — so `bynk.project.inconsistent_test_path` and the
suffix-normalisation are retired. **Source** units keep their path↔name identity
(`bynk.project.inconsistent_commons_name`).

**D6 — `[paths]` is a flat `include`/`exclude`, not roles.** The role-named `src` /
`tests` keys are replaced by a flat `include` (trees to compile) / `exclude` (subtrees
to skip). `[paths]` is optional: `include` defaults to the conventional roots that
exist (`src`, and `tests` when present) or the project root itself — so a conventional
`src/`(+`tests/`) project needs no config, and a flat project (`.bynk` at the root,
no `src/`) compiles with no config too. Discovery additionally skips the tool's own
`out`/`node_modules` caches and dot-directories. `tests/` survives only as a human
convention the build never mentions.

**D7 — A consequence to accept: placement is inert in both directions.** With roles
gone from config, a `context` misplaced under `tests/` now emits like any other unit,
and a `suite` under `src/` is stripped like any other test. A team wanting the old
separation enforces it with a lint, not the build.

## Consequences

- The in-browser / shareable single-file case is unblocked: one `.bynk` can hold a
  program and its tests, ship the program, and drop the tests — the playground's
  natural unit.
- The parser now yields multiple top-level units per file; the project model, the
  formatter (each unit formatted, joined by a blank line), and the tooling that reads
  a file's units all became declaration-oriented rather than one-unit-per-file.
- `bynk.project.inconsistent_test_path` leaves the diagnostic registry; the
  `.test.bynk` suffix and the split-paths `tests` root are gone from the build model
  and the docs. Existing `.test.bynk` files keep working (they are ordinary `.bynk`),
  so migration is a rename-at-leisure, not a break.
- `[paths] src`/`tests` in an old manifest are silently ignored (unknown keys), and
  the conventional default reproduces the old discovery, so existing conventional
  projects build unchanged.
- **Editor limitation (named):** the LSP's recovering parse still surfaces a file's
  *first* unit for symbol/hover/completion views; multi-unit editor awareness for
  atomic files is a follow-on. Compilation, `bynkc test`, and formatting handle all
  units.
- **Scope limitation (named):** the flat `include` supports the common one- and
  two-root layouts (a single tree, or `src`+`tests`); an arbitrary N-root monorepo
  layout is a follow-on. `exclude` prunes any subtree today.
