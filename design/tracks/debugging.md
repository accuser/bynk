# Tooling track — Debugging: source-mapped step debugging for Bynk

- **Phase:** 🟢 **Slice 0 landed — settled, slice 1 next.** No production code yet,
  but the format is no longer open: the two foundational ADRs are accepted
  ([0103](../decisions/0103-source-map-contract.md) source-map contract, [0104](../decisions/0104-debug-launch-model.md) debug-launch model), ratified against a
  throwaway spike that stepped `?`/`match` under the V8 inspector (see *Decision
  log*). Slices 1–4 below now cut as ordinary `vX.Y-<slug>.md` proposals from v0.67
  onward, implementing the settled contract. **Scope decided up front:** a
  *phased* approach — ship the pragmatic, source-map-plus-reused-JS-debugger base
  first, defer the bynk-native semantic layer; two run targets, **local
  `wrangler dev` (workerd)** and the **plain-Node test runner**; **remote /
  deployed Worker debugging is out of scope** for this track.
- **Realises:** the design-notes §19 compilation commitment — the emitted
  TypeScript "preserves source-level identifiers, **carries source maps for stack
  traces and debuggers**" (`bynk-design-notes.md` §19, and the pipeline's phase 8,
  "Bundling and source maps") — which is *committed but unbuilt*. **Note the split:**
  this track discharges the *debugger* half (the live attached session, slices 1–4);
  the *stack-trace* half resolves through the **production bundled map** (phase 8,
  esbuild/`wrangler` chaining), which is the deferred/uncertain path — so slice 1
  realises the §19 debugger promise, **not** the full stack-trace one (see *On
  merge* and open question 3). And it unblocks
  the parked line "a debugger plugin for VS Code is desirable but **follows the
  LSP work**" (`bynk-design-notes.md`): the LSP arc (`tracks/lsp.md`) is
  substantially complete, so the dependency is discharged.
- **Depends on / sequences after:** the **crate-decomposition track** (`bynk-emit`
  now owns TS emission, ADRs 0099–0102) — every source-map change lands in
  `bynk-emit`, not a monolith; and **`bynk dev`** (v0.57, ADR 0096) — the workerd /
  `wrangler dev` orchestration the debugger attaches to. **Refreshes:**
  `bynk-tooling-roadmap.md` (adds a debugging thread under the VS Code §4 work) and
  corrects `tracks/lsp.md`'s desirable-feature survey, which today omits debugger-
  oriented providers as N/A "— Bynk has no debugger." This track flips that premise.
  (Inline values, one of those omitted providers, is driven by the *debug session*,
  not the LSP server — so it belongs to the Phase-2 semantic layer (slice 5), not to
  shipping the base.)

## Why a track and not a single proposal

This earns a persistent doc on two of ADR 0076's triggers. It is **unavoidably
multi-increment**: the source-map foundation, the Node consumer, the workerd
consumer, and the extension wiring are separate slices that *share one artefact*
— a single map format and a single launch surface. And its foundation is
**hard-to-reverse infrastructure**: source maps require threading source spans
through the emitter's core writer (today a one-way string pipe — see *Current
state*), a change every later slice and every future codegen feature inherits. A
lone delete-on-merge proposal would settle the map format for the Node slice and
discard the reasoning the workerd and semantic slices need. It is a **tooling
track** (like `lsp.md` / `crate-decomposition.md`), *not* an ADR 0076 feature
track — no language surface, no `where`-type tension, no threat model — but the
multi-increment, shared-contract shape is the same.

## The throughline

> **The whole feature reduces to one hard capability plus glue.** The hard
> capability is *faithful source maps from the emitter*. Everything downstream —
> breakpoints, stepping, call stacks, variable inspection — is **wiring VS Code's
> existing JavaScript debugger to data we already produce**, because Bynk compiles
> to readable TypeScript running on a **V8 runtime that already speaks the
> inspector protocol** (`workerd`, via `wrangler dev`; and `node --inspect` for
> the test path). We are **not building a Debug Adapter.** We are teaching the
> compiler to emit maps and the extension to launch-and-attach. The pragmatic
> phase puts real breakpoints, stepping, and variables onto `.bynk` source; the
> deferred semantic phase makes the *values* read in Bynk's vocabulary.

Every claim below was confirmed by reading the emitter and the build writer, not
inferred: the string-concatenation emit in `bynk-emit/src/emitter/emit.rs`
(~150 `writeln!`/`push_str` write sites), the central statement writer
`write_line` in `emitter/lower.rs`, the AST's retained `Span` (`bynk-syntax/src/ast.rs`), and the
output writer `write_output` (`bynk-emit/src/lib.rs`) that writes
`file.typescript` with no `.map` sibling.

## Current state — the reality check

**There is no source-map infrastructure, and adding it is the load-bearing
slice.** Three facts set the cost:

1. **The emitter is a one-way string pipe that discards position.** Lowering walks
   the checked AST and *appends strings* — `emit.rs` is ~150 `writeln!`/`push_str`
   write sites; `lower.rs` routes statements through a single `write_line(out, indent, s)`
   helper. The AST nodes in hand **carry `Span`** (`ast.rs` — `pub span: Span` on
   every node, exactly the "full position information for source maps" §19
   promised), but the emitter **never reads it**: input span in, anonymous text
   out. Source maps need both ends — the *input* span and the *output* line/column
   — threaded through that writer. This is the real work; the rest is downstream of
   it.
2. **The build writer emits no map artefact.** `write_output` (`bynk-emit/src/lib.rs`)
   writes each `ProjectOutput` file's `.typescript` to disk and nothing else — no
   `.ts.map` sibling, no `//# sourceMappingURL` trailer. It is the *shared* writer
   behind `bynkc`'s `compile`/`test` and `bynk dev`'s in-process build (slice 7),
   so a map written here reaches every consumer uniformly.
3. **The runtime target already speaks the inspector.** Local dev is `workerd` via
   `wrangler dev` (`bynk dev`, v0.57); production is Cloudflare Workers. `workerd`
   exposes a **V8 inspector (Chrome DevTools Protocol)**, and `wrangler dev`
   supports an inspector port. The test path runs emitted TS on **Node ≥ 18**
   (`NODE_MAJOR_FLOOR`, `bynk-emit/src/lib.rs`), which exposes `--inspect`. Both
   targets are attachable by VS Code's built-in JS debugger today — the only thing
   missing is the maps that relocate its breakpoints and frames onto `.bynk`.

**The extension contributes no debugger.** `vscode-bynk` hosts the LSP
(`LanguageClient` over stdio) and contributes grammar, snippets, tasks, and
commands — but `package.json` has **no `debuggers` / `breakpoints` contribution**
(tooling-roadmap §3–§4). There is nothing to extend; this is greenfield on the
extension side, which is to our advantage — the surface is small.

## The architecture decision — reuse, don't build an adapter

For a compile-to-JS language on a V8 runtime, the idiomatic path is **not** a
from-scratch Debug Adapter. It is:

1. **Emit source maps** in `bynk-emit` mapping generated `.ts` back to `.bynk`.
2. **Run the target under an inspector** — `wrangler dev` with its inspector port,
   or `node --inspect-brk` for the test entry.
3. **Attach VS Code's built-in JavaScript debugger** (`pwa-node` for Node;
   CDP-attach for `workerd`). The source maps relocate breakpoints, call-stack
   frames, and scopes onto `.bynk`; stepping follows source lines; `names` in the
   map let variable inspection read source identifiers.
4. **Contribute a thin `DebugConfigurationProvider`** in the extension that
   resolves a `"type": "bynk"` launch into the underlying JS debug session —
   *compile → start the target with the inspector → hand off*. Glue and
   configuration, not a bespoke protocol implementation.

The cost lands almost entirely in step 1 (the emitter); steps 2–4 are
configuration and a small provider. The deferred **bynk-native** layer (Phase 2)
is the only place a custom adapter or a variable-formatter contribution might earn
its keep — see slice 5.

## The source-map design — the load-bearing questions

The map format is where an ADR earns its place. The genuine decisions:

- **Granularity — line/statement vs expression.** The natural seam is the existing
  `write_line` statement writer: attach one mapping per emitted statement line back
  to its source statement's span. **Leaning line/statement-level first** — it matches
  §19's "readable, source-identifier-preserving TS" intent, is cheap at the one writer
  seam, and is enough for breakpoints and stepping. But this is a *lean, not a settled
  call*: Bynk's lowering is unusually heavy (one source line can explode into many
  generated lines with non-obvious step order — `match`→`switch`, `?` propagation,
  comprehensions, combinator chains), so the slice-0 spike (open question 2) can
  overturn it toward expression-level. Expression-level is a deferred refinement *only
  if* the spike confirms line-level steps acceptably.
- **Identifier names.** §19 already commits the emitter to *preserving
  source-level identifiers*, so the map's `names` array is mostly free — populate
  it so the debugger's variable panes show Bynk names, not lowered temporaries.
- **Placement.** A sibling `<file>.ts.map` per emitted file plus a
  `//# sourceMappingURL=` trailer on the `.ts`; `sources` point at the `.bynk`
  files (relative), `sourcesContent` optional (embed for portability vs. keep maps
  small — settle in the ADR). **One forcing factor on that "optional":** under
  `workerd` the `.bynk` source may not sit at a path the inspector can resolve, which
  can push toward embedding `sourcesContent` regardless — a known pressure, not a free
  choice. `write_output` gains a map-writing pass.
- **Emission gating — and the production-exposure edge it carries.** `write_output` is
  the *shared* writer behind `bynkc compile`, `bynkc test`, and `bynk dev`, so a naïve
  map-writing pass emits maps for **plain production compiles too**, not just debug
  runs. Combined with the `sourcesContent`-embedding pressure above, an always-on map
  would **ship `.bynk` source into the deployed Worker bundle**. So the ADR must settle
  *when* maps emit and *what they carry on release*: debug-build-only emission, or
  always-emit-but-strip-`sourcesContent`-on-release, or map-without-embed. Not a
  detail — it's the source-confidentiality decision, and it belongs beside the
  placement call, not in implementation.
- **The lowering-gap policy.** The high-level constructs §19 lowers — pattern
  matching → `switch`, `?` propagation, comprehensions, combinator chains, `is`/
  `implies` — have **no 1:1 source line**. The map must point each generated line at
  the *nearest meaningful* source span and the policy for "nearest" must be written
  down, because it is exactly where stepping will feel wrong if left implicit. This
  is the real design content of the ADR.
- **The runtime library.** `emitter/runtime.ts` is **hand-written TS** the
  generated code imports. Its frames should map to *itself*, and stepping should
  **skip it** (`skipFiles` / `smartStep` in the launch config) so a user steps over
  capability machinery, not into it. Same for the generated worker glue
  (`workers_entry.rs`, `wrangler.rs` output).
- **Composition through the bundler.** §19 phase 8 bundles to "one or more Worker
  scripts with source maps." Per-file maps must **compose through** the
  `wrangler`/esbuild bundle step (the bundler rewrites positions and chains maps).
  Confirm the chain survives before relying on it — an open question below.

## Internal architecture

A `SourceMapBuilder` accumulating VLQ-encoded segments, fed by the writer: the
**output** line advances as `write_line` emits; the **input** span comes from the
lowering cursor that already holds the AST node. **One unstated data dependency,
load-bearing for slice 1's ADR:** `Span` is *byte offsets* (`bynk-syntax/src/span.rs`
— `{ start, end }`, half-open), but a source map needs *line + column* at the input
end. The resolver already exists — `span::line_col(source, offset)` in the same leaf,
shared by `bynk-render` and the emitter's assertion locations — so the gap is not
writing it but **getting the per-file source text (or a precomputed line-index) to
emit time**, which the emitter doesn't thread today. The builder consumes that
line-index to resolve each `Span` to (line, col). The minimal surface change is
`write_line` (and the handful of direct `push_str` sites that emit
statement-significant text) gaining an *optional source span*, and the builder
emitting the `.map` + trailer alongside each `ProjectOutput` file. **No
re-architecture** — the emitter stays string-append; it gains a position-tracking
sidecar and starts forwarding the span it already has. That is the whole shape:
"thread the span that's in hand to the writer, accumulate, emit a sibling."

## Slice decomposition (proposed) — all v0.67+

0. ✅ **The source-map contract ADR — landed.** Two doc-ADRs (like `lsp.md` slice 0 /
   ADR 0093): [0103](../decisions/0103-source-map-contract.md) fixes granularity (**line-level, spike-ratified**), placement
   (sibling `.map` + `sourceMappingURL`), the `names` policy, the **lowering-gap
   "nearest *enclosing statement*" rule**, the `skipFiles` boundary, and the
   emission-gating / release-`sourcesContent` confidentiality call; [0104](../decisions/0104-debug-launch-model.md) fixes the
   debug-launch model (reuse-not-build). So slices 1–4 implement against a settled
   format. **Not desk-work:** the granularity and nearest-span calls were *gated on a
   throwaway spike* (open question 2, now closed) — a `?`/`match` example compiled with
   the real emitter, hand-given a line-level map, and **stepped under the V8 inspector**;
   the lowered expansion coalesced monotonically (8→3, 13→6 stops), so the ADR ratifies
   an observed decision, not a guessed one. No production code, no version tag.
1. **Emit source maps — the foundation.** Thread spans through `write_line` (and the
   statement-significant `push_str` sites), add the `SourceMapBuilder`, write the
   sibling `.ts.map` + `//# sourceMappingURL` trailer in `write_output`. **Golden
   tests decode the map and assert a sample of source↔generated line mappings**
   (including at least one lowered construct, to pin the gap policy). Its own ADR
   (the writer-threading + granularity decision). **The big one** — the rest is
   downstream.
2. **Node / test-runner debugging.** The smallest *consumer*, and the end-to-end
   validation of the maps without workerd in the loop: a `"type": "bynk"` debug
   config that compiles, launches the emitted test entry under `node --inspect-brk`,
   and attaches `pwa-node`; assert a breakpoint set in `.bynk` binds and pauses.
   Validates slice 1 cheaply, **and deliberately proves the maps independently of the
   workerd attach** — so if slice 3's `wrangler` inspector proves unstable, the
   `bynk dev --inspect` fallback is a known fork, not a surprise. *Prerequisite — now
   met:* the stable emitted **test entry point** landed with **v0.67** pre-execution
   test discovery (`emit_test_main` + `discovery_manifest`, `project/tests_emit.rs`), so slice
   2 launches `emit_test_main`'s output under the inspector; no longer gated on
   in-flight work. *Gated on slice 1.*
3. **workerd / `wrangler dev` debugging.** The headline UX — and the slice on the
   **least-certain dependency** (open question 1, the `wrangler` inspector port): a
   debug config (or a `bynk dev --inspect` mode) that starts `wrangler dev` with its
   inspector and attaches the JS debugger over CDP; resolve the inspector port;
   `skipFiles` the runtime + generated glue per slice 0. Because slice 2 already
   proved the map format, a failure here is isolated to the *attach*, with the
   `bynk dev --inspect` route as the fallback fork. *Gated on slice 1; sequences
   after the `bynk dev` watch loop if it lands first, but doesn't require it.*
4. **Extension wiring + polish.** The `vscode-bynk` surface: `package.json`
   `debuggers` + `breakpoints` (language `bynk`) contributions, the
   `DebugConfigurationProvider` that resolves `bynk` launches into the underlying JS
   session, default `launch.json` snippets, a **"Debug" CodeLens/command** beside
   the existing test lenses, and `smartStep`/`skipFiles` defaults over `runtime.ts`.
   Mostly configuration once 2–3 prove the sessions out.
5. **Phase 2 — the bynk-native semantic layer.** *Named, not scheduled — likely its
   own track once the pragmatic base proves out.* Make *values* read in Bynk's
   vocabulary: `Result`/`Option`/sum values shown unwrapped, contexts/actors as
   scopes, capability calls legible in the stack. Delivered either as a VS Code
   **variable formatter** over the JS session (cheap, partial) or a thin **custom
   adapter** that wraps the JS debugger (full, expensive). The decision waits on
   real use of phases 1's base — premature now.

Each slice except 0 is an ordinary `vX.Y-<slug>.md` proposal citing this doc and
the foundational ADRs; merging that proposal authorises the build. Status tracked
here as slices land.

## Open questions to close in settle

- **`wrangler dev` inspector attachment.** Does it expose a stable, programmatically
  attachable inspector port across the `wrangler` versions `doctor` accepts? *Prior
  art to read:* `workers-sdk` devtools, the `miniflare` inspector, and how the
  Cloudflare VS Code path attaches. Determines whether slice 3 attaches directly or
  routes through a `bynk dev --inspect` we own.
- ✅ **Breakpoint fidelity on lowered constructs — closed (slice-0 spike).** Is
  line-level mapping with a documented "nearest span" rule acceptable for `match`/`?`/
  comprehensions, or does stepping feel wrong enough to force expression-level sooner?
  **Answer: line-level is sufficient.** The spike compiled a two-`?` + `match` example
  with the real emitter, hand-gave it a line-level map (every generated line anchored to
  its *enclosing source statement*), and stepped it under the V8 inspector. Lowering is
  contiguous and order-preserving, so the `?` guard and match `case`/binding lines
  coalesce into their statement's source line: 8 raw V8 stops → 3 source steps
  (`5 → 6 → 7`); 13 → 6 (`11 → 5 → 6 → 7 → 12 → 13`), monotonic. Ratified in [0103](../decisions/0103-source-map-contract.md) D1/D2;
  expression-level deferred until a construct is *shown* to step poorly.
- **Map composition through bundling.** Confirm per-file maps chain correctly
  through the `wrangler`/esbuild bundle step (§19 phase 8) so production stack
  traces and the attached session both resolve to `.bynk`. If the chain breaks, the
  ADR must say whether we emit a post-bundle map instead.

## On merge — each slice updates

The forward-looking claims here are contracts. On landing, each slice's PR updates,
in the same change:

1. **`bynk-design-notes.md` §19** — mark the source-map commitment *realised* once
   slice 1 lands (and phase 8's "source maps" line, once composition is confirmed).
2. **This track's *Decision log*** — a dated entry with the slice's ADR link(s) and
   the one-line decision.
3. **This track's *Phase* bullet and the *slice-decomposition* row** — marked ✅ with
   the version, so the doc never overstates what shipped.
4. **`bynk-tooling-roadmap.md`** — add/advance the debugging thread under §4; and
   **correct `tracks/lsp.md`'s survey line** ("Bynk has no debugger") once the base
   ships.
5. **Golden / fixture tests** — the map-decode goldens (slice 1) and the
   breakpoint-binds session tests (slices 2–3).

## Foundational ADRs — landed (slice 0)

- ✅ **[0103](../decisions/0103-source-map-contract.md) — the source-map contract.** Granularity (line-level, spike-ratified),
  placement (sibling `.map` + `sourceMappingURL`), `names`, the lowering-gap "nearest
  *enclosing statement*" rule, the `skipFiles` boundary, and **emission gating + release
  `sourcesContent` handling** (the source-confidentiality call). The one genuinely
  format-bearing decision; baked once, up front, so slice 1 implements rather than
  discovers it.
- ✅ **[0104](../decisions/0104-debug-launch-model.md) — the debug-launch model.** The reuse-not-build decision made binding: a
  `DebugConfigurationProvider`, no bespoke DAP in phase 1, targets **workerd + Node**,
  remote **deferred**. Fixes the launch surface and the deferral boundary so slices 2–4
  wire against a settled model.

## Decision log

_A dated entry per slice with its ADR link and the one-line decision, mirroring the
actors / LSP tracks._

- **2026-06-21 — slice 0 (settle).** Foundational ADRs [0103](../decisions/0103-source-map-contract.md) (source-map contract)
  and [0104](../decisions/0104-debug-launch-model.md) (debug-launch model) accepted. **Decision:** line-level,
  statement-anchored source maps emitted as siblings by `bynk-emit`; the lowering-gap
  rule anchors every generated line to its enclosing source statement; reuse VS Code's
  JS debugger via a thin `DebugConfigurationProvider`, no bespoke DAP in phase 1.
  **Ratified by spike:** a `?`/`match` example, compiled with the real emitter and
  stepped under the V8 inspector, coalesced 8→3 and 13→6 raw stops into monotonic
  source steps — line-level is sufficient, expression-level deferred. The
  source-confidentiality call ([0103](../decisions/0103-source-map-contract.md) D6): `sourcesContent` and bundle-chaining are
  profile-gated so `.bynk` source never reaches a deployed Worker by default.
