# 0302 — The architecture map renders one node per context/adapter, expanded on demand

- **Status:** Accepted (v0.245)

**Context.** Rendering every service/agent/capability/provider as its own
permanent node does not scale past a small project — the issue's own risk
section names this ("a large project renders an illegible hairball").

**Decision.** Every context/adapter is one compact node by default. The wire
model carries its declaration-level members inline (not behind a second
request) — expand-on-demand is therefore a **client-side** re-render: a small
+/− toggle next to each node (`vscode-bynk/src/webview/architecture-gen.ts`)
flips that one context's name into a local `expanded: Set<string>`, and the
whole Mermaid `flowchart` text is regenerated from the same in-memory model.

**Consequences.** No second LSP request or server round-trip backs expansion.
Because a `flowchart`'s rendered SVG preserves the node id this module
assigns (`id="flowchart-<mermaidId>-<n>"`), click-to-code is wired by id
lookup rather than the DOM-order zip the sequence view needs (`participant-
map.ts`'s long workaround for `sequenceDiagram`'s lack of one) — a flowchart
does not share that fragility, so it doesn't need the workaround either.
