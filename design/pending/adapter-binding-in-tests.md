---
level: minor
probes: ts_writes
changelog: An `as integration` case runs a consumed adapter's real binding, its own `given` capabilities taking the tier's doubles and stubs; a `suite` on an adapter runs the binding at every tier; and a case can call a capability it reaches directly (`let s <- Greeter.hello("ann")`)
---

## ADR: adapter-binding-runs-in-tests
title: An adapter's binding runs in an integration case and in a suite on the adapter
summary: Integration cases build consumed adapters' bindings; adapter suites test the binding; a case's capability call reaches its deps

**Context.** A test module imported no binding module, so every external
provider in `__makeTestDeps` was a placeholder (#1863). An `as integration` case
in a context consuming an adapter therefore had no provider for the adapter's
capability and faulted at run time, though the wrap-a-library guide promised
that promoting the case runs the real binding (#1975). A `suite` on an adapter
passed `bynkc check` and then emitted `/* unknown */.op(…)`: the checker puts the
target's capabilities in scope in a case body, but the lowering's
capability-call branch reads a handler's `given` set, which a case body does not
have. A direct capability call in a context's case failed the same way.

**Decision.** A context's test module gains a second factory,
`__makeIntegrationDeps`, used by its `integration` cases when it differs from
`__makeTestDeps`. It builds each consumed user adapter's provider from the
adapter's binding module, imported as the composition root imports it
(`{ns}__binding`). The binding's own `given` is the deps' entry when the target
reaches that capability too, so a case's `stub` on it applies. Otherwise it is
the capability's deterministic test double, or the placeholder when it has
none. A first-party adapter's binding is never built in a test: it reaches the
platform. A suite on an adapter builds the adapter's capabilities from its
binding in `__makeTestDeps`, since the binding is the unit under test, with its
`given` built the same way. A capability call in a case body lowers to
`deps.Cap.op(…)` when the checker recorded it as one.

**Consequences.** The `unit` tier is unchanged: a consumed adapter's capability
stays a seam that a `stub` answers. A module with no integration case, or with
no adapter binding to build, emits no second factory, so existing output does
not move.
