# landing-and-brand — Documentation track, slice 6: the landing page & brand

- **Scope:** the real front door. Replaces the slice-1 placeholder landing with a navigation-first
  landing page (track §1.1), applies a first brand pass (§11), and lifts the search-engine blocks
  now that the content is real. No grammar/compiler/emitter behaviour change → **unversioned**, no
  release tag; implements ADR [0141](../decisions/0141-documentation-framework.md) — **no new ADR**.
- **Realises:** the track's slice 6. Slice 1 scaffolded the site (placeholder landing + `noindex` +
  disallow-all `robots.txt`); slices 2a/2b migrated the whole Book; mdBook was retired (3a/3b). The
  domain now serves real content but still presents a placeholder and is hidden from search.

## Decisions

- **[A] Lift the indexing blocks.** Remove the `noindex, nofollow` head meta from
  `site/astro.config.mjs`; replace `site/public/robots.txt` (disallow-all) with allow-all + a
  `Sitemap:` line. The Pagefind index and the sitemap already build — they were simply suppressed.
- **[B] A first brand pass (§11).** One electric-indigo accent for links and the primary CTA, set via
  Starlight's `--sl-color-accent` ramp in `site/src/styles/brand.css` (light + dark); the rust/ayu
  code themes stay reserved for code, so the brand never competes with a highlighted block. A
  monoline geometric **bynk** wordmark (`src/assets/wordmark.svg` + a dark-theme variant, wired as
  the Starlight `logo` with `replacesTitle`) and an interlocking-blocks **favicon**
  (`public/favicon.svg`). A starting point, explicitly iterable.
- **[C] The real landing (§1.1).** `site/src/content/docs/index.mdx` — hero (tagline + primary
  "Read the Book" / secondary "Open the playground" CTAs), one real Bynk example (a tiny HTTP
  service), a four-card "why Bynk" grid, and a plain pre-1.0 honesty line. Copy in the repo voice
  (precise, dry, honest about deferral).
- **[D] A persistent cross-surface nav (§1.1).** A `components.Header` override
  (`src/components/Header.astro`) reproduces Starlight's header layout verbatim and adds an
  always-visible nav to the surfaces that **exist today** — Book · Reference · Playground — with
  GitHub as the existing social icon. "By Example" (slice 4) and a developer-docs surface (slice 5)
  are unbuilt, so they are deliberately not linked.
- **[E] Keep the front-door example honest.** The landing's ```bynk block lives outside the Book, so
  the example-compilation gate (`bynkc/tests/doc_examples.rs`) is extended to also compile
  `index.mdx` — the most visible snippet cannot drift from the compiler (track §6).

## End state

`bynk-lang.org` presents a real, indexable landing page with a coherent first brand, a persistent
two-audience nav, and a verified example; the Book is reachable from every page; search and the
sitemap are live.

## Risks & mitigations

- **The landing snippet silently rots.** *Mitigation:* [E] — `doc_examples` now compiles it
  (verified: the gate's compiled-count rose by one).
- **The Header override drifts from upstream Starlight.** *Mitigation:* it copies the upstream layout
  + grid CSS verbatim and only inserts one nav group, so a Starlight bump is a visible, contained
  re-sync rather than a silent break; the build's strict link-checker covers the new links.

## Verification

- **Site:** clean `npm run build` green (strict link validation, mermaid, Pagefind); `noindex` gone
  from `dist/` real pages; `robots.txt` allows + names the sitemap; `sitemap-index.xml` still
  emitted. Landing eyeballed in light **and** dark (wordmark, indigo accent, the example with `bynk`
  highlighting, the persistent nav, the cards). `generate-sidebar`/`build-llms-full`/`check-llms-links`
  drift checks clean.
- **Rust:** `cargo test --workspace` (doc_examples now compiles the landing snippet too) +
  `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` green.

## Out of scope (later slices)

By Example gallery (slice 4), developer-docs surface (slice 5), deep playground embeds (slice 7);
custom web typefaces, a full logo system, versioned docs/redirects (post-1.0), analytics.
