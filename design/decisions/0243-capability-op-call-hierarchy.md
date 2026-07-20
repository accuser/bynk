# 0243 — Capability-op callees join the call-hierarchy graph

- **Status:** Accepted (v0.215)

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
