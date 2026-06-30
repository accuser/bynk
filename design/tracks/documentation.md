# Documentation & web presence — the site, the Book, Bynk by Example, the developer docs, and the playground on-ramp

- **Status:** Draft (settling). Direction not yet merged; no slice authorised. The
  one dependency this track once waited on — the in-browser playground — **shipped**
  (v0.108, ADRs [0136](../decisions/0136-strip-only-emission-invariant.md)–[0140](../decisions/0140-repl-execution-and-sandbox.md);
  the app lives in `playground/`), so this is now the obvious next theme and nothing
  external blocks it.
- **Realises:** the README's promise that "creating excellent documentation … is a
  priority", and `design/bynk-design-notes.md`'s framing of Bynk as an explicitly
  *educational* language. Gives the now-shipped playground (`playground/`,
  [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)) a home to be linked
  *from* and embedded *in*, adopts its orphaned Cloudflare deploy (§9), and gives the
  packaging track's registry a place to be documented.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on two axes: it spans several increments (scaffold → Book migration →
  verification harness → By Example → dev docs → playground integration), and its
  surface is not yet settled (framework, brand). It is *not* a language-surface
  change — no `.bynk` syntax moves — so it lands no normative spec, but it does
  introduce one load-bearing, hard-to-reverse commitment: the documentation
  framework. (The public URL shape is deliberately *not* a hard commitment pre-1.0;
  §5.4.)
- **Deployment target:** `https://bynk-lang.org` (registered, Cloudflare-hosted,
  not yet serving) for the site. The shipped playground's two origins —
  `https://playground.bynk-lang.org` (the app) and `https://sandbox.bynk-lang.org`
  (the execution sandbox) — are not yet deployed either; the in-browser track left
  that as an explicit deferred follow-on, so this track **adopts** it (§9.1).
- **Front-loaded ADRs (named, not numbered):** two — the documentation framework
  (Astro + Starlight, superseding the mdBook/GitHub-Pages posture in `book.toml`) and
  the snippet-verification invariant (every published `bynk` block is compiled in CI).
  Each is created and numbered by the slice that lands it (§12); this doc does not
  pre-allocate numbers, since concurrent tracks would collide. There is **no**
  deep-link-contract ADR to author — that contract is already settled and shipped as
  [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md) (and implemented in
  `playground/src/deeplink.ts`); this track *consumes* it (§10).

---

## 1. Motivation

Bynk has unusually good raw material for documentation and an unusually demanding
audience for it. The raw material: a Diátaxis-structured Book already exists under
`docs/src/`, with tutorials, guides, a reference, and a normative spec; eleven
complete, type-checked example projects under `examples/`; a tree-sitter grammar
and a TextMate grammar that are the single sources of truth for highlighting; and
three in-house mdBook preprocessors that keep the grammar reference and diagnostics
index from drifting. The demand: Bynk's entire thesis is *make illegal states
unrepresentable* and *document what compiles today*. Documentation that drifts from
the compiler would contradict the language's central promise in the most visible
place possible.

What is missing is a **front door** and a **unifying shell**. Today the Book builds
to GitHub Pages at `accuser.github.io/bynk/` (`book.toml:43`), with no landing page,
no curated example gallery on the web, and no developer-facing runtime/CLI home
distinct from the language reference. The registered `bynk-lang.org` is empty. And
while the zero-install **playground shipped** (v0.108, `playground/`), it stands
alone — no origin deployed, woven into no documentation, linked from no tutorial —
so a newcomer still has no in-context path from reading Bynk to running it. The gap
is now *integration*, not absence.

This track designs the whole web presence as one coherent system: a landing page
that welcomes newcomers without slowing experienced developers, the Book as the
canonical first-principles guide, **Bynk by Example** as the problem-first gallery,
**Developer Documentation** for the runtime and CLI, and the integration seams that
wire the shipped **playground** into every runnable snippet as a one-click "run this"
affordance. The unifying decision — taken in §4 — is to move off mdBook onto a single
modern framework so the five content surfaces (landing, Book, By Example,
reference/spec, developer docs) share one design system, one build, one deployment,
and one highlighting pipeline — with the playground as an externally-owned sixth
surface this track deploys and embeds.

### 1.1 The two-audience constraint

The landing page must serve two readers at once, and the design treats this as a
first-class constraint rather than a compromise:

- **The newcomer** needs the one-sentence pitch, a single compelling example, a
  "see it run" button (the playground), and an obvious "start learning" path into
  the Book's first tutorial. They should reach *running code* in under a minute and
  with zero install.
- **The experienced developer** needs to *not be slowed down*: a persistent,
  always-visible top nav (Book · By Example · Reference · Playground · GitHub), a
  command-style search that jumps straight to a reference page, and deep links that
  are stable enough to bookmark. The hero is skimmable in one screen; everything
  marketing sits below the fold and never gates navigation.

The resolution is a landing page that is *navigation-first, marketing-second*: the
nav and search are the product; the hero is a thin, high-quality on-ramp above it.

## 2. Scope and non-goals

**In scope.**

- A single documentation framework hosting all five content surfaces under one domain
  and design system (§4), deployed to Cloudflare (§9).
- Migrating the existing Book (`docs/src/`) into it without losing the
  grammar-embed, diagnostics-semantics, callout, and Mermaid behaviours, nor the
  tree-sitter-faithful `bynk` highlighting (§5).
- A **snippet-verification harness** so every published code block is real,
  compiled Bynk that cannot drift from the toolchain (§6) — the documentation
  analogue of the language's own promise.
- **Bynk by Example**: a problem-first gallery seeded from `examples/`, each page
  playground-runnable (§7).
- **Developer Documentation**: the runtime, the `bynkc`/`bynk` CLIs, the manifest,
  emission, and (optionally) generated crate API docs, as a surface distinct from
  the language reference (§8).
- **Deploying and integrating the shipped playground** — standing up its
  `playground.bynk-lang.org` / `sandbox.bynk-lang.org` origins (the orphaned deploy,
  §9.1), consuming its already-shipped deep-link contract ([ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)),
  and marking which blocks the in-process subset can run (§10).
- A **brand proposal** — name treatment, palette, type, logo direction (§11).

**Non-goals (and why).**

- **Building the playground itself.** The REPL, the wasm toolchain (`bynk-wasm`'s
  `bynk_compile`/`bynk_analyze`), the Browser platform binding, and the sandbox model
  already shipped ([ADRs 0136–0140](../decisions/0140-repl-execution-and-sandbox.md);
  `playground/`). This track does not modify them; it *deploys* the app and *consumes*
  its contracts — the deep-link format and the runnable/​not-runnable marking (§10).
- **Changing any `.bynk` language surface.** No syntax, no diagnostics, no emission
  changes here. Where a doc is *wrong* about the language, that is an ordinary docs
  fix, tracked separately — this doc is about the platform, not the prose.
- **A CMS or a blog engine.** A changelog/release-notes stream is in scope as
  content; a general blogging platform is not. If a blog is later wanted, the chosen
  framework supports it natively, so this is a deferral, not a foreclosure.
- **Versioned docs (multiple concurrent versions) before 1.0.** Pre-1.0 the site
  serves a single "latest" with a clear pre-1.0 banner. The versioning machinery is
  designed for but not built until the language stabilises (§13, open question 1).
- **i18n.** English only for now; the framework is chosen partly because it does not
  paint us into a corner here (§4), but no translation is in scope.

## 3. Five content surfaces (plus the playground), as one site

```
bynk-lang.org/                      Landing — the front door (surface 1)
bynk-lang.org/book/                 The Bynk Book — learn from first principles (surface 2)
bynk-lang.org/by-example/           Bynk by Example — problem-first gallery (surface 3)
bynk-lang.org/reference/  /spec/    Language reference + normative spec (surface 4)
bynk-lang.org/docs/                 Developer Documentation — runtime, CLI, manifest, emission (surface 5)
bynk-lang.org/llms.txt              Already generated; served at root, kept current
playground.bynk-lang.org/           The shipped REPL/playground app  ┐ externally-owned
sandbox.bynk-lang.org/              Its cross-origin execution sandbox ┘ sixth surface (ADR 0140)
```

So: **five content surfaces** on the apex (landing, Book, By Example, reference/spec,
developer docs), plus the **playground** as an externally-owned sixth across two
origins. Reference and spec are *one* surface — the normative spec is the deep end of
the reference, not a separate destination.

The split between **Reference/Spec** (what the *language* does) and **Developer
Documentation** (how the *toolchain* behaves) is the one structural change from
today's Book, where `reference/cli.md`, `reference/bynk-cli.md`,
`reference/manifest.md`, and `reference/emission.md` live amongst the
language-reference pages. Newcomers learning the language and operators running the
toolchain are different jobs; separating them lets each have its own landing and
navigation without bloating the other. (Moving those files breaks in-repo links that
point at them — handled in slice 5; see §8.) The Diátaxis discipline the Book already
follows is preserved within each surface.

The four *documentation* surfaces (the landing page is a front door, not a Diátaxis
mode) map cleanly onto Diátaxis:

| Surface | Diátaxis mode | Reader's question |
|---|---|---|
| The Bynk Book (tutorials + guides + explanation) | learning + tasks | "Teach me the language." |
| Bynk by Example | tasks (worked) | "Show me how to solve *this*." |
| Reference + Spec | information | "What exactly does this do?" |
| Developer Documentation | information + tasks | "How do I run / configure the toolchain?" |

## 4. Framework decision — unify on Astro + Starlight

**Decision: migrate all five content surfaces onto [Astro](https://astro.build) with
the [Starlight](https://starlight.astro.build) docs framework**, replacing mdBook and the
GitHub-Pages posture. The migration is the central, hard-to-reverse commitment of
this track and the subject of the proposed documentation-framework ADR.

### 4.1 Why a unified framework at all

The user's brief is one coherent site spanning a marketing landing page, a
long-form book, a curated example gallery, developer docs, and embedded interactive
playground panels. mdBook is excellent at exactly one of these (the book) and weak
at the rest: it has no first-class landing-page story, no component/island model for
interactive embeds, and a theme system that fights bespoke marketing layouts. Trying
to stitch a hand-built landing page and an example gallery onto an mdBook book under
one domain yields two design systems, two builds, and a seam the reader feels.
Unifying removes that seam.

### 4.2 Why Astro + Starlight specifically

- **Highlighting is already solved, from a source we maintain.** Starlight uses
  [Expressive Code](https://expressive-code.com)/Shiki, which consumes **TextMate
  grammars** directly. The VS Code extension already ships one at
  `vscode-bynk/syntaxes/bynk.tmLanguage.json` (scope `source.bynk`). Pointing Shiki
  at that file gives faithful `bynk` highlighting across the whole site from the same
  grammar the editor uses — no second highlighter to maintain, and one obvious place
  to keep them in step. (If we later want tree-sitter-exact highlighting, Shiki's
  engine is swappable; the tmLanguage path is the pragmatic start. See §5.1.)
- **Static output, Cloudflare-native.** Astro builds to static HTML/CSS/JS and
  deploys to Cloudflare Pages (or Workers static assets) with first-class support —
  matching where the language already lives and where the playground is hosted (the
  playground is itself a static Cloudflare-Pages app, `playground/`). No server
  runtime to operate.
- **Islands for the interactive bits.** The playground embeds, "Run" buttons, and
  any live widgets are Astro *islands* — interactive components dropped into
  otherwise-static pages, hydrated only where needed. This is the clean home for the
  §10 playground seams.
- **MDX + content collections.** Prose is Markdown/MDX, so the existing `docs/src/`
  Markdown ports with minimal churn; content collections give schema-validated
  frontmatter (so a By Example page *must* declare its source file and runnable
  flag), and remark/rehype plugins are the natural reimplementation home for the
  three mdBook preprocessors (§5).
- **Docs defaults built in.** Sidebar, search (Pagefind, local and static —
  no Algolia dependency), table of contents, dark/light, edit-this-page,
  last-updated, and accessible navigation all ship with Starlight, so the bulk of
  the Book's current chrome is replaced rather than rebuilt.
- **Landing + docs in one project.** Astro renders the bespoke marketing landing
  page and the Starlight docs from a single codebase and design system — the precise
  thing mdBook cannot do.

### 4.3 Alternatives considered

- **Keep mdBook, hand-build the rest.** Lowest churn, but it is the two-design-system
  outcome the brief is trying to escape, and it leaves the landing page and gallery
  as bespoke unmaintained HTML. Rejected by the user's "unify everything" steer.
- **VitePress.** Strong, Vue-based, good DX. Comparable on highlighting (Shiki) and
  Cloudflare deploy. Loses on the landing-page/island story (less flexible than
  Astro's framework-agnostic islands) and on content-collection schema validation,
  which we lean on for the verification harness.
- **Docusaurus.** Mature, React-based, versioning and i18n out of the box. Heavier,
  more opinionated theming, a larger runtime, and a less clean static-marketing
  story. Its versioning is attractive for post-1.0; not enough to outweigh the
  weight pre-1.0.
- **Fumadocs / Nextra (Next.js).** Excellent component model, but pull in a Next.js
  server posture that is overkill for a static docs site and a heavier Cloudflare
  deployment than Pages-static. Reconsider only if the site grows app-like surfaces.

The deciding factors are the **existing TextMate grammar** (highlighting for free,
no drift) and the **single-project landing + docs** capability. Astro + Starlight is
the only option that wins both cleanly.

## 5. Preserving the mdBook investment

The current `book.toml` wires three in-house preprocessors and a redirect map. None
of their *behaviours* may be lost in the move; each maps to a concrete Astro
mechanism. The Rust crates that generate canonical data stay — only the rendering
host changes.

### 5.1 `mdbook-bynk-highlight` → Shiki + `bynk.tmLanguage.json`

Today the highlighter renders ```` ```bynk ```` blocks via the tree-sitter grammar.
In Astro, Expressive Code/Shiki loads `vscode-bynk/syntaxes/bynk.tmLanguage.json`
as a custom language. Faithful, zero new artefacts, same grammar as the editor.

Two things the original draft hand-waved are now pinned down:

- **What "the grammars agree" means.** tree-sitter emits a *parse tree* (node types)
  and TextMate emits *regex scopes*; they are not the same token stream, so "agree"
  has to be defined to be testable. The concrete check is a **highlighting corpus**
  (a directory of `.bynk` snippets exercising every construct) rendered both ways and
  compared through an explicit **node-type → TextMate-scope mapping**: each token's
  character span must carry scopes that map to the tree-sitter node covering it. A
  divergence (a construct the tmLanguage mis-scopes) fails CI. Without that mapping
  the check is vacuous; with it, it is a real conformance gate.
- **The escape hatch is already built, not hypothetical.** The original "later option"
  — tree-sitter-exact highlighting via a wasm grammar — *shipped* in `playground/`
  (`scripts/build-grammar.sh` → `tree-sitter build --wasm`, consumed via
  `web-tree-sitter`; the artefact is `playground/dist/tree-sitter-bynk.wasm`). So if
  the tmLanguage proves lossy on any construct, the fallback is a reusable, proven
  asset one directory over, not a research task. That materially de-risks the Shiki
  path: we start with tmLanguage for SSG simplicity, knowing the exact highlighter is
  on the shelf.

### 5.2 `mdbook-bynk-grammar` → a build step + `<Grammar>` component

The `bynk-grammar` crate renders EBNF productions from
`tree-sitter-bynk/src/grammar.json` and the `{{#grammar <rule>}}` / `{{#grammar-semantics
<rule>}}` directives embed them so the reference cannot drift. Keep the crate. Add a
build step that runs it to emit a JSON (or pre-rendered HTML) artefact, consumed by
an Astro `<Grammar rule="..." />` / `<GrammarSemantics rule="..." />` component (or a
remark directive of the same shape). The source of truth — the grammar and
`docs/grammar-semantics.json` — is unchanged; only the embedding host moves from an
mdBook preprocessor to an Astro component fed by the same generator.

### 5.3 `mdbook-bynk-visuals` → Starlight asides + a Mermaid integration

The visuals preprocessor turns `> [!NOTE|TIP|WARNING|DANGER]` blockquotes into
callouts and ```` ```mermaid ```` fences into rendered diagrams. Starlight ships
asides natively (`:::note`, `:::tip`, `:::caution`, `:::danger`); a remark pass can
rewrite the existing `[!KIND]` markers to that syntax at migration time (a mechanical
codemod), or a small remark plugin can keep the `[!KIND]` spelling working verbatim.
Mermaid renders via a rehype-mermaid integration (build-time render preferred, for a
zero-runtime, deterministic, offline build — matching the current vendored-Mermaid
intent in `book.toml`).

### 5.4 The redirect map → dropped (pre-1.0 URLs are transient)

`book.toml` carries a 44-entry redirect table preserving the v0.31 concern-first
reorg's old URLs. **It is not ported.** Pre-1.0, everything is transient: the host
move to `bynk-lang.org` is a clean break, old `accuser.github.io/bynk/*` links are
allowed to go stale, and we do not carry legacy redirects forward. URL stability
becomes a commitment at 1.0, not before — at which point a redirect/permalink policy
is designed deliberately (§13, open question 1). What we *do* keep is a build-time
**internal** link-check (replacing `mdbook-linkcheck`) so the site is never
self-inconsistent; that is a correctness gate, not a legacy-preservation one.

## 6. Documentation that cannot drift — the verification harness

This is the keystone, and the part most aligned with Bynk's identity. The promise
"the Book documents *what compiles today*" should be *mechanically true*, not a
maintenance aspiration. The proposed snippet-verification ADR makes it an invariant: **every published
`bynk` code block is real source that the current toolchain compiles in CI.**

The mechanism, modelled on Rust's `mdbook test`/doctest discipline and Bynk's own
example-project posture:

1. **Authoring.** A runnable block is either *extracted* from a real file under
   `examples/` (or a new `docs/snippets/` corpus of small, self-contained `.bynk`
   units) via a named region, or it is a self-contained block tagged with the
   directive that says "treat me as compilable". Prose-only or deliberately
   non-compiling blocks (showing an error) carry an explicit `ignore` / `compile_fail`
   tag — mirroring rustdoc — and `compile_fail` blocks are *checked to fail with the
   expected diagnostic*, so even the error examples cannot rot.
2. **Extraction & batching.** A build step pulls every tagged block out as a *unit*
   and compiles them **together in one `bynkc check` project**, not one process per
   block — the difference between seconds and minutes at the ~hundreds-of-blocks scale
   (~100 Book pages + By Example + the snippets corpus), and the exact slowness that
   makes rustdoc doctests notorious. There is prior art in this very repo: the `bynkc`
   fixtures were moved to *one* `tsc` pass and went 48s → 4s (commit `beebc03`); the
   harness mirrors that. Blocks that must compile in isolation (a `compile_fail` case,
   or two blocks defining the same name) are their own small units within the same
   batch. The block's rendered text is the *same bytes* that were compiled —
   extraction renders from source, never the reverse. CI is **incremental**: only
   pages whose blocks (or the toolchain) changed are recompiled, keyed on a content
   hash, so the common docs-only PR pays for what it touched.
3. **CI gate.** The site build fails if any non-`ignore` block fails to compile, or
   if any `compile_fail` block compiles, or if its diagnostic code drifts. This runs
   in the same CI that builds the site, so a language change that breaks a doc
   snippet breaks the build — exactly as a broken test does.

**Shared with the playground, not re-derived.** The questions "does this block
compile?" and "is it runnable in-browser, or does it reach Workers-only shapes?" are
answered by the **same platform-lock determination** the shipped wasm path already
computes (`bynk-wasm`'s `bynk_compile`/`bynk_analyze`, [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)).
§6 (native `bynkc` in CI) and §10 (the browser runnable-marking) must therefore read
*one* verdict per block — the platform lock — rather than each inventing its own
notion of "runnable". To be precise about *how* CI reads it (no wasm in the loop): the
extractor obtains the verdict from **`bynkc check --platform browser`**, which raises
`bynk.target.vendor_required` at validate time on any unit that pulls in a
Cloudflare-only surface ([ADR 0138](../decisions/0138-browser-platform.md) D4). That
is the *same lock* the wasm path computes — not duplicated logic, but the same
`bynk-check` core reached through two front ends. Concretely: the extractor records,
per block, the compile result and that platform-lock verdict; the site consumes the
record both to gate the build and to decide whether to render a "Run" button. The playground's own
curated gallery (`playground/src/examples.ts` — "runnable in-process snippets, each
verified to compile + run") is the existence proof that this verdict is exactly what
distinguishes a runnable snippet, and a model for the By Example tier (§7).

The pay-off compounds: By Example (§7) becomes *guaranteed-correct by construction*,
the reference's examples stay honest through every increment, and the "what compiles
today" line in the README graduates from a claim to a CI invariant. It also makes
the playground integration safe: a block marked runnable has *already* been
compiled, so the "Run" button cannot offer a snippet that fails to build.

## 7. Bynk by Example

**Recommended model (my suggestion, per the brief): a hand-curated, Go-by-Example /
Rust-by-Example–style gallery whose code is mechanically extracted and CI-verified —
"curated narrative over generated-and-verified code".** This is the synthesis of the
two pure options: it keeps the editorial quality that makes such galleries the most
beloved docs in their ecosystems, while the §6 harness guarantees the code is real.
Neither pure option is as good: fully auto-generated pages read like a code dump and
under-teach; fully hand-written code drifts. The curated-narrative-over-verified-code
form is strictly better and is what the harness was built to enable.

Concretely:

- **Seeded from `examples/`.** The eleven existing projects already each "lead with a
  different part of the language" (`examples/README.md`) and are type-checked,
  compiled, and tested. By Example pages are cut from them: each page is a worked
  problem ("Shorten a URL with a TTL", "Rate-limit per caller", "Verify a signed
  webhook"), presented in the Go-by-Example two-column form — annotated code beside
  prose — with the code *extracted from the real project*, not retyped.
- **A small-snippets tier.** Below the project-scale examples, a `docs/snippets/`
  corpus of bite-size units (a refined type, a `match`, an `is` narrowing, a
  capability + mock) covers the language's primitives in isolation. These are the
  most playground-friendly because they are `Bundle`-topology and in-process.
- **Each page carries two affordances:** "Open the full project" (deep link to the
  `examples/` directory on GitHub) and "Run in playground" (§10) where the snippet is
  in the runnable subset. Project-scale examples that reach Workers-only shapes
  (Durable-Object agents, cross-context calls) are marked *not runnable in-browser*
  with a one-line why and an install/`bynk dev` pointer — honest about the boundary
  rather than offering a button that can't work (the in-process/`Bundle` subset is
  defined by [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)).
- **Ordering teaches.** The gallery opens with `hello-world` and follows the
  `examples/README.md` arc (refined types → KV persistence → public/authorised routes
  → agents + storage kinds → `Query[T]` joins → cron → webhooks), so reading it
  top-to-bottom is itself a course.

## 8. Developer Documentation

A surface distinct from the language reference, aimed at the person *operating* the
toolchain rather than *writing* the language. Much of the content exists in the Book
today and is re-homed and expanded here:

- **The CLIs.** `bynkc` (`compile`/`check`/`fmt`/`test`) and the `bynk` driver
  (`doctor`/`new`/`dev`) — from `reference/cli.md` and `reference/bynk-cli.md`,
  surfaced as a proper command reference with a page per command, flags, exit codes,
  and worked invocations.
- **The runtime & emission.** What `bynkc` emits, the runtime library, the
  strip-only/TypeScript output, and the Cloudflare Worker shape — from
  `reference/emission.md`, `spec/runtime-library.md`, and `spec/emission.md`,
  framed for an integrator who wants to understand or debug the generated TypeScript.
- **The manifest.** `bynk.toml` — from `reference/manifest.md`, as the operator's
  configuration reference. (When the packaging track lands, the
  `[organisation]`/`[workspace]`/`[package]`/`[dependencies]` surface and `bynk.lock`
  document here.)
- **Editor & tooling.** `bynk doctor`, the formatter, the LSP, the VS Code extension,
  and debugging — re-homing the Book's `guides/editor-and-tooling/` section.
- **Generated crate API docs (optional).** `cargo doc` for the published crates
  (`bynkc`, `bynk`, `bynk-fmt`, `bynk-grammar`, `bynk-lsp`) can be built in CI and
  published under `docs/api/`, linked from but not inlined into the hand-written
  developer docs. Flagged as optional in §12 — valuable for contributors and
  embedders, but it is a separate toolchain (`rustdoc`) and a maintenance surface, so
  it is a deferred slice, not an early commitment.

Re-homing `reference/cli.md`, `reference/bynk-cli.md`, `reference/manifest.md`, and
`reference/emission.md` into this surface **moves files that the repo links to from
elsewhere** — `docs/src/SUMMARY.md`, `README.md`, and ~15 guide/spec pages reference
them today (the same dangling-link hazard as §5.4, internal edition). The slice that
performs the move (slice 5) owns updating those in-repo references in the same change,
and the build-time internal link-check (§9.3) is what catches any it misses.

## 9. Deployment on Cloudflare

### 9.1 Hosting

Astro's static output deploys to **Cloudflare Pages**. **Three** Pages projects,
because this track adopts the playground's orphaned deploy (the in-browser track
shipped the app but left "Cloudflare Pages deployment — two projects + DNS" as an
explicit deferred follow-on, so nobody owned it until now):

1. `bynk-lang.org` — the site (this track builds it).
2. `playground.bynk-lang.org` — the shipped playground app (`playground/`; we deploy
   its existing build, we do not rebuild it).
3. `sandbox.bynk-lang.org` — the playground's cross-origin execution document. A
   *distinct* origin is load-bearing for the safety boundary ([ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)):
   untrusted snippet code executes only here, so it can never reach the app origin's
   storage. This is not optional polish — deploying the app without its separate
   sandbox origin would break the security model.

DNS is the apex `bynk-lang.org` plus the `playground` and `sandbox` subdomains, all
on Cloudflare. Every project here is fully static; no Worker compute is required (the
playground compiles and runs entirely client-side), which keeps hosting trivial. (The
one Worker the playground *did* ship — the snippet-share service written in Bynk under
`playground/share/` — backs the deferred share-id persistence path of §10, not the
hash-fragment contract; it is **not** deployed by this track.)

### 9.2 Build, release & toolchain pinning

Build in CI (GitHub Actions, matching where the repo already runs) on every push and
PR: build the Rust generators (`mdbook-bynk-grammar`'s successor step, the snippet
extractor), run the §6 verification gate, build the Astro site, and deploy. **Per-PR
preview deployments** (Cloudflare Pages gives these natively) let a docs change be
reviewed as a live site before merge. `main` deploys to production. The generated
`llms.txt`/`llms-full.txt` continue to build from source and are served at the site
root, with the existing drift check kept.

**Pinning, to match the repo's fastidiousness.** The repo pins Rust
(`rust-toolchain.toml` + an MSRV CI leg); the docs/Node toolchain gets the same
discipline, and there is already in-repo prior art to follow — `playground/` ships a
committed `package-lock.json`, an esbuild build (`build.mjs`), and a pinned `tsconfig`.
The site therefore commits its `package-lock.json`, pins the **Node version** (a
`.nvmrc`/`engines` field, exercised by the CI leg), and pins the **Astro/Starlight**
major in `package.json`. Astro moves fast, so major-version churn is absorbed
deliberately — a periodic, reviewed bump (a Renovate/Dependabot PR that must pass the
full build + §6 + link-check gate), never an unpinned float — so a green build is
reproducible months later, exactly as the Rust side already guarantees.

### 9.3 URLs

Pre-1.0, URLs are transient (§5.4): no legacy redirect map is carried, and the path
shape in §3 is free to change as the site finds its form. The only gate is a
build-time **internal** link-checker, so the site is never self-inconsistent. URL
stability — permalinks, a redirect policy, "cool URIs don't change" — becomes a
deliberate commitment at 1.0, designed then rather than retrofitted now.

## 10. The playground link contract (already shipped — this track consumes it)

The deep-link contract is **not** something this track authors; it shipped with the
playground as [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md) and is
implemented in `playground/src/deeplink.ts`. The doc's job is to *emit links in that
exact format* and to mark which blocks are runnable:

- **Snippet → deep link. [SHIPPED]** The format is the source carried in the URL
  **fragment** as `#base64url(deflate-raw(utf8(source)))`, produced with the
  browser-native **Compression Streams API** (`CompressionStream("deflate-raw")`), so
  there is **no library dependency on either side** and no server round-trip. The docs
  emit links with the *same* browser-native call the playground decodes with — the
  contract is byte-for-byte, and `deeplink.ts`'s own header comment already names the
  documentation track as the other party. The format is deliberately
  **general-purpose, not docs-only**: it is *the* Bynk snippet-share mechanism, so a
  link shared from a bug report, a chat, or this site is the same artefact. (A
  share-id persistence service is a deferred in-browser follow-on; the hash form
  stands alone and needs nothing from it.)
- **Per-block affordance.** Every runnable block renders a "Run in playground"
  action (an Astro island) that builds the deep-link fragment from the block's *verified*
  source (§6) and opens `playground.bynk-lang.org/#…`. Because the source already
  compiled in CI, the button cannot offer a snippet that fails to build.
- **Runnable marking — the shared verdict.** Whether a block is in the in-process
  (`Bundle`) subset is the platform-lock verdict the extractor already recorded (§6),
  *not* a second heuristic computed here. Workers-only blocks (Durable-Object agents,
  cross-context calls) render no Run button and show a one-line reason plus an
  install/`bynk dev` pointer. Same determination, read once, used in two places.
- **No degradation window.** The origin is stood up in **slice 0** (§12), before any
  slice emits a Run link (By Example is slice 4), so there is never a moment where a
  runnable block exists but its target does not. The earlier "what if the playground
  isn't live yet" hedge is gone: the playground shipped, and this track deploys its
  origin first of all.

## 11. Brand proposal

No fixed brand exists, so this proposes one; it is a starting point for iteration,
not a final identity.

- **Name & wordmark.** "Bynk" — short, four letters, a hard *b*/*k* frame around a
  soft *y*. Set it lower-case as a wordmark in a geometric grotesque, with the *y*'s
  descender as the one expressive stroke. The name reads as a cousin of *link* and
  *byte* — lean into that: the language *links* contexts and compiles to the *byte*
  world of the web.
- **Logo direction.** The language's core idea is *architecture in the language* —
  contexts as blocks that link. A mark of **two interlocking blocks** (or a *b*
  formed from a block and a connector) captures "typed shapes that fit together" and
  works as a favicon at 16px. Avoid anything organic; the language is about precise,
  fitted structure.
- **Palette.** Anchor on the code themes the Book already uses for continuity — the
  warm *rust* light theme and the *ayu* dark theme — and add one brand accent. A
  proposal: a deep slate/ink as the neutral, a single saturated accent (a teal or
  electric-indigo) for links and the primary call-to-action, and the rust/ayu
  families reserved for code. One accent, used sparingly, keeps the experienced-dev
  surface calm.
- **Typography.** A geometric/grotesque sans for UI and headings (e.g. an
  Inter/Geist-class face), a high-legibility mono for code (e.g. a JetBrains
  Mono/Berkeley-class face with good ligature behaviour for `->`, `<-`, `~>`,
  `:=`). Generous line-height in prose; tighter in code.
- **Voice.** The repo's existing voice — precise, dry, em-dash-fond, honest about
  what is deferred — is itself a brand asset. The site copy should match it: confident
  and exact, never breathless. The pre-1.0 banner says "evolving in small, spec-first
  increments" plainly.
- **Taglines (options).** "Architecture-first. Statically typed. Compiles to the
  web." / "Make illegal states unrepresentable — then deploy them to the edge." /
  "The shape of your service, in the language."

A one-page visual mock and a couple of logo sketches are the natural first artefact
of slice 6; they are not produced here because the brief was a plan, not assets.

## 12. Phasing — the slice decomposition

Each slice stands up something real and independently valuable. The one irreversible
decision (the framework) is isolated in its own slice so nothing security-sensitive
or independently-shippable is held hostage to it; the URL shape is deliberately *not*
a pre-1.0 commitment (§5.4).

- **Slice 0 — deploy the shipped playground.** Stand up `playground.bynk-lang.org`
  (the app) and `sandbox.bynk-lang.org` (its cross-origin execution document) as two
  Cloudflare Pages projects from the existing `playground/dist`, with DNS for both
  subdomains and the separate-origin split that the safety boundary requires (§9.1 /
  [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)). Depends on **nothing**
  in this track — no framework, no Astro, no content — so it can land first, today.
  *Value:* the playground is finally reachable on the web; a clean, independently
  valuable milestone, and every later "Run" link has a live target from the outset.
- **Slice 1 — scaffold the framework shell.** Astro + Starlight project; the
  `bynk-lang.org` Pages project + apex DNS; pinned Node/Astro toolchain (§9.2); Shiki
  wired to `bynk.tmLanguage.json`; an internal link-check gate; a placeholder landing
  + an empty Book shell. Lands the documentation-framework ADR — the one
  hard-to-reverse call, now isolated. *Value:* the domain serves and highlighting
  works.
- **Slice 2 — migrate the Book.** Port `docs/src/` content (already Markdown,
  already Diátaxis) into Starlight; reimplement the grammar-embed and
  diagnostics-semantics as components fed by the existing crates (§5.2); rewrite
  callouts/Mermaid (§5.3); land the §5.1 grammar-agreement corpus check (it earns its
  keep here, once there is highlighted content to protect); local Pagefind search;
  link-check gate. *Value:* the Book is live at `bynk-lang.org/book/`, at parity with
  today, better-looking.
- **Slice 3 — the verification harness.** The snippet extractor + `bynkc check`/`test`
  CI gate; tag the existing Book blocks; `compile_fail` diagnostic checks. Lands the
  snippet-verification ADR. *Value:* the Book's code is now CI-guaranteed correct.
- **Slice 4 — Bynk by Example.** The gallery from `examples/` + the `docs/snippets/`
  tier (§7), every block verified by slice 3, every runnable block emitting a
  playground deep-link in the **already-shipped** format (§10 / [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)).
  Authors **no** ADR — it consumes the existing contract, pointing at the origin
  slice 0 already deployed. *Value:* the problem-first gallery is live, correct, and
  runnable.
- **Slice 5 — Developer Documentation.** Re-home and expand the CLI/runtime/manifest/
  emission/tooling content (§8) into `bynk-lang.org/docs/`, **and update the in-repo
  references to the moved files** (`SUMMARY.md`, `README.md`, the guide/spec pages —
  §8) in the same change. *Value:* operators have a home distinct from the language
  reference, with no dangling in-repo links left behind.
- **Slice 6 — the landing page & brand.** The real front door (§1.1) and the brand
  proposal (§11) realised: hero, single live example, the two-audience nav, search,
  and the marketing-below-the-fold structure. *Value:* the front door newcomers and
  experienced devs both want.
- **Slice 7 — deep playground integration.** Beyond the open-in-playground links of
  slice 4, *embed* the REPL inline where a page benefits from live, in-place execution
  (a tutorial step the reader edits and re-runs without leaving the page), using the
  playground's wasm `bynk_compile`/`bynk_analyze` directly. *Value:* zero-install
  "edit and see it run" in the flow of reading, not just a link out.

Slice 0 is independent of everything (it deploys an already-built app); slices 1–6
are independent of every other *track* **to start**, but not to **complete**: slice
5's manifest section reaches its final shape only when the packaging track lands its
`[organisation]`/`[workspace]`/`[package]`/`[dependencies]` surface (§8) — until then
it documents today's `bynk.toml`. Slice 7 is not gated either: with the playground
shipped, it proceeds whenever its predecessors are done. Optional/deferred: generated
crate API docs (§8), versioned docs (post-1.0), a blog/release-notes stream, and i18n.

## 13. Open questions

1. **Versioning & URL stability.** Pre-1.0: single "latest" + a banner, and transient
   URLs with no legacy redirects (recommended; §5.4/§9.3). Both crystallise at 1.0:
   when does versioned docs (Docusaurus-style snapshots, or Starlight's versioning
   approach) switch on, and when do permalinks/redirects become a commitment — at
   1.0, or at the first breaking post-1.0 release? Decide before 1.0, not now.
2. **Search.** Pagefind (local, static, zero-dependency — recommended) versus a
   hosted index (Algolia DocSearch). Pagefind unless the corpus outgrows it.
3. **Generated crate API docs.** Build `cargo doc` into the site under `docs/api/`,
   or link out to docs.rs? Affects slice-5 scope (§8).
4. **Analytics & privacy.** If any analytics, a privacy-respecting, cookieless option
   (e.g. Cloudflare Web Analytics) to match the project's posture. Default to none
   until there is a question only data can answer.
5. **Feedback/comments.** A "was this helpful?" or edit-on-GitHub affordance only
   (recommended), versus a comment system (rejected — moderation cost).

### 13.1 Decided defaults (taken here to keep them out of the open-question pile)

- **Repo identity stays `github.com/accuser/bynk`; only the docs move to the new
  domain. [DECIDED]** This was previously listed as an open question, but it is not a
  symmetric choice — answering it "yes, move the repo" is the single most disruptive
  decision available in this doc: it would rewrite every crate's `repository =
  "github.com/accuser/bynk"` metadata, every ADR's provenance link, the edit-URL
  templates, and the publish-bootstrap, all at once. None of that is necessary to ship
  a site at `bynk-lang.org`. So the default is the **non-disruptive** one: the brand
  and the site live at `bynk-lang.org`; the repository, crate metadata, and edit-URLs
  continue to point at `accuser/bynk`. A repo/org rename, if ever wanted, is its own
  deliberate migration with its own track — explicitly *out of scope* here.

## 14. Risks

- **Highlighting fidelity.** The tmLanguage may diverge from tree-sitter on an edge
  construct. *Mitigation:* the grammar-agreement corpus check defined in §5.1 (token
  spans compared through a node-type → scope mapping, not a hand-wave); and if a
  construct proves un-scopable in TextMate, the **already-built** web-tree-sitter
  highlighter in `playground/` (`build-grammar.sh` → `tree-sitter-bynk.wasm`) is a
  drop-in exact fallback, not a research task.
- **Migration drift.** Porting ~100 Book pages risks silent content loss.
  *Mitigation:* the link-check gate, plus a page-count/heading diff against `docs/src/`
  at slice-2 close.
- **Two sources of truth for highlighting** (editor + site) **diverging.**
  *Mitigation:* they consume the *same* `bynk.tmLanguage.json`; the risk is only if
  someone forks it — the §5.1 corpus check covers this.
- **Docs encoder drifting from the shipped deep-link format.** The contract itself
  cannot churn — it is frozen in [ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)
  and `deeplink.ts`. The residual risk is the *docs-side encoder* diverging from it.
  *Mitigation:* a shared round-trip conformance test — a corpus of snippets encoded by
  the docs side and decoded by the playground's `deeplink.ts` (and vice-versa) must
  round-trip exactly; both sides use the same browser-native Compression Streams call,
  so there is little surface to diverge on.
- **Scope creep into a CMS/blog.** *Mitigation:* §2 non-goals; revisit only on a
  concrete need.

---

*This is a track doc per [ADR 0076](../decisions/0076-feature-track-posture.md):
merging it settles direction. Each slice is still an ordinary proposal under
`../proposals/`, citing this doc and the front-loaded ADRs; merging that proposal is
the authorisation to build.*
