# 0285 — Events joins the closed protocol set; its header is the first to take a type argument

- **Status:** Accepted (v0.237.1)

**Context.** The service protocol set is closed
([ADR 0079](../decisions/0079-protocols-closed-set.md)), so admitting
`from Events(...)` (the Events track, spine #936) is a deliberate extension of
that set, not a neutral addition. `from <protocol>` on the service header
already ships ([ADR 0077](../decisions/0077-service-protocol-on-header.md)),
and `from WebSocket` already proves a `from`-protocol carrying a bundle of
associated surface — but no shipped protocol today takes a type argument on
the header.

**Decision.** Events becomes the service protocol set's sixth member.
`from Events(E)` parameterises the protocol by the event type `E` the service
subscribes to — the event type is the topic, not a string name — reusing the
existing protocol-header grammar position rather than inventing a new one. The
`on event` handler shape joins the per-protocol handler-shape checking that
already rejects a mismatched handler for every other protocol (e.g. a queue
handler returning a `Response`).

**Consequences.** Membership in the closed set is a one-line extension; the
genuinely new slice-0 grammar/checker work is the type-argument
parameterisation itself, which no prior protocol needed. A later slice adds a
structural pattern alongside the type argument
(`events-pattern-dispatch-deliver-and-filter`, this same pending file); that
pattern is additional refinement on top of this header shape, not a second
extension of the closed set.
