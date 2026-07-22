// #846: pure `SequenceModel` -> Mermaid `sequenceDiagram` text generation.
// Kept separate from `main.ts`'s DOM/webview glue so it has no dependency on
// a browser or VS Code webview host — the whole point of a structured
// `SequenceModel` wire shape (rather than server-rendered Mermaid text) is
// that `bynk-check`/`bynk-ide`/`bynk-lsp` stay Mermaid-agnostic; this module
// is the one place that owns "how a `SequenceModel` becomes Mermaid syntax."
//
// Rendering is a recursive walk of the block tree (`AltBlock.parent` +
// `AltBlock.parentBranch`), not a single pass over the flat `messages`
// array: a branch can be entirely message-free (the rate-limiter's
// `if`/`else` gates only the final return) while still containing a nested
// block, so the flat array alone cannot place it. At each level, sibling
// messages and child blocks are merged and ordered by source position
// (`range.start`).
//
// Mermaid's `click` directive support for `sequenceDiagram` is inconsistent
// across versions and does not cover every element kind this renders (an
// `alt`/`opt` header, a `note`), so click-to-code is wired the same way in
// `main.ts`: a DOM-order zip against `participantOrder`/`messageOrder`
// (the emission order — the same order the caller must walk `.actor` /
// message-line elements in the rendered SVG) rather than embedded `click`
// statements.

import type { AltBlock, Message, Participant, Range, SequenceModel } from "./types";

export interface MermaidResult {
  text: string;
  /** `model.participants` order — the caller zips this 1:1 against the
   *  rendered SVG's `.actor` elements, which Mermaid emits in the same
   *  `participant` declaration order this function writes them in. */
  participantOrder: Participant[];
  /** Emission order (source-position order, following nesting) — the
   *  caller zips this 1:1 against the rendered SVG's message-line
   *  elements, which Mermaid emits in the order the diagram text names
   *  them, i.e. this order. */
  messageOrder: Message[];
  /** One entry per rendered `Collapsed` marker, in emission order — zipped
   *  against the rendered SVG's `note` elements the same way. */
  collapsedOrder: AltBlock[];
}

const RESERVED_LABEL_CHARS: Record<string, string> = {
  ":": "#58;",
  ";": "#59;",
};

function escapeLabel(label: string): string {
  const collapsedWhitespace = label.replace(/\s+/g, " ").trim();
  return collapsedWhitespace.replace(/[:;]/g, (ch) => RESERVED_LABEL_CHARS[ch] ?? ch);
}

function rangeKey(range: Range): [number, number] {
  return [range.start.line, range.start.character];
}

function childBlocksOf(blocks: AltBlock[], parentId: number, parentBranch: number): AltBlock[] {
  return blocks.filter((b) => b.parent === parentId && b.parentBranch === parentBranch);
}

type ContentItem =
  | { start: [number, number]; kind: "message"; message: Message }
  | { start: [number, number]; kind: "block"; block: AltBlock };

function orderedContent(messages: Message[], blocks: AltBlock[]): ContentItem[] {
  const items: ContentItem[] = [
    ...messages.map((message) => ({ start: rangeKey(message.range), kind: "message" as const, message })),
    ...blocks.map((block) => ({ start: rangeKey(block.range), kind: "block" as const, block })),
  ];
  items.sort((a, b) => a.start[0] - b.start[0] || a.start[1] - b.start[1]);
  return items;
}

export function toMermaid(model: SequenceModel): MermaidResult {
  const lines: string[] = ["sequenceDiagram"];
  const messageOrder: Message[] = [];
  const collapsedOrder: AltBlock[] = [];

  for (const p of model.participants) {
    lines.push(`    participant P${p.id} as ${escapeLabel(p.name)}`);
  }

  const emitMessage = (m: Message): void => {
    messageOrder.push(m);
    const label = escapeLabel(m.label);
    if (m.kind === "Send") {
      lines.push(`    P${m.from}-)P${m.to}: ${label}`);
    } else if (m.kind === "Call") {
      lines.push(`    P${m.from}->>P${m.to}: ${label}`);
    } else {
      // Return: label is empty (the extractor doesn't synthesise one) —
      // Mermaid requires *some* text after `:`, so fall back to the arrow
      // alone reading as "reply".
      lines.push(`    P${m.from}-->>P${m.to}: ${label || "reply"}`);
    }
  };

  const emitBlock = (block: AltBlock): void => {
    if (block.kind === "Collapsed") {
      collapsedOrder.push(block);
      const anchor = model.participants[0]?.id ?? 0;
      lines.push(`    note over P${anchor}: nested branching — click to view source`);
      return;
    }
    const keyword = block.branches.length <= 1 ? "opt" : "alt";
    block.branches.forEach((branch, branchIndex) => {
      lines.push(`    ${branchIndex === 0 ? keyword : "else"} ${escapeLabel(branch.label)}`);
      const branchMessages = branch.messageIds
        .map((id) => model.messages[id])
        .filter((m): m is Message => m !== undefined);
      const children = childBlocksOf(model.blocks, block.id, branchIndex);
      renderContent(branchMessages, children);
    });
    lines.push("    end");
  };

  function renderContent(messages: Message[], blocks: AltBlock[]): void {
    for (const item of orderedContent(messages, blocks)) {
      if (item.kind === "message") emitMessage(item.message);
      else emitBlock(item.block);
    }
  }

  const rootMessages = model.messages.filter((m) => m.block === null);
  const rootBlocks = model.blocks.filter((b) => b.parent === null);
  renderContent(rootMessages, rootBlocks);

  return {
    text: lines.join("\n"),
    participantOrder: model.participants,
    messageOrder,
    collapsedOrder,
  };
}
