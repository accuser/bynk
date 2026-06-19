# `bynk.mock.*` errors

`Mock[T]` fabricates values **in tests only**. These are its common errors.

## `bynk.mock.outside_test`

```text
[bynk.mock.outside_test] Error: `Mock[T]` is only valid inside a test case body
```

**Cause:** you used `Mock[T]` outside a `test "…" { … }` case — for example in a
regular function.

**Fix:** move the `Mock[T]` into a test case. To construct a value in production
code, use a real constructor instead (`.of` or `.unsafe` for a refined or opaque
type; a record/variant literal otherwise).

## `bynk.mock.needs_pin`

```text
[bynk.mock.needs_pin] bare `Mock[Code]` cannot generate a value for a `Matches` refinement
```

**Cause:** you wrote a bare `Mock[T]` for a type whose refinement is a `Matches`
pattern. Bynk cannot invent a string that matches an arbitrary regex.

**Fix:** pin a concrete value that satisfies the pattern. Given the type:

```bynk
type Code = String where Matches("[a-z]+")
```

…pin the value where you mock it in a test case:

```bynk
let c = Mock[Code]("abc")
```

## Other Mock errors

- The type doesn't resolve, or its kind can't be mocked — check the type name and
  see the [testing reference](../reference/testing.md).
- A pinned value that violates the refinement is rejected for the same reason a
  literal would be (see
  [`bynk.refine.literal_violates`](refine-literal-violates.md)).

## Related

- [Write tests and mock collaborators](../guides/testing/write-tests.md)
- Reference: [testing](../reference/testing.md)
