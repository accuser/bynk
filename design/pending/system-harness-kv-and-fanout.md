---
level: patch
changelog: A `system`-tier suite stands up every subscriber to a participant's events and gives each participant its own in-memory `Kv` namespace per case, so an emitted event is delivered and a `Kv` call works instead of faulting
---

## ADR: system-harness-binds-the-worker-bindings
title: A system suite's harness binds each participant's own Worker bindings
summary: Each participant's env is built from its binding list, with its own in-memory KV namespace; subscribers are stood up as participants

**Context.** A `system`-tier suite stands its participants up as in-process
Workers, each with a hand-built `env`. That `env` held Service Bindings to
consumed participants and in-memory Durable Object namespaces for agents, and
nothing else the Worker's `wrangler.toml` declares. A participant that emitted
had no events fan-out: dispatch failed, the error was logged and swallowed,
and the case passed with no subscriber run. A participant using
`bynk.cloudflare`'s `Kv` had no `env.KV` and faulted. Agents were also built
without `env`, so an agent that emits or rebuilds `given` deps from `env` hit
the same gaps. Participants are the target's `consumes` closure. A subscriber
consumes its publisher, and `consumes` cycles are refused, so a subscriber to
the target was never a participant.

**Decision.** Each participant's `env` holds the bindings its own Worker
declares (`all_context_bindings`, the list its `wrangler.toml`, `Env` and the
duplicate check use), by the same names:

- A Service Binding forwards to that participant's Worker.
- The KV namespace is **a fresh in-memory namespace of the participant's
  own**, as `bynk deploy` provisions one namespace per Worker (the ledger's KV
  entries are keyed by Worker). It is made with the harness, so a case starts
  empty, like agent state. Sharing one across participants would let a case
  pass that deploy fails (review of #1964).
- Each agent and the events fan-out is an in-memory Durable Object namespace,
  constructed with the participant's `env` as workerd constructs it
  (`makeIntegrationDoNamespace(class, env)`).

The harness stands up **every context subscribing to a participant's events,
and its own `consumes` closure, transitively** (`infer_harness_participants`),
so the fan-out delivers to real subscriber Workers across the real wire. Only
the harness widens: a case still reaches the target's `consumes` closure alone
(`infer_participants`), and is checked against that view.

**Consequences.**

- A case can observe an event's effect wherever the subscriber records it in a
  context the case reaches. In fixture 1959, `shop.audit` subscribes to events
  from a handler and from an agent of `shop.order` and records each in
  `shop.ledger`. The case reads the records back.
- A suite over a publisher stands up more Workers than before.
- The in-memory namespace accepts and ignores `expirationTtl`, and `list`
  returns one page, keys in JavaScript string order.
- Existing integration fixtures change only in how agents are constructed:
  the class and `env` are passed instead of a `state`-only closure.
