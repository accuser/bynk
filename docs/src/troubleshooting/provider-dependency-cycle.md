# `bynk.provider.dependency_cycle`

```text
[bynk.provider.dependency_cycle] provider `AImpl` for capability `A` is part of a capability dependency cycle
```

## What it means

Providers depend on other capabilities through `given`, forming a dependency
graph. A **cycle** in that graph — a capability that depends on itself, directly
or transitively — has no valid instantiation order, so it is rejected.

```bynk
provides A = AImpl given B { … }   -- A needs B
provides B = BImpl given A { … }   -- B needs A  → cycle A → B → A
```

The trivial case is a provider listing its own capability:

```bynk
provides Logger = RecursiveLogger given Logger { … }   -- Logger → Logger
```

## Fix

Break the cycle:

- **Extract the shared part** into a third capability that both depend on (a
  common dependency, not a mutual one).
- **Invert one direction** — often one of the two doesn't really need the other;
  remove that `given`.
- **Merge** the two capabilities if they are genuinely one unit.

## Related

- [Compose a provider from other capabilities](../guides/effects-and-capabilities/compose-a-provider.md)
- Reference: [Capabilities & providers](../reference/capabilities.md)
