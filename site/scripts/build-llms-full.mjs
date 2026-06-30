// @ts-check
//! Generate (or verify) llms-full.txt — the whole Bynk Book inlined into one
//! file for LLM context (documentation track). It concatenates the committed
//! Book pages in sidebar order, expanding the mdBook authoring directives with
//! the same committed data the site's remark plugin uses (`generated/grammar.json`,
//! `generated/grammar-semantics.json`, the `diagnostics/` fixtures) — so it can't
//! drift from what the site renders, and code fences stay pristine (HTML
//! extraction would mangle the Expressive Code markup).
//!
//!   node scripts/build-llms-full.mjs            regenerate site/public/llms-full.txt
//!   node scripts/build-llms-full.mjs --check     fail (non-zero) if out of date
//!
//! The `site` CI job runs `--check`. Served at bynk-lang.org/llms-full.txt.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BOOK = path.join(HERE, "../src/content/docs/book");
const GENERATED = path.join(HERE, "../src/generated");
const INCLUDE_BASE = path.join(HERE, "../src"); // `diagnostics/…` resolves against this
const OUT = path.join(HERE, "../public/llms-full.txt");
const SIDEBAR = path.join(GENERATED, "sidebar.json");
const CARGO_TOML = path.join(HERE, "../../Cargo.toml");

const GRAMMAR = JSON.parse(fs.readFileSync(path.join(GENERATED, "grammar.json"), "utf8"));
const SEMANTICS = JSON.parse(fs.readFileSync(path.join(GENERATED, "grammar-semantics.json"), "utf8"));

const RULE = /^\{\{#(grammar|grammar-semantics)\s+(\S+)\}\}$/;
const INCLUDE = /\{\{#include\s+(\S+)\}\}/g;

/** Flatten the sidebar into Book routes in reading order (deduped). */
function routesInOrder() {
  const sidebar = JSON.parse(fs.readFileSync(SIDEBAR, "utf8"));
  const routes = [];
  const seen = new Set();
  const walk = (entries) => {
    for (const e of entries) {
      if (e.link && !seen.has(e.link)) {
        seen.add(e.link);
        routes.push(e.link);
      }
      if (e.items) walk(e.items);
    }
  };
  walk(sidebar);
  return routes;
}

/** A `/book/<slug>/` route → the committed source page, or null if missing. */
function pageForRoute(route) {
  const slug = route.replace(/^\/book\//, "").replace(/\/$/, "");
  const candidates = slug === "" ? ["index.md"] : [`${slug}.md`, `${slug}/index.md`];
  for (const rel of candidates) {
    const file = path.join(BOOK, rel);
    if (fs.existsSync(file)) return { file, rel };
  }
  return null;
}

/** Expand the authoring directives in one page's body (frontmatter stripped),
 *  matching the remark plugin's scoping so literal examples are left alone:
 *  `{{#grammar}}`/`{{#grammar-semantics}}` expand only as a standalone line
 *  outside a code fence (the plugin runs them on paragraphs); `{{#include}}`
 *  expands only inside a fence (where the real fixtures live, never the inline
 *  `` `{{#include …}}` `` examples). */
function expand(markdown, where) {
  const body = markdown.replace(/^---\n[\s\S]*?\n---\n/, "");
  const out = [];
  let inFence = false;
  for (const line of body.split("\n")) {
    if (/^\s*```/.test(line)) {
      inFence = !inFence;
      out.push(line);
    } else if (inFence) {
      out.push(line.replace(INCLUDE, (_m, rel) => readInclude(rel, where)));
    } else {
      const m = RULE.exec(line.trim());
      out.push(m ? (m[1] === "grammar" ? grammarBlock(m[2], where) : semanticsText(m[2])) : line);
    }
  }
  return out.join("\n");
}

function grammarBlock(name, where) {
  const production = GRAMMAR.productions[name];
  if (production == null) throw new Error(`${where}: {{#grammar ${name}}} — unknown rule`);
  return "```\n" + production + "\n```";
}

function semanticsText(name) {
  const diagnostics = Array.isArray(SEMANTICS[name]) ? SEMANTICS[name] : [];
  if (diagnostics.length === 0) {
    return "_No diagnostics constrain this construct directly._";
  }
  const bullets = diagnostics.map((d) => `- \`${d.code}\` — ${d.summary}`).join("\n");
  return `${bullets}\n\nSee the [diagnostic index](/book/reference/diagnostics/) for all codes.`;
}

function readInclude(rel, where) {
  const anchor = rel.indexOf("diagnostics/");
  if (anchor === -1) throw new Error(`${where}: {{#include ${rel}}} — only diagnostics/* supported`);
  const abs = path.join(INCLUDE_BASE, rel.slice(anchor));
  return fs.readFileSync(abs, "utf8").replace(/\n$/, "");
}

function majorMinor() {
  const toml = fs.readFileSync(CARGO_TOML, "utf8");
  const m = /\nversion\s*=\s*"(\d+)\.(\d+)\.\d+"/.exec(toml);
  if (!m) throw new Error("could not read workspace version from Cargo.toml");
  return `${m[1]}.${m[2]}`;
}

const RULE_LINE = "# " + "─".repeat(60);

function build() {
  const mm = majorMinor();
  const header = `# Bynk — Full Documentation

> Bynk is a statically typed, architecture-first programming language for building
> services. Its contexts, services, agents, and the types that flow between them are
> part of the language, not a convention layered on top. Bynk compiles to typed
> TypeScript and targets Cloudflare Workers. It is pre-1.0 (currently v${mm}).

This file inlines the complete Bynk Book — every tutorial, guide, reference page,
and the normative specification — concatenated in reading order, with the mdBook
authoring directives expanded. It is the expanded companion to \`llms.txt\` (the
curated index). Each page below is preceded by a \`# ──\` marker giving its route
under \`/book/\`. The Book documents what compiles today; features marked "planned"
are deferred.

This file is generated by site/scripts/build-llms-full.mjs — do not edit it by
hand; edit the Book under site/src/content/docs/book/ and regenerate. For the
curated index with a shorter primer, see \`llms.txt\`.
`;

  const sections = [];
  for (const route of routesInOrder()) {
    const page = pageForRoute(route);
    if (!page) continue;
    const raw = fs.readFileSync(page.file, "utf8");
    const expanded = expand(raw, page.rel).replace(/^\n+/, "").replace(/\n+$/, "");
    sections.push(`${RULE_LINE}\n# Source: book/${page.rel}\n${RULE_LINE}\n\n${expanded}\n`);
  }
  return `${header}\n${sections.join("\n")}`;
}

function main(argv) {
  const rendered = build();
  if (argv.includes("--check")) {
    const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
    if (current !== rendered) {
      console.error(
        "site/public/llms-full.txt is out of date.\n" +
          "Regenerate with: node site/scripts/build-llms-full.mjs",
      );
      process.exit(1);
    }
    console.log("llms-full.txt is up to date.");
    return;
  }
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, rendered);
  console.log(`wrote llms-full.txt (${rendered.split("\n").length} lines).`);
}

main(process.argv.slice(2));
