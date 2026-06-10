# 0004 — State initialisers are a closed static-expression set

- **Status:** Accepted (v0.11)
- **Spec:** §5.4

## Context
A state initialiser must produce a value before any handler runs — it *is* the
fresh state — so it cannot depend on runtime input.

## Decision
An initialiser is a **static expression**: compile-time literals (per decision
0001), value constructors over static arguments (sum variants, `Ok`/`Err`/
`Some`/`None`, record literals), and opaque/refined construction from a static
literal. Not `self`, parameters, capabilities, binds, or free functions.

## Consequences
The emitter lowers initialisers into the zero-value factory; state creation is
deterministic and effect-free. Effect-derived initialisers remain a deferred,
explicitly separate feature.
