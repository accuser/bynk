# 0103 — The source-map contract: line-level, statement-anchored maps emitted from `bynk-emit`, with a settled lowering-gap rule and a release source-confidentiality boundary

- **Status:** Accepted (doc-ADR; debugging track, slice 0; 2026-06-21)
- **Spec:** `design/bynk-design-notes.md` §19 (compilation: "carries source maps for stack traces and debuggers"), phase 8 ("Bundling and source maps")
- **Realises:** the debugging track, retired, slice 0 (front-loaded ahead of slice 1).
- **Relates:** [[0104]] (the debug-launch model that consumes these maps), [[0099]]/[[0102]] (the maps land in `bynk-emit` / `bynk-syntax` leaves, not a monolith).

## Context

§19 commits the emitter to TypeScript that "preserves source-level identifiers,
carries source maps for stack traces and debuggers." The debugger half is unbuilt:
`bynk-emit`'s emitter is a one-way string pipe (`emit.rs` ~150 write sites routed
through `write_line` in `lower.rs`) that discards the `Span` every AST node carries,
and `write_output` (`bynk-emit/src/lib.rs`) writes each file's `.typescript` with no
`.map` sibling and no `//# sourceMappingURL` trailer. The whole feature reduces to
*faithful maps from the emitter plus glue* — so the one format-bearing decision is
the map contract, and it must be settled before slice 1 threads spans through the
writer, because every later slice and every future codegen feature inherits it.

The genuine risk going in was **granularity**. Bynk lowers heavily: `?` becomes a
temp + an `Err`-guard + an unwrap (1 source line → 3 generated); `match` becomes a
`switch` with a `case`/binding/`return` triple per arm plus a non-exhaustive
`throw`. The fear was that one source line exploding into many generated lines would
make line-level stepping feel wrong — landing the user on phantom guard lines or
stepping out of source order — forcing expression-level mappings into phase 1.

**The spike settles it from observation, not theory.** A throwaway example with two
`?` propagations and a `match` over a `Result` was compiled with the real emitter,
hand-given a line-level map under the rule below, and **stepped under the V8
inspector** (`node --inspect-brk` driven over CDP). Result: lowering is *contiguous
and order-preserving* — every generated line for a source statement sits together,
in source order. Raw V8 stopped 8 times in the two-`?` function and 13 times in the
`match` function; mapped line-level, those coalesced to **3 source steps (`5 → 6 →
7`)** and **6 source steps (`11 → 5 → 6 → 7 → 12 → 13`)** — monotonic, in source
order. The `?` guard and the match `case`/binding lines, anchored to their enclosing
statement, became *invisible* to a source-map-aware stepper. The explosion never
surfaced as bad stepping because it is local and ordered. Line-level is enough.

## Decision

**One contract: line-level, statement-anchored source maps, emitted unconditionally
as siblings by `bynk-emit`, with `sourcesContent`/bundle-chaining gated on build
profile so `.bynk` source never reaches a deployed Worker by default.**

- **D1 — Line-level granularity, statement-anchored (spike-ratified).** One mapping
  per emitted statement line, back to the span of the source statement being
  lowered. The seam is the existing `write_line` writer: it gains an *optional source
  span*, and a `SourceMapBuilder` accumulates `(generated line, source span)` as
  output advances. **Expression-level (sub-line) mappings are explicitly out of phase
  1** — the spike showed they buy nothing for breakpoints or stepping over `?`/`match`/
  comprehensions. They are a later refinement *only if* a future construct is shown to
  step poorly; this ADR is amended with that evidence, not pre-emptively.

- **D2 — The lowering-gap rule: nearest *enclosing statement*.** A generated line
  with no 1:1 source line maps to the source span of the statement whose lowering
  emitted it. Concretely: the `?` `Err`-guard maps to the `?` statement's line; a
  `match` arm's `case`/binding/`return` lines map to that arm's source line; the
  `switch` header maps to the `match` head; the synthetic non-exhaustive `throw` and
  any IIFE wrapper map to the construct's head span. The rule is *enclosing
  statement*, never "leave it unmapped" and never "nearest preceding emitted line" —
  the spike showed enclosing-statement anchoring is exactly what lets a source-map-
  aware stepper coalesce the expansion into one source step. This is the load-bearing
  call: it is where stepping would feel wrong if left implicit, and it is now ratified
  against an observed trace.

- **D3 — Placement: sibling `.map` + trailer.** Each emitted `<file>.ts` gets a
  sibling `<file>.ts.map` (source-map v3 JSON) and a `//# sourceMappingURL=<file>.ts.map`
  trailer. `file` is the `.ts` name; `sources` are the originating `.bynk` paths,
  project-root-relative. `write_output` — the shared writer behind `bynkc compile`/
  `test` and `bynk dev`'s in-process build — gains one map-writing pass, so every
  consumer gets maps uniformly.

- **D4 — `names`: populate from preserved identifiers.** §19 already preserves
  source-level identifiers, so the map's `names` array is near-free: populate it so the
  debugger's variable panes read Bynk names rather than lowered temporaries
  (`__r0`, `outcome.value`). Phase 1 may ship a partial `names` set; the array is part
  of the contract from the start so later slices enrich rather than retrofit it.

- **D5 — The skip boundary is declared here.** `emitter/runtime.ts` (hand-written) and
  the generated worker glue (`workers_entry.rs` / `wrangler.rs` output) are *ours, not
  the user's*: their frames map to themselves, and they are the set a launch config
  marks `skipFiles`/`smartStep` ([[0104]] wires that). Declaring the boundary in the
  map contract — not only in the launch config — keeps "which files are toolchain
  machinery" single-sourced, so stepping never descends into capability plumbing.

- **D6 — Emission is unconditional; `sourcesContent` and bundle-chaining are
  profile-gated (the source-confidentiality call).** `write_output` always emits the
  sibling `.map` — it is a separate file, not inlined, so emitting it costs a deployed
  Worker nothing. But two things *are* gated on build profile, because together they
  would ship source into production:
  - **`sourcesContent`** is *embedded* for dev/test builds and *omitted* on release.
    Embedding is not cosmetic: under `workerd` the inspector often cannot resolve a
    `.bynk` path on disk, so the source must travel inside the map for the dev session
    to show `.bynk` at all. Release builds have no such need.
  - **Map chaining into the deployed bundle** (phase 8, esbuild/`wrangler`) is *off by
    default* and enabled only by an explicit `--source-maps` opt-in for production
    debugging.

  The invariant: **`.bynk` source never reaches a deployed Worker unless the user
  explicitly opts in.** Local dev/test get full-fidelity maps with embedded source;
  production gets none of it by default.

## Consequences

Slice 1 implements a settled format: thread the span through `write_line` and the
statement-significant `push_str` sites, add `SourceMapBuilder`, emit the sibling +
trailer in `write_output`, and pin D1/D2 with golden tests that decode the map and
assert the source↔generated line mappings — including at least one `?` and one
`match`, the constructs the spike exercised. The `Span`-is-bytes / map-wants-line+col
gap is closed by the existing `span::line_col` resolver fed a per-file line index
threaded to emit time (the one new data dependency).

One dependency stays open and is **not** settled here: **map composition through the
bundler** (§19 phase 8). Per-file maps must chain correctly through the
`wrangler`/esbuild bundle so production stack traces and any opt-in production session
resolve to `.bynk`. The contract is "per-file maps chain; if the chain proves lossy,
emit a post-bundle map instead" — confirmed against real `wrangler` output during
slice 3, not asserted now. This is why slice 1 realises only the §19 *debugger*
promise; the *stack-trace* half is marked realised once composition is confirmed.

Accepted costs: line-level means a breakpoint on a `?` line binds at the `Err`-guard
(the first generated statement of that line) — correct, but a user inspecting at that
stop sees the lowered temp in scope, not yet the named binding; the bynk-native
semantic layer (track Phase 2) is where values read in Bynk's vocabulary. And the
`names`/`sourcesContent` policy is a maintenance surface: a new lowering that emits
statement-significant text outside `write_line` must pass its span or it maps to the
nearest enclosing statement by default — usually right, but a construct that lowers
to genuinely out-of-line code (hoisted helpers) is an amendment to D2 with a new
clause, not an ad-hoc unmapped line.
