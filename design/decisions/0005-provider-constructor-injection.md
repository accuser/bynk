# 0005 — Provider composition is constructor injection in topological order

- **Status:** Accepted (v0.12)
- **Spec:** §7.3.6 (compose), §5.5

## Context
A provider with `given` must still conform to its capability's interface, so
dependencies cannot be threaded as extra method parameters.

## Decision
A provider with `given` gains a **constructor taking a by-name deps object**;
its bodies lower capability calls to `this.deps.<Cap>`. The composition root
instantiates providers in **topological order** of the dependency graph.
Handler call sites are unchanged.

## Consequences
Capability interfaces stay exact; `tsc --strict` checks the deps object keys.
This same wiring later carried cross-context capabilities (0008) and adapter
bindings (0019) without structural change.
