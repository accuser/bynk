---
level: patch
changelog: A Worker's `Env` interface now declares exactly the bindings in its `wrangler.toml`, including the Service Bindings to its event subscribers
---

## ADR: worker-env-types-the-binding-list
title: A Worker's Env interface is built from its binding list
summary: Env, wrangler.toml and the duplicate-binding check share one list per Worker; amends ADR 0462's two-places note

**Context.** ADR 0462 made `emitter::wrangler::worker_bindings` the one list a
Worker's `wrangler.toml` is written from and `bynk.context.duplicate_binding`
checks. The Worker's TypeScript `Env` interface (`emitter::workers`) still
derived the same names itself, from consumes less adapters, the KV flag and the
agents, so ADR 0462 noted that a new kind of binding had to be added in both
places. The two lists had already drifted: a publishing context's
`wrangler.toml` binds each subscriber's Worker (the reverse edge the events
fan-out uses), but its `Env` did not declare those bindings. The fan-out types
its own `env` loosely, so nothing failed, but the interface was incomplete.

**Decision.** The Workers build computes each Worker's binding list once and
passes it to both `emit_wrangler_toml` and `emit_worker_compose`. `Env` has one
member per binding, in the list's order, typed by the binding's source: a
Service Binding is `__ServiceBinding`, the KV namespace is `__KVNamespace`,
and an agent or the events fan-out is `__DurableObjectNamespace`. The
duplicate check builds its list with the same helper. A new kind of binding is
added to `worker_bindings` alone. This replaces ADR 0462's note that it must
also be added to `Env`.

**Consequences.** A publishing context's `Env` gains a member for each
subscriber's Service Binding. The other members keep their names. The events
fan-out's member now sorts among the agents by class name, as in
`wrangler.toml`, rather than always last; it still comes last unless an agent's
name begins with a lowercase letter, and no fixture moves. Whether `env` is passed into a handler's deps is
still decided by consumed contexts and agents, not by this list, so the change
does not alter what handlers see.
