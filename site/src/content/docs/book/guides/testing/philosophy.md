---
title: The testing philosophy
---
Testing is built into Bynk rather than bolted on: `suite`/`case` blocks, `expect`,
`property`/`for all`, `Val[T]`, and `mocks` are language constructs. This page
explains why they exist in the form they do.

## One predicate surface

Bynk already has a way to state a checked claim: an **invariant** is a pure `Bool`
predicate — `is`, `implies`, the operators, pure methods — enforced at the commit
boundary. A test's `expect` is *that same predicate*, aimed at a value instead of a
committed state:

```bynk
expect    balance >= 0          -- in a case
invariant nonneg: balance >= 0  -- on an agent
ensures   nonneg: result >= 0   -- on a function
```

There is no second assertion grammar and no matcher library to learn — one
predicate surface across production code and tests (ADR 0144). Moving from writing
code to verifying it introduces no new vocabulary; a failure reports the structure
of the predicate — `expected` versus `actual` — because there is one predicate
shape to render.

## Tests are part of the language, not a library

Because tests are a language construct, the compiler understands them. `expect` is
valid *only* inside a `case` — used anywhere else it is a compile error
(`bynk.expect.outside_case`), so test-only logic can never leak into production
code. The same is true of `Val[T]`. This is the type-system philosophy turned on
the test suite: the boundary between test and production code is enforced, not
merely conventional.

## Supplied vs generated subjects

A test needs subjects to check. There are two honest ways to get them, and Bynk
gives each its own construct.

A **`case` supplies** its subjects: you write the value the check is about, and
`expect` states the claim. When you need *a* value of some type without caring
about its exact contents, `Val[T]` fabricates one for you — for a refined type one
that satisfies the refinement; for a sum a variant; for a record every field. This
is deliberately *different* from real construction: it is an admission that "the
specific value is irrelevant here". When the value *is* relevant, you pin it —
`Val[T](50)` — and the pin is checked against the type's refinement just as a
literal would be.

A **`property` generates** its subjects: `for all x: T` draws inhabitants of `T`
from its refinement domain — boundary values included — and the body's `expect`s
must hold across all of them. Generation is *real*: the same refinement that a
`Val[T]` satisfies once, a `for all` samples across, so a property states a claim
about a *range* of inputs rather than one. This is where a `case` and a `property`
divide — one names a scenario, the other quantifies over a domain.

Some values cannot be generated or fabricated blindly — there is no sensible way to
invent a string matching an arbitrary regular expression — so a bare `Val` (or a
`for all`) over a `Matches`-refined type is rejected
([`bynk.val.needs_pin`](/book/troubleshooting/val-errors/)) and you must supply
one; an agent, which has no domain to draw from, cannot be generated at all. The
language would rather stop than guess.

## Isolation: mocking collaborators

A unit under test usually depends on collaborators — capabilities it asks for with
`given`. Real implementations may be slow, non-deterministic, or have side
effects you do not want in a test. `mocks` lets a test supply a stand-in
implementation, so the unit is exercised in isolation with dependencies you
control.

Crucially, this reuses the same dependency mechanism the production code uses:
a capability is injected the same way whether the provider is the real one or a
test mock. The test does not reach around the design; it substitutes at the seam
the design already has.

## The throughline

Test-only constructs are *checked* to be test-only; fabricated values are
*honestly distinct* from real ones; collaborators are mocked *through the real
seam*. Testing in Bynk follows the same instinct as the rest of the language —
make the safe thing structural — applied to how you verify your code.

## See also

- Tutorial: [Test it](/book/tutorials/06-testing/).
- How-to: [Write tests and mock collaborators](/book/guides/testing/write-tests/).
- Reference: [testing](/book/reference/testing/).
