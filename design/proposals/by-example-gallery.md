# by-example-gallery — Documentation track, slice 4: Bynk by Example (4a)

- **Scope:** stands up the **Bynk by Example** surface (track §7) — a problem-first, Go-by-Example
  gallery whose code is **mechanically extracted from the real `examples/` projects and the
  `src/snippets/` corpus** (never retyped, so it cannot drift), with runnable snippets opening in the
  playground via the **already-shipped** deep-link format ([ADR 0140](../decisions/0140-repl-execution-and-sandbox.md)).
  No grammar/compiler/emitter behaviour change → **unversioned**; implements ADR 0141 — **no new ADR**.
- **This PR (4a)** ships the full machinery + the runnable snippets tier + the gallery index + two
  project pages as proof. **4b** adds the remaining eight project pages (same components, more curated
  prose).

## Decisions

- **[A] The surface.** `/by-example/` is a Starlight docs sub-tree (`site/src/content/docs/by-example/`):
  a gallery `index.mdx`, `snippets/<slug>.mdx`, and `projects/<name>.mdx`. A "By Example" group is
  appended to the Starlight `sidebar` (its own group; the Book stays generated from `SUMMARY.md`), and
  the surface is added to the Header nav. Page order is pinned with `sidebar.order` so the teaching arc
  (refined types → sum types → `is` → `Result`/`Option` → opaque → validate) reads top-to-bottom.
- **[B] Mechanical extraction.** `site/src/components/Example.astro` reads the real source from
  `examples/…` or `src/snippets/…` at build time and renders it with Expressive Code's `<Code>` (faithful
  `bynk` highlighting). The code is never copied into prose, so the gallery can't drift from the projects.
- **[C] Build-time playground links.** `site/src/lib/examples.mjs` ports `encodeSnippet` verbatim from
  `playground/src/deeplink.ts` (`base64url(deflate-raw(utf8(source)))`). Those APIs exist in Node, so the
  link is **precomputed at build** into a plain `<a>` — no client island. Round-tripping a built link
  through `DecompressionStream` reproduces the snippet byte-for-byte.
- **[D] Honest runnable marking.** Bite-size **snippets** are self-contained `commons` units, runnable in
  the browser playground → they carry "Open in playground". **Projects** reach Workers-only shapes (KV,
  Durable-Object agents, cron), so they carry "Open the full project" (GitHub) + a one-line note that they
  run with `bynk dev`. This is the §7 boundary, stated plainly rather than via a button that can't work.
- **[E] Correct by construction (§6).** `bynkc/tests/examples.rs` now compiles **all ten** examples on
  Workers (was just `hello-world`); a new `bynkc/tests/snippets.rs` compiles every `site/src/snippets/*.bynk`
  on Bundle. So neither an extracted project nor a "run me" snippet can fall out of step with the compiler.

## End state (4a)

`/by-example/` is live with the six runnable snippets, the gallery index, and two project pages
(hello-world, link-shortener); the nav links it; the extraction + playground-link + gating machinery is in
place for 4b to add the rest by writing prose only.

## Risks & mitigations

- **Extracted code stops compiling.** *Mitigation:* [E] — the examples + snippets gates compile every
  source the gallery shows.
- **A playground link is malformed.** *Mitigation:* [C] is a verbatim port of the shipped encoder, verified
  by a build-output round-trip through the decoder.
- **Known limitation (not blocking):** By Example pages currently render the Book's sidebar with the "By
  Example" group appended below it (Starlight shows one global sidebar). A per-section sidebar is deferred
  polish.

## Verification

- **Site:** `npm run build` green (139 pages, strict internal-link validation, mermaid, Pagefind); the
  extracted source renders with `bynk` highlighting; a built playground link decodes back to the exact
  snippet; sidebar/llms drift checks clean. Index, a snippet page, and a project page eyeballed light + dark.
- **Rust:** `cargo test --workspace` (examples.rs all ten + new snippets.rs) + `cargo fmt --check` +
  `cargo clippy --workspace --all-targets -- -D warnings` green.

## Out of scope

The remaining eight project pages (4b); inline playground embeds (slice 7); a per-section sidebar; putting
By Example into llms-full; the developer-docs surface (slice 5).
