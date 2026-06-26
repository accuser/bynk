# 0100 — Libraries emit structured data; rendering is a shared presentation layer (`bynk-render`)

- **Status:** Accepted (crate-decomposition track, slice 0; 2026-06-20). Direction-settling; no code lands with this ADR.
- **Realises:** the crate-decomposition track decision **D1** — "the single most important commitment: it is *why* the LSP and CLI stay consistent."
- **Relates:** [[0099]] (the layered graph this rendering crate sits in), [[0102]] (the foundation boundary that puts `CompileError` + the source cache in `bynk-syntax`, which this crate depends on), [[0052]] (LSP project diagnostics — the LSP consumes the *structured* diagnostics, not rendered text).

## Context

`bynkc` today both computes diagnostics and **renders** them: ariadne human
output (`print_errors`), the `short`/`json` machine forms (`render_errors_short`),
and the driver's own `report.rs` (`bynk` renders human/`short`/`json`). If the
library layers each rendered their own way, the LSP, `bynkc`, and `bynk` would
drift in how the same error reads. The decomposition is the moment to fix the
display contract, not fragment it.

Every current renderer already takes the same shape — `&[CompileError]` + source
+ filename — so a single rendering surface over `CompileError` is not a
refactor, it is recognising what the renderers already are.

## Decision

**Library crates return *structured* results** (diagnostics with spans, types,
hints) and are agnostic about display. Rendering is a **presentation layer**, not
a library concern.

A single shared crate, **`bynk-render`**, owns all human + machine rendering:
ariadne human output and the `short`/`json` forms. Both CLI front-ends (`bynkc`
and `bynk`) depend on it, so **they render identically by construction** — a
type-level guarantee, not a drift test. The LSP does **not** use `bynk-render`:
it maps the structured diagnostics to the LSP protocol and never touches ariadne.

**Load-bearing invariant — `bynk-render` depends on `bynk-syntax` only.**

> `bynk-render` operates over `bynk-syntax::CompileError` and the
> `bynk-syntax` source cache (which it needs to draw carets — see [[0102]]).
> `AttributedError` (a `CompileError` + a `source_path`) lives in `project` →
> `bynk-emit`. The `AttributedError → CompileError` flattening **stays up** in
> the emit / front-end layer and must **never** cross into `bynk-render`. If a
> later change adds an `AttributedError`-aware entry point to `bynk-render`, it
> creates a `render → emit` edge — the upward cycle the layering ([[0099]])
> forbids. The D1 ADR pins this so that edge is rejected on sight.

## Consequences

- The LSP and both CLIs stay consistent **because** the structured/rendered split
  is enforced by the crate graph: the libraries cannot render, and the one crate
  that can renders one way for everyone.
- A new rendering need (a different machine format, a richer human frame) is added
  **once**, in `bynk-render`, and every front-end inherits it.
- The `render → emit` non-edge is the rule most likely to be violated by a
  well-meaning "just let render take an `AttributedError`" change. It is called
  out here and re-stated in the track doc so the reviewer of slice 6 (introduce
  `bynk-render`) knows to reject it.
- The LSP's protocol mapping is the one place a structured diagnostic becomes a
  non-ariadne presentation; that mapping lives with the LSP, not in `bynk-render`.
