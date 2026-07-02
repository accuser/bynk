---
title: Test it
---
A language built around correctness should make tests easy, and Bynk builds
testing in: `suite`/`case` blocks, `expect`, value fabrication with `Val[T]`, and
collaborator mocking with `mocks`. In this final tutorial we test the shortener
from [Tutorial 5](/book/tutorials/05-stateful-agent/) and meet each of those tools.

## Lay out a test project

Tests are ordinary `.bynk` files — a `suite` is a test wherever it lives.
Conventionally they sit in a `tests/` tree beside `src/`, declared with a
`bynk.toml` manifest:

```text
url-shortener/
├── bynk.toml
├── src/
│   └── shortener.bynk
└── tests/
    └── shortener.bynk
```

The manifest just names the project — the conventional `src/`+`tests/` layout
needs no `[paths]` config:

```toml
[project]
name = "url-shortener"
version = "0.1.0"
```

Move the `shortener.bynk` you built into `src/`. A `suite` names the unit it
tests, so `tests/shortener.bynk` — or a `suite` block right inside
`src/shortener.bynk` — tests the `shortener` context. (When you build, the
`suite` is stripped; only `bynkc test` runs it.)

## Write a test and assert

A test file is a `suite` block naming its target, containing one or more named
cases. Inside a case, `expect` checks a predicate. Put this in
`tests/shortener.bynk`:

```bynk,ignore
suite shortener

case "a fresh code resolves to NotFound" {
  match ShortCode.of("fresh2") {
    Err(_) => expect false
    Ok(code) => {
      let link = Link(code)
      let outcome <- link.resolve()
      expect outcome is Err(_)
    }
  }
}

case "register then resolve returns the target" {
  match ShortCode.of("reg001") {
    Err(_) => expect false
    Ok(code) => match Url.of("https://example.com/page") {
      Err(_) => expect false
      Ok(url) => {
        let link = Link(code)
        let _ <- link.register(url)
        let outcome <- link.resolve()
        match outcome {
          Ok(view) => expect view.target == url
          Err(_) => expect false
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

Run the whole suite with `bynkc test`:

```sh
bynkc test .
```

`bynkc` compiles the project (including the tests), type-checks the generated
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
(`bynk.assert.outside_test`), so test-only checks can never leak into production
code.

## Fabricate values with `Val[T]`

Tests often need a value of some type without caring exactly what it is.
[`Val[T]`](/book/reference/glossary/#term-val) fabricates one. For a refined type
it produces a value that satisfies
the refinement; pass an argument to pin a specific one:

```bynk,ignore
case "a fabricated code is a valid ShortCode" {
  let code = Val[ShortCode]
  expect code == code
}

case "a pinned value takes the given value" {
  let code = Val[ShortCode]("abc123")
  expect code == code
}
```

`Val[ShortCode]` yields a valid `ShortCode`; `Val[ShortCode]("abc123")` pins it,
checked against the refinement at compile time. Like `expect`, `Val[T]` is
test-only (`bynk.val.outside_test` outside a test). Some types need a pin — a
`Matches`-refined string can't be fabricated blindly, so a bare `Val` of one is
rejected with `bynk.val.needs_pin`; pin it and you are fine.

To check a claim across a *range* of generated inputs rather than one fabricated
value, reach for a `property` and its `for all` — see the
[testing reference](/book/reference/testing/).

## Mock a collaborator with `mocks`

The shortener's `create` service depends on the `CodeGen` capability (it asks for
it with `given CodeGen`). In a test you replace that collaborator with a
deterministic stand-in using `mocks`, declared at the top of the test block:

```bynk,ignore
mocks CodeGen = TestCodeGen {
  fn next() -> Effect[String] {
    "test01"
  }
}

case "create mints a code via the mocked generator" {
  match Url.of("https://example.com") {
    Err(_) => expect false
    Ok(url) => {
      let outcome <- create.call(url)
      expect outcome is Ok(_)
    }
  }
}
```

`TestCodeGen` replaces the real `CodeGen` for these tests, so `create` mints the
predictable `"test01"` instead of whatever production would. We call the service
through `create.call(url)` and assert it succeeded.

Run everything again:

```sh
bynkc test .
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
> [capabilities how-to guides](/book/guides/effects-and-capabilities/) for the full
> treatment, and [Test a flow across Workers](/book/guides/testing/integration/)
> for testing across contexts.

## What you have done — and where to go

You laid out a test project, wrote `test` cases with `assert`, ran them with
`bynkc test`, fabricated values with `Val[T]`, and mocked a collaborator with
`mocks`. More than that: you have built one system the whole way — from a first
compiled program, through an HTTP service, a data model, refined types, and a
stateful agent, to a tested URL shortener.

From here:

- **Have a specific task?** The [how-to guides](/book/guides/) are recipes
  for individual jobs.
- **Need exact behaviour?** The [reference](/book/reference/) is the
  consultable source of truth.
- **Want the reasoning?** The [explanation](/book/guides/) section
  covers the *why* behind Bynk's design.

---

*For the reasoning behind `Val[T]` and test isolation, see
[The testing philosophy](/book/guides/testing/philosophy/). For exact rules,
see the [testing reference](/book/reference/testing/).*
