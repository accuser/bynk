# 0474 — An agent reaches consumed services and other agents through its Durable Object's own env

- **Status:** Accepted (v0.333.1)

**Context.** An agent handler could call a consumed context's service, as
`consumes` allows, but its body was always lowered for the bundle target: on
Workers it read `deps.surface.<Alias>.<service>(…)` while its `deps` type and
value carried only `env`, so every call faulted. Lowering it for the Workers
target alone is not enough. An agent's `deps` reach it over the Durable
Object's JSON wire, and a Service Binding or a Durable Object namespace does not
survive serialisation: inside the DO, the caller's `deps.env.<BINDING>` is a
dead plain object. A second shape had the same root: an agent handler that
constructed another agent built it from the isolate-local bundle registry, not
the agent's Durable Object.

**Decision.** An agent handler's body lowers for the build target, like a
service handler's, so a cross-context call is `__callService(deps.env.<BINDING>,
…)` and an agent construction is `__makeX(key, deps.env)`, which is what its
`deps` type already declared. An agent whose handlers do either takes the
Worker `env` in its constructor, and its fetch dispatch puts that live `env`
into the handler's `deps` over the wire's copy, as it already rebuilds `given`
providers and `__eventsDispatch` (#527). The DO class lives in the Worker
script that declares the agent, so its `env` already holds every binding the
handler reads. We considered giving agents the composed cross-context
`surface` instead. We rejected it because `env` is how Workers deps are already
typed and threaded, and a composed surface would cross the same wire and die
the same way.

In bundle mode, a handler that constructs an agent in a context that consumes
services now carries `surface` in its `deps` type, since the agent method it
calls may read it. Workers mode already threads the whole `env` in that case.

**Consequences.** An agent can call a consumed context's service and construct
another agent on Workers. The agent it constructs is the durable one, shared
with every other caller. A bundle handler that calls an agent may name
`surface` in its `deps` type even when the agent does not use it. This is
harmless, because compose always passes the full deps.
