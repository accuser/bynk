---
level: minor
changelog: A `Matches` refinement that nests unbounded quantifiers is rejected (`bynk.types.catastrophic_regex`) to close a ReDoS hole on the request boundary
---

## ADR: matches-no-nested-quantifiers
title: A `Matches` refinement rejects nested unbounded quantifiers (ReDoS guard)
summary: Why refined-string patterns forbid star height ≥ 2, and how it is detected

**Context.** A refined `String where Matches(p)` compiles to a boundary check
that runs `new RegExp("^(?:" + p + ")$").test(value)` in the emitted Worker
(`bynk-emit/src/emitter/emit.rs`, `lower.rs`). The pattern is string-escaped
(no injection) and validated as syntactically valid at check time against the
same ECMAScript engine (`regress`) the runtime uses, so an invalid pattern is a
compile error, not a runtime throw. But JS `RegExp` is a **backtracking**
engine, and nothing rejected a pattern with catastrophic backtracking. A
pattern that nests an unbounded quantifier inside a repeated group — star
height ≥ 2, e.g. `(a+)+` — takes exponential time on a crafted near-miss input
(`"aaaa…!"`). When such a type sits on an HTTP boundary (a body field or path
param), every request revalidates it, so an unauthenticated client can stall
the Worker with one small string: a denial of service (#724).

**Decision.** Reject a `Matches` pattern that nests unbounded quantifiers at
check time, as `bynk.types.catastrophic_regex`. "Unbounded" is `*`, `+`, or
`{n,}` (open upper bound); `?` and `{n,m}` are bounded and cannot explode.
Detection is a purely structural scan of the already-valid pattern
(`has_nested_unbounded_quantifier` in `bynk-check/src/checker/refinements.rs`):
it tracks quantifier nesting through groups and flags any unbounded quantifier
applied to a sub-expression that itself contains an unbounded quantifier. Inner
unbounded quantifiers propagate up through bounded ones, so `((a+)?)+` is caught
too. The rule is a deliberately conservative approximation — it can reject a
star-height-2 pattern whose sub-expressions provably never overlap — but every
*nested-quantifier* blowup is flagged (see the deferred cases below for what it
does not cover). The diagnostic teaches the author to restructure so no
unbounded quantifier nests inside another.

**Consequences.** A refined-string pattern that could hang a Worker on crafted
input now fails to compile with a spanned diagnostic, closing a common
unauthenticated-DoS class at the source rather than shipping it to the
boundary. This is a language increment: a `Matches` pattern that nests unbounded
quantifiers — vanishingly rare in a real validator, which wants a *linear*
shape anyway — is rejected where it previously compiled.

The guard is intentionally scoped to the **nested-quantifier** subclass
(star height ≥ 2). It does **not** close catastrophic backtracking in general.
Two families are knowingly deferred: (a) *ambiguous alternation* under a single
quantifier — `(a|a)+`, `(\d|\d\d)+`, `(foo|foobar)+` — which is also
**exponential** (EDA: two paths spell the same string, so `2ⁿ` labelings of
`aⁿ` are explored) yet star height 1, so this structural check cannot see it —
detecting it needs branch-overlap analysis; and (b) the lower-severity
**polynomial** class (e.g. `\d*\d*`, quadratic). Both, plus bounding validation
input length, remain available as later defense-in-depth. The emitted check
still reconstructs its `RegExp` per `.of()` call rather than hoisting it to a
module constant (an orthogonal efficiency nit noted in #724); that is deferred.
