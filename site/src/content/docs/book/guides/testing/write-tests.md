---
title: "Write tests, mock collaborators, and pin a `Val[T]`"
---
**Goal:** write and run tests, state expectations, fabricate values, and replace a
dependency.

Tests live in a project's `tests/` tree (see
[Lay out a project](/book/guides/projects-build-and-deployment/layout/)). A test file is a `suite` block naming
its target unit, containing named `case`s.

## Write and run

```bynk
suite counters {
  case "a fresh counter starts at zero" {
    let n <- Counter(CounterId.unsafe("fresh")).current()
    expect n == 0
  }
}
```

Run the suite:

```sh
bynkc test .
```

`bynkc test` compiles the project, type-checks it with `tsc`, and runs it with
Node, so both must be on your path. `expect` is valid only inside a `case`. It
takes the same `Bool` predicate an `invariant` does (`is`, `implies`, the
operators, pure methods) — one predicate surface across code and tests — and a
failure reports the predicate structure: `expected` versus `actual`.

## Fabricate values with `Val[T]`

`Val[T]` produces a value of `T`. For a refined type it satisfies the
refinement; pass an argument to pin a specific value:

```bynk
suite quantities {
  case "vals" {
    let a = Val[Quantity]       -- a valid Quantity
    let b = Val[Quantity](50)   -- pinned to 50
    expect a == a
    expect b == b
  }
}
```

A `Matches`-refined string cannot be fabricated blindly — a bare `Val` of one is
rejected ([`bynk.val.needs_pin`](/book/troubleshooting/val-errors/)); pin it
instead. `Val[T]` is test-only.

## Constrain a function with `requires` / `ensures`

A **contract** states what a pure function guarantees, right on its signature —
between the return type and the body. `requires` clauses are preconditions over
the parameters; `ensures` clauses are postconditions over the parameters and
`result`, the return value:

```bynk
commons commerce.money

fn discount(p: Int, pct: Int) -> Int
  requires p_nonneg: p >= 0
  requires pct_in_range: pct >= 0 && pct <= 100
  ensures never_above: result <= p
{
  p - (p * pct) / 100
}
```

You write no test for this: in the dev/test build every call checks the contract,
and the runner **generates** arguments (filtered by `requires`) to attack the
`ensures` — reporting a shrunk counterexample if one breaks. In the deploy build
the checks are stripped, so contracts cost nothing in production. A contract is a
property that is always on; reach for a `property` only when a claim is relational
or spans calls. See the [testing reference](/book/reference/testing/#contracts).

## Constrain a state change with `transition`

Where an `ensures` constrains one function call and an `invariant` constrains one
committed state, a **`transition`** constrains the *move* between two — declared on
the agent, over the `old`/`new` state pair:

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

Again you write no test: a `transition` is checked at the commit boundary (from the
second commit — the first has no `old`), so it holds under every `case` at every
tier for free. See
[Agent invariants → Step invariants](/book/reference/agent-invariants/#step-invariants).

## Check a claim across inputs with `property` / `for all`

Where a `case` supplies its subjects, a `property` **generates** them and checks a
claim holds across many. `for all x: T` binds `x` to a generated inhabitant of
`T`; an optional `where` filters the generated tuples:

```bynk
suite pricing {
  property "more discount, never a higher price" {
    for all p: Price, a: Percent, b: Percent where a <= b {
      expect discount(p, b) <= discount(p, a)
    }
  }
}
```

Generation draws from each type's refinement domain (including boundary values).
Reach for a `property` when a claim should hold across a *range* of inputs; reach
for a `case` when one named scenario is the point. On failure a property prints a
shrunk counterexample and a reproduce line — see
[Run your tests](/book/guides/testing/run-tests/) and the
[testing reference](/book/reference/testing/).

## Mock a collaborator with `mocks`

Replace a capability the code under test depends on:

```bynk
suite payments {
  mocks Logger = SilentLogger {
    fn log(msg: String) -> Effect[()] {
      ()
    }
  }

  case "authorise succeeds for a positive amount" {
    let r <- authorise.call(100)
    expect r is Ok(_)
  }
}
```

The `SilentLogger` stands in for the real `Logger` for these cases.

## Related

- Tutorial: [Test it](/book/tutorials/06-testing/).
- Reference: [testing](/book/reference/testing/).
- Troubleshooting: [`bynk.val.*` errors](/book/troubleshooting/val-errors/).
