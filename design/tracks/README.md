# Feature tracks

Persistent design docs for **far-reaching, multi-increment language features** —
the artefact introduced by [ADR 0076](../decisions/0076-feature-track-posture.md),
run GitHub-native per [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md):
the track's *state* lives on the tracker (a spine issue, its sub-issues, PR draft
status); the committed doc here carries the *design*.

A feature track applies when a feature has two or more of: it spans several
increments, its surface is not yet settled, or it is a security/safety boundary.
For everything else, the standard single-increment
[proposal](../proposals/README.md) still applies.

## What a track doc is — and isn't

- **Persistent, not transient.** Unlike a proposal (an issue closed by the PR
  that implements it), a track doc lives for the whole feature: it is the living
  map the per-slice proposals are cut from, updated as each slice lands, retired
  only when the theme completes.
- **A realisation of the design notes, not a replacement.** It sharpens the
  conceptual commitment in `../bynk-design-notes.md` into a concrete surface,
  an internal architecture, a security/threat model, and an ordered slice
  decomposition. The design notes stay the north star.
- **Not a build authorisation.** Merging a track doc settles *direction*. Each
  slice is still an ordinary [increment proposal](../proposals/README.md) —
  an issue citing this doc and the foundational ADRs; *accepting that proposal*
  (label `accepted`) is the approval to build.

A track whose theme is one phase of a longer programme names its **trajectory
document** ([`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md)
is the first) as its theme, rather than a design-notes section. The trajectory
holds the endpoint and the phase ordering; the track holds the *method* for its
own phases. Both are living documents, but only the track retires when its
slices land.

## Lifecycle

1. **Propose.** Open a **track issue** from the
   [feature-track template](../../.github/ISSUE_TEMPLATE/feature-track.md)
   (label `track`): the theme, the ADR 0076 trigger check, the open design
   questions, and the candidate slice decomposition. This issue is the track's
   **spine** — it stays open for the track's whole life, collects the slice
   proposals as **sub-issues** (GitHub's progress bar is the live slice
   status), and is closed only at retirement.
2. **Settle.** Open a **draft PR** adding `tracks/<slug>.md`, referencing the
   spine (*"Part of #\<n\>"* — never `Closes`, which would kill the spine at
   adoption). The PR's draft status *is* the settling phase: close the open
   design questions (investigation + prior art) under line-anchored review,
   and identify the load-bearing, hard-to-reverse **ADRs to land up front**.
   Marking the PR **ready for review** asserts the questions are closed;
   **merging it settles direction** — it is not a build authorisation. (If a
   settling PR is marked ready and merged without that assertion actually
   being tested under review, the track's real phase stays **Settling** past
   the merge until a follow-up re-settling PR — step 4 — closes the doc's
   open questions for real.)
3. **Slice.** Cut each increment as an ordinary
   [increment proposal](../proposals/README.md), opened as a **sub-issue of
   the spine** and citing the doc and the foundational ADRs; `accepted` on
   that sub-issue is the approval to build, and the implementing PR closes it.
   Marking the slice done in the doc (and on the spine's checklist) rides the
   implementing PR — a routine update needing no ceremony of its own.
4. **Re-settle when direction changes.** A *substantive* change of direction
   after adoption — a surface decision reversed, a slice re-scoped away, a new
   phase — gets its own small reviewed PR against the doc (a mini settling
   pass), not a ride-along on an implementing PR.
5. **Retire.** When the last slice lands, a **retirement PR** removes the doc,
   appends its closing summary to
   [`../archive/retired-tracks.md`](../archive/retired-tracks.md), and closes
   the spine (`Closes #<n>`). The decisions live on in the ADRs and the
   spec-in-place.

## Active tracks

Live state — which slices are done, what is settling, what is next — is on
each track's spine issue; this table is deliberately just the map.

| Track doc | Spine issue | Phase | Theme |
|---|---|---|---|
| [`documentation.md`](documentation.md) | [#557](https://github.com/accuser/bynk/issues/557) | Slicing (slice 0 shipped) | Documentation & web presence: the Astro + Starlight migration, a CI snippet-verification harness, playground integration seams |
| [`idempotency-capability.md`](idempotency-capability.md) | [#921](https://github.com/accuser/bynk/issues/921) | Slicing (slice 0 shipped, #929; call-site key scoping follow-up shipped, #934) | The `Idempotency` capability: mechanical dedup for at-least-once delivery, per design notes §4, §12 |
| [`content-ownership.md`](content-ownership.md) | [#1086](https://github.com/accuser/bynk/issues/1086) | Slicing (slice 0 shipped, #1089) | `bynk-lsp` becomes the sole reader of `.bynk` source content — realises R2.3's `fs_below_driver` probe for `bynk-emit`/`bynk-ide` |

(`documentation.md` pre-dates the GitHub-native flow, so its doc was
committed by an ordinary PR rather than a settling draft PR; the spine issue
was opened retroactively. `lsp-foundations.md` (now retired) was the first
track to run the ADR 0167 flow from the start — spine issue first, doc via a
settling draft PR; `testing-the-boundary.md` (now retired) was the second;
`locale-capability.md` (now retired) was the third; `message-bundles.md`
(now retired) was the fourth; `idempotency-capability.md` is the fifth;
`compiler-architecture.md` (now retired) was the sixth, and its theme was
internal architecture rather than language surface — as `crate-decomposition.md`
and `increment-allocation.md`, both retired, were before it.
`identity-and-totality.md` (now retired) was the seventh, continuing
`compiler-architecture.md`'s internal-architecture theme as phase 3 of the
same trajectory — all nine of its slices shipped, closing R2.2, R2.4
(expressions), R3.10, R4.1, R4.2, and R4.3 in full, R2.5/R4.9 functionally but
not structurally (see `retired-tracks.md`'s closing summary for the open
question that leaves). Its retirement also closed a documentation gap
`compiler-architecture.md`'s own
retirement PR left open: that PR ([#996](https://github.com/accuser/bynk/issues/996)'s
closing commit, `85bc851e`) removed the doc and appended its closing summary
to `retired-tracks.md` but did not update this table or its "Retired tracks"
list below at the time — both were caught up alongside opening the phase-3
row, which has itself now retired in turn.
`agent-capability-encapsulation.md` is a committed Draft that appears in
neither this table nor `retired-tracks.md`; it predates this row's addition
and needs a spine issue or a retirement — tracked separately, not by this
table. `idempotency-capability.md`'s settling PR (#922) was marked ready for
review and merged 55 seconds later with no review (`reviews: []`) — the
step-2 carve-out above applied to it for a time, since the "ready for
review" assertion that §3's questions were closed had never actually been
tested. A genuine re-settling pass (#924, #927) closed all four of §3's
questions for real (three settled, one deferred to a future track) before
slice 0 (#929) shipped, so the carve-out no longer applies to this row.)

## Retired tracks

A retired track's closing summary — what shipped, which ADRs carry its
decisions, the named follow-ons — is kept for the record in
[`../archive/retired-tracks.md`](../archive/retired-tracks.md):

- **`identity-and-totality.md`** — phase 3 of
  [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md), opened directly by
  `compiler-architecture.md`'s retirement below: `ExprId`/`FileId` node identity, `Ty::Error` and
  total `expr_types`, `Ty` interned behind `Copy`-cheap `TyId`, and `certify`/`CheckedProgram` as
  the analysis/emission boundary. Settled 3 August 2026; all nine slices shipped (T3.0, T3.3a,
  T3.3b, T3.4, T3.5, T3.6a, T3.7a, T3.7b, T3.6b), closing R2.2, R2.4 (expressions), R3.10, R4.1,
  R4.2, and R4.3 — R2.5/R4.9 functionally but not structurally (`expr_types` stays a `HashMap`, not
  the `IndexVec` those rules literally name; a named open question, not silent residue, per the full
  closing summary). Decisions in ADRs
  [0316](../decisions/0316-ty-interning-interior-mutability.md)–[0318](../decisions/0318-ty-interning-one-table-per-build.md).
  Retired 4 August 2026. Opens phase 4 (`bynk-project`, per the trajectory).
- **`compiler-architecture.md`** — phases 0–2 of
  [`../bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md): the compiler testable at crate
  granularity (phase 0, Seams), the named small defects closed with the registries single-sourced
  (phase 1, Paydown), and `lower_expr` returning what it produced instead of appending it to a
  caller-supplied sink (phase 2, Typed lowering) — no language surface change. Settled 1 August 2026
  (settling PR [#997](https://github.com/accuser/bynk/pull/997), v0.246.1); ten Tier A slices plus the
  decision slice and Tier B's three typed-hoist slices all shipped, v0.246.2–v0.247.4. Retired
  3 August 2026 once Tier B's probe (`hoist_sinks`) read zero and both of ADR 0311 D3's phase-3
  triggers had fired. Decisions in ADRs
  [0309](../decisions/0309-refactor-acceptance-gate-per-tier.md),
  [0310](../decisions/0310-the-emit-abi-is-published-the-codegen-is-not.md),
  [0311](../decisions/0311-the-lowering-substrate.md), and
  [0312](../decisions/0312-narrow-bynkc-public-api.md). Opens directly into
  `identity-and-totality.md` above (phase 3, spine
  [#1046](https://github.com/accuser/bynk/issues/1046), now itself retired); three named residue
  items filed as
  [#1047](https://github.com/accuser/bynk/issues/1047)–[#1049](https://github.com/accuser/bynk/issues/1049).
  Full closing summary in `../archive/retired-tracks.md`.
- **`message-bundles.md`** — the `messages` construct, the checked catalogue,
  and the bundle lookup that turned the shipped, bundle-free `render` (ADR
  0256) into a real localiser — the sibling `locale-capability.md` named but
  left unfiled. All three slices shipped: the construct + a single-reference
  bundle, multi-locale completeness + placeholder agreement, and ICU
  MessageFormat (`plural`/`select`/`number`/`date`, host-`Intl` delegation,
  no CLDR data bundled); shipped v0.228.0–v0.230.0 (ADRs 0272–0273, 0276).
- **`locale-capability.md`** — Bynk's first i18n surface: an ambient `Locale`
  capability paired with a pure, total `render(tag, msg) -> String`. All
  three named slices resolved: the capability + `LocaleTag`/`Message`/
  `render` (slice 1), real Cloudflare `Accept-Language` negotiation (slice
  2, shipped with a `uses`-collision limitation later closed by a
  `bynk.locale.types` leaf-commons split), and ICU MessageFormat retired in
  favour of message-bundles' own slice 3; shipped v0.221.0–v0.232.0 (ADRs
  0256, 0277–0278). The track's own stated payoff — automatic boundary-codec
  integration from a refinement failure to a localised message — never
  shipped: it depends on the still-unfiled `predicate`-declaration language
  change; every `render` call across all three slices is manual,
  handler-authored.
- **`deploy.md`** — the `bynk deploy` verb: provisioning + remote deploy, and
  the load-bearing provisioning-state model (`bynk.deploy.lock`); the first
  driver command with irreversible, outward-facing side effects. All six
  slices shipped: state model + KV MVP, DO/queue provisioning, multi-context
  ordering, secrets, environments, and reconciliation maturity + `--prune`;
  shipped v0.154–v0.220.2 (ADRs 0179–0180, 0193–0196, 0254–0255).
- **`increment-allocation.md`** — the version and the ADR number, two serial
  counters that collided across parallel increments, are now assigned by a
  per-merge stamp on `main` (an `xtask` command + a workflow) from a numberless
  `design/pending/` file, not chosen in the feature PR; the surface-shrink also
  made `llms-full.txt` a build artifact. Slices 0–3 shipped (ADR 0206).
- **`lsp-foundations.md`** — the foundation under the shipped LSP surface: one
  project model shared with `bynkc`, a freshness contract, real multi-root
  workspaces, startup analysis + server-registered watchers, one scheduler, and
  the `[lib]` test seam; shipped v0.175–v0.184 (ADRs 0198, 0201, 0202, 0204).
- **`editor-currency.md`** — the editor surface (hover, completion, scaffolds,
  menus/keybindings, codelens) brought back in step with the language and held
  there by a mechanical floor; shipped v0.121–v0.129 (ADRs 0156–0157).
- **`testing.md`** — one predicate surface: examples, properties, contracts,
  invariants, and interaction checks unified over the invariant predicate;
  shipped v0.112–v0.119 (ADRs 0144–0155).
- **`in-browser.md`** — the Browser platform, the JS emit path, the wasm
  toolchain, and the in-browser REPL/playground; shipped v0.108.0–.5
  (ADRs 0136–0140).
- **`websocket.md`** — real-time Bynk: `Stream[T]`, streaming HTTP,
  held-resource linearity, and the `from WebSocket` protocol; shipped
  v0.100–v0.107 (ADRs 0128–0135).
- **`storage.md`** — the agent-local storage-kind catalogue: `store` fields,
  the five kinds, the parity cutover, rehydration validation; shipped
  v0.82–v0.97 (ADRs 0108–0113, 0121–0124).
- **`query-algebra.md`** — the read/transform combinator vocabulary:
  `Query[T]`, `@indexed`, joins & grouping; shipped v0.88–v0.94
  (ADRs 0114–0120).
- **`debugging.md`** — source-mapped step debugging, phase 1 + the phase-2
  on-ramp; shipped v0.67–v0.73 (ADRs 0103–0104).
- **`semantic-debugging.md`** — the debugger speaks Bynk: variables, scopes,
  and the call stack rewritten to Bynk's vocabulary; shipped v0.74–v0.77
  (ADR 0105).
- **`crate-decomposition.md`** — `bynkc` decomposed into the layered library
  set; shipped v0.60–v0.66 (ADRs 0099–0102 + the 0084 amendment).
- **`actors.md`** — actor declarations as boundary contracts; the inaugural
  feature track, Q1–Q7 shipped v0.45–v0.54 (ADRs 0080–0082, 0085, 0088–0092).
- **`lsp.md`** — the editor-experience connective plan; slices 0–7 + 9 shipped
  from v0.24 (ADRs 0093–0095), the feature spec living on in
  [`../bynk-lsp-spec.md`](../bynk-lsp-spec.md).
