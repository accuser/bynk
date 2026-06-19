# The testing philosophy

Testing is built into Bynk rather than bolted on: `test` blocks, `assert`,
`Mock[T]`, and `mocks` are language constructs. This page explains why they exist
in the form they do.

## Tests are part of the language, not a library

Because tests are a language construct, the compiler understands them. `assert` is
valid *only* inside a test case — used anywhere else it is a compile error
(`bynk.assert.outside_test`), so test-only logic can never leak into production
code. The same is true of `Mock[T]`. This is the type-system philosophy turned on
the test suite: the boundary between test and production code is enforced, not
merely conventional.

## Fabricated values vs real construction

A test often needs *a* value of some type without caring about its exact
contents. Constructing one by hand is tedious and, for refined or opaque types,
requires going through validation you do not care about for this test.

`Mock[T]` fabricates a value for you. For a refined type it produces one that
satisfies the refinement; for a sum it picks a variant; for a record it fills
every field. This is deliberately *different* from real construction: a mock is an
admission that "the specific value is irrelevant here". When the value *is*
relevant, you pin it — `Mock[T](50)` — and the pin is checked against the type's
refinement just as a literal would be.

Some values cannot be fabricated blindly — there is no sensible way to invent a
string matching an arbitrary regular expression — so a bare `Mock` of a
`Matches`-refined type is rejected ([`bynk.mock.needs_pin`](../../troubleshooting/mock-errors.md))
and you must supply one. The language would rather stop than guess.

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

- Tutorial: [Test it](../../tutorials/06-testing.md).
- How-to: [Write tests and mock collaborators](write-tests.md).
- Reference: [testing](../../reference/testing.md).
