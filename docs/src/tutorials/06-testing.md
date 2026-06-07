# 6. Test it

A language built around correctness should make tests easy, and Karn builds
testing in: `test` blocks, `assert`, value fabrication with `Mock[T]`, and
collaborator mocking with `mocks`. In this final tutorial we test the shortener
from [Tutorial 5](05-stateful-agent.md) and meet each of those tools.

## Lay out a test project

Tests live in their own tree, declared in a `karn.toml` manifest. Arrange the
project like this:

```text
url-shortener/
├── karn.toml
├── src/
│   └── shortener.karn
└── tests/
    └── shortener.karn
```

The manifest names the two trees:

```toml
[project]
name = "url-shortener"
version = "0.1.0"

[paths]
src = "src"
tests = "tests"
```

Move the `shortener.karn` you built into `src/`. Each test file's path under
`tests/` mirrors the unit it tests, so `tests/shortener.karn` tests the
`shortener` context.

## Write a test and assert

A test file is a `test` block naming its target, containing one or more named
cases. Inside a case, `assert` checks a condition. Put this in
`tests/shortener.karn`:

```karn,ignore
test shortener

test "a fresh code resolves to NotFound" {
  match ShortCode.of("fresh2") {
    Err(_) => assert false
    Ok(code) => {
      let link = Link(code)
      let outcome <- link.resolve()
      assert outcome is Err(_)
    }
  }
}

test "register then resolve returns the target" {
  match ShortCode.of("reg001") {
    Err(_) => assert false
    Ok(code) => match Url.of("https://example.com/page") {
      Err(_) => assert false
      Ok(url) => {
        let link = Link(code)
        let _ <- link.register(url)
        let outcome <- link.resolve()
        match outcome {
          Ok(view) => assert view.target == url
          Err(_) => assert false
        }
      }
    }
  }
}
```

A few things to notice. We address an agent by constructing it with a key —
`Link(code)` — and call its handlers on the result. Because handlers return an
`Effect`, we bind their results with `<-` rather than `=`. The first test proves
*fresh-state initialisation*: a code never registered reads `target: None`, so
`resolve` reports `NotFound`. The second proves state *persists* — we register,
then resolve and get the target back. Note `assert outcome is Err(_)`: `is`
matches a value against a pattern and yields a `Bool`, perfect for "this is an
`Err`, I don't care about the payload".

## Run the tests

Run the whole suite with `karnc test`:

```sh
karnc test .
```

`karnc` compiles the project (including the tests), type-checks the generated
TypeScript with `tsc`, and runs it with Node. You will need `tsc` and `node` on
your path. The output:

```text
Running tests...

shortener:
  ✓ a fresh code resolves to NotFound
  ✓ register then resolve returns the target

2 passed, 0 failed.
```

`assert` is only valid inside a test case — using it elsewhere is a compile error
(`karn.assert.outside_test`), so test-only checks can never leak into production
code.

## Fabricate values with `Mock[T]`

Tests often need a value of some type without caring exactly what it is.
`Mock[T]` fabricates one. For a refined type it produces a value that satisfies
the refinement; pass an argument to pin a specific one:

```karn,ignore
test "a fabricated code is a valid ShortCode" {
  let code = Mock[ShortCode]
  assert code == code
}

test "a pinned mock takes the given value" {
  let code = Mock[ShortCode]("abc123")
  assert code == code
}
```

`Mock[ShortCode]` yields a valid `ShortCode`; `Mock[ShortCode]("abc123")` pins it,
checked against the refinement at compile time. Like `assert`, `Mock[T]` is
test-only (`karn.mock.outside_test` outside a test). Some types need a pin — a
`Matches`-refined string can't be fabricated blindly, so a bare `Mock` of one is
rejected with `karn.mock.needs_pin`; pin it and you are fine.

## Mock a collaborator with `mocks`

The shortener's `create` service depends on the `CodeGen` capability (it asks for
it with `given CodeGen`). In a test you replace that collaborator with a
deterministic stand-in using `mocks`, declared at the top of the test block:

```karn,ignore
mocks CodeGen = TestCodeGen {
  fn next() -> Effect[String] {
    "test01"
  }
}

test "create mints a code via the mocked generator" {
  match Url.of("https://example.com") {
    Err(_) => assert false
    Ok(url) => {
      let outcome <- create.call(url)
      assert outcome is Ok(_)
    }
  }
}
```

`TestCodeGen` replaces the real `CodeGen` for these tests, so `create` mints the
predictable `"test01"` instead of whatever production would. We call the service
through `create.call(url)` and assert it succeeded.

Run everything again:

```sh
karnc test .
```

```text
shortener:
  ✓ a fresh code resolves to NotFound
  ✓ register then resolve returns the target
  ✓ create mints a code via the mocked generator
  ✓ a fabricated code is a valid ShortCode
  ✓ a pinned mock takes the given value

5 passed, 0 failed.
```

> Capabilities and `given`-based dependency injection are a topic in their own
> right; here we only need enough to mock one. See the
> [capabilities how-to guides](../how-to/capabilities/index.md) for the full
> treatment, and [Test a flow across Workers](../how-to/testing/integration.md)
> for testing across contexts.

## What you have done — and where to go

You laid out a test project, wrote `test` cases with `assert`, ran them with
`karnc test`, fabricated values with `Mock[T]`, and mocked a collaborator with
`mocks`. More than that: you have built one system the whole way — from a first
compiled program, through an HTTP service, a data model, refined types, and a
stateful agent, to a tested URL shortener.

From here:

- **Have a specific task?** The [how-to guides](../how-to/index.md) are recipes
  for individual jobs.
- **Need exact behaviour?** The [reference](../reference/index.md) is the
  consultable source of truth.
- **Want the reasoning?** The [explanation](../explanation/index.md) section
  covers the *why* behind Karn's design.

---

*For the reasoning behind `Mock[T]` and test isolation, see
[The testing philosophy](../explanation/testing-philosophy.md). For exact rules,
see the [testing reference](../reference/testing.md).*
