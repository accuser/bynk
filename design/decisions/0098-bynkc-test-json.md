# 0098 — `bynkc test --format json` and a VS Code test runner

- **Status:** Accepted (v0.59)
- **Realises:** the v0.59 `bynkc test --format json` proposal (consumed and
  removed on landing, per the proposals lifecycle; history in `git log --
  design/proposals/`).
- **Relates:** [[0083]] (the `bynk` driver — a future `bynk test` shells `bynkc`
  rather than linking it), [[0009]] (integration tests run a simulated wire in
  Node), [[0071]] (the `bynkc check --format` selector this sits beside).

## Context

`bynkc test` reported results only as human `✓ / ✗` lines plus an exit code —
nothing a CI gate or an editor could read structurally. This increment adds a
`--format json` surface and a VS Code Test Explorer on top of it. It deliberately
does **not** extract a `bynk-test` crate or add a `bynk test` driver command: the
extraction's only forcing motivation was a driver command, and that can be added
later by **shelling** `bynkc test --format json` (no library linkage, so no
`bynkc → bynk-test → bynkc` cycle) — exactly how `dev`/`doctor` shell `bynkc`.
No language surface changes.

## Decision

**(D0, spike-first) Real assertion locations.** A precondition spike found the
shared `assert` lowering emitted only a **byte offset** (`"offset 104"`) with no
source path, so *neither* unit nor integration failures carried a usable
location, and the emitted runtime could not derive line/col. Rather than ship
click-through against a location that didn't exist, the lowering now emits a
**project-root-relative `path:line:col`** (forward-slashed, so it is identical on
Windows), computed at emit time from the assert's span + the test file's source.
This uplifts unit and integration uniformly — the location mechanism isn't
special-cased per kind. It also improves the human `assertion failed at …` line,
so the proposal's "byte-for-byte unchanged human output" softens to
**structurally unchanged** (the assert-failure location is now a real
`path:line:col`).

**(D1) A `bynkc`-consistent `--format` vocabulary.** `bynkc test --format` is a
**per-command** `{ rich, json }` whose value names match `bynkc check`'s `rich`
([[0071]]) rather than the driver's `human` — in-binary consistency wins. It is
*not* a shared enum: `test` has no `short` behaviour yet, and a value that parses
but does nothing is worse than its absence. The human `rich` default is
unchanged.

**(D2) NDJSON is an internal protocol; the document is the surface.** The
generated runner emits one JSON event per line when `BYNK_TEST_FORMAT=ndjson`;
`run_test` captures that stream and renders the single pinned **document**.
NDJSON is a runner↔driver protocol, *not* a consumer contract — chosen over
having Node `JSON.stringify` the whole document because (a) the compile-failure
path never runs Node, so `run_test` must own a document emitter anyway — one
emitter, no drift; (b) a mid-run crash leaves a complete NDJSON *prefix* (a
truncated JSON blob would be unparseable); (c) it leaves a clean path to live
streaming later. The document is built from `#[derive(Serialize)]` structs in
**declaration order** — field order *is* the contract (`bynk/src/report.rs`
discipline); never `json!`, so `preserve_order` unification can't reorder it.

**(D3) Three terminal states on a closed `error.kind`.** A consumer keys on
`error`/`error.kind` (`compile | runtime`): a **normal** run has `suites` and no
`error` (it may have `failed > 0`); a **compile** failure has no `suites` and
`error.kind == "compile"` carrying the `bynkc` `path:line:col: severity[category]:
message` diagnostic lines (the shape the editor's `$bynkc` matcher re-parses —
factored out of the existing short printer); a **crashed** run carries the
observed `suites` prefix *and* `error.kind == "runtime"` with the captured
stderr. The **exit code follows the runner's own process status**, so a mid-run
crash (a complete prefix but no `run-end`) is never reported as success.

**(D4) Editor wiring shells the JSON.** The VS Code Test Explorer runs `bynkc
test --format json` (resolving `bynkc` the same way the `bynkc: check` task does)
and reports per-case results; a failing case's `location` becomes a
`vscode.Location` for click-through. Failure handling branches on `error.kind`:
`compile` → the Problems panel (the `$bynkc` shape); `runtime` → a run-level
note, not a diagnostic. Discovery is **lazy from a run**.

**(D5, testing) Toolchain-free goldens.** The document goldens are built from
synthetic models (the `bynk doctor` precedent — deterministic, no toolchain); the
NDJSON→document parser is exercised on fixture streams including a
truncated/crashed one; the runner's NDJSON emission is pinned by the e2e
emitted-source snapshot. A true end-to-end (Node actually emitting NDJSON) stays
in the toolchain-gated suites, off the golden CI gate.

## Consequences

CI and tooling get a stable, pinned results document, and the editor gets a Test
Explorer with click-through to the failing `.bynk` line — the felt goal. Because
a future `bynk test` would shell this exact surface, the increment is a strict
prerequisite for the driver command either way, with no crate extraction.

The spike (D0) was the load-bearing risk at sign-off, and it changed scope: it
turned "thread the existing location through" into "compute a location that
didn't exist," touching the `assert` emission the proposal's non-goals had
fenced off. The Windows path-separator divergence it introduced was caught by CI,
not local runs — the location is now normalised at the single formatting site.

Deferred and logged as next intent: per-test timing (`durationMs` — net-new
instrumentation inside each emitted module's runner, not just `main.ts`), a
pre-execution `--no-run --format json` discovery document (case names aren't
retained Rust-side today), and live NDJSON streaming to the editor (D2 keeps the
door open). A `bynk-test` crate / `bynk test` driver command remain out of scope.
