# 0305 — The wire-contract peek covers HTTP and cross-context `on call` handlers; agent/queue/WebSocket handlers are deferred

- **Status:** Accepted (v0.246)

**Context.** A Bynk project crosses a trust boundary at several sites: an
HTTP route, a cross-context `on call`, an agent's own `on call` (a
Durable-Object RPC boundary), a queue `on message`, and a WebSocket
`on open`/`on close`. The issue's worked examples name exactly two of these —
an HTTP route (the rate-limiter's `GET /check/:client`) and a cross-context
`on call` — and `bynk_check::wire::WireModel` (the shared IR both the codec
and the peek render) makes no distinction between boundary kinds: it derives
purely from a handler's params/return type and the type table, so nothing in
the IR itself limits which handler kinds could show a peek.

**Decision.** `bynk_ide::wire_contract` is scoped to **service handlers
only**. An agent's `on call` handler crosses a boundary too, but
`ContextBoundaryInfo` (the retained per-round table this module reads) does
not retain agents' own handler bodies in a form this module needs, and
extending it was out of scope for getting the issue's two worked examples
correct end-to-end. `BoundaryKind` (the peek's own protocol-kind enum) already
carries `Cron`/`Message`/`Open`/`Close`/`Event` variants alongside `Http` and
`Call` — a handler of any of those kinds gets a wire-contract model (request
envelope, boundary types), just never a `NoCrossContextReason` distinct from
`NotACallHandler`/`SingleContext`, since only `on call` has a cross-context
contract form at all.

**Consequences.** An agent-handler peek, and any queue-/WebSocket-specific
framing the panel might eventually want (e.g. a queue's redelivery semantics
rendered beside the envelope), is left for a later slice if requested — the
IR underneath does not need to change to add one, only
`bynk_ide::wire_contract`'s handler-resolution walk (`wire_contract_at`,
currently `CommonsItem::Service` only) and the LSP's header-rung guard
(`hover.rs`'s `header_handler_at`, likewise service-scoped).
