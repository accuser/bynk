# 0034 — Collections: thin built-in kernel, Karn-written combinator stdlib

- **Status:** Accepted (v0.20b)
- **Spec:** §5.10, §6.2, §8.4

## Context
v0.20a shipped first-class and generic functions with no real consumer.
Collections need both a compiler-known core (Karn has no loops, so iteration
cannot be user-written) and a combinator library (`map`/`filter`/`find`/
`traverse`/…) that would be pure boilerplate as compiler magic.

## Decision
Split the surface in two. Only the **irreducible primitives** are
compiler/emitter built-ins (the kernel, 0036); **everything derivable is
ordinary Karn** in the first-party `karn.list`/`karn.map` commons (0037),
written over the kernel using v0.20a generics, lambdas, and effectful
traversal. The stdlib is the first real consumer of the functional core —
its compilation is a standing proof that generic functions, closures over
capabilities, and `<-` confinement compose.

## Consequences
Dogfooding immediately surfaced (and fixed) three v0.20a gaps: type
parameters now resolve in body `let` annotations, an expected lambda return
carrying only the enclosing fn's *rigid* type variables counts as
constrained, and a generic fn calling another generic fn may instantiate the
callee at its own rigid vars. The stdlib can grow without compiler changes;
a wrong kernel primitive is the one thing that would force combinators back
into the compiler (0036 records the shape that prevents this).
