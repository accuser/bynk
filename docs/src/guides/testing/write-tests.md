# Write tests, mock collaborators, and pin a `Mock[T]`

**Goal:** write and run tests, assert outcomes, fabricate values, and replace a
dependency.

Tests live in a project's `tests/` tree (see
[Lay out a project](../projects-build-and-deployment/layout.md)). A test file is a `test` block naming
its target unit, containing named cases.

## Write and run

```bynk
test counters {
  test "a fresh counter starts at zero" {
    let n <- Counter(CounterId.unsafe("fresh")).current()
    assert n == 0
  }
}
```

Run the suite:

```sh
bynkc test .
```

`bynkc test` compiles the project, type-checks it with `tsc`, and runs it with
Node, so both must be on your path. `assert` is valid only inside a test case.

## Fabricate values with `Mock[T]`

`Mock[T]` produces a value of `T`. For a refined type it satisfies the
refinement; pass an argument to pin a specific value:

```bynk
test quantities {
  test "mocks" {
    let a = Mock[Quantity]       -- a valid Quantity
    let b = Mock[Quantity](50)   -- pinned to 50
    assert a == a
    assert b == b
  }
}
```

A `Matches`-refined string cannot be fabricated blindly — a bare `Mock` of one is
rejected ([`bynk.mock.needs_pin`](../../troubleshooting/mock-errors.md)); pin it
instead. `Mock[T]` is test-only.

## Mock a collaborator with `mocks`

Replace a capability the code under test depends on:

```bynk
test payments {
  mocks Logger = SilentLogger {
    fn log(msg: String) -> Effect[()] {
      ()
    }
  }

  test "authorise succeeds for a positive amount" {
    let r <- authorise.call(100)
    assert r is Ok(_)
  }
}
```

The `SilentLogger` stands in for the real `Logger` for these cases.

## Related

- Tutorial: [Test it](../../tutorials/06-testing.md).
- Reference: [testing](../../reference/testing.md).
- Troubleshooting: [`bynk.mock.*` errors](../../troubleshooting/mock-errors.md).
