# Tooling track — Semantic debugging: make the debugger speak Bynk

- **Phase:** 🟢 **Slice 0 landed — settled; slice 1 next.** The load-bearing
  question is answered: a `DebugAdapterTracker` **can** mutate a DAP response in
  flight (rung 1 of the ladder — spike-confirmed and committed as a guard), so the
  semantic layer is a cheap, **editor-side, runtime-agnostic** rewrite — no proxy
  adapter, and it reaches workerd. The model is fixed in ADR
  [0105](../decisions/0105-semantic-debug-interposition.md). Slices 1–4 below now cut
  as ordinary `vX.Y-<slug>.md` proposals implementing it.
- **Continues:** the [debugging track](debugging.md)'s **Phase 2**. Phase 1 (slices
  0–4, v0.67–v0.72) put real breakpoints, stepping, and the call stack onto `.bynk`
  source by *reusing* js-debug — "wiring, not a Debug Adapter." Phase 2 slice 5
  (v0.73) shipped the cheap, runtime-side half of the semantic layer — **value
  descriptions** (`Ok(42)`) — via js-debug's `customDescriptionGenerator`. That
  mechanism went as far as it can; **everything left needs the debugger to report
  *differently*, not just *describe* differently**, which the cheap path can't do.
  This track owns that remainder.
- **Realises:** ADR [0104](../decisions/0104-debug-launch-model.md) **D1** — "a
  custom adapter (or a variable-formatter contribution) is the *only* place the
  deferred bynk-native semantic layer might earn its keep — showing `Result`/
  `Option`/sum values unwrapped, **contexts/actors as scopes**, **capability calls
  legible in the stack**." Slice 5 took the variable-formatter half; this track
  takes the **custom-adapter** half D1 named, for the asks the formatter can't reach.
- **Depends on / sequences after:** the debugging track's launch surface (the
  `bynk` debug type + `DebugConfigurationProvider`, `vscode-bynk/src/debug.ts`) and
  its source maps (`bynk-emit`). This track interposes *on top of* that session — it
  does not replace the attach. **Refreshes** `bynk-tooling-roadmap.md` (the debugging
  thread under §4).

## Why a track and not a single proposal

ADR 0076 triggers, squarely:

- **Unavoidably multi-increment, sharing one artefact.** Value rewriting for both
  runtimes, scopes-as-actors, stack legibility, and temp-noise suppression are
  separate slices that all flow through **one interposition layer** — the thing
  that sits in the DAP stream and transforms js-debug's responses. The layer is the
  shared contract; each slice adds a transform.
- **Hard-to-reverse, and genuinely risky infrastructure.** The interposition
  mechanism (below) is an architectural commitment every later slice inherits, and
  its feasibility is *not yet known* — the opposite of Phase 1, where the throughline
  was "wiring data we already produce." A delete-on-merge proposal would bet the
  whole semantic layer on an unspiked mechanism and discard the reasoning the later
  reshapes need.

It is a **tooling track** (like `lsp.md`, `debugging.md`) — no language surface, no
threat model — but the multi-increment, shared-and-uncertain-foundation shape is the
ADR 0076 case.

## The throughline — and the one hard capability

> **Every remaining ask reduces to: rewrite what the debugger *reports* — the
> `variables`, `scopes`, and `stackTrace` it sends to the UI — into Bynk's
> vocabulary, on the *editor* side, so it is runtime-agnostic.** Slice 5's generator
> ran *in the debuggee* (and so died on workerd, which forbids the evaluation). The
> editor-side rewrite has no such limit: it transforms js-debug's response after the
> fact, the same for Node and workerd. The hard part is **getting into that stream
> with the ability to change it** — VS Code's public hook for watching a debug
> session (`DebugAdapterTracker`) is *observe-only*. That gap is this track's load-
> bearing risk, and slice 0's spike.

Grounded in the Phase-1/slice-5 reality, confirmed by reading the code:

- The `bynk` session today is a **delegated `pwa-node` attach**: the provider's
  `resolveDebugConfiguration` returns a `type: "node"` config and js-debug runs it
  (`vscode-bynk/src/debug.ts`). We never see its DAP traffic with intent to change it.
- A `DebugAdapterTracker` (which slice 4's entry-resume and the slice-5 spikes use)
  receives `onWillReceiveMessage`/`onDidSendMessage` but is **documented as a
  tracker** — observation, not mutation.
- Bynk values are **uniform tagged objects** (`{ tag: "Ok", value }`, `bynk-emit`'s
  runtime) — recognisable structurally, no metadata needed (slice 5 proved this).
  Compiler temps already carry a **`__`-prefix convention** (`__r0`, `__d` from the
  `?`/`match` lowering) — also recognisable. Capability handles arrive as a `deps`
  object (`http_GET(deps: { Logger })`). So *some* reshapes are inferable from shape
  and naming; the richer ones (frame → capability, context → scope) may want emitter
  **debug-metadata**, decided per slice, not up front.

## The interposition ladder — slice 0's spike

The mechanism is unknown, so slice 0 climbs a **cheap-to-expensive ladder** and
stops at the first rung that works, exactly as Phase 1 spiked the cheap path first:

1. **Tracker mutation (cheapest).** Does mutating the message object in a
   `DebugAdapterTracker.onDidSendMessage` actually change what the UI renders? It's
   undocumented, possibly fragile — but if VS Code forwards the same object, a
   one-file rewrite layer drops in with zero session-wrapping. *Spike this first.*
2. **A wrapping proxy adapter (the real custom adapter).** Register a
   `DebugAdapterDescriptorFactory` for the `bynk` type whose adapter forwards DAP to
   js-debug underneath and rewrites `variables`/`scopes`/`stackTrace` responses
   (tracking `variablesReference` for lazy children). This is ADR 0104 D1's "thin
   custom adapter that wraps the JS debugger" — full control, but it must drive
   js-debug as a sub-adapter without breaking its child-session/CDP machinery.
3. **Bespoke DAP-over-CDP (last resort).** Only if wrapping js-debug proves
   intractable. ADR 0104 D1 deliberately avoided this in Phase 1; rebuilding
   breakpoints/stepping/source-maps is the expensive thing — named here only so the
   ladder is honest, not as a plan.

Slice 0 settles which rung, in **ADR 0105 (the semantic-debugging adapter model)**,
and the transform contract (what each rewrite does, the inference-vs-metadata line)
in **ADR 0106** if the spike shows it needs pinning.

## Slice decomposition

0. ✅ **Settle (no version — ADR + guard).** Spiked the ladder: **rung 1
   (tracker mutation) works** — a committed guard
   (`test/suite/semdbg_interpose_spike.test.ts`) reads a tracker-rewritten value back
   through the DAP. Landed ADR [0105](../decisions/0105-semantic-debug-interposition.md)
   (editor-side rewrite, rung-1-with-fallback, inference-first). **ADR 0106 proved
   unnecessary** — transforms are inference-first and per-slice (ADR 0105 D5), so each
   slice pins its own. No production code.
1. ✅ **Values through the interposer — *both* runtimes (v0.74).** A
   `DebugAdapterTracker` rewrites js-debug's `variables`/`evaluate` responses
   editor-side, parsing the value preview and re-rendering Bynk constructor syntax —
   so **workerd** gets semantic values, the gap slice 5 couldn't close. A real
   recursive parser (`src/semanticValues.ts`, adversarial-unit-tested) and total.
   **Depth finding:** js-debug's preview elides deep nesting, so the editor-side path
   renders one level (`Ok(…)`, inner one expand away); the slice-5 in-debuggee
   generator is therefore **kept for the Node test path** (full inline nesting) and the
   interposer covers workerd — they compose (the rewrite is idempotent). Structural
   recognition, no emitter change.
2. ✅ **Contexts/capabilities as Bynk structure (v0.75).** The interposer relabels
   the Local-scope `variables` response — `deps` → a **`Capabilities`** group, an
   agent's `currentState` → a **`State`** group, floated to the top, still expandable,
   user bindings untouched — so a handler frame reads in Bynk structure, not a flat JS
   local list. **Inference-only, no emitter change** (recognised by the emitter's fixed
   names). Relabelling within the Local scope, not true synthetic Scopes-pane rows (the
   synchronous rewrite can't synthesise a scope's `variablesReference` cheaply).
   **Deferred (the D5 line):** the `by` actor (not dependably a local) and the **emitter
   debug-metadata sidecar** it needs — the named follow-on; compiler-temp suppression is
   slice 4.
3. ✅ **Capability calls legible in the stack (v0.76).** The Call Stack names a handler
   frame for its Bynk operation (`GET "/"`, `bump(amount)`) instead of the emitted
   `http_GET`. **The first feature inference can't carry** — the route/signature isn't in
   the emitted name — so it **introduces the emitter debug-metadata sidecar**
   (`<file>.bynkdbg.json`, additive, no emitted-`.ts` change); the interposer loads it
   and rewrites the `stackTrace` response. Glue-frame collapse was already free (Phase
   1's `skipFiles` → `deemphasize`), confirmed by a stack capture. The actor still rides
   the sidecar as a follow-on.
4. **Quiet the lowered-temp noise.** Hide or group the compiler temporaries
   (`__r0`, `__d`, the `?`/`match` spill bindings) in the Variables pane, so stepping
   shows the user's bindings, not the lowering's. Partly an **emitter** slice —
   formalise the temp-naming convention the interposer keys on.

Each slice (except 0) is an ordinary `vX.Y-<slug>.md` proposal citing this doc and
ADR 0105; merging it authorises the build. Status tracked here as slices land.

## Open questions to close in settle

- ✅ **Can a `DebugAdapterTracker` mutate responses? — Yes (slice-0 spike).** Mutating
  the response object in `onDidSendMessage` propagates to the consumer; a
  tracker-rewritten value reads back through the DAP. Rung 1 chosen (ADR 0105 D2). The
  caveat — it's undocumented — is bounded by a version pin + the committed guard + the
  rung-2 fallback (D3). *The whole track got dramatically cheaper.*
- **Can a proxy adapter wrap js-debug without breaking it?** (Rung 2.) js-debug spawns
  child sessions (per worker/subprocess) and owns the CDP connection; a proxy must
  relay DAP and rewrite responses while leaving that intact. Confirm before committing.
- **`variablesReference` lifecycle.** Rewriting a `variables` response means owning the
  reference graph for lazily-expanded children — the rewrite must be stable across
  expand requests, not just the top frame. Establish the bookkeeping in slice 0/1.
- **Inference vs emitter metadata.** How far do shape (`tag`, `deps`) and naming
  (`__`-temps) carry the reshapes before a debug-metadata sidecar earns its cost?
  Draw the line per slice; keep the emitter untouched where inference suffices.
- **Does the interposer compose with slice 4's provider hand-off?** The `bynk` type
  resolves to a `type: "node"` config today; rungs 1 and 2 attach to that differently.
  Settle how the interposition binds to the delegated session.

## On merge — each slice updates

1. **This track's *Phase* bullet and the *slice-decomposition* row** — ✅ with the
   version, so the doc never overstates what shipped.
2. **This track's *Decision log*** — a dated entry with the slice's ADR link(s) and
   the one-line decision (mirroring the debugging / actors / LSP tracks).
3. **The [debugging track](debugging.md)** — advance its Phase-2 "still open" list as
   each remainder ask lands here.
4. **`bynk-tooling-roadmap.md`** — advance the debugging thread under §4.
5. **Tests** — the session-level rewrite tests (does a `Result` read `Ok(42)` on
   workerd; does a handler frame show a *capabilities* scope), guarded like the
   debugging track's workerd tests (local/opt-in; CI has no `wrangler`).

## Foundational ADRs — landed (slice 0)

- ✅ **[0105](../decisions/0105-semantic-debug-interposition.md) — the semantic-
  debugging interposition model.** Editor-side rewriting of js-debug's DAP responses
  via a `DebugAdapterTracker` (rung 1, spike-confirmed); bounded by a version pin + a
  committed guard + a rung-2 wrapping-proxy fallback; bespoke DAP-over-CDP stays out
  ([0104](../decisions/0104-debug-launch-model.md) D1 holds). Runtime-agnostic, so it
  reaches workerd. Binds only to
  `__bynkChild` sessions; inference-first, emitter metadata per-slice.
- ❌ **0106 — not needed.** The transform contract is inference-first and per-slice
  (0105 D5); each slice pins its own rewrite in its proposal, so no standalone
  contract ADR is warranted up front.

## Decision log

_A dated entry per slice with its ADR link and the one-line decision._

- **2026-06-22 — slice 3 (v0.76).** *Capability calls legible in the stack.* Implements
  ADR [0105](../decisions/0105-semantic-debug-interposition.md), and **introduces the
  emitter debug-metadata sidecar** the track anticipated. A stack capture showed Phase 1
  already collapses glue (js-debug `deemphasize`s every `skipFiles` frame) and points
  handler frames at their `.bynk` line — so the only gap is the frame *name*, and a rich
  label (`GET "/"`, `bump(amount)`) needs the route/signature, which **isn't in the
  emitted name**. So `bynk-emit` emits a `<file>.bynkdbg.json` (`fn → label`, built by
  re-walking handlers with the emitter's own naming functions; additive, no emitted-`.ts`
  change, decode-golden tested), and the interposer loads it per session to rewrite the
  `stackTrace` response — total-by-default. **This crosses the D5 line for the first
  time** (metadata earns its cost); the `by` actor now rides the same sidecar as a
  follow-on. Runtime-agnostic.
- **2026-06-22 — slice 2 (v0.75).** *Contexts/capabilities as Bynk structure.*
  Implements ADR [0105](../decisions/0105-semantic-debug-interposition.md) D4 (rewrite
  structure, not just values): the interposer relabels the Local-scope `variables`
  response — `deps` → `Capabilities`, an agent's `currentState` → `State`, floated up,
  references preserved so they still expand. **The D5 line, drawn:** capabilities + state
  ship by **name-inference, no emitter change**; the **`by` actor is deferred** — it
  isn't dependably a local, so it's the case that earns the emitter **debug-metadata
  sidecar** (its own follow-on slice). **DECISION C held:** relabel within Local, not
  true synthetic Scopes-pane rows — the synchronous rewrite can't cheaply synthesise a
  scope reference. Runtime-agnostic by construction (same path slice 1 proved on
  workerd); `relabelBynkLocals` is pure + unit-tested.
- **2026-06-22 — slice 1 (v0.74).** *Values through the interposer — both runtimes.*
  Implements ADR [0105](../decisions/0105-semantic-debug-interposition.md): a
  `DebugAdapterTracker` (bound to Bynk sessions and their js-debug child sessions)
  rewrites `variables`/`evaluate` responses editor-side, parsing js-debug's value
  preview into Bynk constructor syntax. **The workerd payoff lands** — `Some("hi")`
  renders over `wrangler`'s inspector where slice 5's generator couldn't run.
  **DECISION B resolved by the depth spike:** js-debug's preview elides deep nesting
  (`Ok({…})`), so the editor-side path is shallower than the in-debuggee generator;
  rather than regress Node, **keep the generator for Node** (full inline nesting) and
  use the interposer for workerd + as the universal path — they compose, since
  `renderBynkValue` is idempotent on already-rendered values. The parser is total and
  adversarial-unit-tested. No emitter change.
- **2026-06-22 — slice 0 (settle).** *The interposition model.* Realises
  [0105](../decisions/0105-semantic-debug-interposition.md). The make-or-break spike
  climbed the ladder and **stopped at rung 1**: a `DebugAdapterTracker` *can* mutate a
  DAP response in flight (a tracker-rewritten `variables` value reads back through the
  DAP as the rewrite), so the semantic layer is a cheap, editor-side, runtime-agnostic
  rewrite — no wrapping proxy, and it reaches workerd. The undocumented-behaviour risk
  is bounded by a version pin, a **committed regression-guard spike**, and the rung-2
  fallback behind one interface. ADR 0106 dropped — transforms are inference-first and
  per-slice. No production code; slice 1 (values through the interposer, both runtimes)
  is the first build.
- **2026-06-22 — track drafted.** Spun out of the [debugging track](debugging.md)'s
  Phase 2 after slice 5 (v0.73) shipped value descriptions via the cheap
  `customDescriptionGenerator` and the spike proved that mechanism **(a)** can't reach
  workerd (the runtime forbids in-debuggee evaluation) and **(b)** can only *describe*,
  not *reshape* (`scopes`/`stackTrace` are out of its reach). The remainder —
  workerd-vocabulary values, contexts/actors as scopes, capability-stack legibility,
  lowered-temp noise — all need editor-side response rewriting, a different and
  riskier mechanism (ADR 0104 D1's custom-adapter half). Hence its own track, settle-
  first: slice 0 spikes the interposition ladder and lands ADR 0105 before any build.
