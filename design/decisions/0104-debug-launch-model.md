# 0104 — The debug-launch model: reuse VS Code's JavaScript debugger via a thin `DebugConfigurationProvider`; no bespoke Debug Adapter in phase 1; targets workerd + Node, remote deferred

- **Status:** Accepted (doc-ADR; debugging track, slice 0; 2026-06-21)
- **Spec:** `design/bynk-design-notes.md` §19; `design/bynk-tooling-roadmap.md` §4 (VS Code work)
- **Realises:** the [debugging tooling track](../tracks/debugging.md), slice 0 (front-loaded ahead of slices 2–4).
- **Relates:** [[0103]] (the source-map contract these sessions consume), [[0096]] (`bynk dev` — the `wrangler dev` orchestration the workerd target attaches to).

## Context

Bynk compiles to readable TypeScript on V8 runtimes that *already speak the V8
inspector / Chrome DevTools Protocol*: `workerd` (via `wrangler dev`, [[0096]]) for
local serving, and Node ≥ 18 (`NODE_MAJOR_FLOOR`) for the emitted test runner. VS
Code ships a mature JavaScript debugger (`vscode-js-debug`: `pwa-node` for Node,
CDP-attach for arbitrary V8 inspectors). The `vscode-bynk` extension contributes no
debugger today — no `debuggers`/`breakpoints` in `package.json` — so the launch
surface is greenfield.

The open question was whether faithful Bynk debugging needs a bespoke Debug Adapter
(a DAP implementation that owns breakpoints, stepping, scopes) or whether the
existing JS debugger plus [[0103]]'s source maps suffice. The slice-0 spike answers
it: stepping the emitted TS under the raw V8 inspector, the *only* thing standing
between generated-line stops and clean `.bynk` stepping was the source map — once the
map coalesced the lowered expansion (8→3, 13→6 stops; see [[0103]]), stepping was
correct with no custom adapter logic. The debugger already does breakpoints,
stepping, call stacks, and scopes against mapped source. A DAP would re-implement
what we get for free.

## Decision

**Reuse VS Code's JavaScript debugger. The extension contributes glue — a thin
`DebugConfigurationProvider` — not a protocol. No bespoke DAP in phase 1.**

- **D1 — No Debug Adapter in phase 1.** The session *is* a `vscode-js-debug` session
  over [[0103]]'s maps. The map relocates breakpoints, call-stack frames, and scopes
  onto `.bynk`; stepping follows source lines; `names` makes variable panes read Bynk
  identifiers. A custom adapter (or a variable-formatter contribution) is the *only*
  place the deferred bynk-native semantic layer (track Phase 2) might earn its keep —
  showing `Result`/`Option`/sum values unwrapped, contexts/actors as scopes. That is
  explicitly out of scope here and waits on real use of the base.

- **D2 — A thin `DebugConfigurationProvider` resolves `"type": "bynk"`.** It performs
  *compile → start the target under an inspector → hand off to the underlying JS debug
  session*. It resolves a `bynk` launch into a `pwa-node` attach (Node) or a CDP
  attach (workerd) and returns; the JS debugger runs the session. Glue and
  configuration, not a bespoke loop.

- **D3 — Two targets; remote deferred.**
  - **Node test runner.** Compile, launch the emitted test entry
    (`emit_test_main`'s output, the stable entry that landed with v0.67 test
    discovery) under `node --inspect-brk`, attach `pwa-node`. This is the smallest
    consumer and the end-to-end proof of the maps without `workerd` in the loop —
    slice 2.
  - **workerd / `wrangler dev`.** Start `wrangler dev` with its inspector port and
    attach the JS debugger over CDP — slice 3, the headline UX. Because the Node
    target already proves the map format, a failure here is isolated to the *attach*.
    The fallback fork is a `bynk dev --inspect` mode we own, used if the `wrangler`
    inspector port proves unstable across the versions `doctor` accepts (the track's
    open question 1).
  - **Remote / deployed Worker debugging is out of scope** for this track. The launch
    surface targets local processes only.

- **D4 — `skipFiles`/`smartStep` defaults over toolchain machinery.** Per [[0103]]
  D5, `emitter/runtime.ts` and the generated worker glue (`workers_entry`/`wrangler`
  output) are toolchain code; the default launch config marks them `skipFiles` and
  enables `smartStep`, so a user steps over capability plumbing, never into it.

- **D5 — The extension contributes the standard surface.** `vscode-bynk`'s
  `package.json` gains `debuggers` + `breakpoints` (language `bynk`) contributions,
  default `launch.json` snippets, and a "Debug" CodeLens/command beside the existing
  test lenses — slice 4, mostly configuration once D2/D3's sessions prove out.

## Consequences

Slices 2–4 wire against a settled launch model: slice 2 builds the Node attach (and,
by proving the maps independently of `workerd`, de-risks slice 3), slice 3 the
workerd attach with the `bynk dev --inspect` fallback identified up front, slice 4
the extension contributions. No slice has to decide "adapter or not" again.

The bet is that `vscode-js-debug`'s source-map handling is faithful enough that
glue-only suffices — the spike supports it for stepping and breakpoints; variable
inspection at lowered-temp granularity is the known rough edge that Phase 2's
semantic layer, not a phase-1 adapter, addresses. The deferral of remote debugging
is a deliberate scope cut: attaching to a deployed Worker is a different trust and
transport problem ([[0103]] D6 keeps source out of deployed bundles by default), and
nothing in this model precludes adding it later as a third target.
