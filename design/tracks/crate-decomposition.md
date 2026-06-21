# Tooling track — Crate decomposition: `bynkc` becomes a library set, the driver becomes the front-end

- **Phase:** **🟢 In progress — slices 0–5 landed** (ADRs 0099–0102;
  `bynk-syntax` v0.60; `bynk-fmt` v0.61; `bynk-check` v0.62; `bynk-emit` v0.63;
  `bynk-ide` v0.64).** The library set is fully extracted and `bynk-lsp` no longer
  links `bynkc`. **Two slices remain:** slice 6 (introduce `bynk-render` — the
  shared ariadne/`short`/`json` layer) and slice 7 (binary topology). The
  load-bearing ADRs
  landed up front per ADR 0076: [0099](../decisions/0099-crate-layering-dependency-direction.md)
  (layering & dependency direction), [0100](../decisions/0100-structured-data-rendering-separation.md)
  (structured-data / rendering split), [0101](../decisions/0101-front-end-links-pipeline-binary-topology.md)
  (binary topology), [0102](../decisions/0102-foundation-types-boundary.md)
  (foundation-types boundary).
- **Realises:** the standing "keep `bynkc` focused on the compilation pipeline"
  concern — `bynkc` today is compiler + formatter + IDE engine + test-emitter +
  CLI, and it only grows as language features land. Sharpens
  `bynk-tooling-roadmap.md` (crate structure) into a concrete layered graph.
- **Depends on / sequences after:** nothing blocking; it is pure internal
  re-architecture. **Interacts with:** the [LSP track](lsp.md) (whose
  completion/diagnostics queries are exactly the IDE surface this track relocates
  into `bynk-ide`) and the v0.59 [`bynkc test --format json`
  proposal](../proposals/v0.59-bynkc-test-json.md) (whose deferred `bynk test`
  changes from *shell* to *link* under this track — see Decision D2).

## Why a track and not a single proposal

A **tooling track**, not an ADR 0076 *feature* track — no language surface, no
threat model. It earns a persistent doc on the other 0076 trigger: it is
**unavoidably multi-increment and the increments are connected**. Every
extraction shares one dependency graph, one foundation-types boundary, and one
rendering contract; a lone delete-on-merge proposal would have to settle the
whole graph in one PR or discard the connective design between slices. The doc is
the living map each slice is cut from.

## The throughline

> **A crate that depends on `bynkc` can only wrap it — never slim it.** The
> existing `bynk-fmt` proves this: it is one line (`pub use bynkc::fmt::*`) over a
> `bynkc` dependency, so depending on it still links the whole compiler. To make
> `bynkc` focused on compilation, code moves **down** into leaf crates that
> `bynkc` depends *on*, and the human-facing CLI moves **up** into the driver.
> `bynkc` stops being a monolith and becomes one consumer among several of a
> layered library set; the libraries emit **structured data** and never render.

## Current state (grounded)

The workspace today (verified against `Cargo.toml`s and `lib.rs`):

- **`bynkc`** (lib + bin) — the monolith; **21 public modules** in `lib.rs`.
  Pipeline (`lexer`, `parser`, `ast`, `span`, `keywords`, `error`, `resolver`,
  `checker`, `emitter`, `project`) **plus** cross-cutting concerns: the formatter
  (`fmt`); the IDE surface (`hints`, `index`, `locals` modules; the
  `diagnose_project` fn and its `unit_sources` *field*; `expr_types`); the test
  emitter (`project/tests_emit.rs`); the **diagnostic-code registry**
  (`diagnostics.rs`, 46 KB — the single source of truth for `bynk.*` codes,
  generates `docs/src/reference/diagnostics.md`, pinned by
  `tests/diagnostics_registry.rs`); the **kernel-method registry**
  (`kernel_methods.rs` — the enumerable view the LSP reads for `.`-member
  completion, pinned to the checker by `tests/kernel_registry.rs`); the **CLI
  defs** (`cli.rs` — clap tree + `render_markdown` generating
  `docs/src/reference/cli.md`); and the `compile`/`check`/`fmt`/`test` binary
  (where `test` shells `node`/`tsc`/`tsx` — an acknowledged layering smell).
  Diagnostic *rendering* (ariadne `print_errors`, `render_errors_short`) also
  lives here. `builtin_names`, `firstparty`, `actors` round out the 21.
- **`bynk-fmt`** (lib) — a thin **upward façade**: `pub use bynkc::fmt::{…}` over
  `bynkc.workspace = true`. Narrows the *name*, not the *link cost*.
- **`bynk-grammar`** (lib) — genuinely standalone (`serde_json` only); proof that
  leaf crates work where the concern doesn't need the pipeline.
- **`bynk-lsp`** (bin) — links `bynkc` + `bynk-fmt`; consumes the IDE surface.
- **`bynk`** (driver, lib + bin) — `doctor`/`dev`/`new`; **shells** the `bynkc`
  *binary* for compilation (`BYNK_BYNKC` → PATH → sibling) and "does not link the
  pipeline" (`bynk/Cargo.toml`). But note it **already links the `bynkc` lib**
  for leaf helpers — `bynkc::lexer::tokenize` (`new.rs:103`),
  `bynkc::read_project_paths` (`main.rs:95`), `NODE_MAJOR_FLOOR`. So D2 re-points
  an *existing* lib dependency at leaves; it does not introduce linking from
  scratch. Renders human/`short`/`json` in `report.rs`.

So two crates already point the right way (`bynk-grammar` is a leaf; the driver
is a thin front-end), and one points the wrong way for this goal (`bynk-fmt`
wraps rather than extracts).

## Internal architecture (target)

A layered set, every arrow pointing **down**:

```
bynk-syntax     lexer · parser · ast · span · keywords · error(CompileError) · diagnostics(registry)   [leaf]
   ▲
bynk-check      resolver · checker · expr_types · kernel_methods · builtin_names · firstparty · actors
                · index · hints (captured TABLES — written during analysis; queries live up in bynk-ide)
   ▲   ▲
   │   └── bynk-fmt        re-pointed onto bynk-syntax only (a real leaf)
   │
bynk-emit       emitter · project · tests_emit   ("build orchestration + TS emission" — see C)
   ▲
bynk-ide        diagnose_project (fn) · index/hints/locals QUERIES (over the bynk-check tables) · unit_sources (field)   (LSP surface)

bynk-render     ariadne human + short/json rendering over bynk-syntax::CompileError ONLY   [shared]

bynkc (bin)     thin compile/check binder over the libs + bynk-render; owns cli.rs   (or dissolved — D2)
bynk  (bin)     human workflow CLI: links the libs + bynk-render        (D1/D2)
bynk-lsp (bin)  links bynk-ide (+ syntax + fmt); drops the whole-bynkc dependency
```

Homes for the three modules a `lib.rs` cross-check would flag: **`diagnostics.rs`**
→ `bynk-syntax` (it is about `CompileError.category`), but it *generates docs* and
is *pinned by a workspace test*, so its placement and that test's new cross-crate
shape are a foundation-boundary decision, not a detail (see the ADRs).
**`kernel_methods.rs`** → `bynk-check` (the checker dispatches it) but it is read
by the LSP, so it is part of the check↔IDE seam below. **`cli.rs`** → travels with
the `bynkc` binary front-end (clap tree + the `render_markdown` that generates the
CLI reference).

The foundation boundary is the hinge: `span`, `error`/`CompileError`, the
source-cache type, and the `diagnostics` registry must sit in the lowest leaf
(`bynk-syntax`) so diagnostics, positions, and codes cross every crate without a
cycle. Get this wrong and the whole graph fights back.

**The check↔IDE seam is three modules, not two.** `expr_types` and `locals` are
*captured* during checking but *consumed* by the IDE; `kernel_methods` is the same
shape — dispatched by the checker, but the enumerable table the LSP reads for
`.`-member completion, pinned to the checker by `tests/kernel_registry.rs`. The
captured types/tables live in `bynk-check`; the scope-at-offset / completion
queries live in `bynk-ide`. Settle all three in the `bynk-check` slice — and with
them, where the cross-crate drift tests live (question A).

**`bynk-emit` is the project driver, not just a code generator.** `project.rs` is
142 KB — the largest file in the tree — and it conducts discovery, the dependency
graph, consistency, validation, symbols, and paths; `compile_project` lives here.
The name `bynk-emit` undersells it: read it as "build orchestration + TS
emission" (orchestration drives emission). Flagged so a later reader is not
surprised to find `compile_project` in something called `-emit`.

## ▶ Key decisions for reviewers

**D1 — Structured data vs. rendering (the principle that makes this pay off).**
Library crates return *structured* results (diagnostics with spans, types,
hints) and are agnostic about display. Human rendering (ariadne) and machine
rendering (`short`/`json`) are a presentation layer. *Recommendation:* a shared
**`bynk-render`** crate over `bynk-syntax::CompileError`, used by both CLI
front-ends, so they render identically; the LSP maps structured diagnostics to
the LSP protocol and never touches ariadne. This is the single most important
commitment — it is *why* the LSP and CLI stay consistent.

> **Invariant the D1 ADR must pin:** `bynk-render` depends on `bynk-syntax`
> **only**. Every current renderer already takes `&[CompileError] + source +
> filename`, so this holds — but `AttributedError` (a `CompileError` + a
> `source_path`) lives in `project` → `bynk-emit`. The `AttributedError →
> CompileError` flattening must stay **up** in the emit/front-end layer and never
> cross into `bynk-render`, or someone later adds an `AttributedError`-aware entry
> point and creates the `render → emit` cycle the layering forbids. (Rendering
> also needs the source cache to draw carets — which reinforces putting the
> source-cache type in `bynk-syntax`.)

**D2 — Shell vs. link, and one binary or two.** Once the pipeline is libraries,
the driver no longer needs to *shell* `bynkc` to avoid pulling in a monolith — it
can **link the leaves** and get structured data in-process. *Recommendation:*
link. This revises the current "driver does not link the pipeline" rule (a rule
that existed *because* `bynkc` was a monolith) and raises the follow-on: does
`bynkc` survive as a minimal `compile`/`check` binary (kept for CI/build
determinism and `cargo install bynkc`), or does `bynk` become the single CLI?
*Recommendation:* keep a thin `bynkc` for the pure-pipeline path; make `bynk` the
human front-end. Knock-on: v0.59's deferred `bynk test` switches from "shell
`bynkc test --format json`" to "link `bynk-emit`'s test emission and run node" —
v0.59 stays correct as shipped; only the long-term mechanics change.

**D3 — Naming.** `bynk-syntax` / `bynk-check` / `bynk-emit` / `bynk-ide` /
`bynk-render`, with `bynk-fmt` retained and re-pointed. Matches the `bynk-*`
convention already in the workspace.

**D4 — `bynk-fmt`'s fate.** Keep it, but re-point its dependency from `bynkc` to
`bynk-syntax` and move the implementation (`bynkc/src/fmt.rs`) down with it —
turning the existing cosmetic façade into a real leaf. Formatting needs syntax,
not the checker or emitter.

## Slice decomposition (proposed)

Leaf-first, validate-before-proceeding. Each slice is an ordinary
`vX.Y-<slug>.md` proposal citing this doc and the ADRs; status tracked here as
slices land.

0. **Land the ADRs** ✅ **done (2026-06-20)** — [0099](../decisions/0099-crate-layering-dependency-direction.md)
   layering, [0100](../decisions/0100-structured-data-rendering-separation.md) D1,
   [0101](../decisions/0101-front-end-links-pipeline-binary-topology.md) D2,
   [0102](../decisions/0102-foundation-types-boundary.md) foundation boundary.
   Direction only; no code.
1. **Extract `bynk-syntax`** ✅ **done (v0.60)** — lexer, parser, ast, span,
   keywords, error **and diagnostics** moved into the leaf; `bynkc` depends on it
   and re-exports the modules so its public API is unchanged. Verified the seven
   modules had zero upward `crate::` edges, so the move was mechanical and
   behaviour-preserving (the whole suite passed unchanged — the validation gate).
   `diagnostics.rs` came too (ADR 0102); its `diagnostics_registry` /
   `doc_diagnostics` pins stay in `bynkc` until the emission sites split out.
2. **Re-point `bynk-fmt` onto `bynk-syntax`** ✅ **done (v0.61)** — `fmt.rs` moved
   down into `bynk-fmt`, which now depends on `bynk-syntax` only (verified via
   `cargo tree`: zero `bynkc` in its dependency tree). The former façade is now
   the formatter's real home; `bynkc` re-exports it as `bynkc::fmt`. `bynk-fmt`
   and the LSP's formatting path stop linking the checker/emitter. Golden +
   round-trip suites byte-identical (no behaviour change).
3. **Extract `bynk-check`** ✅ **done (v0.62)** — resolver, checker,
   `expr_types`, `locals`, `kernel_methods`, builtins, firstparty, actors, **and
   the captured `index` + `hints` tables** → over `bynk-syntax`. **The feared
   three-module seam was mostly cosmetic:** a grounded scan found `kernel_methods`
   clean, the `expr_types`/`locals`→`hints` "edges" were rustdoc links only, and
   the one real edge was resolver/checker writing the index sink
   (`RefSink`/`SymbolKind`). **`index` + `hints` went into `bynk-check`, not
   `bynk-ide`** — they are captured tables written during analysis (graph
   corrected below). Drift tests: `kernel_registry` stays in `bynkc` (sees both
   halves via the re-export — its "straddles check↔IDE" worry dissolved since
   both halves are in `bynk-check`); `diagnostics_registry` now scans
   `bynk-check/src` too. Boundary fixes were three `pub(crate)`→`pub` promotions
   and moving the emitter's `runtime.ts` shim back beside the emitter.
4. **Extract `bynk-emit`** ✅ **done (v0.63)** — emitter (+ `emitter/`), project
   (+ `project/`, incl. `tests_emit`) → over `bynk-syntax` + `bynk-check` (no
   external crates). `bynkc` is now just the CLI + thin compile/diagnose glue and
   re-exports the modules. Prereq: `line_col` moved to `bynk-syntax` (the sole
   lib-glue edge; pre-positions `bynk-render`). `test_json` stayed in `bynkc`
   (CLI output, not emission). One boundary promotion
   (`check_function_type_boundary_items` `pub(crate)`→`pub`).
   `diagnostics_registry` now scans `bynk-emit/src` too.
5. **Extract `bynk-ide`** ✅ **done (v0.64)** — `diagnose`, `diagnose_project`,
   and the result types (`Diagnostic`/`FileDiagnostics`/`ProjectDiagnostics`) →
   over `bynk-syntax`+`bynk-check`+`bynk-emit`. **`bynk-lsp` re-pointed off
   `bynkc` entirely** → `bynk-ide`+`bynk-check`+`bynk-syntax`+`bynk-fmt` (verified
   `cargo tree -p bynk-lsp` has zero `bynkc`): the editor server no longer links
   the CLI/`test_json`/ariadne it never used — the track's original motivation.
   Investigation found most of `bynk-lsp`'s ~80 `bynkc::` imports were
   pass-through, so the re-point was a mechanical import rewrite (~7k lines).
   Prereq: `Severity` moved to `bynk-syntax` (shared by the IDE diagnose path and
   `bynkc`'s `short`/`json` render — avoids an upward edge; pre-positions
   `bynk-render`). `bynkc` re-exports the IDE items so its 12 index/diagnose tests
   stay stable. The index/hints **table types** already lived in `bynk-check`
   (slice 3); the LSP's completion/nav **query logic** lives in `bynk-lsp` itself
   and was untouched — relocated, not rewritten; the full `bynk-lsp` suite passes.
6. **Introduce `bynk-render`** (D1); move ariadne/`short`/`json` rendering out of
   `bynkc` into it; both front-ends adopt it.
7. **Resolve the binary topology** (D2): `bynk` links the libs and becomes the
   human front-end; `bynkc` reduced to thin `compile`/`check`. Re-mechanise the
   v0.59 `bynk test` deferral onto linking.

Slices 1–2 are shippable and valuable on their own (fmt stops over-linking) even
if the track later stalls — a deliberately low-regret ordering.

## Foundational ADRs — landed (slice 0, 2026-06-20)

- **[0099](../decisions/0099-crate-layering-dependency-direction.md) — Crate
  layering & dependency direction.** The graph above; arrows point down; no
  upward façades introduced to "decompose". **Publishing story settled: published
  lockstep** — the five new crates are published to crates.io with
  `version.workspace = true` (ten crates cut per release instead of five), making
  `bynk-syntax` and the layers above reusable by third-party tooling.
  - **Operational — seed each new crate before trusted publishing.** The
    tag-driven `release.yml` publishes via crates.io **trusted publishing**
    (OIDC), which a crate name can only use *after* it exists on crates.io. So a
    brand-new published crate's **first** publish must go through the
    **`release-bootstrap.yml` seed workflow** (token auth, manual dispatch);
    trusted publishing is then enabled for it on crates.io, and subsequent
    releases flow through `release.yml` (its `curl` guard skips the
    already-seeded version, so there is no double-publish). This applies once per
    new crate: `bynk-syntax` (slice 1) and the four still to come — `bynk-check`
    (3), `bynk-emit` (4), `bynk-ide` (5), `bynk-render` (6). Each must be added
    to **both** publish loops (it already is for `bynk-syntax`, before `bynkc`).
- **[0100](../decisions/0100-structured-data-rendering-separation.md) —
  Structured-data / rendering separation (D1).** Libraries never render;
  `bynk-render` is **one shared crate** over `bynk-syntax::CompileError`
  **only**, so both CLIs render identically by construction; the `AttributedError
  → CompileError` flattening stays above render (no `render → emit` edge); the
  LSP maps to protocol.
- **[0101](../decisions/0101-front-end-links-pipeline-binary-topology.md) —
  Front-end links the pipeline; binary topology (D2).** Amends ADR 0083's "driver
  does not link the pipeline" note (the driver already links the lib). **A thin
  `bynkc` survives** (CI/build determinism + `cargo install bynkc`); `bynk`
  becomes the human front-end.
- **[0102](../decisions/0102-foundation-types-boundary.md) — Foundation-types
  boundary.** `span`/`error`/source-cache **and the `diagnostics` code registry**
  live in the lowest leaf (`bynk-syntax`); the rule that keeps the graph acyclic.
  Settles the now-cross-crate drift-test homes: `diagnostics_registry` becomes a
  workspace integration test; `kernel_registry` lands in `bynk-ide` dev-depending
  on `bynk-check`.

## Decision log (track-level)

- **Decompose downward, not via façades.** Settled — ADR 0099: the goal is to
  slim `bynkc`, and only downward extraction or front-end thinning does that
  (`bynk-fmt` is the cautionary example, `bynk-grammar` the model).
- **Driver owns human output; libraries own structured data.** Settled — ADR
  0100: `bynk-render` is **one shared crate** (ratified, not just recommended).
- **Binary topology (D2).** Settled — ADR 0101: **keep a thin `bynkc`**; `bynk`
  becomes the human front-end and links the leaves.
- **Publishing story.** Settled — ADR 0099: **published lockstep**
  (`version.workspace = true`), not path-only internal.
- **`diagnostics.rs`'s home + the cross-crate drift tests.** Settled — ADR 0102:
  `diagnostics.rs` → `bynk-syntax`; `diagnostics_registry` → workspace
  integration test; `kernel_registry` → `bynk-ide` dev-depending on `bynk-check`.
- **Open (deferred to their extraction slices):** the three-module check↔IDE seam
  (`expr_types`/`locals`/`kernel_methods` — settled in the `bynk-check` slice, 3).

## On merge — each slice updates

- This doc (mark the slice done; correct the graph if a boundary moved).
- `bynk-tooling-roadmap.md` crate-structure section.
- The relevant ADR(s) and the workspace `Cargo.toml` members list.
