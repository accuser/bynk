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

_None at present — the real-time / WebSocket track just retired (see below)._

## Retired tracks

Per the lifecycle above (step 3), a completed track doc is removed once its
decisions live on in the ADRs and the spec-in-place. Retired so far:

- **`websocket.md`** — real-time Bynk: the `Stream[T]` value-over-time primitive, a
  streaming-HTTP (SSE-shaped) response terminal consuming it, and the `from WebSocket`
  protocol with held `Connection[F]` resources transferred from a service to an agent.
  Realised design notes §7 (the WebSocket protocol) and §20 Example 2 (the chat-room),
  and sharpened `bynk-type-system.md` §2.9 (`Held[T]`/`Connection[F]` linearity). All
  slices shipped (v0.100–v0.107): `Stream[T]` (0), streaming HTTP (1), held-resource
  linearity (2), the `from WebSocket` bundle (3a), Workers edge-auth + DO-hosted on-open
  (3b-i), hibernation (3b-ii), inbound `on message`/`on close` (3b-iii), and broadcast +
  the §20 chat-room end-to-end (4). Decisions in ADRs
  [0128](../decisions/0128-stream-value-over-time-primitive.md) (`Stream[T]` primitive),
  [0129](../decisions/0129-streaming-http-response.md) (streaming-HTTP response),
  [0130](../decisions/0130-held-resource-linearity.md) (held-resource linearity),
  [0131](../decisions/0131-from-websocket-protocol-bundle.md) (`from WebSocket` bundle),
  [0132](../decisions/0132-from-websocket-protocol-workers.md) (Workers edge-auth +
  on-open), [0133](../decisions/0133-from-websocket-hibernation.md) (hibernation),
  [0134](../decisions/0134-from-websocket-inbound.md) (inbound frames), and
  [0135](../decisions/0135-ws-broadcast-closure.md) (broadcast + closure); spec-in-place
  in `docs/src/spec/syntactic-grammar.md` + `static-semantics.md` and
  `docs/src/reference/websocket.md`. **Deferred follow-ons** (none blocking the theme):
  the `.values` accessor, lambda parameter-type inference, a non-Cloudflare `Connection`
  binding, and a streaming `Ai`/`Queue`-out consumer.
- **`storage.md`** — the agent-local storage-kind catalogue of design notes §10:
  `store` fields replacing the `state { }` record, the five kinds
  (`Cell`/`Map`/`Set`/`Cache`/`Log`; `Queue` ruled out as a delivery concern), the
  `:=`/kind-op write forms, access-pattern annotations, the parity cutover, and
  load-time rehydration validation. All slices shipped (v0.82–v0.97): `Cell` +
  handler-atomic commit (0/1), `Map` (2), `Set` (3), the annotation surface (3a),
  the `Duration` primitive (3b), `Cache` (3c), `Log` (4), the **parity cutover**
  removing `state { }`/`commit`/`self.state` (1p, v0.96), and the **rehydration
  validation gate** (6r, v0.97). Decisions in ADRs
  [0108](../decisions/0108-state-record-to-store-fields.md) (`store` replaces
  `state { }`), [0109](../decisions/0109-handler-atomic-commit.md) (handler-atomic
  commit), [0110](../decisions/0110-storage-map-vs-value-map.md) (`Map`
  storage-vs-value by receiver provenance),
  [0111](../decisions/0111-storage-annotation-surface.md) (annotation surface),
  [0112](../decisions/0112-duration-primitive.md) (`Duration`),
  [0113](../decisions/0113-cache-ttl-eviction.md) (`Cache` TTL eviction),
  [0121](../decisions/0121-log-append-and-retention.md) (`Log` append/retention),
  [0122](../decisions/0122-queue-is-a-delivery-concern.md) (`Queue` is a delivery
  concern, not a storage kind),
  [0123](../decisions/0123-state-block-cutover-and-codemod.md) (the parity cutover),
  and [0124](../decisions/0124-rehydration-validation-and-migration.md) (rehydration
  validation). Spec-in-place in `docs/src/spec/syntactic-grammar.md` +
  `static-semantics.md` and `docs/src/reference/agents.md` + `grammar.md`.
  **Deferred follow-ons** (none blocking the theme): a versioned-schema migration
  capability, per-field default-on-read, a soft recovery handler, whole-collection
  invariant quantifiers (ADR 0123 D4), per-entry DO storage keys, and refined
  non-textual-key rehydration validation (ADR 0124 D5).

- **`query-algebra.md`** — the read/transform combinator vocabulary of design
  notes §11 (lazy `Query[T]` on storage, eager on in-memory collections; builders
  + terminals; `@indexed` secondary indexes with build-time hygiene; joins &
  grouping). All core slices shipped (v0.88–v0.94): the eager `List` vocabulary
  (slice 1), the `Instant` primitive (1b), the `bynk.list`→methods deprecation
  (1c), the lazy `Query` over storage `Map` (2), `@indexed` with routing + hygiene
  warnings (3), and joins & grouping in the **combiner form** (4). Decisions in ADRs
  [0114](../decisions/0114-instant-primitive.md) (`Instant`),
  [0115](../decisions/0115-query-model-lazy-eager-dispatch.md) (`Query[T]` model +
  dispatch), [0116](../decisions/0116-query-vocabulary-and-ordering.md) (vocabulary
  + `Ordering`), [0117](../decisions/0117-non-failing-warning-channel.md) (the
  non-failing warning channel — built here as a prerequisite),
  [0118](../decisions/0118-indexed-indexing-model.md) (`@indexed`),
  [0119](../decisions/0119-durable-object-query-lowering.md) (DO lowering), and
  [0120](../decisions/0120-join-group-combiner-form.md) (the combiner form, no pair
  type); spec-in-place in `docs/src/spec/static-semantics.md` (the query-vocabulary
  section). **Deferred follow-ons** (none blocking the theme): in-memory effectful
  iteration as a uniform method surface (`traverse`/`traverseAll`/`parTraverse`/
  `parTraverseAll` — the original slice 5, tangential to read/transform querying;
  needs its own settling vs the existing `bynk.list.traverse`); the cross-shape
  `Map × Log` join + `Log` time-window builders (land with the storage `Log` slice);
  `@indexed`'s `bynk.index.ambiguous` note + add/remove auto-fixes (await
  compound-predicate routing); **labelled call arguments** (would realise the join
  combiners' `left:`/`right:`/`into:` named surface — v1 is positional); a general
  n-ary **tuple**; and per-entry DO storage keys (turn the index/query CPU wins into
  I/O wins).
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
