---
level: minor
changelog: The held-resource linearity pass now governs `match`-arm pattern bindings — a `Connection` bound out of an `Option`/`Result` in a match arm must be disposed, closing a leak the pass missed (#719)
---

## ADR: match-arm-held-linearity
title: The held-resource linearity pass governs match-arm pattern bindings
summary: A held value bound out of a `match` arm pattern is registered as owned and must be disposed

**Context.** The held-resource linearity pass ([ADR 0130](../decisions/0130-held-resource-linearity.md))
seeds owned held bindings from handler/function **parameters** and tracks the
held bindings introduced by **`let`** / **`let <-`**. Nested payload patterns
([ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md))
let a `match` arm bind a held value straight out of its wrapper —
`Some(conn)` over `Option[Connection[F]]`, `Ok(conn)` over
`Result[Connection[F], E]` — but the pass never registered those arm bindings.
The discriminant walk consumes the scrutinee, the pass is satisfied, and the
real connection the arm extracts escapes the disposal check entirely:

```
let c <- Gateway.accept()          -- Effect[Option[Connection[F]]]
match c {
  Some(conn) => Effect.pure(()),   -- `conn` never disposed, no leak reported
  None => Effect.pure(())
}
```

Because shadowing an outer binding is legal (the resolver only rejects
shadowing types/fns), the gap had a second face: an arm binding named for an
outer held binding misattributed the arm's `close`/transfer to the *outer*
value, producing both false negatives and spurious `use_after_consume` /
`branch_divergence` errors. Reported as #719.

**Decision.** Each `match` arm registers the **held-typed** identifiers its
pattern binds (recursing through nested payloads) as `Owned` for that arm's
scope, and the arm must dispose them — `close`, store, or transfer — exactly as
a `let`-bound held value must. An arm binding is scoped to its arm: it is
leak-checked at the arm's end and, when it shadows an outer held binding, the
outer binding's state is saved before the arm and restored after, so a consume
of the inner value is never misattributed to the outer one and the two are
unified independently across arms. The checker records each pattern binding's
resolved type at its identifier span (patterns are not expressions, so this is
their only entry into `expr_types`), and the linearity pass reads that to decide
which arm bindings are held — reusing the checker's own type derivation rather
than re-deriving payload types.

**Consequences.** A held value reached through a `match` arm is now governed by
the §2.9 discipline identically to a `let`-bound one — the leak above is
reported (`bynk.held.leak`), and the shadowing misattribution is gone. This
tightens the accepted-program set: a program that leaked a match-bound
connection compiled before and is now correctly rejected. No grammar, runtime,
or diagnostic-vocabulary change — the fix reuses `bynk.held.leak` and the
existing branch-unification machinery.
