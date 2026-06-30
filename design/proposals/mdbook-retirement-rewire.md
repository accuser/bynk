# mdbook-retirement-rewire — Documentation track, mdBook retirement 3a: cut the cord

- **Scope:** an **infrastructure increment** — it repoints the doc generators, gates, and
  scripts from `docs/` (mdBook) to `site/` (Astro/Starlight) and stops building/gating mdBook,
  without yet deleting anything that still compiles. No grammar/compiler/emitter behaviour
  change, so it is **unversioned** and ships no release tag. **mdBook retirement, step 3a of
  the documentation track**; implements ADR [0141](../decisions/0141-documentation-framework.md)
  — **no new ADR**.
- **Realises:** the track's retirement of mdBook. Slices 2a/2b moved the whole Book to
  `site/`; this severs the remaining couplings to `docs/` so the site is self-sufficient. The
  `docs/` tree and the three `mdbook-bynk-*` preprocessor crates stay physically in the repo
  (now dead) as a one-cycle fallback; **step 3b** deletes them.

## Context

Several Rust correctness gates and Rust-generated reference pages still read or wrote `docs/`.
They are repointed to the Book under `site/`, so retiring mdBook removes nothing the project
relies on. mdBook stays authoritative only until this lands.

## Decisions

- **[A] Diagnostics fixtures relocated** `docs/diagnostics/` → `site/src/diagnostics/`.
  `bynkc/tests/doc_diagnostics.rs` and the remark plugin's `includeBase` (in
  `site/astro.config.mjs`) repoint there; committed `{{#include}}` paths are unchanged (the
  plugin resolves the `diagnostics/…` suffix).
- **[B] The four Rust-generated reference pages now write into the Book.**
  `diagnostics.md`, `grammar-appendix.md`, `cli.md`, `keywords.md` are emitted into
  `site/src/content/docs/book/reference/` by their drift-guard tests, wrapped to Starlight
  form (frontmatter title + `/book/` links) by a shared test helper (`bynkc/tests/common/mod.rs`)
  that ports the migration codemod's transform — so the regenerated pages are byte-identical
  to the migrated originals. The mdBook-only `docs/grammar-semantics.json` output is dropped
  (the site uses its own committed copy from `generated_site_json.rs`).
- **[C] The doc gates read the Book.** `doc_examples.rs` (compile every ```bynk block),
  `grammar_coverage.rs` (the `{{#grammar}}` bijection + `{#rule-}` anchors), and
  `doc_version.rs` (version banners) repoint from `docs/src` to
  `site/src/content/docs/book`; `scripts/bump-version.sh` edits the banners there.
  `introduction.md`'s banner is now `book/index.md`.
- **[D] llms-full.txt is generated from the Book markdown with directives expanded.**
  `site/scripts/build-llms-full.mjs` concatenates the committed Book pages in sidebar order,
  expanding `{{#grammar}}`/`{{#grammar-semantics}}`/`{{#include}}` from the same committed JSON
  + fixtures the site uses (so it cannot drift, and code fences stay pristine — HTML extraction
  would mangle the Expressive Code markup). Output `site/public/llms-full.txt` (served), with a
  `--check` drift mode. The curated `llms.txt` moves to `site/public/llms.txt` (its links
  rewritten to `/book/` routes) and is validated by `site/scripts/check-llms-links.mjs`. The
  old `scripts/build-llms-full.sh` + `scripts/check-llms-links.sh` are removed.
- **[E] The sidebar source moved** `docs/src/SUMMARY.md` → `site/src/SUMMARY.md`;
  `generate-sidebar.mjs` repoints. The page-migration codemod `migrate-page.mjs` is retired
  (its only input was `docs/src`).
- **[F] mdBook is no longer built or gated.** The `ci.yml` "Build the book (html + linkcheck)"
  job and the `docs` path filter are removed; the llms + sidebar drift checks run in the `site`
  job; the `rust` filter gains the Book paths so a content edit still runs the doc gates;
  `docs` is dropped from `ci-green`. `.github/workflows/pages.yml` (GitHub Pages) is deleted —
  `bynk-lang.org` (Cloudflare) is the sole home; `accuser.github.io/bynk/` will 404.
- **[G] The contributing "docs" guide is rewritten** for the Astro/Starlight workflow.

## End state

Every doc gate and generator runs against `site/`; nothing reads `docs/`. `docs/`, `book.toml`,
`docs/theme/`, and the three `mdbook-bynk-*` crates remain in the tree (dead, still compiling)
until step 3b removes them.

## Risks & mitigations

- **A gate silently stops covering content.** *Mitigation:* the `rust` CI filter now includes
  `site/src/content/docs/book/**` + `site/src/diagnostics/**`, so a Book edit runs
  `doc_examples`/`doc_diagnostics`/`doc_version`/`grammar_coverage` and the generated-page guards.
- **Generated pages drifting during the repoint.** *Mitigation:* the shared helper reproduces
  the codemod exactly — re-blessing produced **zero** diff against the committed pages.

## Verification

- **Rust:** `BYNK_BLESS=1 cargo test` then a clean `cargo test --workspace` + `cargo fmt --check`
  + `cargo clippy --workspace --all-targets -- -D warnings` green; the four generated pages
  re-bless byte-identically; the repointed gates pass against the Book.
- **Site:** clean `npm ci` + `npm run build` green (strict link validation, mermaid);
  `generate-sidebar.mjs --check`, `build-llms-full.mjs --check`, `check-llms-links.mjs` clean.
- **Decoupling proof:** no live read of `docs/` remains in `site/`, `bynkc/tests`, or `scripts/`;
  mdBook is referenced by no workflow. `actionlint` clean.
