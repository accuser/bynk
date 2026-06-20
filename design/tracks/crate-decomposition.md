# Tooling track — Crate decomposition: `bynkc` becomes a library set, the driver becomes the front-end

- **Phase:** **🟡 Draft — direction proposed; no ADRs landed, no slices cut.**
  This doc settles *direction* only. The load-bearing ADRs (layering, the
  structured-data/rendering split, the binary topology) must land up front per
  ADR 0076 before the first extraction slice.
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
   ▲   ▲
   │   └── bynk-fmt        re-pointed onto bynk-syntax only (a real leaf)
   │
bynk-emit       emitter · project · tests_emit   ("build orchestration + TS emission" — see C)
   ▲
bynk-ide        index · hints · locals (queries) · diagnose_project (fn) · unit_sources (field)   (LSP surface)

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

0. **Land the ADRs** (below) — layering, D1, D2, foundation boundary. Direction
   only; no code.
1. **Extract `bynk-syntax`** (lexer, parser, ast, span, keywords, error) as a
   leaf; `bynkc` depends on it. Largest mechanical move, lowest conceptual risk —
   nothing else should change behaviour. The validation gate for the whole track.
2. **Re-point `bynk-fmt` onto `bynk-syntax`** (D4); move `fmt.rs` down. `bynk-fmt`
   and the LSP's formatting path stop linking the checker/emitter.
3. **Extract `bynk-check`** (resolver, checker, `expr_types`/`locals` capture
   types, `kernel_methods`, builtins, firstparty, actors) → `bynk-syntax`. Settle
   the three-module check↔IDE seam here, **and** decide where the drift tests go:
   `kernel_registry` (registry-vs-checker-dispatch) wants both halves visible, so
   it needs a home that sees `bynk-check` and `bynk-ide` — likely an integration
   test in `bynk-ide`'s suite dev-depending on `bynk-check`. Pin this before the
   extraction, or it silently blocks slice 5.
4. **Extract `bynk-emit`** (emitter, project, tests_emit) → `bynk-check`.
5. **Extract `bynk-ide`** (diagnose_project, index, hints, locals queries,
   unit_sources (field)) → `bynk-check`; re-point `bynk-lsp` onto `bynk-ide` (+ syntax +
   fmt), dropping its whole-`bynkc` dependency. Must not break the [LSP
   track](lsp.md) queries — this is those queries relocated, not rewritten.
6. **Introduce `bynk-render`** (D1); move ariadne/`short`/`json` rendering out of
   `bynkc` into it; both front-ends adopt it.
7. **Resolve the binary topology** (D2): `bynk` links the libs and becomes the
   human front-end; `bynkc` reduced to thin `compile`/`check`. Re-mechanise the
   v0.59 `bynk test` deferral onto linking.

Slices 1–2 are shippable and valuable on their own (fmt stops over-linking) even
if the track later stalls — a deliberately low-regret ordering.

## Foundational ADRs to land (up front)

- **Crate layering & dependency direction** — the graph above; arrows point down;
  no upward façades introduced to "decompose". **Also settles the publishing
  story:** are `bynk-syntax`/`-check`/`-emit`/`-ide`/`-render` published crates
  (lockstep `version.workspace = true`, five new release surfaces) or path-only
  internal crates? One line decides ongoing release cost.
- **Structured-data / rendering separation (D1)** — libraries never render;
  `bynk-render` is the shared presentation layer over `bynk-syntax::CompileError`
  **only**; the `AttributedError → CompileError` flattening stays above render (no
  `render → emit` edge); the LSP maps to protocol.
- **Front-end links the pipeline; binary topology (D2)** — supersedes the "driver
  does not link the pipeline" note (the driver already links the lib); states
  whether a thin `bynkc` survives or dissolves into `bynk`.
- **Foundation-types boundary** — `span`/`error`/source-cache **and the
  `diagnostics` code registry** live in the lowest leaf (`bynk-syntax`); the rule
  that keeps the graph acyclic. Also settles where the now-cross-crate drift
  tests live: `diagnostics_registry` (codes-vs-usage, spans all phases) and
  `kernel_registry` (registry-vs-dispatch, straddles check↔IDE).

## Decision log (track-level)

- **Decompose downward, not via façades.** Settled in principle: the goal is to
  slim `bynkc`, and only downward extraction or front-end thinning does that
  (`bynk-fmt` is the cautionary example, `bynk-grammar` the model).
- **Driver owns human output; libraries own structured data.** Settled in
  principle (D1); the `bynk-render` form is the recommendation, not yet ratified.
- **Open:** D2 (keep a thin `bynkc` vs. dissolve into `bynk`); the three-module
  check↔IDE seam (`expr_types`/`locals`/`kernel_methods`) and where its drift
  tests live; whether `bynk-render` is one crate or a module each front-end owns;
  the publishing story (published lockstep vs. path-only). `diagnostics.rs`'s home
  is **settled-pending-ADR** (`bynk-syntax`); only its now-cross-crate
  `diagnostics_registry` test shape is open.

## On merge — each slice updates

- This doc (mark the slice done; correct the graph if a boundary moved).
- `bynk-tooling-roadmap.md` crate-structure section.
- The relevant ADR(s) and the workspace `Cargo.toml` members list.
