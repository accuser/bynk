// #846: the sequence-diagram webview's own bundle entry (`out/webview.js`,
// a separate esbuild target from the extension host's `out/extension.js` —
// see `package.json`'s `build` script). Runs inside the webview's browser
// context, never the extension host: no `vscode` module, only
// `acquireVsCodeApi()`.
//
// Reads the `SequencePayload` the extension host embeds as
// `window.__BYNK_SEQUENCE_MODEL__`, renders it via vendored Mermaid (no
// CDN — the page's CSP forbids one), and wires click-to-code: Mermaid's
// `click` directive support for `sequenceDiagram` is inconsistent across
// versions and doesn't cover every element this renders (an `alt` header, a
// `note`), so instead this walks the rendered SVG's `.actor` /
// `.messageText` / `.loopText` / `.noteText` elements in DOM order — the
// same order `mermaid-gen.ts` emitted them in — and zips each against the
// `{uri, range}` it corresponds to, posting a `reveal` message back to the
// extension host on click.

import mermaid from "mermaid";

import { toMermaid } from "./mermaid-gen";
import type { RevealMessage, SequencePayload } from "./types";

declare function acquireVsCodeApi(): {
  postMessage(message: unknown): void;
};

const vscodeApi = acquireVsCodeApi();

function reveal(uri: string, range: SequencePayload["model"]["messages"][number]["range"]): void {
  const message: RevealMessage = { type: "reveal", uri, range };
  vscodeApi.postMessage(message);
}

function wireClickable(el: Element, uri: string, range: SequencePayload["model"]["messages"][number]["range"]): void {
  el.addEventListener("click", () => reveal(uri, range));
  (el as HTMLElement).style.cursor = "pointer";
}

async function main(): Promise<void> {
  const root = document.getElementById("root");
  const payload = (window as unknown as { __BYNK_SEQUENCE_MODEL__?: SequencePayload })
    .__BYNK_SEQUENCE_MODEL__;
  if (!root) return;
  if (!payload) {
    root.textContent = "No sequence model to render.";
    return;
  }

  mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });

  const { text, participantOrder, messageOrder, collapsedOrder } = toMermaid(payload.model);
  let svg: string;
  try {
    ({ svg } = await mermaid.render("bynk-sequence-diagram", text));
  } catch (err) {
    root.textContent = `Failed to render diagram: ${String(err)}`;
    return;
  }
  root.innerHTML = svg;

  // Participants (skip `Entry`, which carries no span) — `.actor` boxes,
  // Mermaid's documented sequence-diagram theming class.
  const actorEls = root.querySelectorAll(".actor");
  participantOrder.forEach((p, i) => {
    if (p.range) {
      const el = actorEls[i];
      if (el) wireClickable(el, payload.uri, p.range);
    }
  });

  // Messages, in the same order `mermaid-gen.ts` emitted them (`.messageText`).
  const messageEls = root.querySelectorAll(".messageText");
  messageOrder.forEach((m, i) => {
    const el = messageEls[i];
    if (el) wireClickable(el, payload.uri, m.range);
  });

  // `alt`/`opt` branch headers (`.labelText`/`.loopText` depending on
  // Mermaid version) link to their block's own span — every branch header
  // for a given block points at the same span, since the block (not the
  // branch) is what has a source location.
  const headerEls = root.querySelectorAll(".labelText, .loopText");
  let headerIndex = 0;
  for (const block of payload.model.blocks) {
    if (block.kind === "Collapsed") continue;
    for (let b = 0; b < block.branches.length; b++) {
      const el = headerEls[headerIndex++];
      if (el) wireClickable(el, payload.uri, block.range);
    }
  }

  // Collapsed-nesting notes (`.noteText`), in emission order.
  const noteEls = root.querySelectorAll(".noteText");
  collapsedOrder.forEach((block, i) => {
    const el = noteEls[i];
    if (el) wireClickable(el, payload.uri, block.range);
  });
}

void main();
