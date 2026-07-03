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

- **`documentation.md`** — documentation & web presence: the migration off mdBook
  onto Astro + Starlight (one design system across the landing page, the Book, Bynk by
  Example, the developer docs, and the reference/spec), a CI snippet-verification
  harness so every published `bynk` block compiles, and the integration seams wiring the
  shipped playground into every runnable snippet. Realises the README's documentation
  priority and the design notes' *educational language* framing. **Slice 0** (deploy the
  shipped playground) shipped; slices 1–7 (framework scaffold → Book migration →
  verification harness → By Example → developer docs → landing & brand → deep playground
  integration) follow, each landing as its own proposal.

## Retired tracks

Per the lifecycle above (step 3), a completed track doc is removed once its
decisions live on in the ADRs and the spec-in-place. Retired so far:

- **`testing.md`** — one predicate surface: a far-reaching rethink of how Bynk expresses
  tests, unifying examples, properties, contracts, invariants, and interaction checks as
  facets of the **invariant predicate** the language already has — along a ladder of
  subjects (value → domain → call → snapshot → step → history), sourced by
  supply-or-generation, checked at one of three checkpoints (commit boundary / dev call
  site / test runner), and run at one of three tiers (`unit`/`integration`/`system`). It
  sharpened the testing philosophy and reference and extended the agent-invariant model's
  thesis — *"invariants are the contract half of validation; tests are the behaviour
  half."* All slices shipped (v0.112–v0.119): (1a) `expect` + `suite`/`case` with
  structural failure reporting (v0.112) → (1b) structural test-ness + flat `[paths]`
  (v0.113) → (2) `property`/`for all` and `Val[T]` replacing `Mock[T]` (v0.114) →
  (3) function contracts `requires`/`ensures` (v0.115) → (4) step invariants `transition`
  (v0.116) → (5) the observation surface `expect Cap.op called …` + `trace` (v0.117) →
  (6) the tier dial `as unit | integration | system` + per-seam `provides`, retiring
  `mocks`/`suite integration`/`wires` (v0.118) → (7) history properties
  `for all run: History[Agent]`, the visionary tail (v0.119). Decisions in ADRs
  [0144](../decisions/0144-one-predicate-surface.md) (one predicate surface, landed up
  front), [0145](../decisions/0145-expect-replaces-assert.md) (`expect` replaces
  `assert`), [0146](../decisions/0146-suite-case-vocabulary.md) (`suite`/`case`
  vocabulary), [0147](../decisions/0147-structural-test-ness-and-flat-paths.md)
  (structural test-ness & flat paths),
  [0148](../decisions/0148-val-replaces-mock.md) (`Val[T]` replaces `Mock[T]`),
  [0149](../decisions/0149-generation-is-valid-inhabitants.md) (generation is valid
  inhabitants), [0150](../decisions/0150-contracts-are-invariants-for-functions.md)
  (contracts are invariants for functions),
  [0151](../decisions/0151-the-invariant-subject-widens-to-the-step.md) (the invariant
  subject widens to the step),
  [0152](../decisions/0152-observation-is-auto-recorded-at-the-capability-seam.md)
  (observation auto-recorded at the seam),
  [0153](../decisions/0153-tier-is-a-dial-on-the-case-header.md) (tier is a dial on the
  case header), [0154](../decisions/0154-test-doubles-are-provides.md) (test doubles are
  `provides`), and [0155](../decisions/0155-history-properties-are-runner-only.md)
  (history properties are runner-only); spec-in-place in
  `site/src/content/docs/book/spec/syntactic-grammar.md` + `static-semantics.md` and
  `site/src/content/docs/book/reference/testing.md` + `agent-invariants.md`, with
  `guides/testing/philosophy.md` the keystone rewrite around the spine. **Deferred
  follow-ons** (none blocking the theme): multi-agent protocol properties (the history
  rung is single-agent only — ADR 0155); the universal-emission guarantee that still has
  no home (design DECISION U); a declaration-positional enum `Ord` for ordered-status
  transitions (DECISION O); and whether `example` earns its own keyword over a
  pinned-subject single-case `for all`.
- **`in-browser.md`** — the Browser platform, the JS emit path, the wasm toolchain, and
  the in-browser REPL/playground. Realised design notes §18 (Tier-3 platform bindings)
  and §19 (additional backends; the "a REPL is ambitious and probably v2 or v3" aside) —
  turning the zero-install playground the design notes always pointed at into a shipped
  on-ramp. All slices shipped (v0.108.0–.5): the strip-only emission invariant (0), the
  first-class JS artefact `--emit js` (1), the `--platform browser` binding (2), the
  wasm toolchain `bynk_compile` (3), the REPL/playground itself (4), and slice-5 polish —
  an examples gallery, web-tree-sitter highlighting, a snippet-share service **written
  in Bynk**, and live on-type diagnostics. Decisions in ADRs
  [0136](../decisions/0136-strip-only-emission-invariant.md) (strip-only emitter),
  [0137](../decisions/0137-first-class-js-artefact.md) (JS artefact),
  [0138](../decisions/0138-browser-platform.md) (Browser platform),
  [0139](../decisions/0139-wasm-toolchain.md) (wasm toolchain), and
  [0140](../decisions/0140-repl-execution-and-sandbox.md) (REPL execution & sandbox); the
  playground app lives in `playground/` (outside the Rust workspace). **Deferred
  follow-ons** (none blocking the theme): Cloudflare Pages deployment (two projects +
  DNS), a share-id persistence upgrade beyond the hash form, and LSP-in-browser
  hover/completion. Bynk's `from http` gained no CORS in the process — a noted candidate
  future language feature (same-origin routing sidesteps it for the playground).
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
  in `site/src/content/docs/book/spec/syntactic-grammar.md` + `static-semantics.md` and
  `site/src/content/docs/book/reference/websocket.md`. **Deferred follow-ons** (none blocking the theme):
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
  validation). Spec-in-place in `site/src/content/docs/book/spec/syntactic-grammar.md` +
  `static-semantics.md` and `site/src/content/docs/book/reference/agents.md` + `grammar.md`.
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
  type); spec-in-place in `site/src/content/docs/book/spec/static-semantics.md` (the query-vocabulary
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
  `site/src/content/docs/book/guides/editor-and-tooling/debugging.md`. Phase 2's remainder was carried
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
