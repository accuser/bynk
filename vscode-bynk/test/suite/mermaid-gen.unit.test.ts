// #846: unit coverage for `toMermaid` (src/webview/mermaid-gen.ts) — the one
// piece of the sequence-diagram feature that is a pure function, testable
// directly against hand-built `SequenceModel` fixtures without a live
// webview/Mermaid render. Rendered-diagram correctness (does Mermaid accept
// this text, does the SVG's `.actor`/`.messageText` order match what
// main.ts zips against) needs a real browser and is out of reach here —
// this only pins the generated Mermaid *text*.

import * as assert from "assert";

import { toMermaid } from "../../src/webview/mermaid-gen";
import type { SequenceModel } from "../../src/webview/types";

const ZERO_RANGE = { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } };

describe("toMermaid", () => {
  it("renders a degenerate (entry-only) model as just the participant line", () => {
    const model: SequenceModel = {
      participants: [{ id: 0, kind: "Entry", name: "api call", range: null }],
      messages: [],
      blocks: [],
    };
    const { text } = toMermaid(model);
    assert.strictEqual(text, "sequenceDiagram\n    participant P0 as api call");
  });

  it("renders a Call+Return pair and a fire-and-forget Send with distinct arrows", () => {
    const model: SequenceModel = {
      participants: [
        { id: 0, kind: "Entry", name: "api call", range: null },
        { id: 1, kind: "Capability", name: "Clock", range: ZERO_RANGE },
      ],
      messages: [
        { from: 0, to: 1, kind: "Call", label: "now()", range: ZERO_RANGE, block: null },
        { from: 1, to: 0, kind: "Return", label: "", range: ZERO_RANGE, block: null },
        { from: 0, to: 1, kind: "Send", label: "info(\"hi\")", range: ZERO_RANGE, block: null },
      ],
      blocks: [],
    };
    const { text, messageOrder } = toMermaid(model);
    assert.ok(text.includes("P0->>P1: now()"), text);
    assert.ok(text.includes("P1-->>P0: reply"), text);
    assert.ok(text.includes("P0-)P1: info(#58;\"hi\")") || text.includes('P0-)P1: info("hi")'), text);
    assert.strictEqual(messageOrder.length, 3);
  });

  it("renders a return-gating if/else even when both branches carry no messages", () => {
    // The corrected extractor rule's own regression case: rate-limiter's
    // `if view.allowed { Ok(view) } else { TooManyRequests(...) }` produces
    // an AltBlock whose branches are message-free — the block itself must
    // still render.
    const model: SequenceModel = {
      participants: [{ id: 0, kind: "Entry", name: "api call", range: null }],
      messages: [],
      blocks: [
        {
          id: 0,
          kind: "If",
          branches: [
            { label: "then", messageIds: [] },
            { label: "else", messageIds: [] },
          ],
          range: ZERO_RANGE,
          parent: null,
          parentBranch: null,
        },
      ],
    };
    const { text } = toMermaid(model);
    assert.ok(text.includes("alt then"), text);
    assert.ok(text.includes("else else"), text);
    assert.ok(text.includes("end"), text);
  });

  it("nests a child block under the correct parent branch, not just the parent block", () => {
    // Two sibling blocks under the same parent id but different branches
    // must not be conflated — this is the regression `parent`/
    // `parentBranch` (rather than `parent` alone) exists to prevent.
    const model: SequenceModel = {
      participants: [
        { id: 0, kind: "Entry", name: "api call", range: null },
        { id: 1, kind: "Capability", name: "Clock", range: ZERO_RANGE },
      ],
      messages: [
        {
          from: 0,
          to: 1,
          kind: "Call",
          label: "now()",
          range: { start: { line: 2, character: 0 }, end: { line: 2, character: 1 } },
          block: 1,
        },
        {
          from: 1,
          to: 0,
          kind: "Return",
          label: "",
          range: { start: { line: 2, character: 1 }, end: { line: 2, character: 2 } },
          block: 1,
        },
      ],
      blocks: [
        {
          id: 0,
          kind: "If",
          branches: [
            { label: "then", messageIds: [] },
            { label: "else", messageIds: [] },
          ],
          range: { start: { line: 0, character: 0 }, end: { line: 5, character: 0 } },
          parent: null,
          parentBranch: null,
        },
        {
          id: 1,
          kind: "If",
          branches: [
            { label: "then", messageIds: [0, 1] },
            { label: "else", messageIds: [] },
          ],
          range: { start: { line: 1, character: 0 }, end: { line: 3, character: 0 } },
          parent: 0,
          parentBranch: 0, // nested in block 0's "then" branch
        },
      ],
    };
    const { text } = toMermaid(model);
    // The nested `alt` (block 1) must appear between block 0's `alt then`
    // and its `else` — i.e. inside the "then" branch, not after `end`.
    const outerThen = text.indexOf("alt then");
    const outerEnd = text.lastIndexOf("end");
    const innerAlt = text.indexOf("alt then", outerThen + 1);
    assert.ok(outerThen >= 0 && innerAlt > outerThen && innerAlt < outerEnd, text);
    assert.ok(text.includes("P0->>P1: now()"), text);
  });

  it("renders a Collapsed block as a note, not an alt/opt", () => {
    const model: SequenceModel = {
      participants: [{ id: 0, kind: "Entry", name: "api call", range: null }],
      messages: [],
      blocks: [
        {
          id: 0,
          kind: "Collapsed",
          branches: [],
          range: ZERO_RANGE,
          parent: null,
          parentBranch: null,
        },
      ],
    };
    const { text, collapsedOrder } = toMermaid(model);
    assert.ok(text.includes("note over P0"), text);
    assert.ok(!text.includes("alt "), text);
    assert.strictEqual(collapsedOrder.length, 1);
  });

  it("renders opt (not alt) for a single-branch block", () => {
    const model: SequenceModel = {
      participants: [{ id: 0, kind: "Entry", name: "api call", range: null }],
      messages: [],
      blocks: [
        {
          id: 0,
          kind: "Match",
          branches: [{ label: "Some(x)", messageIds: [] }],
          range: ZERO_RANGE,
          parent: null,
          parentBranch: null,
        },
      ],
    };
    const { text } = toMermaid(model);
    assert.ok(text.includes("opt Some(x)"), text);
  });
});
