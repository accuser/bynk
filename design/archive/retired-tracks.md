# Retired feature tracks — the closing summaries

The historical record of completed [feature tracks](../tracks/README.md). A
track's retirement PR removes its doc from `design/tracks/` (the decisions live
on in the [ADRs](../decisions/README.md) and the spec-in-place), appends its
closing summary here — what shipped, which ADRs carry its decisions, and the
named follow-ons — and closes the track's spine issue. Newest first is not
imposed; entries keep the order they were retired in.

- **`testing-the-boundary.md`** — the rung the retired `testing.md` subject
  ladder never had: the **boundary**. Bynk's pitch rests on the edge — types enforced,
  identity sealed, the author writing neither check — yet *no Bynk test could observe
  any of it*: across the fixtures, the set of tests that both drove a `from http`
  service and asserted a boundary claim was empty, and `scheduled`/`queue` had never
  been executed by anyone, including the compiler. The track taught the existing tier
  dial ([ADR 0153](../decisions/0153-tier-is-a-dial-on-the-case-header.md)) the entry
  it was never taught — **no new axis, no new harness**. All four planned slices
  shipped: **0** (v0.181, [ADR 0203](../decisions/0203-test-body-service-calls-resolved.md),
  #662) — the checker resolves the addressed handler (closed the #654 crash); **A**
  (v0.185, [ADR 0205](../decisions/0205-unit-tier-service-address.md), #664) — the
  unit-tier surface: address `http`/`cron`/`queue` from a `case` with
  `by <Actor>(<identity>)`, giving `scheduled`/`queue` their first-ever execution
  coverage; **B** (v0.187, [ADR 0207](../decisions/0207-system-tier-http-boundary.md),
  #667/#697) — the system-tier boundary: drive an http route over a real `worker.fetch`
  with a framework-signed credential the real auth seam verifies, `system_needs_wire`
  relaxed to a serialisation edge; **C** (v0.189,
  [ADR 0210](../decisions/0210-system-tier-wire-rejection.md), #702/#704) — the
  rejection paths: `Wire(<String>)` hands the router raw, pre-validation input so a
  case observes the boundary *reject* it (`Rejected`) or *handle* it (`Handled`),
  decoded on shape not status. Along the way the track's own thesis-in-miniature
  surfaced and closed a real defect: boundary-rejection `400`s shipped without
  `nosniff` ([#659](https://github.com/accuser/bynk/issues/659), v0.188.1,
  [ADR 0209](../decisions/0209-boundary-rejection-security-headers.md)) — *the
  router's behaviour is exactly what no Bynk test could observe*. Surface lives in
  `bynk-emit/src/project/tests_emit.rs` (the test emitter), `bynk-check/src/checker/calls.rs`
  (address resolution), and the `responseToHttpResult`/`responseToHttpOutcome` runtime
  decoders. **Deferred follow-ons** (none blocking the theme, all from ADR 0210):
  rejection-*kind* discrimination ([#705](https://github.com/accuser/bynk/issues/705) —
  `is` tests one level), the `401` path
  ([#706](https://github.com/accuser/bynk/issues/706) — needs a credential override),
  the `405` fall-through ([#707](https://github.com/accuser/bynk/issues/707) — needs
  wrong-method addressing), and mixed typed+`Wire` arguments
  ([#708](https://github.com/accuser/bynk/issues/708)).
- **`editor-currency.md`** — a tooling track closing the drift between what the
  Bynk language *is* and what the editor surface (`bynk-lsp` + `vscode-bynk`)
  shows: hover, completion, scaffolds, menus/keybindings, and codelens brought
  back in step with the language, and held there by a **mechanical floor** so the
  next language slice cannot silently re-open the gap. All six slices shipped
  (v0.121–v0.127): the guardrail — a keyword coverage test + a scaffold-compiles
  test (slice 0, v0.121) → parameter/local hover (1, v0.122) → hover depth for
  declarations (2, v0.123) → completion depth (3, v0.124) → scaffold refresh
  (4, v0.125) → the VS Code UI surface, menus/keybindings/editor-config (5,
  v0.126) → codelens depth, the per-case test-run filter + the capability
  provider lens (6, v0.127) — plus two named fast-follows: `match`-arm pattern
  completion (v0.128, the deferred half of slice 3) and the refinement-family
  codelens (v0.129, closing [#259](https://github.com/accuser/bynk/issues/259) —
  the parked `refines` half of slice 6). Decisions in ADRs
  [0156](../decisions/0156-editor-surface-tracks-language.md) (the editor surface
  tracks the language, with a mechanical floor over hover and completion) and
  [0157](../decisions/0157-scaffolds-cannot-drift.md) (editor scaffolds cannot
  drift — each catalogue compiles in CI, independently). The guardrail lives in
  `bynk-lsp/tests/editor_coverage.rs` + `scaffolds_compile.rs`; the surface is
  `bynk-lsp` (hover/completion/signature-help/codelens) and `vscode-bynk` (the
  manifest). Tooling-only — no language, grammar, or emitter-output change across
  the whole track. **Deferred follow-ons** (none blocking the theme): the
  Marketplace publish ([#258](https://github.com/accuser/bynk/issues/258)) and
  editor-agnostic docs ([#257](https://github.com/accuser/bynk/issues/257)).
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
  [#257](https://github.com/accuser/bynk/issues/257) (editor-agnostic docs) and
  [#258](https://github.com/accuser/bynk/issues/258) (marketplace publishing).
  ([#259](https://github.com/accuser/bynk/issues/259), refinement-families nav,
  shipped v0.129 under the retired `editor-currency.md` track.)
- **`lsp-foundations.md`** — the foundation *under* the shipped LSP surface. An
  external review found the feature surface "unusually feature-complete" and then
  four foundational gaps that shared one shape: every gap was in the
  transport/lifecycle layer, and every test of that layer asserted on static
  shape rather than behaviour over time. The LSP analysed a different project than
  `bynkc`; cached rounds had no freshness gate; workspace folders were advertised
  but unimplemented; there was no startup analysis or dynamic watcher
  registration. Seven slices closed them (v0.175–v0.184): **0** — file identity, a
  project-relative `identity_path` beside the tree-relative `source_path` unit
  validation needs ([ADR 0198](../decisions/0198-file-identity-is-not-the-unit-validation-path.md));
  **A** — one project model, `bynk-ide` reading the manifest's `[paths]`
  `include`/`exclude` exactly as `bynkc` does, so the server analyses the *same*
  files ([ADR 0201](../decisions/0201-the-lsp-analyses-the-compilers-project-model.md));
  **B** — the freshness contract, an index-backed request refreshing to the buffer
  the client holds, never answering stale ([ADR 0202](../decisions/0202-the-freshness-contract.md));
  **C** — the `[lib]` seam, the server moved to `src/lib.rs` so integration tests
  name the crate and the `#[path]` hack retired (no ADR — a refactor); **D** —
  per-workspace state, a project-root-keyed map routing by the file's nearest
  `bynk.toml`, `did_change_workspace_folders`, the multi-root capability made true
  ([ADR 0204](../decisions/0204-per-workspace-project-state.md)); **E** — startup
  analysis + server-registered watchers, a `bynk.toml` tree-walk warming every
  project on activation and the server registering `didChangeWatchedFiles` itself
  so any client is notified (no ADR); **F** — one diagnostics scheduler, a single
  generation-based debounce over both modes at the configured delay (no ADR).
  Q1–Q6 all settled; the recurring lesson was to trace how each handler *uses* the
  analysis, not just how it reads it (rename *writes* many files → needs a
  whole-buffer freshness gate). The decisions live on in ADRs
  [0198](../decisions/0198-file-identity-is-not-the-unit-validation-path.md)/[0201](../decisions/0201-the-lsp-analyses-the-compilers-project-model.md)/[0202](../decisions/0202-the-freshness-contract.md)/[0204](../decisions/0204-per-workspace-project-state.md)
  and the spec-in-place ([`../bynk-lsp-spec.md`](../bynk-lsp-spec.md), consolidated
  in slice G). The **first track to run the [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)
  GitHub-native flow from the start** — spine issue first, doc via a settling
  draft PR. **Deferred follow-ons** (none blocking the theme): a per-URI root cache
  (routing re-walks the FS per request — a static→stateful change, its own perf
  increment), and the capability depth the spec's §8 now lists — local-binding
  rename, match-arm navigation, the consumed-context navigation half, auto-import,
  a test-run codelens.
- **`increment-allocation.md`** — a process track that removed the one structural
  reason parallel increments conflicted: the repo version and the ADR number are
  two strictly-increasing counters, and both were transcribed into shared files
  *while a PR was authored* (`scripts/bump-version.sh` into ~15 files; the ADR file
  + its index row), so two increments developed in parallel collided on every
  version-bearing file and the index table — and the loser silently shipped a
  number another had already taken. The fix: a feature PR now declares only
  **intent** in one `design/pending/<slug>.md` (bump level, changelog blurb, ADR
  prose — **no numbers**), and a per-merge automation assigns the counters on
  `main`, in merge order, where they cannot collide. All four slices shipped:
  the pending-increment format + a `cargo xtask check-pending` validator, in a new
  unpublished `xtask` workspace member (slice 0, [#688](https://github.com/accuser/bynk/pull/688))
  → the `cargo xtask stamp` command — read the pending files, assign each its next
  version + ADR number, run `bump-version.sh`, prepend the changelog row(s), write
  the numbered ADR(s) + index row(s), and delete the consumed file (slice 1,
  [#690](https://github.com/accuser/bynk/pull/690)) → the per-merge GitHub Actions
  workflow that runs the stamp on merge and pushes the result (slice 2,
  [#692](https://github.com/accuser/bynk/pull/692), v0.186) → `llms-full.txt` moved
  from a committed, drift-guarded copy to a build artifact regenerated by a
  `prebuild` npm hook (slice 3, the deferrable surface-shrink,
  [#694](https://github.com/accuser/bynk/pull/694)). The load-bearing decision lives
  in ADR [0206](../decisions/0206-allocation-on-main.md) (allocation happens on
  `main` by automation, in merge order — **per-merge, not a batched release PR**,
  because the ADR number must be assigned at every merge regardless; and
  **delete-what-you-consume** idempotency, which entangles version and ADR
  assignment into one atomic pass — the finding that folded the original slices 1
  and 2 into one and split the workflow out as its own). Two slices carried no ADR
  (infra-only). It lives on in the `xtask` crate (`stamp` / `check-pending`),
  `.github/workflows/stamp.yml`, `design/pending/` and its README, and ADR 0206.
  **Named follow-ons / caveats:** the **first live stamp run had not yet happened
  at retirement** — the workflow fires on the next increment that carries a
  `design/pending/` file, and that merge is the real end-to-end test; the
  surface-shrink stopped at `llms-full` (the 12 crate READMEs are published to
  crates.io and the 7 Book banners were not worth an Astro build-time injection, so
  both stay committed); and the mechanism depends on `main` being **unprotected**
  (the default `GITHUB_TOKEN` pushes directly and triggers no CI) — if `main` is
  ever branch-protected, the stamp must move to a GitHub App with a push-bypass or
  a stamp PR, as ADR 0206 records.
- **`deploy.md`** — the `bynk deploy` verb: provisioning + remote deploy, the
  capstone of the driver arc `doctor → new → dev → deploy`, realising the
  tooling roadmap §5.1 and the deferral `bynk dev` (ADR 0096 D4) named by
  name: "real, provisioned remote support is `deploy`'s defining problem, the
  next slice." The track's one genuinely new idea — the **provisioning-state
  model** (`bynk.deploy.lock`, the deploy-time analogue of `bynk.lock`: real
  Cloudflare resource ids live in persistent driver state, injected into
  regenerated config just before Wrangler runs, never sourced from it) — and
  its one genuinely new responsibility: this is the **first driver command
  with irreversible, outward-facing side effects**, the reason it was a track
  and not a fourth additive verb. All six slices shipped: **0** — the
  provisioning-state model + KV-only single-context MVP (v0.154,
  [ADR 0179](../decisions/0179-deploy-provisioning-state.md)/[0180](../decisions/0180-deploy-orchestration-idempotency.md),
  [#583](https://github.com/accuser/bynk/issues/583)) → **1** — DO migrations
  + queue provisioning, queues reconciling by create-every-run-and-treat-
  already-exists-as-success (v0.171,
  [ADR 0194](../decisions/0194-deploy-queues-and-delegated-do-migrations.md),
  [#600](https://github.com/accuser/bynk/issues/600)) → **2** — multi-context
  topology + Service-Binding deploy ordering, confirming empirically that
  Cloudflare resolves bindings at upload (a hard barrier, not a soft nicety)
  (v0.170, [ADR 0193](../decisions/0193-multi-context-deploy-ordering.md),
  [#601](https://github.com/accuser/bynk/issues/601)) → **3** — secrets at
  deploy time, the declared/read/supplied floor-not-census contract, values
  moved to `wrangler secret put` on stdin and never persisted (v0.172,
  [ADR 0195](../decisions/0195-secrets-at-deploy.md),
  [#602](https://github.com/accuser/bynk/issues/602); follow-up
  [#632](https://github.com/accuser/bynk/issues/632) on computed
  `Secrets.get` names, [ADR 0196](../decisions/0196-secret-reads-and-computed-names.md))
  → **4** — environments, `--env` threaded through the ledger and a
  driver-synthesised `[env.<name>]` config section (confirmed against
  Cloudflare's own docs that bindings are non-inheritable into a named
  environment, so the emitter — which never sees a deploy-time environment
  name — could not do this itself), queue/Service-Binding names qualified to
  avoid cross-environment collision, extended in the same PR to
  `bynk dev -- --remote` after review caught it reading only the default
  ledger section (v0.220.1,
  [ADR 0254](../decisions/0254-deploy-environments.md),
  [#835](https://github.com/accuser/bynk/issues/835)) → **5** — reconciliation
  maturity: per-resource-kind orphan reporting (a pure offline ledger-vs-source
  diff, so `--dry-run` never authenticates), KV drift detection once per
  deploy run (closing the one asymmetry where queues already self-healed but a
  deleted-out-of-band KV namespace did not), and `--prune` scoped to KV
  namespaces and queues alone — never a Worker, whose blast radius (routes,
  custom domains, cron triggers) is categorically larger — with idempotent
  deletion confirmed empirically against a real account rather than assumed
  (v0.220.2, [ADR 0255](../decisions/0255-deploy-reconciliation.md),
  [#839](https://github.com/accuser/bynk/issues/839)). Spec-in-place in
  `site/src/content/docs/book/guides/projects-build-and-deployment/deploy-to-cloudflare.md`
  and `run-locally.md`; surface lives in `bynk/src/deploy.rs`. **Deferred
  follow-ons** (none blocking the theme): release semantics — rollback,
  versioned deploys, traffic splitting — stayed an explicit non-goal
  throughout (§2), noted for a future track; the pre-flight's account
  selection stays `wrangler`-deferred and account-blind (a user pointed at
  the wrong Cloudflare account for a given environment still gets a
  pre-flight pass); pruning an orphaned Worker (`wrangler delete`) and the
  race window between `--prune`'s report and its delete call are both named,
  unclosed gaps in [ADR 0255](../decisions/0255-deploy-reconciliation.md).
  **One unresolved, load-bearing risk, not just a nice-to-have:** the
  packaging track (still an uncommitted local draft with no spine issue of
  its own) plans to re-address contexts as `org.package.context`, and
  `deploy`'s Worker names and ledger keys assume today's flat identity —
  whoever picks up packaging must sequence its naming cutover against
  `deploy`'s provisioned state, or a rename orphans already-live resources
  with no automatic recovery.
- **`message-bundles.md`** — the sibling `locale-capability.md` named but left
  unfiled at settling time: turned [ADR 0256](../decisions/0256-locale-capability-slice-1.md)'s
  shipped, bundle-free `render` (a `tag` it accepted but never consulted, "no
  bundle/lookup mechanism") into a real localiser, using mechanisms this
  compiler already had rather than inventing new ones — the multi-file-commons
  merge ([ADR 0160](../decisions/0160-multi-file-commons-test-barrel.md)) for
  "one locale per file, several files, one bundle," and `match` exhaustiveness's
  bounded-structural-coverage shape
  ([ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md))
  for reference-bundle completeness. All three named slices shipped: **1**
  (v0.228.0, [ADR 0272](../decisions/0272-messages-construct-slice-1.md),
  [#859](https://github.com/accuser/bynk/issues/859)) — the `messages <tag>
  @reference { "code" => "template" }` construct as a commons item, a
  `(tag, code) -> template` lookup, and a generated bundle-scoped `render`
  composing with `bynk.locale.render`'s existing floor, given a real
  checker-visible signature via a synthetic function-table entry; **2**
  (v0.229.0, [ADR 0273](../decisions/0273-messages-checked-catalogue-slice-2.md),
  [#874](https://github.com/accuser/bynk/issues/874)) — multi-locale bundles
  actually render (`tag` finally read, not just accepted),
  `bynk.messages.incomplete` (reference-bundle completeness, one diagnostic
  per missing `(locale, code)` witness) and `bynk.messages.placeholder_mismatch`
  (cross-locale template-placeholder *set* agreement, order-insensitive), and
  the exported `messagesLocales`/`messagesReferenceLocale` set that unblocked
  the Locale track's own negotiation slice; **3** (v0.230.0,
  [ADR 0276](../decisions/0276-messages-icu-format-slice-3.md),
  [#878](https://github.com/accuser/bynk/issues/878)) — ICU MessageFormat
  (`plural`/`select`/`number`/`date` placeholders), parsed by a new
  self-contained mini-parser (`bynk-emit/src/emitter/icu.rs`, no
  `bynk-syntax` grammar change) and rendered by delegating to the host `Intl`
  object — no CLDR data bundled in the compiler — plus
  `bynk.messages.format_mismatch` and `bynk.messages.malformed_icu_syntax`.
  Spec-in-place in `design/tracks/message-bundles.md`'s own §4 (now retired
  with the doc; the decisions live on in the ADRs above); surface lives in
  `bynk-emit/src/emitter/emit.rs`, `bynk-emit/src/emitter/icu.rs`, and
  `bynk-emit/src/project/validate.rs`. **Deferred follow-ons, named not
  silently assumed away:** construction-site catalogue checking — does a
  `message(code).withText(...)` builder chain actually supply the parameter
  names its code's reference template declares — has no precedent anywhere
  in this compiler (every existing "declared shape checked at use" mechanism
  keys on an identifier, never a runtime `String` value) and was deliberately
  left unbuilt (§4.3/§7 M1); code identity ships as a bare, unnamespaced
  dotted `String` pending the still-unfiled packaging identity model (§4.5/§7
  M5 — the same gap `deploy.md`'s own retirement summary above names);
  slice 3's own named exclusions — `selectordinal`, `plural`'s `offset:`/`=N`,
  CLDR skeletons beyond a fixed style-keyword set, nested ICU dispatch, and
  construction-site argument-type checking against a code's declared ICU
  usage — each diagnosed rather than silently mishandled, not built. A real,
  pre-existing rough edge surfaced during slice 3 but not fixed: a context
  consuming a different commons' message bundle for its own `render` hits a
  `bynk.uses.name_conflict` if it also needs `bynk.locale`'s
  `message`/`withWhole`-family constructors (both export a symbol named
  `render`) — no fixture across any slice exercises cross-context bundle
  consumption, only a bundle testing its own commons.
- **`locale-capability.md`** — Bynk's first i18n surface: an ambient `Locale`
  capability (`current() -> Effect[LocaleTag]`) paired with a pure, total
  `render(tag, msg) -> String` — the runtime seam a validation message needs
  to become localised text, without touching predicate purity. All three
  named slices resolved: **1** (v0.221.0,
  [ADR 0256](../decisions/0256-locale-capability-slice-1.md),
  [#844](https://github.com/accuser/bynk/issues/844), PR #845) — the
  capability, `LocaleTag`/`Message`/`MessageArg`, and a bundle-free `render`
  (a fixed `"en"` on every platform this slice, `tag` accepted but unused),
  plus the `message`/`withText`/`withWhole`/`withNum`/`withMoment` builder
  API, living in a new firstparty commons `bynk.locale` (a plain `fn` inside
  an `adapter` has no export mechanism, forcing this placement); **2**
  (v0.231.0, [ADR 0277](../decisions/0277-locale-negotiation-slice-2.md),
  [#882](https://github.com/accuser/bynk/issues/882), PR #884) — Cloudflare-
  only real `Accept-Language` negotiation via RFC 4647 basic filtering
  against a context's uniquely-detected message bundle, shipped with a real,
  named limitation: a `uses`-clause name collision meant the shipped wiring
  could never be exercised end-to-end, verified only at the unit level;
  **closed by** (v0.232.0,
  [ADR 0278](../decisions/0278-locale-types-split.md),
  [#886](https://github.com/accuser/bynk/issues/886), PR #888) — splitting
  `bynk.locale` into a dependency-free leaf, `bynk.locale.types`
  (`LocaleTag`/`MessageArg`/`Message`), so a context calling
  `Locale.current()` no longer collides with a message-bundle commons's own
  synthesised `render` — closing the exact rough edge the sibling
  message-bundles track's own retirement summary above names, and verified
  end-to-end this time, not just at the unit level
  (`bynkc/tests/fixtures/positive/817_locale_bundle_wrapper_e2e`); **3** —
  retired in favour of message-bundles' own slice 3
  ([#878](https://github.com/accuser/bynk/issues/878),
  [ADR 0276](../decisions/0276-messages-icu-format-slice-3.md)): ICU
  MessageFormat resolved the ICU/CLDR dependency decision (L4) for both
  tracks. Surface lives in `bynk-check/src/firstparty/bynk.locale.bynk` /
  `bynk.locale.types.bynk`, the three platform bindings
  (`bynk-check/src/firstparty/bindings/bynk-{node,browser,cloudflare}.ts`),
  and `bynk-emit/runtime/src/locale.ts` (`negotiateLocale`). **Deferred
  follow-on, named not silently assumed away — the track's own stated payoff
  never shipped:** spine issue #838 framed this track's payoff as "a
  validation error escaping a boundary reaches the caller in their language
  with no handler code" — automatic boundary-codec integration, turning a
  refinement failure directly into a localised `Message`. That depends on a
  `predicate`-declaration language change (turning `ValidationError.message`
  from a free-text string into a `Message { code, params }` descriptor)
  which was never filed, has no design-notes section, and does not exist as
  of this retirement — every `render` call across all three slices is
  manual, handler-authored. A future track picking this up starts from the
  `predicate`-declaration gap named here, not from `Locale` itself (which is
  complete).
