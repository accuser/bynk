// @ts-check
//! Migrate one mdBook Book page into a Starlight content page (documentation
//! track, slice 2). Slice 2a runs this on a few proof pages; slice 2b runs it
//! over the whole Book. The transform is purely structural — directives,
//! callouts and mermaid are left for the build-time remark plugin
//! (`remark-bynk-directives`); only what Starlight needs at the source level is
//! rewritten here:
//!
//!   1. Lift the leading `# H1` into `title:` frontmatter and drop the heading
//!      (Starlight renders the page title from frontmatter).
//!   2. Rewrite relative `.md` links to Starlight slug links: a Book page at
//!      `docs/src/<p>.md` is served at `/book/<p>/`, so `(../guides/x.md#a)`
//!      becomes `(/book/guides/x/#a)`.
//!   3. Neutralise the escaped `\{{#…}}` form so it renders literally instead of
//!      being expanded by the remark plugin.
//!
//! Usage:
//!   node scripts/migrate-page.mjs <src.md> <dest.md> [--src-root <dir>]
//! `--src-root` defaults to `../docs/src` (relative to this script).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));

function main(argv) {
  const args = argv.slice(2);
  const positional = args.filter((a) => !a.startsWith("--"));
  const srcRootFlag = flag(args, "--src-root");
  const [src, dest] = positional;
  if (!src || !dest) {
    console.error("usage: migrate-page.mjs <src.md> <dest.md> [--src-root <dir>]");
    process.exit(2);
  }
  const srcRoot = path.resolve(srcRootFlag ?? path.join(HERE, "../../docs/src"));
  const out = migrate(fs.readFileSync(src, "utf8"), path.resolve(src), srcRoot);
  fs.mkdirSync(path.dirname(path.resolve(dest)), { recursive: true });
  fs.writeFileSync(path.resolve(dest), out);
  console.log(`migrated ${src} -> ${dest}`);
}

/**
 * @param {string} source  raw mdBook markdown
 * @param {string} srcPath absolute path of the source page
 * @param {string} srcRoot absolute path of `docs/src`
 * @returns {string} the Starlight page
 */
export function migrate(source, srcPath, srcRoot) {
  const { title, body } = liftTitle(source);
  let out = body;
  out = rewriteLinks(out, srcPath, srcRoot);
  out = neutraliseEscapedDirectives(out);
  const frontmatter = `---\ntitle: ${yamlString(title)}\n---\n`;
  return frontmatter + out.replace(/^\n+/, "");
}

/** Split off the first `# H1` as the title; the rest is the body. */
function liftTitle(source) {
  const lines = source.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const match = /^#\s+(.+?)\s*$/.exec(lines[i]);
    if (match) {
      lines.splice(i, 1);
      return { title: match[1], body: lines.join("\n") };
    }
    if (lines[i].trim() !== "") break; // content before any H1 — bail out
  }
  throw new Error(`${source.slice(0, 40)}…: no leading \`# H1\` to lift into the title`);
}

/** Rewrite relative `*.md` links to `/book/<slug>/` routes. */
function rewriteLinks(body, srcPath, srcRoot) {
  const srcDir = path.dirname(srcPath);
  return body.replace(/\]\(([^)]+)\)/g, (whole, target) => {
    if (/^(https?:|mailto:|#|\/)/.test(target)) return whole; // external / in-page / already-rooted
    const [pathPart, anchor = ""] = target.split("#");
    if (!pathPart.endsWith(".md")) return whole;
    const abs = path.resolve(srcDir, pathPart);
    const rel = path.relative(srcRoot, abs); // e.g. guides/x.md
    if (rel.startsWith("..")) return whole; // outside the Book tree — leave as-is
    const slug = rel.replace(/\.md$/, "").replace(/\/index$/, "").replace(/\\/g, "/");
    const hash = anchor ? `#${anchor}` : "";
    return `](/book/${slug}/${hash})`;
  });
}

/** `\{{#…}}` should render literally — wrap it so the remark plugin skips it. */
function neutraliseEscapedDirectives(body) {
  return body.replace(/\\\{\{#([^}]*)\}\}/g, "`{{#$1}}`");
}

function flag(args, name) {
  const i = args.indexOf(name);
  return i !== -1 ? args[i + 1] : undefined;
}

function yamlString(value) {
  // Quote when YAML could misread the bare scalar; escape backslashes first,
  // then embedded quotes (order matters — escaping quotes adds backslashes).
  if (/^[A-Za-z][\w ,.'’!?()&/-]*$/.test(value) && !/:\s/.test(value)) return value;
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv);
}
