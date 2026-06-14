# 0067 — Call hierarchy from a preserved-owner call graph

- **Status:** Accepted (v0.34)
- **Spec:** `design/karn-lsp-spec.md` §3.18
- **Relates to:** ADR 0053 (the binding index this extends)

## Context
Call hierarchy needs caller→callee edges with containing-declaration
attribution. The binding index records *that* a reference occurs and *which*
symbol it references, but the assembled `SiteRef` does not say which declaration
encloses it. That attribution already exists one layer up: every `RefEdge`
carries `owner` — the enclosing top-level declaration's display name
(`index.rs:73`), set around each fn/service/agent/provider/capability body — and
the index uses it at assembly only for file re-attribution, then drops it.

## Decision
Preserve `owner`, resolved to the caller's `SymbolKey`, as a **`CallEdge { caller,
callee, site }` side table** on `ProjectIndex`, built in `IndexBuilder::build`
alongside the existing reference push.

- **Caller resolution mirrors re-attribution.** A new `owner_keys: HashMap<(unit,
  name), SymbolKey>`, populated in `add_def` next to the existing `owner_files`,
  resolves an edge's `(namespace, owner)` to the caller key — the same lookup
  shape the file re-attribution already uses, so it cannot be *more* wrong.
- **`Fn` callees only.** Method callees, capability ops, and agent/service
  dispatch are not index symbols (deferred index kinds), so they record no edge.
  Every edge shown is a real fn call; the gaps line up exactly with that
  deferral, and the edges join the graph for free once those kinds are indexed.
- **Any indexed owner may be a caller.** A service handler that calls a free fn
  shows the *service* as an incoming caller. Method owners (`"T.m"`, registered
  via `add_owner`, not `add_def`) have no `owner_keys` entry and so record no
  edge — the same boundary, visible as: a callee's reference count can exceed its
  incoming-call count.
- **The call site is the reference span.** The callee-name span (re-attributed to
  the caller's file) is the LSP `fromRanges` for both directions.
- **Identity travels in `data`.** `prepareCallHierarchy` round-trips the resolved
  `SymbolKey` through `CallHierarchyItem.data` (as a `SerKey`, since the index
  kind isn't `Serialize`); incoming/outgoing resolve straight off it, never
  re-inferring from a position. A missing/garbled payload returns no calls.
- **Pure queries.** `index_queries::{prepare_call_hierarchy, incoming_calls,
  outgoing_calls}` over `ProjectIndex.calls`, served from the cached round; the
  transport layer only converts positions and maps `SymbolKind`.

## Consequences
Call hierarchy falls out of data the index already collected — a `CallEdge`
vector, two filtering accessors, three thin queries, and three transport
handlers, with no new analysis pass. The graph reflects the last clean round
(§3.2). Method/op/dispatch edges are the known gap, gated on the deferred index
kinds; type-definition/implementation navigation (the other half of A-3) is a
separate increment that needs provider metadata exported into the analysis.
