---
level: patch
changelog: The lowering pass returns hoisted statements instead of writing them into a caller-supplied sink, deleting the predictive classifier that gated the ternary-form `if`
closes_rule: R6.2
---

T2.1 (#1017) finishes the signature migration [#955][pr955] started. `lower_expr`
returns `Lowered { pre, expr }` at every site in `bynk-emit`; the sink-passing
`lower_expr_into` and its `stmts: &mut Vec<String>` parameter are gone, and the
`hoist_sinks` probe reads 0.

The consequence R6.2 asked for is that a lowering function can no longer hand
statements to its caller by any route other than its return value. Concretely,
that retires `simple_expr` — the hand-maintained classifier that predicted, before
lowering, whether a branch would hoist, so the ternary-form `if`/`else` could skip
the hoist-safe wrapper. The ternary path now lowers each branch and reads
`Lowered.pre`, which cannot disagree with what lowering actually did. The
`debug_assert!(stmts.is_empty())` pair that stood in for the guarantee the
classifier could not make is deleted with it.

The classifier's blanket `_ => true` arm was live, not theoretical: `ListLit`,
`InterpStr`, `RecordSpread`, `EffectPure`, `Val` and `Wire` all thread
sub-expressions through the hoist path, and all fell through it. `let xs: List[Int]
= if c { [risky()?] } else { [] }` panicked the ternary path's own `debug_assert!`
in debug builds and emitted a reference to an undeclared `__r0` in release builds.
Fixture `1017_ternary_branch_hoist_fallthrough` closes it.

Threading also closes the `if`-expression half of R6.6's residual wrapper gap. A
value-position `if` whose branch hoists used to be wrapped in `(() => { … })()`,
which makes a hoisted `?`'s early return exit the arrow rather than the enclosing
function — `tsc --strict` rejects the result. `lower_if` now carries its own `pre`,
so the `if` hoists as a real statement in the caller's statement position. The
matching gap in `lower_bin_op`'s short-circuit right operand does **not** close:
that hoist must be *skipped* when the operator doesn't reach it, and a statement
cannot be conditionally skipped, so the arrow there is load-bearing. It stays open
and is named in the code.

Making the `if` hoist for real moved three things that only the new statement shape
could reach, each closed here with a named fixture. The declared error embedding
(ADR 0178) must still apply to a `?` inside a hoisted branch, because its `return`
now exits the enclosing function — so `lower_if` no longer clears `return_ty` the
way the arrow path must. The slot a hoisted `if` assigns to needs the *unwrapped*
type in async-tail position, where `Effect.pure(x)` emits a bare `x`. And an agent's
static field initialiser now wraps a hoist in an IIFE rather than splicing it into a
comma sequence, which only ever parsed for expression-shaped hoists.

No `.bynk` surface, grammar, checker, or runtime change. Every one of the 383
positive fixtures — including the four regression fixtures for the defects R6.2
names (`945`–`948`) — reproduces byte-identically.

[pr955]: https://github.com/accuser/bynk/pull/955
