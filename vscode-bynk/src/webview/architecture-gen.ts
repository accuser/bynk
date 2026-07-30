// #851: pure `ArchModel` -> Mermaid `flowchart` text generation. Kept
// separate from `architecture.ts`'s DOM/webview glue for the same reason
// `mermaid-gen.ts` (#846) is separate from `main.ts`: no dependency on a
// browser or VS Code webview host, so it's directly unit-testable.
//
// Click-to-code: unlike the sequence view (whose `sequenceDiagram` render
// has no reliable per-element id, forcing a DOM-order zip — see
// `participant-map.ts`'s long war story), Mermaid's `flowchart` gives every
// node a stable id in the rendered SVG (`id="flowchart-<mermaidId>-<n>"`,
// preserving the id this module assigns). So instead of an order-based zip,
// `clickTargets` maps each mermaid node id directly to what clicking it
// should do — the caller (`architecture.ts`) looks elements up by id, which
// survives Mermaid's dagre layout reordering the DOM (a real risk for a
// non-linear diagram, unlike the strictly left-to-right sequence view).
//
// DECISION A (context-level nodes, expand-on-demand): every context/adapter
// is one compact node by default. A synthetic "expand" node, linked by a
// dotted edge, toggles that one context into its declaration-level members
// (capabilities/providers/services/agents) on the *next* render — expansion
// state lives in the caller (`architecture.ts`) and is threaded back in via
// `expanded`; this module stays a pure `(model, expanded) -> text` function,
// re-invoked on every toggle rather than mutating the rendered SVG in place.

import type {
  ArchAgent,
  ArchCapability,
  ArchEdge,
  ArchLoc,
  ArchModel,
  ArchNode,
  ArchProvider,
  ArchService,
} from "./architectureTypes";

export type ClickTarget =
  | { kind: "reveal"; loc: ArchLoc }
  | { kind: "toggle"; name: string };

export interface ArchMermaidResult {
  text: string;
  /** Mermaid node id -> the click behaviour for that node. */
  clickTargets: Map<string, ClickTarget>;
}

function esc(label: string): string {
  return label.replace(/"/g, "&quot;").replace(/\n/g, " ").trim();
}

/** `flowchart` node ids must be identifier-like — a qualified name's dots
 *  and any other punctuation are not safe there (the label carries the real
 *  display name instead), so nodes are addressed by array index. */
function contextId(i: number): string {
  return `ctx${i}`;
}

function toggleId(i: number): string {
  return `${contextId(i)}_t`;
}

function memberId(i: number, m: number): string {
  return `${contextId(i)}_m${m}`;
}

function memberLine(
  id: string,
  ownerId: string,
  label: string,
  loc: ArchLoc,
  clickTargets: Map<string, ClickTarget>,
  lines: string[],
): void {
  lines.push(`  ${id}("${esc(label)}"):::memberNode`);
  lines.push(`  ${ownerId} --- ${id}`);
  clickTargets.set(id, { kind: "reveal", loc });
}

export function toMermaidArch(model: ArchModel, expanded: ReadonlySet<string>): ArchMermaidResult {
  const lines: string[] = ["flowchart LR"];
  const clickTargets = new Map<string, ClickTarget>();
  const idByName = new Map<string, string>();
  model.nodes.forEach((n, i) => idByName.set(n.name, contextId(i)));

  model.nodes.forEach((node: ArchNode, i) => {
    const id = contextId(i);
    const isExpanded = expanded.has(node.name);
    const cls = node.kind === "Adapter" ? "adapterNode" : "contextNode";
    lines.push(`  ${id}["${esc(node.name)}"]:::${cls}`);
    clickTargets.set(id, { kind: "reveal", loc: node.loc });

    const tId = toggleId(i);
    const hasMembers =
      node.capabilities.length + node.providers.length + node.services.length + node.agents.length > 0;
    if (hasMembers) {
      lines.push(`  ${tId}(("${isExpanded ? "−" : "+"}")):::toggleNode`);
      lines.push(`  ${id} -.- ${tId}`);
      clickTargets.set(tId, { kind: "toggle", name: node.name });
    }

    if (isExpanded) {
      let m = 0;
      const emitCapability = (c: ArchCapability): void => {
        const label = c.local ? `${c.name} (capability)` : `${c.name} (from ${c.from ?? "?"})`;
        memberLine(memberId(i, m++), id, label, c.loc, clickTargets, lines);
      };
      const emitProvider = (p: ArchProvider): void => {
        memberLine(memberId(i, m++), id, `${p.capability} = ${p.providerName}`, p.loc, clickTargets, lines);
      };
      const emitService = (s: ArchService): void => {
        memberLine(memberId(i, m++), id, `service ${s.name} (${s.handlerCount})`, s.loc, clickTargets, lines);
      };
      const emitAgent = (a: ArchAgent): void => {
        memberLine(memberId(i, m++), id, `agent ${a.name} (${a.handlerCount})`, a.loc, clickTargets, lines);
      };
      node.capabilities.forEach(emitCapability);
      node.providers.forEach(emitProvider);
      node.services.forEach(emitService);
      node.agents.forEach(emitAgent);
    }
  });

  for (const edge of reachableEdges(model)) {
    const from = idByName.get(edge.from);
    const to = idByName.get(edge.to);
    if (!from || !to) continue;
    const label = edge.capabilities.length > 0 ? esc(edge.capabilities.join(", ")) : "";
    lines.push(label ? `  ${from} -->|"${label}"| ${to}` : `  ${from} --> ${to}`);
  }

  return { text: lines.join("\n"), clickTargets };
}

/** Every edge in `model` whose endpoints both resolved to a rendered node —
 *  a defensive floor (the server itself never emits a dangling edge, see
 *  `bynk-ide`'s `architecture.rs` module doc) that also protects the loop
 *  above: without it, a dangling edge's `idByName.get` miss would still be
 *  caught by the `if (!from || !to) continue` there, but factoring the
 *  filter out keeps that loop's job to just "render", not "validate". */
export function reachableEdges(model: ArchModel): ArchEdge[] {
  const names = new Set(model.nodes.map((n) => n.name));
  return model.edges.filter((e) => names.has(e.from) && names.has(e.to));
}
