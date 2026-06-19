# 0011 — `consumes U { Cap, … }` flattens selected capabilities; clashes are rejected

- **Status:** Accepted (v0.17)
- **Spec:** §4.1.17, §5.8

## Context
Consuming an adapter's capabilities through qualified `given U.Cap` everywhere
is noisy; the common case wants bare names. The mixin edge had to be `consumes`
(the effectful edge), not `uses` (pure vocabulary).

## Decision
The braced **capability-selection** form flattens each selected name into the
consumer's local capability namespace. Flattening is **general** (any exporting
unit, not adapter-only). A bare-name collision — with a local capability or
another flattened name — is **rejected** (`bynk.consumes.capability_name_clash`),
resolved by the qualified form or an alias.

## Consequences
`given Clock` / `Clock.now()` read identically for local and consumed
capabilities. The flattening map became the resolution path adapter
dependencies (0019) recurse through.
