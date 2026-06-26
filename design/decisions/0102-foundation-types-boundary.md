# 0102 — The foundation-types boundary: `span` / `CompileError` / source-cache / diagnostics-registry live in the lowest leaf

- **Status:** Accepted (crate-decomposition track, slice 0; 2026-06-20). Direction-settling; no code lands with this ADR.
- **Realises:** the crate-decomposition track "Foundation-types boundary" ADR — "the rule that keeps the graph acyclic."
- **Relates:** [[0099]] (the layered graph this boundary makes acyclic), [[0100]] (`bynk-render` depends on this leaf *only*).

## Context

A layered graph with arrows pointing only down ([[0099]]) needs its
**foundation types** in the lowest leaf, or every other crate would have to depend
*upward* to reach them and the graph would cycle. Four things cross every layer:

- **`span`** — positions, attached to every diagnostic and every AST node.
- **`error` / `CompileError`** — the structured diagnostic every phase produces
  and `bynk-render` ([[0100]]) consumes.
- the **source-cache type** — needed to draw carets during rendering and to
  attribute diagnostics to files.
- the **`diagnostics` code registry** (`diagnostics.rs`) — the single source of
  truth for `bynk.*` codes. It is *about* `CompileError.category`, but it also
  **generates `docs/src/reference/diagnostics.md`** and is **pinned by a
  workspace test** (`tests/diagnostics_registry.rs`). Two other registries have
  the same cross-crate shape: `kernel_methods.rs` (dispatched by the checker,
  *read by the LSP* for `.`-member completion, pinned by `tests/kernel_registry.rs`).

Get the placement wrong and "the whole graph fights back."

## Decision

`span`, `error` / `CompileError`, the **source-cache type**, and the
**`diagnostics` code registry** live in the **lowest leaf, `bynk-syntax`**. This
is the hinge that keeps the graph acyclic: diagnostics, positions, codes, and
caret-drawing source cross every crate without any crate depending upward.

`diagnostics.rs`'s home is therefore **`bynk-syntax`** (settled-pending this
ADR). The doc generation it drives (`docs/src/reference/diagnostics.md`) travels
with it.

**The now-cross-crate drift tests get explicit homes** (they were single-crate
`bynkc` tests; the decomposition splits the two halves they compare):

- **`diagnostics_registry`** (codes-vs-usage; spans all phases) — the codes live
  in `bynk-syntax` but are *emitted* across `bynk-check` / `bynk-emit`. It moves
  to a **workspace integration test** that dev-depends on the crates whose
  emission it scans, comparing them against the `bynk-syntax` registry.
- **`kernel_registry`** (registry-vs-checker-dispatch) — `kernel_methods` is
  dispatched in `bynk-check` but read by the LSP in `bynk-ide`. The test needs
  **both halves visible**, so it lands as an **integration test in `bynk-ide`'s
  suite, dev-depending on `bynk-check`**. This must be pinned before the
  `bynk-check` extraction (track slice 3) or it silently blocks the `bynk-ide`
  extraction (slice 5).

The captured-but-IDE-consumed tables (`expr_types`, `locals`, `kernel_methods`)
are produced in `bynk-check` and queried in `bynk-ide` — the check↔IDE seam,
settled in the `bynk-check` slice, not here. This ADR fixes only the *foundation*
leaf and the *drift-test* homes.

## Consequences

- `bynk-syntax` is a true leaf carrying the cross-cutting vocabulary: any crate
  can produce a spanned `CompileError` with a registered code and any crate can
  render it, all without an upward edge.
- The two registry drift guards (`decisions_index`-style CI truths) keep working
  across the crate split — but as **integration tests** dev-depending on the
  crates they compare, not in-crate unit tests. Their new homes are fixed here so
  the extraction slices don't each re-litigate them.
- `kernel_registry`'s home (`bynk-ide` dev-depending on `bynk-check`) is a
  precondition of slice 5; naming it now is what stops slice 3 from silently
  stranding it.
- Placing the source-cache type in `bynk-syntax` is what lets `bynk-render`
  depend on `bynk-syntax` **only** ([[0100]]) while still drawing carets.
