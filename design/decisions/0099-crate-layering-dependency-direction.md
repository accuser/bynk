# 0099 — Crate layering & dependency direction: `bynkc` decomposes downward into a layered, published library set

- **Status:** Accepted (crate-decomposition track, slice 0; 2026-06-20). Direction-settling; no code lands with this ADR.
- **Realises:** the crate-decomposition track — the standing "keep `bynkc` focused on the compilation pipeline" concern. Front-loaded per [[0076]] (feature-track posture: load-bearing ADRs land up front).
- **Relates:** [[0060]] (named single-concern modules — the in-crate precedent this extends across crate boundaries), [[0083]] (the `bynk` driver as a thin orchestrator — amended by [[0101]]), [[0100]] (structured-data / rendering split), [[0101]] (binary topology), [[0102]] (foundation-types boundary).

## Context

`bynkc` is today a monolith: 21 public modules spanning the compilation pipeline
(`lexer`, `parser`, `ast`, `span`, `keywords`, `error`, `resolver`, `checker`,
`emitter`, `project`) **plus** cross-cutting concerns — the formatter (`fmt`),
the IDE surface (`hints`, `index`, `locals`, `diagnose_project`, `expr_types`),
the test emitter (`project/tests_emit.rs`), the diagnostic-code registry
(`diagnostics.rs`), the kernel-method registry (`kernel_methods.rs`), the CLI
defs (`cli.rs`), and diagnostic *rendering* (ariadne). It only grows as language
features land.

The throughline (track doc): **a crate that depends on `bynkc` can only wrap it
— never slim it.** `bynk-fmt` proves this — it is one line (`pub use
bynkc::fmt::*`) over a `bynkc` dependency, so depending on it still links the
whole compiler. Narrowing the *name* does not narrow the *link cost*. The only
moves that slim `bynkc` are extracting code **down** into leaf crates `bynkc`
depends *on*, and moving the human CLI **up** into the driver.

`bynk-grammar` (a genuine `serde_json`-only leaf) is the model; `bynk-fmt` (an
upward façade) is the cautionary example.

## Decision

Decompose `bynkc` into a **layered library set**, every dependency arrow
pointing **down**. No upward façades are introduced to "decompose" — extraction
is downward or it does not happen.

```
bynk-syntax   lexer · parser · ast · span · keywords · error(CompileError) · diagnostics(registry)   [leaf]
   ▲
bynk-check    resolver · checker · expr_types · kernel_methods · builtin_names · firstparty · actors
   ▲   ▲
   │   └── bynk-fmt        re-pointed onto bynk-syntax only (a real leaf — see D4)
   │
bynk-emit     emitter · project · tests_emit   (build orchestration + TS emission)
   ▲
bynk-ide      index · hints · locals · diagnose_project · unit_sources · expr_types queries   (LSP surface)

bynk-render   ariadne human + short/json over bynk-syntax::CompileError ONLY   [shared — see [[0100]]]

bynkc (bin)   thin compile/check binder over the libs + bynk-render; owns cli.rs   (see [[0101]])
bynk  (bin)   human workflow CLI: links the libs + bynk-render                    (see [[0101]])
bynk-lsp(bin) links bynk-ide (+ syntax + fmt); drops the whole-bynkc dependency
```

`bynk-emit` is named for its product but is the **project driver**: `project.rs`
(the largest file in the tree) conducts discovery, the dependency graph,
consistency, validation, symbols, and paths, and `compile_project` lives there.
Read the name as "build orchestration + TS emission" — orchestration drives
emission.

**Publishing — published lockstep.** The five new crates
(`bynk-syntax`/`-check`/`-emit`/`-ide`/`-render`) are **published to crates.io**,
versioned `version.workspace = true` in lockstep with the existing crates, and
declared in `[workspace.dependencies]` with the established `{ path, version }`
pair so published crates carry a crates.io requirement while in-workspace builds
use the path. This makes `bynk-syntax` (and the layers above it) reusable by
third-party tooling, at the cost of five additional release surfaces to version
and publish on every cut.

## Consequences

- `bynkc` stops being a monolith and becomes **one consumer among several** of
  the library set; `bynk-lsp`, `bynk`, and `bynk-fmt` link only the layers they
  need (the LSP stops linking the emitter; `bynk-fmt` stops linking the checker).
- Five new published crates: ten crates cut and published per release instead of
  five. The lockstep version (`version.workspace = true`) keeps them from
  skewing against each other.
- The layering is a standing invariant, not a one-time shape: any future
  "decompose by wrapping" PR is rejected by this ADR. The acyclicity that makes
  the graph hold depends on the foundation-types boundary ([[0102]]) and the
  render-edge rule ([[0100]]).
- Extraction is sliced leaf-first (track doc §Slice decomposition); each slice is
  an ordinary `vX.Y-*.md` proposal citing this ADR. Slices 1–2 (`bynk-syntax`,
  `bynk-fmt` re-point) are independently valuable even if the track later stalls.
