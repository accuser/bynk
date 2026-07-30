---
level: minor
changelog: A VS Code webview maps a whole project's contexts, their `consumes` edges, and the capabilities/providers/services/agents each one binds
---

## ADR: architecture-map-data-source
title: The architecture map reads CrossContextInfo and a re-parse, not the binding index's call graph
summary: Citation correction — the index has no context symbol kind and no consumes edges

**Context.** The increment proposal (#851) described `architecture_model` as
"a pure, read-only query over the binding index and call graph"
(`bynk-check/src/index.rs`, ADR 0053/0067) — the same table `call_hierarchy`
already reads. Implementation found this citation does not hold:
`index::SymbolKind` has no `Context`/`Adapter` variant (its own doc lists
`Type, Fn, Capability, Service, Agent, Provider, Method, Field, CapabilityOp,
Actor, Handler, Messages`), so a context is not an index symbol at all, and
`ProjectIndex.calls` — the call graph — does not carry `consumes` edges either;
the field's own comment already flags this: "service cross-context dispatch
remains the one uncovered relation (no per-handler index symbol to be its
callee)".

**Decision.** Source nodes and edges from `ContextSequenceInfo::cross_context`
(`resolver::CrossContextInfo`) instead — the exact table `bynk/sequenceModel`'s
own classifier (`sequence::Builder::classify_cross_context`) already resolves
consumed-context names and aliases against — plus a re-parse of each unit's own
committed snapshot text (`bynk_syntax::parser::parse_unit_with_recovery`) for
its local declarations (capabilities, providers, services, agents) and its raw
`consumes` clauses. There is no retained AST after a round to read instead —
the same constraint `sequence_request`/`documentation_request` re-parse
against for their own file-scoped queries.

**Consequences.** `bynk_ide::architecture::architecture_model` takes
`unit_sources`, `snapshots`, and `sequence_info` — the same three tables
already threaded `ProjectAnalysis` → `ProjectDiagnostics` → the LSP
`Analysis` for #846/#847/#848 — rather than `ProjectIndex`. No index change
was needed for this increment; a future increment that wants a `Context`
index symbol (e.g. to back go-to-definition on a bare context name outside a
`uses`/`consumes` clause) is unrelated follow-up, not a prerequisite this one
discovered.

## ADR: architecture-map-node-granularity
title: The architecture map renders one node per context/adapter, expanded on demand
summary: DECISION A — context-level nodes by default, declaration-level members inline in the model

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

## ADR: architecture-map-project-scope
title: The architecture map covers the active file's project, resolved the same way as the extension's own project lookup
summary: DECISION B — per-project (nearest bynk.toml), not per-workspace

**Context.** A workspace may hold several Bynk projects. Rendering every
project superimposed in one diagram produces an unreadable tangle with no
project boundary drawn between them.

**Decision.** `bynk/architectureModel`'s params are a bare `textDocument` —
used only to resolve *which* project's committed round to read (the same
`committed_analysis` gate every pull-based request already uses), never to
restrict the result to that one file. The VS Code command requires an active
`.bynk` editor for the same reason the sequence/documentation commands do:
resolving a project needs a file to walk up from.

**Consequences.** A workspace with several open Bynk projects shows one map
per invocation, scoped to whichever project the active editor belongs to. A
multi-project picker (distinct from this scoping question) is left as a
follow-up; the command's `when` clause stays `editorLangId == bynk`,
consistent with the sequence/documentation commands, rather than introducing
a new `bynk.hasProject` context key this increment does not otherwise need.

## ADR: architecture-map-capability-binding-depth
title: The architecture map shows capability binding, not residency
summary: DECISION C — braced consumes selections bind capabilities onto nodes; residency is deferred

**Context.** A `consumes U { Cap, … }` selection flattens `Cap` into the
consumer's own namespace (§3.3); a whole-unit `consumes U` (no braces) grants
qualified access to everything `U` exports without flattening anything. The
built-in `consumes bynk { Clock }` form (every project that uses a toolchain
capability) resolves to the synthetic `bynk` unit, which has no project file
and so is never itself rendered as a node — an edge to it would dangle.

**Decision.** A braced selection's capability labels are recorded twice: once
as the `consumes` edge's own label (only when the target resolves to a real
node), and once as a bound-capability entry directly on the *consuming*
node's `capabilities` list (origin `Consumed { from }`), regardless of
whether the target became a node. A whole-unit consumes draws an edge only
(unlabelled) and binds nothing onto the node — nothing is flattened to
enumerate. Residency/boundary-tier annotation is out of scope for this
increment.

**Consequences.** The rate-limiter fixture's `consumes bynk { Clock }`
renders as a single node with a bound `Clock` capability and *no* edge (the
synthetic `bynk` unit contributes no node) — the concrete case the "Done
when" fixture combination in #851 calls out by name. A future increment that
adds residency/boundary-tier data is additive, not a rework of this shape.
