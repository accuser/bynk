# 0077 — Service protocol moves to the header: `service <Name> from <protocol>`

- **Status:** Accepted (v0.44)
- **Spec:** `syntactic-grammar.md §4.4`, `static-semantics.md` (service rules)

## Context

Through v0.43 the protocol lived **per handler** — `on http GET "/x"`, `on cron
"…"`, `on queue "…"` — while the design notes (*Services and Protocol
Composition*) frame a service as conforming to a single **protocol** declared on
the service, with the transport a separate platform concern. The per-handler
form also gave HTTP two bare config literals (verb *and* route), breaking the
"one config literal per handler" rhythm exactly once, and left the
yet-to-be-built Events surface (`from events(…)`) with nowhere to put its
parameterised subscription.

## Decision

The protocol moves onto the service header: `service <Name> from <protocol> { … }`,
one protocol per service. The keyword is **lowercase** (`from http`/`from cron`/
`from queue("name")`) to signal a sealed keyword, not a resolvable name. Config
splits by **cardinality**: a protocol that binds a single named transport
resource (queue name, event type) carries the binding on `from` and one handler;
a protocol that addresses many endpoints (HTTP routes, cron schedules) carries
the bare protocol on `from` and the endpoint on each handler via a **builder** —
`on GET("/route")` (reusing the `http_method` verb set as pure handler-config
grammar, denoting no value) and `on schedule("expr")`; queue handlers are
`on message(m: T)`. A service with **no** `from` clause is the contract-mediated
internal-RPC default and admits **only** `on call`; mixing a wire protocol with
`on call`, or putting a wire handler on a `from`-less service, is rejected
(`bynk.service.{mixed_protocols,missing_from}`). This supersedes the design
notes' provisional handler surface (capitalised `from HTTP`, retained
`on POST "/route"`), a revision the notes invite.

## Consequences

A reader scanning a long body must look to the `from` header to know the
protocol — the deliberate DRY win, paid for with one indirection. The emitted
Worker topology is unchanged (HTTP/cron golden output is byte-identical); only
the source surface moves. The three handler productions collapse to one protocol
descriptor (see 0079), and the header shape generalises to the unbuilt
Events/Alarm/WebSocket protocols.
