# 0079 — Protocols are a closed set; transports are open

- **Status:** Accepted (v0.44)
- **Spec:** `lexical-grammar.md` (reserved `protocol`), `static-semantics.md` (`unknown_protocol`)

## Context

Moving the protocol onto the header (0077) raised: where is `http` defined, and
may a developer write `from kafka`? Adapters are open (libraries are unbounded),
so a parallel openness for protocols is the obvious question.

## Decision

Protocols stay a **closed, compiler-known set** — no `protocol` declaration kind
in v0.44. An adapter is *outbound* (the app calls a set of functions; openness
earns its keep). A protocol is *inbound* — a driver that owns dispatch,
lifecycle, boundary validation, actor verification, and atomicity-at-the-edge —
far too sharp a surface to hand to a TS binding, and the service-tier shapes are
a near-complete cover. The thing that genuinely varies — transport and codec —
is already openable via adapters, where a Kafka/MQTT *transport* binds.

`from kafka` / `from mqtt` are therefore rejected with a fix-it
(`bynk.service.unknown_protocol`): they are transports, not protocols (use
`from queue`, with the broker bound at the platform layer). The `protocol`
keyword is **reserved** so the door stays open; if ever opened, `protocol`
should pair a baked-in lifecycle shape with an adapter-supplied transport, not
author a driver from scratch.

The three http/cron/queue parser+checker arms collapse to **one protocol
descriptor** (shape constraints + per-protocol param obligations + return type +
emitter), sealed but uniform — so a future opening is new *surface* only, not a
re-architecture, mirroring the `bynk`-surface treatment of capabilities.

## Consequences

The closed set keeps dispatch total and the boundary trustworthy; opening later
is widening the descriptor, not adding a declaration kind. Each migrated obligation
(`bynk.{http,cron,queue}.*`) is preserved by the descriptor and pinned by its
existing negative fixture.
