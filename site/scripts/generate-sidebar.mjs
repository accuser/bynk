// @ts-check
//! Generate (or verify) the Starlight sidebar from `site/src/SUMMARY.md`
//! (documentation track). `SUMMARY.md` is the Book's authored table of
//! contents; this turns it into the committed `site/src/generated/sidebar.json`
//! that `astro.config.mjs` imports, so the sidebar tracks the Book's own order
//! and grouping rather than directory structure.
//!
//!   node scripts/generate-sidebar.mjs            regenerate sidebar.json
//!   node scripts/generate-sidebar.mjs --check     fail (non-zero) if out of date
//!
//! The `site` CI job runs `--check`; a `SUMMARY.md` edit that isn't regenerated
//! fails the build. Mirrors the repo's generate→commit→drift-guard pattern.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SUMMARY = path.join(HERE, "../src/SUMMARY.md");
const OUT = path.join(HERE, "../src/generated/sidebar.json");

/** Sidebar labels are plain text, so drop the inline-code backticks the Book's
 *  SUMMARY uses (e.g. "`bynk.toml` manifest" → "bynk.toml manifest"). */
function clean(label) {
  return label.replace(/`/g, "");
}

/** A Book page path (relative to the Book root) → its Starlight route under /book/. */
function slugFor(pagePath) {
  let s = pagePath.replace(/\.md$/, "").replace(/(?:^|\/)index$/, "");
  return s === "" ? "/book/" : `/book/${s}/`;
}

/**
 * Parse SUMMARY.md into { prefix, parts }.
 * - `# Part` headers (after the `# Summary` title) open a part.
 * - a bare `[Title](path.md)` before any part is the prefix chapter (Book root).
 * - `- [Title](path.md)` items nest by indentation (2 spaces per level).
 * - `---` separators are ignored.
 */
function parseSummary(text) {
  const lines = text.split("\n");
  let prefix = null;
  const parts = [];
  let current = null;

  for (const line of lines) {
    if (/^#\s+Summary\s*$/.test(line)) continue;
    const part = /^#\s+(.+?)\s*$/.exec(line);
    if (part) {
      current = { label: part[1], entries: [] };
      parts.push(current);
      continue;
    }
    const item = /^(\s*)-\s+\[(.+?)\]\((.+?)\)\s*$/.exec(line);
    if (item) {
      current?.entries.push({ depth: item[1].length / 2, label: clean(item[2]), path: item[3] });
      continue;
    }
    const bare = /^\[(.+?)\]\((.+?)\)\s*$/.exec(line);
    if (bare && !current) prefix = { label: clean(bare[1]), path: bare[2] };
  }
  return { prefix, parts };
}

/** Build a nesting tree from a part's flat, indentation-tagged entries. */
function buildTree(entries) {
  const root = { children: [] };
  const stack = [{ depth: -1, node: root }];
  for (const entry of entries) {
    while (stack[stack.length - 1].depth >= entry.depth) stack.pop();
    const node = { label: entry.label, path: entry.path, children: [] };
    stack[stack.length - 1].node.children.push(node);
    stack.push({ depth: entry.depth, node });
  }
  return root.children;
}

/** A tree node → a Starlight sidebar entry. A node with children becomes a group
 *  whose own page is surfaced as a leading "Overview" link (stock Starlight groups
 *  carry no link of their own). */
function toEntry(node) {
  if (node.children.length === 0) return { label: node.label, link: slugFor(node.path) };
  return {
    label: node.label,
    items: [{ label: "Overview", link: slugFor(node.path) }, ...node.children.map(toEntry)],
  };
}

/** A part's children → its group items, collapsing the common case where a part
 *  wraps a single index page (e.g. `# Guides` → `[Guides](guides/index.md)` + its
 *  subsections) so the sidebar reads "Guides › …" rather than "Guides › Guides". */
function partItems(children) {
  if (children.length === 1 && children[0].children.length > 0) {
    const only = children[0];
    return [{ label: "Overview", link: slugFor(only.path) }, ...only.children.map(toEntry)];
  }
  return children.map(toEntry);
}

function buildSidebar(text) {
  const { prefix, parts } = parseSummary(text);
  const sidebar = [];
  if (prefix) sidebar.push({ label: prefix.label, link: "/book/" });
  for (const part of parts) {
    sidebar.push({ label: part.label, items: partItems(buildTree(part.entries)) });
  }
  return sidebar;
}

function main(argv) {
  const check = argv.includes("--check");
  const rendered = JSON.stringify(buildSidebar(fs.readFileSync(SUMMARY, "utf8")), null, 2) + "\n";
  if (check) {
    const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
    if (current !== rendered) {
      console.error(
        "site/src/generated/sidebar.json is out of date with site/src/SUMMARY.md.\n" +
          "Regenerate with: node site/scripts/generate-sidebar.mjs",
      );
      process.exit(1);
    }
    console.log("sidebar.json is up to date.");
    return;
  }
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, rendered);
  console.log(`wrote sidebar.json (${buildSidebar(fs.readFileSync(SUMMARY, "utf8")).length} top-level entries).`);
}

main(process.argv.slice(2));
