# 0314 — ADR 0309's Structural tier requires an LSP-surface fixture for consumer crates with developer-facing behaviour

- **Status:** Accepted (v0.247.5)

**Context.** ADR 0309 (`design/decisions/0309-refactor-acceptance-gate-per-tier.md`) defines the
refactor acceptance gate per tier: Enablers / Paydown / Structural / Layering. "Structural" — the tier
Tier B (the typed hoist) ran under — requires crate-local fixtures over the in-memory `sources` seam,
a named regression fixture per closed defect, and byte-identical goldens.

Phase 3 (`design/tracks/identity-and-totality.md`, spine #1046) resembles Structural in shape, but its
blast radius differs in kind from Tier B's: Tier B touched one crate's internals (`bynk-emit`) and a
byte-identical-emission gate was a real test of it. Phase 3 touches the **read** side of crates that
ship developer-facing behaviour today — `bynk-ide` and `bynk-lsp`'s hover, completion, and live
diagnostics, per the pipeline review's own finding that a span collision already produces a wrong
hover result today ("hover over an else-less `if`'s then-branch reports `()` where the branch is
`Effect[()]`"). A byte-identical-emission gate says nothing about whether that class of regression is
introduced or fixed by a given slice.

**Decision.** ADR 0309's Structural tier gains one added requirement, effective for any track citing
it whose slices touch a crate with LSP-facing behaviour: each such consumer crate carries an
LSP-surface fixture (hover, completion, or diagnostic-shape, as applicable to what the slice changes)
in addition to any emitted-TypeScript fixture already required. The gate's overall shape is unchanged
— fixture-backed, byte-identical goldens, per-defect regression fixtures — this adds a coverage
requirement, not a new tier.

**Consequences.** `identity-and-totality.md`'s T3.1 (and later slices touching `bynk-ide`/`bynk-lsp`)
must ship an LSP-surface fixture per migrated consumer crate, not only a `bynkc` emission fixture. Any
future Structural-tier track whose slices touch `bynk-ide` or `bynk-lsp` inherits the same
requirement.
