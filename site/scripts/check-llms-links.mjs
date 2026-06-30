// @ts-check
//! Verify every `/book/` link in the curated `public/llms.txt` index resolves to
//! a real Book page (documentation track). `llms.txt` is hand-authored, so it can
//! drift when a page is renamed or moved; this confirms each Book route maps to a
//! committed source page. External links and in-code `](…)` (e.g. type signatures)
//! are not `/book/` routes, so they are naturally skipped.
//!
//!   node scripts/check-llms-links.mjs     report broken /book/ links; non-zero if any
//!
//! The `site` CI job runs this.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const LLMS = path.join(HERE, "../public/llms.txt");
const BOOK = path.join(HERE, "../src/content/docs/book");

const text = fs.readFileSync(LLMS, "utf8");
const broken = [];
for (const m of text.matchAll(/\]\((\/book\/[^)]*)\)/g)) {
  const route = m[1];
  const slug = route.replace(/^\/book\//, "").replace(/\/$/, "");
  const candidates = slug === "" ? ["index.md"] : [`${slug}.md`, `${slug}/index.md`];
  if (!candidates.some((rel) => fs.existsSync(path.join(BOOK, rel)))) {
    broken.push(route);
  }
}

if (broken.length > 0) {
  console.error(`llms.txt has ${broken.length} broken /book/ link(s):`);
  for (const r of broken) console.error(`  ${r}`);
  process.exit(1);
}
console.log("llms.txt links all resolve.");
