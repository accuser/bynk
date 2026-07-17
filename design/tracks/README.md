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
   **merging it settles direction** — it is not a build authorisation.
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
| [`deploy.md`](deploy.md) | [#558](https://github.com/accuser/bynk/issues/558) | Settling — no slice authorised | The `bynk deploy` verb: provisioning + remote deploy, and the load-bearing provisioning-state model |
| [`testing-the-boundary.md`](testing-the-boundary.md) | [#656](https://github.com/accuser/bynk/issues/656) | Slicing (slice 0 shipped) | Driving a service endpoint from a `case`, and the identity that reaches it: teaching the tier dial the public door it was never taught |
| [`increment-allocation.md`](increment-allocation.md) | [#685](https://github.com/accuser/bynk/issues/685) | Core complete (Slices 0–2 shipped; Slice 3 deferrable) | Defer the version and ADR-number assignment to a per-merge stamp on `main`, so parallel increments stop conflicting on the two serial counters |

(`documentation.md` and `deploy.md` pre-date the GitHub-native flow, so their
docs were committed by ordinary PRs rather than settling draft PRs; the spine
issues were opened retroactively. `deploy.md` continues settling via reviewed
PRs against the doc. `lsp-foundations.md` (now retired) was the first track to
run the ADR 0167 flow from the start — spine issue first, doc via a settling
draft PR; `testing-the-boundary.md` is the second.
`agent-capability-encapsulation.md` is a committed Draft that appears in neither
this table nor `retired-tracks.md`; it predates this row's addition and needs a
spine issue or a retirement — tracked separately, not by this table.)

## Retired tracks

A retired track's closing summary — what shipped, which ADRs carry its
decisions, the named follow-ons — is kept for the record in
[`../archive/retired-tracks.md`](../archive/retired-tracks.md):

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
