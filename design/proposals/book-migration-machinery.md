# book-migration-machinery — Documentation track, Slice 2a: the Book-migration machinery

- **Scope:** an **infrastructure increment** — it adds one library function and two
  drift-guard tests to existing Rust crates (no grammar, compiler, or emitter change)
  plus Node build tooling, so it is **unversioned** (`<slug>.md`) and ships no release
  tag, mirroring slices 0–1. **Slice 2a of the documentation track**
  (`../tracks/documentation.md`); implements the framework ADR
  [0141](../decisions/0141-documentation-framework.md) — **no new ADR** (0141 D4
  already settled "the Rust generators stay canonical; only the embedding host moves").
- **Realises:** the track's §5 Book migration, split so this slice **builds and proves
  every transform** the 129-page mdBook Book needs, on a few of the hardest real pages.
  Slice 2b is then a mechanical bulk run over the whole Book.

## Context

mdBook stands authoritative and untouched in `docs/`; the Astro/Starlight site (slice 1)
serves a placeholder at `bynk-lang.org`. Migrating the Book means reproducing four
mdBook authoring affordances under Starlight without re-implementing any generator logic
in JavaScript (the no-drift rule, ADR 0141 D4): the `{{#grammar}}` / `{{#grammar-semantics}}`
directives, `{{#include}}`, `[!KIND]` callouts, ` ```mermaid ` diagrams, custom `{#id}`
heading anchors, and relative `.md` links. This slice stands the machinery up alongside
mdBook — a *parallel* proof, not a cutover. The bulk port, the full `SUMMARY.md` → sidebar,
the grammar appendix page, and mdBook's retirement are later slices.

## Decisions

- **[A] Grammar artifacts come from the Rust renderer, committed + drift-guarded.**
  A new `bynk_grammar::render_site_json` emits the same per-rule productions the mdBook
  `{{#grammar}}` preprocessor embeds (over `embeddable_rules` / `render_production`) plus
  the appendix, into a committed `site/src/generated/grammar.json`. The diagnostics→rule
  map is re-emitted (from the same `render_grammar_semantics_json` the compiler's registry
  already feeds) into `site/src/generated/grammar-semantics.json`, so the site never reads
  from `docs/` (which retires later). Both are guarded by `BYNK_BLESS` tests
  (`bynk-grammar/tests/generated_grammar_json.rs`, `bynkc/tests/generated_site_json.rs`),
  mirroring the repo's existing generate→commit→drift-guard pattern — a grammar or
  diagnostics change that isn't re-blessed fails CI. The Astro build stays **node-only**
  (no cargo in the site build).
- **[B] One remark plugin expands the directives at build time** —
  `site/src/plugins/remark-bynk-directives.mjs`: `{{#grammar}}` → a code block from the
  committed JSON (unknown rule → build error); `{{#grammar-semantics}}` → the diagnostic
  bullet list (or the "no diagnostics" line); `{{#include}}` → the diagnostics fixture
  inlined inside its fence; `[!NOTE|TIP|WARNING|DANGER]` → a Starlight aside
  (`note`/`tip`/`caution`/`danger`); `## Heading {#id}` → a pinned heading id (the Book's
  intra-page anchors); and mdBook's comma fence annotations (` ```bynk,fail `) reduced to
  the bare language so the block still highlights.
- **[C] Mermaid renders to inline SVG at build time** via `rehype-mermaid` (`inline-svg`),
  matching the track's offline-SVG intent (no client JS). The remark plugin rewrites a
  ` ```mermaid ` fence to a real `<pre class="mermaid">` element *before* Expressive Code
  claims the fence, so the diagram reaches rehype-mermaid intact. rehype-mermaid drives a
  headless Chromium, so the `site` CI job and the deploy workflow gain a
  `playwright install --with-deps chromium` step.
- **[D] A committed codemod does the structural rewrite** — `site/scripts/migrate-page.mjs`:
  lift the leading `# H1` into `title:` frontmatter (Starlight renders the title), rewrite
  relative `.md` links to `/book/<slug>/` routes, and neutralise escaped `\{{#…}}`.
  Directives, callouts and mermaid are left for the remark plugin. Slice 2a runs it on the
  proof pages; slice 2b runs the same script over all 129.
- **[E] EBNF is rendered as a plain code frame.** Shiki ships no EBNF grammar, so the
  grammar productions render as an unhighlighted (monospace) block rather than carry a
  second hand-maintained grammar; can be revisited if EBNF highlighting is wanted.
- **[F] Link validation is relaxed for the partly-migrated Book.** `starlight-links-validator`
  excludes the `/book/**` space while only a few pages exist there; every other link is
  still strictly checked. Slice 2b drops the exclude to restore full strict validation
  once the whole Book is present.

## Proof pages

Four real pages, migrated by the codemod and committed under `site/src/content/docs/book/`
(mdBook's originals untouched), chosen to exercise **every** transform:
`reference/grammar.md` (`{{#grammar}}` ×331 + `{{#grammar-semantics}}` ×79 + `{#id}`
anchors at scale), `introduction.md` (callouts + the frontmatter/H1 baseline),
`about/why-bynk-exists.md` (`{{#include}}` + a `bynk,fail` fence), and
`guides/program-structure/how-a-program-is-shaped.md` (mermaid + callouts + cross-links).
Added to the Starlight sidebar; the full `SUMMARY.md` → sidebar is 2b.

## Risks & mitigations

- **A headless browser in CI/deploy (mermaid).** *Mitigation:* a single pinned
  `playwright install chromium` step; build-time SVG keeps it off the client and the
  rendered pages need no runtime JS.
- **The remark plugin drifting from the mdBook preprocessors' output.** *Mitigation:* the
  grammar/semantics content comes from the same Rust renderers (drift-guarded), so only
  the *embedding* is JS; fidelity was spot-checked against mdBook's rendered output.
- **Pre-existing broken intra-page anchors in the source.** The stricter validator surfaced
  several (e.g. `#test_decl` where the heading id is `rule-test_decl`); one was fixed at
  source on the proof page. *Mitigation:* 2b's bulk run reports the rest across the corpus —
  a validation win, not a regression.

## Verification

- **Local:** clean `npm ci` + `npm run build` green — grammar directives expand, semantics
  render as bullet lists with the diagnostic-index link, the `{{#include}}` fixtures inline,
  callouts become Starlight asides, the mermaid diagram renders to inline SVG, custom
  heading ids resolve, and internal links validate. `BYNK_BLESS=1 cargo test` then a clean
  `cargo test` proves both generated JSON files are committed-current.
- **CI:** the `site` job (now with the Chromium step) builds the proof pages; the Rust test
  job runs the two new drift guards. mdBook's `docs` job and `doc_examples` are untouched.
