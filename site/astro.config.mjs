// @ts-check
import { fileURLToPath } from "node:url";
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightLinksValidator from "starlight-links-validator";
// The single source of truth for Bynk highlighting is the VS Code TextMate
// grammar — the same file the editor uses (scopeName: source.bynk). Shiki
// consumes it directly, so the site never maintains a second highlighter.
import bynkGrammar from "../vscode-bynk/syntaxes/bynk.tmLanguage.json" with { type: "json" };
// Expand the mdBook authoring directives ({{#grammar}}, {{#grammar-semantics}},
// {{#include}}, [!KIND] callouts) so the Book's source renders under Starlight.
import remarkBynkDirectives from "./src/plugins/remark-bynk-directives.mjs";
// Render the Book's ```mermaid diagrams to inline SVG at build time (offline, no
// client JS). Needs a headless Chromium (CI runs `playwright install chromium`).
import rehypeMermaid from "rehype-mermaid";

// `{{#include}}` diagnostics fixtures live in `docs/diagnostics/` (alongside the
// mdBook). Repointed when those fixtures relocate in the retirement slice.
const includeBase = fileURLToPath(new URL("../docs", import.meta.url));

// https://astro.build/config
export default defineConfig({
  site: "https://bynk-lang.org",
  markdown: {
    remarkPlugins: [[remarkBynkDirectives, { includeBase }]],
    rehypePlugins: [[rehypeMermaid, { strategy: "inline-svg" }]],
  },
  integrations: [
    starlight({
      title: "Bynk",
      // Slice 1 ships a placeholder scaffold, not real content — keep it out of
      // search indexes until the landing + Book are real (removed in slice 6).
      head: [{ tag: "meta", attrs: { name: "robots", content: "noindex, nofollow" } }],
      // Build-time internal link checking (the link-check gate). During the
      // Book migration only a few `/book/**` pages exist, so links into that
      // (partly migrated) tree are not yet validated; everything else still is.
      // Slice 2b drops this exclude to restore strict validation of the Book.
      plugins: [starlightLinksValidator({ exclude: ["/book/**"] })],
      // Faithful `bynk` highlighting from the editor's own grammar; the fenced
      // language id is `bynk` (the grammar's own name is the display "Bynk").
      expressiveCode: {
        shiki: { langs: [{ ...bynkGrammar, name: "bynk" }] },
      },
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/accuser/bynk" }],
      // Slice 2a proves the migration on a handful of pages; the full
      // SUMMARY.md → sidebar generation lands in slice 2b.
      sidebar: [
        {
          label: "The Book",
          items: [
            { label: "Overview", link: "/book/" },
            { label: "Introduction", link: "/book/introduction/" },
            { label: "Why Bynk exists", link: "/book/about/why-bynk-exists/" },
            {
              label: "How a program is shaped",
              link: "/book/guides/program-structure/how-a-program-is-shaped/",
            },
            { label: "Syntax & grammar", link: "/book/reference/grammar/" },
          ],
        },
      ],
    }),
  ],
});
