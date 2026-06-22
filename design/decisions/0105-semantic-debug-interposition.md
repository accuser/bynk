# 0105 — The semantic-debugging interposition model: rewrite js-debug's DAP responses editor-side via a `DebugAdapterTracker`; a wrapping proxy is the documented fallback; runtime-agnostic so it reaches workerd

- **Status:** Accepted (doc-ADR; semantic-debugging track, slice 0; 2026-06-22)
- **Spec:** `design/bynk-design-notes.md` §19; `design/bynk-tooling-roadmap.md` §4 (VS Code work)
- **Realises:** the semantic-debugging track (`design/tracks/semantic-debugging.md`, retired), slice 0 (settle) — front-loaded ahead of slices 1–4.
- **Relates:** [[0104]] D1 (the custom-adapter-*or*-variable-formatter half this discharges; its "no bespoke Debug Adapter" still holds), [[0103]] (the source maps the session consumes).

## Context

The debugging track (`design/tracks/debugging.md`, retired) Phase 1 reused VS Code's JavaScript
debugger (`vscode-js-debug`) for breakpoints, stepping, and the call stack —
"wiring, not a Debug Adapter" ([[0104]]). Phase 2 slice 5 (v0.73) shipped the cheap,
*runtime-side* half of the semantic layer — value descriptions (`Ok(42)`) — via
js-debug's `customDescriptionGenerator`, a function evaluated **in the debuggee**.
That mechanism is fundamentally limited:

1. **It can't reach workerd.** The Cloudflare Workers runtime forbids the in-debuggee
   evaluation the generator needs; setting it breaks variable inspection outright.
   Slice 5 ships Node-only as a result.
2. **It can only *describe*, not *reshape*.** A description generator rewrites one
   string per object. It cannot regroup `scopes`, relabel `stackTrace` frames, or hide
   variables — the remaining Phase-2 asks (contexts/actors as scopes, capability-stack
   legibility, lowered-temp suppression).

[[0104]] D1 named the home for that remainder: "a custom adapter *or a variable-
formatter contribution* … showing `Result`/`Option`/sum values unwrapped, contexts/
actors as scopes." The open question was **how to interpose on the live session to
rewrite what the debugger reports**, and at what cost — VS Code's public
session-observation hook, `DebugAdapterTracker`, is documented as *observe-only*.

The track framed a cheap→expensive **ladder**: (1) tracker mutation, (2) a wrapping
proxy `DebugAdapterDescriptorFactory`, (3) a bespoke DAP-over-CDP adapter. Slice 0
spiked the cheapest rung first.

**Spike result (committed as a regression guard,
`test/suite/semdbg_interpose_spike.test.ts`):** mutating the response object in
`DebugAdapterTracker.onDidSendMessage` **does** propagate to the consumer. A tracker
that rewrote a local's `value` to a sentinel was read back — through the DAP — as the
sentinel. Rung 1 is viable.

## Decision

**[D1] Interpose, don't rebuild — and do it editor-side.** The semantic layer
*transforms js-debug's responses* (`variables`, `scopes`, `stackTrace`, `evaluate`)
after the fact; it does not replace the session. Phase 1's attach, breakpoints,
stepping, and source maps ([[0104]]/[[0103]]) are untouched. Because the rewrite runs
in the **extension host, not the debuggee**, it is *runtime-agnostic* — the same code
serves Node and `workerd`, closing the gap slice 5 could not.

**[D2] Mechanism: a `DebugAdapterTracker` that mutates responses (rung 1).** Register a
tracker for the delegated session and rewrite the response body in `onDidSendMessage`.
Despite the API being documented observe-only, mutation propagates on the pinned
VS Code / js-debug build (spike-confirmed). This is the cheapest viable rung — a
one-file rewrite, no proxy, no sub-adapter orchestration. The wrapping proxy (rung 2)
is **not** built now.

**[D3] The undocumented-behaviour risk is bounded by a pin + a guard + a fallback.**
Rung 1 leans on behaviour VS Code does not contract. Mitigations: the harness pins
`VSCODE_TEST_VERSION`; the committed spike is the **regression guard** (a VS Code
upgrade that copies responses before the tracker reaches them fails CI); and the
transforms sit behind a thin interposer interface, so if the guard ever goes red the
implementation climbs to **rung 2** (a wrapping proxy `DebugAdapterDescriptorFactory`)
without rewriting the transforms. **Rung 3 (bespoke DAP-over-CDP) stays out of scope**
— [[0104]] D1's "no bespoke Debug Adapter" holds; rebuilding breakpoints/stepping/
source-maps is the expensive thing we still decline.

**[D4] The rewrite owns the `variablesReference` graph.** A `variables` response is
rewritten on *every* request — the top frame *and* every lazily-expanded child — by
re-recognising structure each time, never as a one-shot on the first frame. The
interposer is stateless w.r.t. references: it transforms whatever `variables`/`scopes`
response flows past. (Bookkeeping pinned concretely in slice 1.)

**[D5] Inference first; emitter debug-metadata only where shape won't tell.** Values
(`{tag}`), compiler temps (the `__`-prefix convention), and capability handles (the
`deps` object) are recognisable from shape and naming — no compiler change. Reshapes
that structure can't carry (a frame → its Bynk capability call, a context → a scope)
*may* add an emitter debug-metadata sidecar; that is a **per-slice** decision, gated by
its own spike, **not settled here**. So **ADR 0106 (a standalone transform contract) is
not needed up front** — each slice pins its own transform in its proposal.

**[D6] Bind only to Bynk sessions, reusing Phase 1's plumbing.** The `bynk` debug type
still resolves to a delegated `type: "node"` attach ([[0104]] D2 — Phase 1 unchanged);
the provider already stamps that config with a `__bynkChild` marker. The tracker keys
on that marker, so it rewrites **only** Bynk-launched sessions, never a user's
unrelated Node debugging. The whole layer is gated by the existing
`bynk.debug.semanticValues` toggle (extended from slice 5's value-only scope).

## Consequences

- **The semantic layer is cheap and unified.** One editor-side interposer serves every
  transform and both runtimes — workerd included. The expensive proxy/bespoke paths are
  avoided unless the pin-and-guard ever forces rung 2.
- **Slice 5's generator becomes redundant for Node** once slice 1 delivers values
  through the interposer (which also covers workerd). Slice 1 decides whether to retire
  the generator or keep it as a belt-and-braces Node path; either way the toggle stays.
- **Reliance on undocumented behaviour is the standing risk**, accepted because the
  payoff is large, the blast radius is contained (rung-2 fallback behind one interface),
  and CI catches a regression the day a VS Code bump lands.
- **No language surface, no runtime change, no Phase-1 regression** — the interposer is
  additive and Bynk-session-scoped.
