// #851: unit coverage for `toMermaidArch` (src/webview/architecture-gen.ts) —
// the pure part of the architecture-map feature, testable directly against
// hand-built `ArchModel` fixtures without a live webview/Mermaid render.
// Same posture as `mermaid-gen.unit.test.ts` (#846).

import * as assert from "assert";

import { reachableEdges, toMermaidArch } from "../../src/webview/architecture-gen";
import type { ArchModel, ArchNode } from "../../src/webview/architectureTypes";

const ZERO_LOC = { uri: "file:///a.bynk", range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };

function contextNode(name: string, overrides: Partial<ArchNode> = {}): ArchNode {
  return {
    name,
    kind: "Context",
    loc: ZERO_LOC,
    capabilities: [],
    providers: [],
    services: [],
    agents: [],
    ...overrides,
  };
}

describe("toMermaidArch", () => {
  it("renders a single node with no members and no toggle", () => {
    const model: ArchModel = { nodes: [contextNode("ratelimit")], edges: [] };
    const { text, clickTargets } = toMermaidArch(model, new Set());
    assert.ok(text.includes('ctx0["ratelimit"]:::contextNode'), text);
    assert.ok(!text.includes("ctx0_t"), "no members means no expand toggle");
    assert.strictEqual(clickTargets.size, 1);
    assert.deepStrictEqual(clickTargets.get("ctx0"), { kind: "reveal", loc: ZERO_LOC });
  });

  it("renders a toggle node when a context has any members, collapsed by default", () => {
    const model: ArchModel = {
      nodes: [
        contextNode("ratelimit", {
          capabilities: [{ name: "Clock", local: false, from: "bynk", loc: ZERO_LOC }],
        }),
      ],
      edges: [],
    };
    const { text, clickTargets } = toMermaidArch(model, new Set());
    assert.ok(text.includes('ctx0_t(("+")):::toggleNode'), text);
    assert.deepStrictEqual(clickTargets.get("ctx0_t"), { kind: "toggle", name: "ratelimit" });
    // Collapsed: the capability member itself is not rendered.
    assert.ok(!text.includes("ctx0_m0"), text);
  });

  it("expands a node's declaration-level members when its name is in `expanded`", () => {
    const model: ArchModel = {
      nodes: [
        contextNode("ratelimit", {
          capabilities: [{ name: "Clock", local: false, from: "bynk", loc: ZERO_LOC }],
          services: [{ name: "api", handlerCount: 1, loc: ZERO_LOC }],
          agents: [{ name: "Limiter", handlerCount: 1, loc: ZERO_LOC }],
        }),
      ],
      edges: [],
    };
    const { text, clickTargets } = toMermaidArch(model, new Set(["ratelimit"]));
    assert.ok(text.includes("ctx0_t"), "toggle now shows collapse");
    assert.ok(text.includes('(("−")):::toggleNode'), text);
    assert.ok(text.includes('ctx0_m0("Clock (from bynk)"):::memberNode'), text);
    assert.ok(text.includes('ctx0_m1("service api (1)"):::memberNode'), text);
    assert.ok(text.includes('ctx0_m2("agent Limiter (1)"):::memberNode'), text);
    assert.ok(text.includes("ctx0 --- ctx0_m0"), text);
    assert.strictEqual(clickTargets.size, 5, "context + toggle + 3 members");
    assert.deepStrictEqual(clickTargets.get("ctx0_m1"), { kind: "reveal", loc: ZERO_LOC });
  });

  it("labels an edge with its selected capabilities and leaves a whole-unit edge unlabelled", () => {
    const model: ArchModel = {
      nodes: [contextNode("consumer"), contextNode("platform")],
      edges: [
        { from: "consumer", to: "platform", capabilities: ["Clock"], loc: ZERO_LOC },
      ],
    };
    const { text } = toMermaidArch(model, new Set());
    assert.ok(text.includes('ctx0 -->|"Clock"| ctx1'), text);

    const wholeUnit: ArchModel = {
      nodes: [contextNode("consumer"), contextNode("platform")],
      edges: [{ from: "consumer", to: "platform", capabilities: [], loc: ZERO_LOC }],
    };
    const { text: text2 } = toMermaidArch(wholeUnit, new Set());
    assert.ok(text2.includes("ctx0 --> ctx1"), text2);
    assert.ok(!text2.includes("-->|"), text2);
  });

  it("drops an edge whose endpoint has no corresponding node (defensive floor)", () => {
    const model: ArchModel = {
      nodes: [contextNode("consumer")],
      edges: [{ from: "consumer", to: "bynk", capabilities: ["Clock"], loc: ZERO_LOC }],
    };
    const { text } = toMermaidArch(model, new Set());
    assert.ok(!text.includes("-->"), text);
    assert.deepStrictEqual(reachableEdges(model), []);
  });

  it("styles an adapter node distinctly from a context node", () => {
    const model: ArchModel = { nodes: [contextNode("tokens", { kind: "Adapter" })], edges: [] };
    const { text } = toMermaidArch(model, new Set());
    assert.ok(text.includes(":::adapterNode"), text);
    assert.ok(!text.includes(":::contextNode"), text);
  });

  it("escapes double quotes and collapses newlines in labels", () => {
    const model: ArchModel = {
      nodes: [contextNode('weird "name"\nwith a newline')],
      edges: [],
    };
    const { text } = toMermaidArch(model, new Set());
    assert.ok(text.includes("&quot;name&quot;"), text);
    assert.ok(!text.includes('"name"'), text);
    assert.ok(!text.includes("\nwith"), text);
  });
});
