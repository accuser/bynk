---
level: patch
changelog: A handler's Workers `deps.env` type is built from the Worker's binding list and names only consumed contexts with services, so a bare `consumes bynk` beside a cross-context call no longer fails `tsc --strict`
---

## ADR: handler-env-projects-the-binding-list
title: A handler's deps.env is a projection of the Worker's binding list
summary: deps.env takes its bindings from the constructors worker_bindings uses, filtered by kind; amends ADR 0464's separate-deps.env note

**Context.** ADR 0464 made one list per Worker (`worker_bindings`) the source
of its `wrangler.toml`, its `Env` interface and the duplicate-binding check. It
recorded one exception: the type of a handler's `deps.env` on Workers
(`workers_env_ty`) still derived binding names itself, so a new kind of binding
handlers reach had to be added there too. That type cannot simply be read from
the list. The handler is lowered while its unit is checked, before the list
can be built: the list needs the checked callees (whether the context emits)
and the KV closure, which come later.

**Decision.** Every `WorkerBinding` is built by one constructor per kind next
to `worker_bindings`. `handler_bindings` builds `deps.env`'s bindings with the
same constructors: one Service Binding per consumed context with services, then
every local agent's Durable Object binding when the handler constructs an
agent. It is a projection of the list by kind. The KV namespace (reached
through the `WorkersKv` provider), the events fan-out (through
`deps.__eventsDispatch`) and subscribers' Service Bindings (called only by the
fan-out) never reach handlers. `BindingSource`'s variant docs record this per
kind. Each binding's TypeScript type comes from one mapping
(`binding_ts_type`), shared with `Env`.

The projection is a subset of the list by construction: a consumed context
with services is never an adapter (`bynk.adapter.disallowed_item` rejects a
`service` in one), so the Worker has its Service Binding, and every local
agent has its Durable Object binding. Unit tests pin the subset, and
`tsc --strict` over every Workers fixture enforces it in emitted output,
because the compose module passes `Env` where `deps.env` is expected. A new
kind of binding is added as a constructor and in `worker_bindings`. If handlers
reach it directly, it is added to `handler_bindings` too, and its variant doc
says so.

`deps.env` keeps a consumed unit exactly when it has services: those are what
a handler calls through `env`. The old filter dropped a unit only when it was
consumed in the selecting form (`consumes bynk { Clock }`) *and* had no
services. So a bare `consumes bynk` beside a cross-context call put
`BYNK: __ServiceBinding` in `deps.env`, a binding the Worker never has, and the
compose module failed `tsc --strict` (review of #1951).

**Consequences.** A handler's `deps.env` no longer names a consumed context
with no services (fixture 1820's `SHOP_FEES`, consumed only for a
capability) or an adapter consumed bare. Handlers never read either: a
consumed capability is provided in-process by compose. The Worker's `Env` and
`wrangler.toml` are unchanged. Fixture 1951 pins the bare-adapter case through
`tsc --strict`. Places that read a binding at
runtime (`env.X` in handler and compose bodies, the fan-out's routing table)
still call the name functions directly. They are values, not the `deps.env`
type, and use the same functions the constructors call.
