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
added to `worker_bindings` alone, for `wrangler.toml`, `Env` and the check.
This replaces ADR 0462's note that it must also be added to `Env`. The types
`Env` names (`__KVNamespace`, the local `__DurableObjectNamespace` alias) are
declared when the list holds such a binding, so they come from the list too.

The handler-facing `deps.env` type (`workers_env_ty` in `emitter::emit`) is
separate and still built from consumed contexts and local agents. A new kind of
binding that handlers must reach needs adding there as well.

**Consequences.** A publishing context's `Env` gains a member for each
subscriber's Service Binding. The other members keep their names. The events
fan-out's member now sorts among the agents by class name, as in
`wrangler.toml`, rather than always last; it still comes last unless an agent's
name begins with a lowercase letter. A unit test pins the order and that `Env`
lists the `wrangler.toml` bindings. Whether `env` is passed into a handler's
deps is still decided by consumed contexts and agents, not by this list, so the
change does not alter what handlers see.

`bynk.context.duplicate_binding` is now needed for valid TypeScript as well
as a valid `wrangler.toml`: `worker_bindings` can return two bindings with one
name, which would be two `Env` members with one name. The check runs before
emission and stops the build, so it cannot be relaxed without handling that.
