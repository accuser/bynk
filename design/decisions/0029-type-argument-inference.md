# 0029 — Type arguments: argument-directed inference + explicit fallback

- **Status:** Accepted (v0.20a)
- **Spec:** §5.2

## Context
Call-site ergonomics want inference; full Hindley–Milner makes errors
non-local and the implementation unbounded.

## Decision
Two-pass, argument-directed unification: non-lambda arguments first (left to
right), lambda arguments after, against the substituted expectations — an
expected lambda *return* variable is captured from the lambda's actual type,
and a fully-annotated lambda may ground variables itself. Conflicting
inferences must agree **exactly** (`karn.generics.type_arg_mismatch`).
Anything undetermined is `karn.generics.uninferable_type_arg`, with
`name[T](…)` as the explicit pressure valve. No inference between lambdas;
none from the call's own expected type; a **bare generic function as a
value** is rejected in v0.20a (instantiation-against-expected is additive
later).

## Consequences
Errors stay local and predictable; the stdlib's map/filter/fold/traverse
shapes all infer; the dumb-but-explicit design resists creeping toward HM.
