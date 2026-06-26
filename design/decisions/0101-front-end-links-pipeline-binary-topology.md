# 0101 — The front-end links the pipeline; a thin `bynkc` binary survives (binary topology)

- **Status:** Accepted (crate-decomposition track, slice 0; 2026-06-20). Direction-settling; no code lands with this ADR.
- **Realises:** the crate-decomposition track decision **D2**.
- **Amends:** [[0083]] — the note that the `bynk` driver "does not link the pipeline" and *shells* the `bynkc` binary for compilation. That rule existed **because** `bynkc` was a monolith; once the pipeline is libraries it no longer holds.
- **Relates:** [[0099]] (the library set the front-end links), [[0100]] (`bynk-render`, shared by both binaries), [[0098]] (the v0.59 `bynkc test --format json` whose deferred `bynk test` is re-mechanised here).

## Context

[[0083]] kept `bynk` a *thin orchestrator* that **shells** the `bynkc` binary
(`BYNK_BYNKC` override → PATH → sibling) and deliberately "does not link the
pipeline" — to avoid pulling the whole compiler monolith into the driver. That
rationale evaporates under the decomposition ([[0099]]): the pipeline is now leaf
libraries, so the driver can **link the leaves** and obtain structured data
in-process, with no monolith to avoid.

Two facts sharpen the call:

- `bynk` **already links the `bynkc` lib** for leaf helpers —
  `bynkc::lexer::tokenize` (`new.rs`), `bynkc::read_project_paths` (`main.rs`),
  `NODE_MAJOR_FLOOR`. So this re-points an *existing* lib dependency at leaves;
  it does not introduce linking from scratch.
- Shelling a separate binary is *why* [[0083]] owes a **driver↔compiler skew
  check**. Linking the libraries in-process removes the skew surface for the
  linked path entirely — there is no second binary to drift.

## Decision

**The human front-end links the library set** ([[0099]]) and gets structured
diagnostics in-process via `bynk-render` ([[0100]]) — it no longer shells `bynkc`
for the linked path. This supersedes the [[0083]] "driver does not link the
pipeline" rule (already partially untrue).

**A thin `bynkc` binary survives.** `bynkc` is reduced to the pure
`compile`/`check` path (a minimal binder over the libs + `bynk-render`; it
retains `cli.rs`), kept for **CI/build determinism** and **`cargo install
bynkc`**. `bynk` becomes the human front-end. Two binaries, with clear roles —
`bynkc` is the pure-pipeline tool, `bynk` is the human workflow CLI — rather than
dissolving `bynkc` into `bynk` (which would break `cargo install bynkc`, CI
invocations, and the `vscode-bynk` `bynkc`/`bynkc-lsp` resolution order).

**Knock-on — v0.59 `bynk test`.** [[0098]] deferred `bynk test` as "shell `bynkc
test --format json`". Under this ADR that deferral re-mechanises to "**link**
`bynk-emit`'s test emission and run node" — structured in-process rather than
shelled. v0.59 stays correct **exactly as shipped**; only the long-term
mechanics of the not-yet-built `bynk test` change.

## Consequences

- The driver gets structured data in-process: no subprocess, no serialise /
  reparse, no skew on the linked path. The [[0083]] skew machinery remains only
  for any path that still shells an external `bynkc` (e.g. a user-overridden
  `BYNK_BYNKC`), narrowed accordingly.
- Keeping `bynkc` preserves three contracts: `cargo install bynkc`, CI's
  pure-pipeline invocations, and the `vscode-bynk` resolution that finds
  `bynkc`/`bynkc-lsp` beside itself. None of these has to migrate.
- This ADR amends but does not retire [[0083]]: the driver remains a thin
  front-end; what changes is *how* it reaches the pipeline (link, not shell).
- The binary-topology slice is last in the track ordering (track doc §Slice 7):
  it lands after the libraries exist, and re-mechanises the v0.59 `bynk test`
  deferral onto linking at that point.
