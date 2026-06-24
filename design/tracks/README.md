# Feature tracks

Persistent design docs for **far-reaching, multi-increment language features** —
the artefact introduced by [ADR 0076](../decisions/0076-feature-track-posture.md).

A feature track applies when a feature has two or more of: it spans several
increments, its surface is not yet settled, or it is a security/safety boundary.
For everything else, the standard single-increment
[proposal](../proposals/README.md) still applies.

## What a track doc is — and isn't

- **Persistent, not transient.** Unlike a proposal (deleted by the PR that
  implements it), a track doc lives for the whole feature: it is the living map
  the per-slice proposals are cut from, updated as each slice lands, retired only
  when the theme completes.
- **A realisation of the design notes, not a replacement.** It sharpens the
  conceptual commitment in `../bynk-design-notes.md` into a concrete surface,
  an internal architecture, a security/threat model, and an ordered slice
  decomposition. The design notes stay the north star.
- **Not a build authorisation.** Merging a track doc settles *direction*. Each
  slice is still an ordinary `vX.Y-<slug>.md` proposal under `../proposals/`,
  citing this doc and the foundational ADRs; *merging that proposal* is the
  approval to build, per `../proposals/README.md`.

## Lifecycle

1. **Settle.** Draft the doc; close its open design questions (investigation +
   prior art); land the load-bearing, hard-to-reverse **ADRs up front**.
2. **Slice.** Cut each increment as an ordinary proposal that cites the doc and
   the ADRs; build / land / delete as usual. Mark the slice done here.
3. **Retire.** When the last slice lands, the doc is removed (or archived); its
   decisions live on in the ADRs and the spec-in-place.

## Active tracks

- **`storage.md`** — the agent-local storage-kind catalogue of design notes §10
  (`store` fields; `Cell`/`Map`/`Set`/`Log`/`Queue`/`Cache`; the `:=`/`.update`
  write forms; access-pattern annotations). Settling phase: foundational ADRs are
  [0108](../decisions/0108-state-record-to-store-fields.md) (`store` replaces the
  `state { }` record) and
  [0109](../decisions/0109-handler-atomic-commit.md) (handler-atomic commit), plus
  the storage-representation and `Map` value-vs-storage ADRs still to write. The
  query algebra (§11) is a
  sequenced sibling track landing before the Set/Log slices. No slices landed yet.

## Retired tracks

Per the lifecycle above (step 3), a completed track doc is removed once its
decisions live on in the ADRs and the spec-in-place. Retired so far:

- **`debugging.md`** — source-mapped step debugging for Bynk. **Phase 1** (the
  pragmatic base: breakpoints, stepping, and the call stack on `.bynk` source under
  the Node test runner and `workerd`/`wrangler dev`) shipped over v0.67–v0.72 (slices
  0–4), plus **Phase 2's on-ramp** (slice 5, v0.73: value descriptions via js-debug's
  in-debuggee generator). Reuses VS Code's JavaScript debugger via a thin
  `DebugConfigurationProvider` — no bespoke Debug Adapter. Decisions in ADRs
  [0103](../decisions/0103-source-map-contract.md) (source-map contract) and
  [0104](../decisions/0104-debug-launch-model.md) (debug-launch model); guide at
  `docs/src/guides/editor-and-tooling/debugging.md`. Phase 2's remainder was carried
  by `semantic-debugging.md` below.
- **`semantic-debugging.md`** — making the debugger *speak Bynk*: an editor-side
  `DebugAdapterTracker` that rewrites js-debug's `variables`/`scopes`/`stackTrace`
  responses into Bynk's vocabulary (runtime-agnostic, so it reaches `workerd`). Slices
  0–4 (v0.74–v0.77) shipped: the interposition model, values on both runtimes,
  capabilities/state as frame groups, the call stack named by Bynk operation (with the
  emitter `<file>.bynkdbg.json` sidecar), and lowered-temp suppression. Decision in ADR
  [0105](../decisions/0105-semantic-debug-interposition.md). The one named follow-on —
  surfacing the `by` actor in the frame — is parked in
  [issue #286](https://github.com/accuser/bynk/issues/286).

- **`crate-decomposition.md`** — a tooling track: `bynkc` decomposed from a
  monolith into a layered library set
  (`bynk-syntax`/`-render`/`-fmt`/`-check`/`-emit`/`-ide`), the human CLI moving
  up into the driver. All slices shipped (v0.60–v0.66); decisions in ADRs
  [0099](../decisions/0099-crate-layering-dependency-direction.md)–[0102](../decisions/0102-foundation-types-boundary.md)
  (+ the 0084 amendment).
- **`actors.md`** — actor declarations as boundary contracts (the `actor`
  declaration, the `by` clause, authentication schemes, identity). Q1–Q7 shipped
  (v0.45–v0.54); decisions in ADRs
  [0080](../decisions/0080-actor-schemes-closed-nominal.md)–0082, 0085,
  0088–[0092](../decisions/0092-cross-context-caller-value.md). The inaugural
  feature track. Q8 (replay/ordering) deferred to a future Events track —
  [issue #260](https://github.com/accuser/bynk/issues/260).
- **`lsp.md`** — the editor-experience connective plan (completion overhaul,
  navigation round-out, editor polish). Slices 0–7 + 9 shipped (v0.24–);
  decisions in ADRs
  [0093](../decisions/0093-completion-surface-contract.md)–[0095](../decisions/0095-unit-source-map.md),
  with the feature spec in [`../bynk-lsp-spec.md`](../bynk-lsp-spec.md). Remaining
  work tracked in issues
  [#257](https://github.com/accuser/bynk/issues/257) (editor-agnostic docs),
  [#258](https://github.com/accuser/bynk/issues/258) (marketplace publishing),
  [#259](https://github.com/accuser/bynk/issues/259) (refinement-families nav).
