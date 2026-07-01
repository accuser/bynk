# developer-docs — Documentation track, slice 5: Developer Documentation (5a)

- **Scope:** stands up the **Developer Documentation** surface (track §8) — a home for the person
  *operating the toolchain* (the `bynkc`/`bynk` CLIs, the `bynk.toml` manifest, emission, and the
  editor/formatter/LSP tooling), kept distinct from the *language* reference and spec. It re-homes the
  fifteen operator pages scattered through the Book's `reference/`, `guides/editor-and-tooling/`, and
  `tooling/` folders into a new `/docs/` surface, wires its sidebar + nav, and repoints every in-repo
  reference to the moved pages (the §8 obligation). No grammar/compiler/emitter behaviour change →
  **unversioned**; implements ADR 0141 — **no new ADR**.
- **This PR (5a)** does the move + wiring + link repointing + the surface overview page. **5b** expands
  the re-homed pages (fuller flag/exit-code/invocation coverage; an integrator-framed emission
  walkthrough; generated crate API docs under `/docs/api/`).

## Decisions

- **[A] The surface.** `/docs/` is a Starlight docs sub-tree (`site/src/content/docs/docs/` — the inner
  `docs/` is the URL segment) holding an overview `index.mdx` plus the fifteen moved pages: `cli`,
  `bynk-cli`, `manifest`, `emission`, the `editor-and-tooling/` how-tos (×5), and the `tooling/`
  references (×5). A curated "Developer Docs" group is appended to the Starlight `sidebar` (its own
  group; the Book stays generated from `SUMMARY.md`), and the surface gets a Header nav entry after
  Reference.
- **[B] What moves, what stays.** The fifteen pages are operator/toolchain content. The two **normative**
  pages — `spec/emission.md` (§7 Meaning by translation) and `spec/runtime-library.md` (§7.4) — **stay in
  the spec**: they carry section numbering and dense inbound spec cross-references, so the dev-docs
  emission page *links* to them rather than absorbing them. ADR provenance links pointing at
  `spec/emission.md` are therefore untouched.
- **[C] History-preserving move.** Pages move with `git mv`. URLs change by a pure prefix swap —
  `/book/reference/{cli,bynk-cli,manifest,emission}/` → `/docs/{…}/`,
  `/book/guides/editor-and-tooling/` → `/docs/editor-and-tooling/`, `/book/tooling/` → `/docs/tooling/`
  — so anchors (`#bynk-dev`, …) are preserved.
- **[D] No dangling links.** Every inbound reference is repointed in the same change: ~60 internal links
  across the Book, the seven repo READMEs (root + `bynk`/`bynkc`/`bynk-fmt`/`bynk-lsp`/`vscode-bynk`),
  the moved entries removed from `SUMMARY.md` (regenerating `sidebar.json`), and the curated `llms.txt`
  index regrouped under a "Developer documentation" block. `llms-full.txt` is regenerated; the moved
  pages fall out of it automatically (it walks the Book-only sidebar), so Developer Docs stays out of
  `llms-full`, mirroring By Example.
- **[E] The drift guard follows the surface.** `check-llms-links.mjs` validated only `/book/` links;
  it is extended to resolve `/docs/` links against the new tree, so the dev-doc links in `llms.txt` are
  guarded rather than silently skipped. The build's `starlight-links-validator` remains the net for any
  internal link missed by the prefix swap.

## End state (5a)

`/docs/` is live: the overview front door, the fifteen re-homed pages, a curated Developer Docs sidebar
group, and a nav entry. The Book's reference/guides no longer carry toolchain pages (they point across
to `/docs/` instead); no in-repo link dangles; all drift checks are clean.

## Risks & mitigations

- **A moved-page link left pointing at `/book/…`.** *Mitigation:* [E] — the build link-checker fails on
  any internal dangling link; the repo-wide grep for the six old prefixes returns zero.
- **A normative spec cross-reference broken by the move.** *Mitigation:* [B] — the spec pages do not
  move, so spec-internal numbering and references are untouched.
- **Known limitation (not blocking):** like By Example, Developer Docs renders the Book's global sidebar
  with its own group appended (Starlight shows one global sidebar). A per-section sidebar is deferred
  polish.

## Verification

- **Site:** `npm run build` green (strict internal-link validation, mermaid, Pagefind); the moved pages
  render under `/docs/…` with `bynk` highlighting; the Developer Docs nav entry + sidebar group appear.
  `generate-sidebar --check`, `build-llms-full --check`, `check-llms-links` (now covering `/docs/`),
  and the British-English/glossary checks all clean. Overview + a moved page eyeballed light + dark.
- **No Rust gate** — content-only, zero `.rs` changes (READMEs don't affect cargo), as with slice 4b.

## Out of scope

Per-page expansion + generated crate API docs (5b); inline playground embeds (slice 7); a per-section
sidebar; putting Developer Docs into `llms-full`; versioned docs / redirects (post-1.0). The manifest
page documents today's `bynk.toml` until the packaging track lands its
`[organisation]`/`[workspace]`/`[dependencies]` surface.
