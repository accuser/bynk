---
level: patch
changelog: A Worker binds only the contexts it calls, so an event subscriber that `consumes` its publisher for the event type no longer forms a Service Binding cycle that `bynk deploy` refuses; a subscriber that also calls its publisher is still refused, now with the cause named
---

## ADR: service-binding-only-when-called
title: A consumed context gets a Service Binding only when it is called
summary: Bind what a Worker calls, not what it consumes; a subscriber that calls its publisher stays a refused cycle, not a two-phase deploy

**Context.** [[0193]] D2 held that `bynk deploy` never meets a Service Binding
cycle: a cycle needs a `consumes` cycle, which `bynk.context.consumes_cycle`
rejects. [[0288]] C broke that premise without saying so. A publisher's Worker
binds to every subscriber so its fan-out can deliver, the reverse of the
subscriber's `consumes` edge, and the subscriber had to `consumes` the
publisher to name the event at all. Owner-only emission ([[0289]]) rules out
putting the event in a third context. So every cross-context subscription
became a two-Worker cycle, and `deploy` refused it (#1961). Cloudflare
resolves a binding at upload and fails a Worker whose bound target does not
exist yet ([[0193]] D1 quotes the documentation), so no upload order fixes it.

Before this change, every consumed context that was not an adapter got a
binding. But a handler reaches a consumed context's Worker only through a
cross-context service call (`Callee::Cross`, lowered to
`callService(deps.env.<BINDING>, …)`). Consuming a context for its types or
events puts nothing on the wire. Its capabilities run in-process (`CrossCap`)
and need no binding either. The subscriber's binding to its publisher was
declared and then never used.

**Decision.** (D1) A Worker binds a consumed context only when some body of it
calls one of that context's services. These are the keys of
`called_cross_context_services`, the same set that already determines the
contracts manifest's `expects`. The binding list is still built in one place
(`worker_bindings::context_bindings`), and `wrangler.toml`, `Env`, the
duplicate check and the integration harness all read it. A handler's
`deps.env` type now names only the contexts that handler calls. That set is
always a subset of the Worker's bindings, so `compose`'s `env` still satisfies
it. With this change, a subscriber that only consumes its publisher binds
nothing back, and the deploy order runs subscriber first, then publisher.

(D2) A subscriber that also calls its publisher (directly or through others)
is still a true cycle. `deploy` refuses it as before, but the error now names
the cause. It names each publisher on the cycle (a Worker declaring the
`__EventsFanout` Durable Object), says that event delivery closes the loop,
and says how to break it: move the called services out of the publishing
context. The events guide documents the limit.

(D3) Two-phase upload is not built. One run would push the publisher without
its fan-out bindings, then the subscriber, then the publisher again. The
fan-out edge only needs the subscriber to exist, so this would work. It still
cuts across the push loop, the ledger's per-context deployed record and its
resumability ([[0180]], [[0233]]), and the generated config, which would need a
second, filtered `wrangler.toml` per phase. It is also the protocol [[0193]]
D2 chose not to build. D1 removes the cycle from every subscription that does
not call back, and that is the only shape the issue and its downstream report
needed. If a real project needs the remaining shape, two-phase upload is the
known escape and builds on this decision.

**Consequences.** Every subscription that does not call its publisher now
deploys. Workers lose bindings they never used, which shows up in generated
`wrangler.toml` and `compose.ts` for any context that consumes without calling.
A Worker already deployed with such a binding drops it on its next push, which
is harmless because nothing read it. `bynk dev` builds from the same binding
list. It never needed the dropped bindings, so its behaviour is unchanged.
[[0193]] D2's premise is amended: the compiler can produce a binding cycle,
but only through event delivery, and `deploy` refuses it by name.

Narrowing the bindings exposed two emit gaps that the unconditional binding
had been hiding. Both are fixed here. First, a call whose receiver is a local
shadowing a consumed-context alias was lowered over the wire, even though the
checker resolves it locally. The lowering now defers to the checker's `Callee`.
Second, a subscriber whose publisher has no callable service never imported
the publisher's type namespace, which its event codec needs. On workers, the
owner of each subscribed event is now imported.
