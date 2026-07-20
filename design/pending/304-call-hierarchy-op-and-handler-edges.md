---
level: minor
changelog: Call hierarchy now records capability-op and agent-handler-dispatch call edges, closing the under-reporting gap in #304
---

## ADR: capability-op-call-hierarchy
title: Capability-op callees join the call-hierarchy graph
summary: Reverses ADR 0069's "stays out of scope" call for capability-op call edges

**Context.** ADR 0069 (slice 2) indexed capability ops as first-class symbols —
references, rename, goto-def, and semantic tokens all work — but deliberately
excluded them from the call-hierarchy graph, on the reasoning that "call hierarchy is
the fn/method relation; op dispatch is a different (effectful capability-use)
relation." Issue #304 named the user-visible cost of that scoping: a capability op
becomes a callee whose reference count exceeds its incoming-call count, and the
handler that calls it appears in outgoing-calls to call nothing — reading as a bug,
not a deliberate boundary.

**Decision.** Widen `IndexBuilder::build`'s callee-kind filter
(`bynk-check/src/index.rs`) to include `SymbolKind::CapabilityOp` alongside `Fn` and
`Method`. No new machinery: both the local (`Cap.op(...)`) and cross-context capability-op
call sites already record a ref with the correct enclosing `owner` (ADR 0069), and
`owner_keys` is already populated for every `add_def`'d symbol regardless of kind — so,
exactly as ADR 0069 predicted for methods, the edges "join the graph for free" once the
filter admits the kind.

**Consequences.** A capability op and its call sites are now call-hierarchy-navigable
in both directions, and the reference-count/incoming-call inconsistency #304 flagged is
gone for this kind. Service cross-context dispatch callee edges remain a separate,
unaddressed gap (see the sibling ADR in this file) — a service has no per-handler name
to index, unlike a capability's ops or an agent's handlers.

## ADR: agent-handler-dispatch-index
title: Agent handler dispatch calls become index symbols
summary: New SymbolKind::Handler indexes agentInstance.handler(...) calls, following ADR 0069's compound-name convention

**Context.** Of the three callee kinds issue #304 named as missing from call
hierarchy (method, capability-op, dispatch), agent-handler dispatch
(`agentInstance.handlerName(args)`, checked in
`bynk-check/src/checker/calls.rs`'s agent-dispatch branch) was the only one with *no*
index presence at all — not a reference, not a symbol, nothing. Every other member
kind (method, field, capability op) had already been lit up by ADR 0069; this was the
one genuinely unbuilt gap the issue's "gated on... extend the binding index" language
described.

**Decision.** Add `SymbolKind::Handler`, compound-named `"Agent.handler"`, following
ADR 0069's convention exactly: `bynk-emit/src/project/symbols.rs` `add_def`s one per
agent handler at the project walk (`Handler.method_name`, which is `Some` for agent
handlers and always `None` for service handlers — so this is naturally agent-only,
no extra filtering needed); the checker's dispatch branch records a ref at the
resolved call site, in the same position (after the handler resolves, before arity
checking) the ordinary instance-method call already uses; and `Handler` joins the
same call-edge callee filter as the sibling ADR's `CapabilityOp` change.

**Consequences.** Agent-handler dispatch calls are now fully navigable —
references, rename, goto-def, semantic tokens, and call hierarchy all follow from the
one `ctx.refs.record` call, the same "joins the graph for free" pattern ADR 0069
established for methods. This closes the last named gap in #304. Service-handler
dispatch has no per-handler name to index (a service exposes one dispatchable
`on call`) and remains a separate, unaddressed gap — worth its own future issue if a
per-service-handler relation is ever wanted, but not what #304 asked for.
