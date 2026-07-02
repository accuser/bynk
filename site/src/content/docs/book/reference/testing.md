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

## Contracts — `requires` / `ensures` {#contracts}

A **contract** is the invariant predicate attached to a function. Between a pure
function's return type and its body, declare any number of named `requires`
(preconditions) and `ensures` (postconditions):

```bynk
commons commerce.money

fn discount(p: Int, pct: Int) -> Int
  requires p_nonneg: p >= 0
  requires pct_in_range: pct >= 0 && pct <= 100
  ensures never_above: result <= p
  ensures never_negative: result >= 0
{
  p - (p * pct) / 100
}
```

- **`requires <name>: <pred>`** is a precondition over the parameters. `result`
  is **not** in scope (`bynk.contract.result_in_requires`).
- **`ensures <name>: <pred>`** is a postcondition over the parameters **and**
  `result` — the return value (the awaited element for an `Effect` return).
  Outside an `ensures`, `result` is an ordinary identifier.
- Each predicate is the **same predicate surface** as a `case`, a `property`, or
  an `invariant`: a pure `Bool` with `implies`, `is`, operators, and pure methods
  — no effects, capabilities, `expect`, or `Val`
  (`bynk.contract.impure_predicate`, `bynk.contract.not_bool`).

**Checked at two points, for free.** A contract needs no test to run:

1. **At every call** in the dev/test build, a call-site guard checks each
   `requires` on entry and each `ensures` on exit, throwing a contract failure
   that names the clause and the offending arguments/`result`. The guard is
   **stripped from the deploy build** (`bynkc compile`) — contracts add no
   production cost and never change production behaviour.
2. **By the runner.** For every contracted function reachable from a test target,
   the runner **generates** arguments over the parameter domains (the same engine
   `for all` uses — boundary-inclusive, seeded, shrinking), **filters** them by the
   `requires` (exactly as a `for all … where` does — inputs failing a precondition
   are discarded), calls the function, and checks the `ensures`. A failure reports
   the case count, the seed, and a shrunk counterexample with the same reproduce
   line a `property` gives. A contract is a property that is always on.

**`ensures` vs `property`.** A claim about *one* result belongs in `ensures` —
checked everywhere and generated for free. A `property` earns its keep only when
the claim is **relational or spans calls** (monotonicity, a round-trip) — which no
per-call postcondition can express. A `case`/`property` that merely restates a
contract already declared at the source is redundant and flagged
`bynk.contract.restated_by_test` (a conservative, syntactic check).

See [`bynk.contract.*` errors](/book/troubleshooting/contract-errors/).

## Step invariants — `transition` {#transitions}

Where an `ensures` constrains one function *call* and an `invariant` constrains one
committed *state*, a **`transition`** constrains the *move* between two committed
states — declared on the agent, over the `old`/`new` state pair:

```bynk
agent Order {
  key id: OrderId

  store status: Cell[OrderStatus] = Pending

  transition paid_is_terminal:
    old.status is Paid implies new.status is Paid

  on call pay() -> Effect[()] {
    status := Paid
    ()
  }
}
```

A `transition` is checked at the **commit boundary**, from the second commit
onward (the genesis commit has no `old` and is skipped), so — like an invariant —
it is carried by the agent and inherited by *every* `case` for free, at every tier;
you never write a test for it. It is **not** attacked by the runner: a fabricated
agent state is valid but not necessarily reachable, so behavioural generation over
transitions is a runner-driven handler-sequence concern, not value fabrication.

Full reference: [Agent invariants → Step invariants](/book/reference/agent-invariants/#step-invariants).
See [`bynk.transition.*` errors](/book/troubleshooting/transition-errors/).

## Observation — `expect Cap.op called …` {#observation}

Where the rungs above assert over *values* and *state*, observation asserts over
*interaction*: that the unit under test called a capability, with what arguments,
how many times, and in what order. Because a capability is injected at a known
seam, its calls are **recorded automatically** in the test build — a
pure-observation `case` needs no `mocks` or setup at all:

```bynk
suite orders {
  case "an oversized order is rejected and logged" {
    let r <- place.call(50000)
    expect r is Err(_)
    expect Logger.log called once with msg == "rejected: amount too large"
    expect Store.put never called            -- a rejected order writes nothing
  }
}
```

The subject is a **`Cap.op` reference** — the capability and one of its operations,
*named, not called* (no argument list). The sugar forms are:

| Form | Holds when |
|------|-----------|
| `expect Cap.op called` | at least one call |
| `expect Cap.op never called` | zero calls |
| `expect Cap.op called once` | exactly one call |
| `expect Cap.op called <n> times` | exactly `<n>` calls (`<n>` an integer literal) |
| `expect Cap.op called with <pred>` | at least one call whose arguments satisfy `<pred>` |
| `expect Cap.op called <n> times with <pred>` | exactly `<n>` calls, and they match |
| `expect A.op before B.op` | both occurred, and the first `A.op` precedes the first `B.op` |

A **`with` predicate** is the ordinary predicate surface with the operation's
parameters in scope by their declared names (`Logger.log(msg: String)` → `msg`), so
`with msg == "…"` reads directly; it must be pure `Bool`.

For anything the sugar does not cover, the **escape hatch** binds the recorded calls
as an ordinary value:

```bynk
let calls = trace(Logger.log)
expect calls.length() == 2
expect calls.all((c) => c.msg.length() > 0)
```

`trace(Cap.op)` yields a `List` of per-operation call records in call order — each
record's fields are the operation's parameters (`{ msg: String }` for `Logger.log`)
— so it is asserted with the `List` surface you already know (`length()`, `all` /
`any`, indexing). There is no test-only iteration construct: "for every recorded
call …" is `calls.all((c) => …)`.

Recording is emitted **only** under `bynkc test`; the deploy build calls the seam
directly, so observation adds no production cost. Observation is *scenario-specific*
— a claim about one case; a *universal* guarantee ("every payment audits, on every
path") is a policy, not a test.

See [`bynk.observe.*` errors](/book/troubleshooting/observation-errors/).

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
