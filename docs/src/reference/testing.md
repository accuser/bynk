# Testing

## `test` blocks

A test file is a `test` block naming its target unit, containing named cases:

```karn
test counters {
  test "a fresh counter starts at zero" {
    let n <- Counter(CounterId.unsafe("fresh")).current()
    assert n == 0
  }
}
```

Case descriptions within a block must be unique
(`karn.test.duplicate_case_name`); the target must exist
(`karn.test.unknown_target`). Test files live under the project's `tests/` tree —
see [Lay out a project](../how-to/projects/layout.md).

## `assert`

`assert <bool-expr>` checks a condition. It exists in both statement form (a line
in a test body) and expression form (e.g. inside a `match` arm). The expression
must be `Bool` (`karn.assert.non_bool`), and `assert` is valid **only** inside a
test case (`karn.assert.outside_test`). Pairs naturally with `is`:
`assert r is Ok(_)`.

## `mocks` — collaborator substitution

Replace a capability the unit under test depends on with a test implementation:

```karn
test payments {
  mocks Logger = SilentLogger {
    fn log(msg: String) -> Effect[()] {
      ()
    }
  }

  test "…" { … }
}
```

The mock's signatures must match the capability (`karn.mock.signature_mismatch`);
a target may be mocked once (`karn.mock.duplicate_target`) and must be in scope
(`karn.mock.unknown_target`).

## `Mock[T]` — value fabrication

`Mock[T]` fabricates a value of `T`; `Mock[T](pin)` pins a specific one.

| Kind | Bare `Mock[T]` yields |
|---|---|
| `Int where Positive` | `1` |
| `Int where NonNegative` | `0` |
| `Int where InRange(a, b)` | `a` |
| `String where MinLength(k)` / `Length(k)` | a string of length `k` |
| `String where Matches(…)` | **error** — must pin (`karn.mock.needs_pin`) |
| sum | the first variant (payloads recursively mocked) |
| record | every field mocked |
| opaque | `.unsafe(<base zero>)` |

`Mock[T]` is test-only (`karn.mock.outside_test`). A pin must be a compile-time
literal (`karn.mock.pin_not_literal`), must satisfy the refinement
(`karn.mock.literal_violates`), and is only accepted where the kind supports it
(`karn.mock.pin_unsupported`). See
[`karn.mock.*` errors](../how-to/troubleshooting/mock-errors.md).

## Running

```sh
karnc test .
```

`karnc test` compiles the project (including tests), type-checks the output with
`tsc`, and runs it with Node — both must be on your path. `--no-run` emits the
TypeScript without running it. Exit code is non-zero if any test fails.
