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
  /** One entry per rendered `note` element, in emission order — the block
   *  each note belongs to (a `Collapsed` marker, a branch's reply outcome, or
   *  an empty-branch placeholder). The caller zips this 1:1 against the
   *  rendered SVG's `.noteText` elements and links each to its block's span,
   *  so every note is click-to-code just like the collapsed markers always
   *  were. (A single block can contribute several notes — one reply per
   *  branch — so this is not 1:1 with `model.blocks`.) */
  noteOrder: AltBlock[];
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
  const noteOrder: AltBlock[] = [];

  // The entry lifeline (participant 0) — where every note (a collapsed
  // marker, a branch reply, an empty-branch placeholder) is anchored, since
  // the handler *is* the entry and its reply has no separate caller lifeline.
  const entryAnchor = model.participants[0]?.id ?? 0;

  for (const p of model.participants) {
    // The principal renders as an `actor` (stick figure); everyone else is a
    // `participant` box.
    const keyword = p.kind === "Actor" ? "actor" : "participant";
    lines.push(`    ${keyword} P${p.id} as ${escapeLabel(p.name)}`);
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

  const emitNote = (block: AltBlock, text: string): void => {
    noteOrder.push(block);
    lines.push(`    note over P${entryAnchor}: ${text}`);
  };

  const emitBlock = (block: AltBlock): void => {
    if (block.kind === "Collapsed") {
      emitNote(block, "nested branching — click to view source");
      return;
    }
    const keyword = block.branches.length <= 1 ? "opt" : "alt";
    block.branches.forEach((branch, branchIndex) => {
      lines.push(`    ${branchIndex === 0 ? keyword : "else"} ${escapeLabel(branch.label)}`);
      const branchMessages = branch.messageIds
        .map((id) => model.messages[id])
        .filter((m): m is Message => m !== undefined);
      const children = childBlocksOf(model.blocks, block.id, branchIndex);
      const before = lines.length;
      renderContent(branchMessages, children);
      // The branch's own reply — the value the handler yields on this path —
      // as a note over the entry lifeline. This is the content that keeps a
      // return-gating block (both branches call no lifeline) from rendering
      // as an empty, mangled `alt` box.
      if (branch.reply) emitNote(block, escapeLabel(branch.reply));
      // A branch that emitted nothing at all (no messages, no nested block,
      // no reply — e.g. an explicit `{ () }`) would still collapse the box;
      // anchor it with a placeholder so the diagram stays legible.
      if (lines.length === before) emitNote(block, "…");
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
    noteOrder,
  };
}
