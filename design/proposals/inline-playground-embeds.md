# Inline playground embeds — the documentation track's final slice (slice 7)

- **Scope:** a **docs-only** increment on the documentation track (`design/tracks/documentation.md`,
  slice 7 in §12). It touches only `site/`: the `Example.astro` component, the six
  runnable By Example snippet pages, and one tutorial step. **No grammar, compiler,
  emitter, or tooling change; no `.bynk` language surface moves.** Per the proposals
  README lifecycle, it carries **no version prefix, no version bump, and is not
  git-tagged** — it ships with its PR and this file is deleted on merge.
- **Realises:** the track's §10 promise, taken past slice 4. Slice 4 gave every
  runnable block an *Open in playground* link (a jump *out* to
  `playground.bynk-lang.org/#<deeplink>`); slice 7 turns that link into *in-place*
  execution — a reader edits and re-runs a snippet or tutorial step **without leaving
  the page**. It is the last slice, so it closes the track.
- **Consumes (does not author):** the deep-link + execution/sandbox contract frozen in
  [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md). This slice authors **no
  ADR** — it emits the already-shipped fragment format and frames the already-shipped
  app. Nothing under `playground/` changes.

## 1. Context — why embed at all

By the time a reader reaches a runnable snippet they can already *read* verified code
(slice 3) and *open* it in the playground (slice 4). The remaining gap is flow: the
open-in-playground link is a context switch — a new tab, the reading thread dropped.
For the places where seeing-it-run is part of the *teaching* (the first-program
tutorial's "Add a function" step; the six By Example primitives), the higher-value
affordance is to run the snippet inline, in the reader's flow, editable in place. That
is what §12 reserved slice 7 for: "edit and see it run in the flow of reading, not just
a link out."

## 2. The decision — frame the shipped playground; do **not** build a native in-page REPL

**[DECISION A] The inline embed is a lazily-loaded cross-origin `<iframe>` of the
already-deployed playground, seeded with the slice-4 deep link — not an in-page REPL
island.** On user click the component frames
`https://playground.bynk-lang.org/#<deeplink>`; the playground decodes its own
`location.hash` at startup (ADR 0140 D5) and comes up with the snippet already loaded,
editable, and runnable. The whole editor/compiler/sandbox machinery is reused wholesale.

The rejected fork is a **native island** — bundling the wasm compiler
(`bynk_compile`/`bynk_analyze`) and an editor into the docs site as an Astro island.
Three facts kill it:

- **Weight.** The debug compiler wasm is ~18 MB (ADR 0140, Consequences). Shipping that
  into the docs build — or lazy-loading it from a docs-owned asset — duplicates an
  artefact the playground already serves, for no gain over framing the playground.
- **The safety boundary is not ours to reproduce.** ADR 0140 requires untrusted snippet
  code to execute only on the **separate, opaque `sandbox.bynk-lang.org` origin** the
  playground owns (D2/D4). A native in-page REPL would either execute untrusted Bynk on
  the docs origin — forbidden — or force this slice to re-implement the cross-origin
  Worker sandbox the playground already ships. Framing the playground inherits that
  boundary intact: the docs never host execution at all.
- **The track non-goal.** §2 is explicit that this track "does not modify the
  playground". Framing consumes it; a native REPL would fork its compiler/editor into a
  second home. So this slice touches **nothing under `playground/`** and ships **no wasm
  into the docs**.

The chosen fork is therefore the one that reuses the whole playground, ships no compiler,
and keeps the security model where ADR 0140 put it.

## 3. Mechanism (delta against the shipped `Example.astro`)

Everything below is additive to the slice-4 component; the static preview and the
open-in-playground link are unchanged.

- **Reuse the build-time deep link.** `embed` is a new boolean prop that only applies
  *with* `runnable` (`canEmbed = embed && runHref`). The embed frames the **same**
  `runHref` slice 4 already computes via `playgroundLink()` — one build-time encode of
  the block's verified source into the ADR-0140 fragment, used for both the link and the
  frame. No new encoder, no second contract.
- **Click-to-load laziness — wasm only on intent.** The frame is **not** in the initial
  DOM. The component renders an *Edit & run in-page* button; a `<script>` creates the
  `<iframe src=runHref>` on first click, so the playground (and its ~18 MB wasm) loads
  **only when the reader asks to run**, never on page load. *Close editor* removes the
  frame (`mount.replaceChildren()`), unloading the wasm and restoring the static preview.
- **Graceful degradation.** The static, highlighted code preview and the *Open in
  playground ↗* fallback link are always rendered and work with **JavaScript off**, and
  if framing is blocked the fallback link still opens the playground in a tab. The embed
  is pure enhancement over a page that is already complete without it.
- **[DECISION B] No restrictive `sandbox=` attribute on the outer iframe.** The framed
  document is **our own trusted app** (`playground.bynk-lang.org`), whose job is to
  isolate untrusted execution in its *own* nested sandbox — the opaque
  `sandbox.bynk-lang.org` Worker of ADR 0140 D2. Clamping the outer frame with a
  restrictive `sandbox=` would fight that: it would strip the app the `allow-scripts` /
  origin it needs to stand up its own inner sandbox, breaking the very isolation model we
  are relying on. The isolation lives one level in, where ADR 0140 designed it; the outer
  frame trusts the app it owns. (We do not restate that contract here — see ADR 0140.)

## 4. Scope — the chosen set, and the first client-side JS

The embed is applied to a **deliberately small, high-teaching-value set**, not
backfilled everywhere:

- **The six runnable By Example snippet pages** —
  `site/src/content/docs/by-example/snippets/{refined-type, opaque-type, is-narrowing,
  result-and-option, sum-type-and-match, validate-and-transform}.mdx` — each passing
  `runnable embed`, sourced from the `site/src/snippets/*.bynk` corpus.
- **One tutorial step** — the first-program tutorial's "Add a function" step
  (`site/src/content/docs/book/tutorials/01-first-program.mdx`), sourced from the new
  gated snippet `site/src/snippets/first-program.bynk`.

**This is the site's first client-side JavaScript.** Until now every surface built to
static HTML/CSS. The added `<script>` in `Example.astro` is inert on every page that
carries no `<Example runnable embed>` — it only binds hosts marked `data-embed-href` — so
the blast radius is exactly the pages above.

## 5. Engineering note — the tutorial `.md` → `.mdx` conversion and its test gate

Using a component (`<Example>`) requires MDX, so the first-program tutorial was renamed
`01-first-program.md` → `.mdx`. That has one non-obvious consequence worth recording:

- The tutorial's example moves **off** the `.md`-only fence-scraping gate in
  `bynkc/tests/doc_examples.rs`, which no longer sees the page.
- The "Add a function" program is preserved as a **gated snippet** —
  `site/src/snippets/first-program.bynk`, compiled by `bynkc/tests/snippets.rs` (the same
  corpus test that gates the six By Example snippets). So the tutorial's non-trivial
  program is still CI-guaranteed to compile; only the *gate* changed, not the *coverage*.
- The trivial first teaching block (the bare `commons demo { type Id = Int }`) stays an
  inline fence. It needs no separate gate because `first-program.bynk` is a **superset**
  of it (same module, plus `classify`), so compiling the snippet transitively exercises
  that program too.

Net: the `.md`→`.mdx` move loses no compile coverage; it relocates it from the fence
gate to the snippet gate, with the trivial block covered by construction.

## 6. Risks

- **The playground origin must stay framable.** `playground.bynk-lang.org` must not send
  `X-Frame-Options` or a CSP `frame-ancestors` that excludes `bynk-lang.org`; today it
  sends neither, so it frames freely. *Mitigation / standing note:* if a future
  playground lockdown adds `frame-ancestors`, it **must** allowlist the docs origin, or
  the embed silently falls back to the open-in-a-tab link. Recorded here so that future
  change knows the docs depend on it.
- **Dependency on the playground origin being live.** The embed can only run once
  `playground.bynk-lang.org` is deployed (slice 0). This is the **same** dependency slice
  4's open-in-playground links already carry — it adds no *new* liveness requirement, and
  degrades identically (static preview + fallback link) when the origin is unreachable.

## 7. Docs delta

The book *is* touched — but there is **no spec, changelog, roadmap, or version impact**,
and this is stated explicitly per the README's "say why" discipline (adapted: the pages
change, the language and toolchain do not):

- **Changed pages:** the first-program tutorial
  (`book/tutorials/01-first-program.mdx`, incl. the `.md`→`.mdx` rename); the six By
  Example snippet pages (`by-example/snippets/*.mdx`); and the shared `Example.astro`
  component. New input file: `site/src/snippets/first-program.bynk` (a compiled fixture,
  not a page).
- **No spec change.** No `.bynk` syntax, diagnostics, or emission moves — nothing in
  `book/spec/` is affected.
- **No new language or tooling surface**, so **no reference or guide page** is added and
  **no `spec/appendix-version-history.md` entry** is needed.
- **No changelog entry, no roadmap change, no currency-banner advance, no version bump.**
  The increment changes *no* language or tooling behaviour — only how the site presents
  already-shipped, already-verified snippets — so there is nothing for the changelog or
  the roadmap to record and no version to bump. (This is the docs-only lifecycle of the
  proposals README §Lifecycle step 2: no versioned artefact exists to bump.)

## 8. Out of scope

- **Modifying `playground/`.** No `?embed` mode, no chromeless variant, no compiler or
  sandbox change — the app is framed as-is (track §2 non-goal).
- **A native in-page REPL** (DECISION A's rejected fork) — no wasm shipped into the docs.
- **Embedding project-scale / Workers-only examples.** Only in-process (`Bundle`) blocks
  are runnable at all (ADR 0140 D6); Workers-only examples keep their slice-4
  *not-runnable, use `bynk dev`* treatment and get no embed.
- **CSP / security-header changes** on any origin — noted as a risk (§6), not changed
  here.
- **Backfilling embeds** beyond the six snippet pages + the one tutorial step. Broader
  adoption, if wanted, is an ordinary later docs edit, not part of this slice.

## Done when

- `Example.astro` supports `embed` (with `title`) — a click-to-load cross-origin frame of
  `playground.bynk-lang.org/#<slice-4 deeplink>`, with the static preview and
  open-in-playground link retained as JS-off / framing-blocked fallbacks.
- The six By Example snippet pages and the first-program "Add a function" step render the
  inline embed; `first-program.bynk` compiles under `bynkc/tests/snippets.rs`.
- Site build + link-check green; the snippet gate green. No version bump, no changelog
  row, no tag.
- On merge: **delete this proposal**. With slice 7 landed, the documentation track is
  complete.
