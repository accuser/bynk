# 0292 — The envelope's `publisherId` is the emitting context, not the emitting agent, amending design/bynk-design-notes.md §7

- **Status:** Accepted (v0.240)

**Context.** `design/bynk-design-notes.md` §7 (lines 229–234) asserts "the
publisher is the emitting agent, not the bounded context — agents are the
unit of state and effect, so they are the unit of ordering," and shows the
envelope example as `env.publisherId` alongside that claim. This does not
hold uniformly against the shipped surface: `Events.emit[E](event)` is also
legal from a plain, keyless `service` handler with no agent instance at all
— confirmed by the existing owner-only negative fixture
`bynkc/tests/fixtures/negative/503_events_emit_outside_owner` (a bare
`service leak { on call() -> Effect[()] given Events { Events.emit[...] } }`,
negative only because of the *owner* check, not the handler shape) and its
agent-handler analogue at `953_agent_handler_event_emit_outside_owner`. A
plain service handler has no `self`, no key field, and no DO instance — at
that call site there is no agent to be "the publisher."

Agent self-identity does exist as a real, narrow, already-shipped mechanism
(`self.<keyField>` lowers to `this.state.id.toString()` inside an agent
handler, `bynk-emit/src/emitter/lower.rs`'s agent-handler `self` rewrite),
but it is unavailable in a plain service handler, and a per-agent-instance
`publisherId` would additionally invite a subscriber to correlate an
ordering guarantee across an agent's own emissions that §3.4's own ADR
(`events-per-publisher-fifo-scope`) already measured as **not** holding
across concurrent invocations of one agent — advertising an identity that
implies a false guarantee is worse than not advertising one.

**Decision.** `publisherId: String` is the emitting *context's* fully
qualified name (e.g. `"commerce.order"`) — a compile-time-known string,
identical for every invocation regardless of whether the emission happened
inside an agent handler, a plain service handler, or a composed provider
body, since `HandlerShared.owning_context` (a new field threaded alongside
the existing `handler_scope`, `bynk-emit/src/emitter.rs`) is populated
identically at every one of those construction sites. This amends §7's
"the publisher is the emitting agent" framing to "the publisher is the
emitting context" — a smaller, honest guarantee that matches what's
actually knowable uniformly, rather than one that only works for the agent
case and silently omits the service case.

**Consequences.** A subscriber that needs per-agent-instance correlation
(e.g. detecting that two emissions came from the *same* `Ledger` instance,
not just the same context) has no support from `publisherId` alone — it
needs its own identifier carried in the event payload itself, the same
constraint §6's ordering-guarantee note already places on cross-publisher/
concurrent-invocation ordering. Not a regression: no shipped surface before
this slice exposed publisher identity at all.
