---
level: patch
changelog: The phase-3 track (identity and totality) settles on ExprKey(Span) scaffolding before ExprId, and amends ADR 0309 with an LSP-surface-fixture requirement
---

## ADR: phase-3-scaffolding-before-retrofit
title: Phase 3 opens on ExprKey(Span) scaffolding, not a direct ExprId retrofit
summary: The migration technique for identity and totality — cheap scaffolding first, proven mechanics before the arena-index change
status: Accepted

**Context.** `design/tracks/identity-and-totality.md` (spine #1046) opens phase 3 of the compiler
trajectory. The 2026-07-27 pipeline review explicitly rejected a naive full `NodeId` retrofit as too
large — "a large change across a 2,806-line AST and three consumer crates" — and named three cheaper
steps as "the migration scaffolding if the retrofit ever happens": newtype the key as `ExprKey(Span)`,
give blocks their own map, add a debug-only uniqueness check, and replace the
`_ => "unknown".to_string()` miss branch with a loud internal error.

The track doc's first draft argued that scoping condition ("if the retrofit ever happens") no longer
applies, since this track *is* the retrofit's opening, and recommended allocating `ExprId` at parse
time directly instead of the newtype.

**Decision.** The draft's recommendation is reversed. The track lands `ExprKey(Span)` first, as its
own complete slice (T3.1/T3.2), not as a detour. Real `ExprId` allocation at parse time, and the total
`IndexVec<ExprId, TyId>` it enables, is a later slice (T3.4) in the same track, gated on T3.1 having
proven the per-consumer-crate migration mechanics (dual-map, cutover one crate at a time, old form
deleted last) across all seven crates that read the span-keyed channel today — `bynk-check`,
`bynk-emit`, `bynk-ide`, `bynk-lsp`, `bynk-syntax`, `bynk-wasm`, and `bynkc`'s test suite.

The reasoning: "the world where phase 3 wasn't open" was the *scaffolding's* scoping condition, not a
reason to skip it. `ExprId`-at-parse touches the parser and every one of the seven consumer crates at
once, and leaves open the arena/generational-index question of what happens to a held `ExprId` across
a re-parse — a question R2.4 does not itself answer. `ExprKey(Span)` touches neither: it is a
type-alias-shaped change over ~11 signatures, already fully specified by the review, and it closes two
real defects (bug #844's class, the else-less-`if` collision) on its own merits whether or not
`ExprId` ever lands. This is the same "parallel data, single pipeline" technique
`design/tracks/compiler-architecture.md` (retired) named for this phase, applied one layer earlier
than its own forward reference assumed it would be.

**Consequences.** T3.1/T3.2 are gated on nothing but this decision and are ready to slice. T3.4 (real
`ExprId`) is explicitly *not* pre-designed here — cutting its signature now, before T3.1 has shipped
and proven the seven-crate migration mechanics, would be exactly the "an unopened phase whose slices
are already written is a wish list" failure `compiler-architecture.md` §7 warned against one level up.
If the scaffolding's mechanics turn out not to generalise across all seven crates, the cost already
paid is one newtype, not a parser change with seven crates mid-migration.

## ADR: refactor-acceptance-gate-lsp-surface-requirement
title: ADR 0309's Structural tier requires an LSP-surface fixture for consumer crates with developer-facing behaviour
summary: Amends ADR 0309 — byte-identical emission goldens don't cover a hover/completion/diagnostic regression
status: Accepted

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
