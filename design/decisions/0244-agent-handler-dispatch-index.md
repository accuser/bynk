# 0244 — Agent handler dispatch calls become index symbols

- **Status:** Accepted (v0.215)

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
