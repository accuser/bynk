---
level: patch
changelog: A `system`-tier suite stands up every subscriber to a participant's events and backs `bynk.cloudflare`'s `Kv` with one in-memory namespace per case, so an emitted event is delivered and a `Kv` call works instead of faulting
---

## ADR: system-harness-binds-the-worker-bindings
title: A system suite's harness binds each participant's own Worker bindings
summary: Each participant's env is built from its binding list; subscribers are stood up as participants; one in-memory KV namespace per case is shared by all

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
- The KV namespace is **one in-memory namespace per case, shared by every
  participant that binds it**, as a project's Workers share one namespace when
  deployed. It is made fresh with the harness, so a case starts empty, like
  agent state.
- Each agent and the events fan-out is an in-memory Durable Object namespace,
  constructed with the participant's `env` as workerd constructs it
  (`makeIntegrationDoNamespace(class, env)`).

The harness stands up **every context subscribing to a participant's events,
and its own `consumes` closure, transitively** (`infer_harness_participants`),
so the fan-out delivers to real subscriber Workers across the real wire. Only
the harness widens: a case still reaches the target's `consumes` closure alone
(`infer_participants`), and is checked against that view.

**Consequences.**

- A case can observe an event's effect: fixture 1959 has a subscriber write to
  KV on delivery, and the case reads the value back through another
  participant.
- A suite over a publisher stands up more Workers than before.
- `expirationTtl` is accepted and ignored by the in-memory namespace, and
  `list` returns one page.
- Existing integration fixtures change only in how agents are constructed:
  the class and `env` are passed instead of a `state`-only closure.
