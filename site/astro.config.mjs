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
// The Book sidebar, generated from the mdBook SUMMARY.md by
// `scripts/generate-sidebar.mjs` (CI runs `--check` to guard drift).
import bookSidebar from "./src/generated/sidebar.json" with { type: "json" };

// `{{#include}}` diagnostics fixtures live in `site/src/diagnostics/` (the
// remark plugin resolves the `diagnostics/…` suffix against this base).
const includeBase = fileURLToPath(new URL("./src", import.meta.url));

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
      // The geometric wordmark replaces the text title (track §11). Starlight
      // renders the logo as an <img>, which can't inherit the theme ink, so we
      // ship light/dark variants (identical geometry, different stroke).
      logo: {
        light: "./src/assets/wordmark.svg",
        dark: "./src/assets/wordmark-dark.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      // The brand: one electric-indigo accent (rust/ayu stay reserved for code).
      customCss: ["./src/styles/brand.css"],
      // Persistent cross-surface nav in the header (track §1.1).
      components: { Header: "./src/components/Header.astro" },
      // Build-time internal link checking (the link-check gate): a broken
      // in-site link fails `astro build`. The whole Book is migrated, so it is
      // validated strictly along with everything else.
      plugins: [starlightLinksValidator()],
      // Faithful `bynk` highlighting from the editor's own grammar; the fenced
      // language id is `bynk` (the grammar's own name is the display "Bynk").
      expressiveCode: {
        shiki: { langs: [{ ...bynkGrammar, name: "bynk" }] },
      },
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/accuser/bynk" }],
      // The Book sidebar, generated from src/SUMMARY.md.
      sidebar: bookSidebar,
    }),
  ],
});
