---
title: Testing
---
## `suite` blocks

A test file is a `suite` block naming its target unit, containing named `case`s:

```bynk
suite counters {
  case "a fresh counter starts at zero" {
    let n <- Counter(CounterId.unsafe("fresh")).current()
    expect n == 0
  }
}
```

Case descriptions within a suite must be unique
(`bynk.suite.duplicate_case_name`); the target must exist
(`bynk.suite.unknown_target`). Test files live under the project's `tests/` tree —
see [Lay out a project](/book/guides/projects-build-and-deployment/layout/).

## `expect`

`expect <bool-predicate>` checks a predicate. It exists in both statement form (a
line in a `case` body) and expression form (e.g. inside a `match` arm). The
predicate must be `Bool` (`bynk.expect.not_bool`), and `expect` is valid **only**
inside a `case` (`bynk.expect.outside_case`). It is the **same predicate surface**
as `invariant`/`ensures` — `is`, `implies`, the operators, pure methods (one
predicate surface, ADR 0144) — so it pairs naturally with `is`: `expect r is
Ok(_)`. When the predicate is a top-level comparison (`==`, `!=`, `<`, `<=`, `>`,
`>=`), a failure reports the predicate and its **expected-vs-actual** operands, not
just a location.

## `mocks` — collaborator substitution

Replace a capability the unit under test depends on with a test implementation:

```bynk
suite payments {
  mocks Logger = SilentLogger {
    fn log(msg: String) -> Effect[()] {
      ()
    }
  }

  case "…" { … }
}
```

The mock's signatures must match the capability (`bynk.mock.signature_mismatch`);
a target may be mocked once (`bynk.mock.duplicate_target`) and must be in scope
(`bynk.mock.unknown_target`).

## `Val[T]` — value fabrication

`Val[T]` fabricates a valid inhabitant of `T` drawn from its refinement domain;
`Val[T](pin)` pins a specific one, refinement-checked at compile time.

| Kind | Bare `Val[T]` yields |
|---|---|
| `Int where Positive` | `1` |
| `Int where NonNegative` | `0` |
| `Int where InRange(a, b)` | `a` |
| `String where MinLength(k)` / `Length(k)` | a string of length `k` |
| `String where Matches(…)` | **error** — must pin (`bynk.val.needs_pin`) |
| sum | the first variant (payloads recursively fabricated) |
| record | every field fabricated |
| opaque | `.unsafe(<base zero>)` |

`Val[T]` is test-only (`bynk.val.outside_test`). A pin must be a compile-time
literal (`bynk.val.pin_not_literal`), must satisfy the refinement
(`bynk.val.literal_violates`), and is only accepted where the kind supports it
(`bynk.val.pin_unsupported`). See
[`bynk.val.*` errors](/book/troubleshooting/val-errors/).

## `property` / `for all` — generative tests

A `property` is the generative sibling of `case`, legal in the same `suite`. Where
a `case` supplies its subjects, a `property` **generates** them and checks that a
claim holds across many:

```bynk
property "more discount, never a higher price" {
  for all p: Price, a: Percent, b: Percent where a <= b {
    expect discount(p, b) <= discount(p, a)
  }
}
```

`for all x: T` binds `x` to a *generated* inhabitant of `T` (comma-separated for
multiple bindings). An optional `where <pred>` — a pure `Bool` — filters generated
tuples before the body runs (a non-`Bool` filter is `bynk.property.where_not_bool`).
The body is one or more `expect`s: the **same predicate surface** as a `case`, an
`invariant`, or an `ensures`.

Generation draws from `T`'s **refinement domain** and includes boundary values:

| Type | `for all` / `Val` generates |
|---|---|
| `Int where Positive` | `1`, small positives, and the boundary |
| `Int where NonNegative` | `0` and small non-negatives |
| `Int where InRange(a, b)` | `a`, `b`, and interior values |
| `String where MinLength(k)` / `Length(k)` | strings at and above length `k` |
| `String where Matches(…)` | **must pin** (`bynk.val.needs_pin`) — no generator |
| sum | each variant |
| record | each field generated |
| opaque | over the base type |

A type must be **refinement-generable** to appear in `for all` (or `Val`): a
`String where Matches(re)` has no generator and must be pinned instead; an **agent**
cannot be generated (`bynk.val.agent_not_generable`) — behavioural agent testing
over handler sequences is a later slice.

**When a `property` earns its keep.** Reach for a `property` when a claim should
hold across a *range* of inputs — a relationship between inputs and an output
(monotonicity, a round-trip, an ordering). Reach for a `case` when one specific,
named scenario is the point. A `property` that merely re-checks a refinement its
type already guarantees (e.g. `for all q: Quantity { expect q > 0 }` when
`Quantity` is `Int where Positive`) proves nothing and is flagged
`bynk.property.restates_refinement` (a conservative, syntactic check).

On failure a property reports the case count, the run's root seed, and a **shrunk**
counterexample with a copy-paste reproduce line — see
[Run your tests](/book/guides/testing/run-tests/) and
[`bynk.val.*` errors](/book/troubleshooting/val-errors/).

## `suite integration` — multi-Worker integration tests

A `suite` block exercises **one** unit, with collaborators replaced by `mocks`. A
`suite integration` block exercises a **flow across several contexts**, each stood
up as the Worker it actually deploys as, with **no** mocks — so the real
cross-context wire (serialise → JSON → deserialise → structural projection) is
under test, which unit tests never touch.

```bynk
suite integration "checkout" {
  wires shop.orders, shop.payment

  case "small order authorises across the wire" {
    let r <- shop.orders.place(100)
    expect r is Ok(_)
  }
}
```

- **`wires`** lists the participating contexts (at least two —
  `bynk.integration.too_few_participants`). Each must be a declared context
  (`bynk.integration.unknown_participant`), listed once
  (`bynk.integration.duplicate_participant`).
- The set must be **closed** under `consumes`: if a participant consumes another
  context, that context must also be wired
  (`bynk.integration.unwired_dependency`).
- A case calls into a participant by **qualified name** —
  `shop.orders.place(100)` (a service) — exactly as a cross-context caller would.
  The call travels a simulated Service Binding into the target Worker; any further
  cross-context calls it makes (e.g. `orders → payment`) cross the wire too.
- Integration tests take **no `mocks`** (`bynk.integration.mock_in_integration`) —
  the point is real wiring. Suite names are unique
  (`bynk.integration.duplicate_suite`).

Cross-context capabilities (`given B.Cap`) are wired as in production: the
provider is instantiated locally in the consumer Worker (v0.15 model A1).
**Agents** (Durable Objects) work too: a participant's agents are backed by
in-memory Durable Object instances — same key, same instance **within a case**;
state starts empty and is **fresh per case**. See
[Test a flow across Workers](/book/guides/testing/integration/).

`bynkc test` runs integration tests in plain Node alongside unit tests — it
compiles the participants in workers mode under `out/workers/`, stands them up
in-process, and routes the real wire between them. No `wrangler`/`miniflare`
needed.

## Running

```sh
bynkc test .
```

`bynkc test` compiles the project (including tests), type-checks the output with
`tsc`, and runs it with Node — both must be on your path. `--no-run` emits the
TypeScript without running it. Exit code is non-zero if any test fails.

### Debugging under Node (`--inspect`)

`bynkc test --inspect` launches the test runner under Node's inspector
(`node --inspect-brk`) and prints an inspector URL:

```sh
bynkc test . --inspect
# → Debugger listening on ws://127.0.0.1:9229/…
```

Attach any JavaScript debugger to that URL (VS Code's built-in Node debugger,
Chrome DevTools, …). Breakpoints set in your **`.bynk` sources** bind and pause
there — the compiler emits source maps (since v0.68) and, under `--inspect`, runs
the emitted TypeScript directly so those maps resolve breakpoints back to `.bynk`.
This requires **Node ≥ 22.6** (it relies on Node's TypeScript type-stripping) and
does not run `tsc`. Breakpoints bind on the statement you click — both in the code a
test exercises and **inside the test body itself** (since v0.70 maps test-case and
handler bodies per-statement). A one-click VS Code launch is in progress.

### Machine-readable output (`--format json`)

`bynkc test --format json` emits a single pinned JSON document of results
instead of the human ✓ / ✗ output — one `suites` array of `{ name, kind, cases }`,
each case `{ name, outcome, message?, location? }` with `outcome` one of `"pass"`
or `"fail"`. A project that doesn't compile yields `error.kind == "compile"`
(the diagnostic lines); a runner that crashes mid-stream yields
`error.kind == "runtime"` with the observed prefix and captured stderr.

Add `--no-run` for **discovery**: `bynkc test --no-run --format json` lists every
suite and case **without running them** — a pure compile (no `tsc`, no Node, no
`out/` written). Each case carries `outcome: "discovered"` and its declaration
`location` (the `case "…"` name). The suite/case names match a normal run's, so a
consumer can list tests first and fold in pass/fail from a later run. This is how
the VS Code Test Explorer populates its tree before you run anything.
