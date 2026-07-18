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

A case drives the target's services by their natural surface — resolved against
the declared handler and checked for arity and argument types:

| service | address |
|---|---|
| `on call` | `svc.call(args)` |
| `from http` | `svc.GET("/path")`, `svc.POST("/path", body)` — the path is the route **pattern**, then the handler's params (path params, then body) |
| `from cron` | `svc.schedule("<expr>")` — matched to the handler with that schedule |
| `from queue` | `svc.message(msg)` |

A route/schedule the service does not declare is `bynk.test.service_unknown_route`;
`svc.call(...)` on a service with no `on call` handler is
`bynk.test.service_no_call_handler`.

### Acting as an actor — `by <Actor>(<identity>)` {#call-site-by}

A handler guarded by an actor (`by u: User`) runs as a verified identity. A case
supplies that identity with a call-site `by` clause on the effect-let:

```bynk
case "each owner's list is private" {
  let _    <- api.POST("/todos", AddRequest { title: "bob's" }) by User("bob")
  let mine <- api.GET("/todos")                                 by User("carol")
  expect mine is Ok(_)
}
```

- `by User("bob")` — the actor and the identity value. The value is typed against
  the actor's identity type, so `by User("")` fails `UserId`'s refinement.
- `by Visitor` — a unit-identity actor takes **no** argument
  (`bynk.test.actor_no_identity` if one is given; `bynk.test.actor_identity_required`
  if an identity-carrying actor is written bare).
- cron and queue run as their internal actor and need no `by`.

At the `unit` tier the identity is *given*, not verified — the handler runs
in-process against fresh, per-case agent state. **Promote the same case to `as
system`** and the identical body drives the *deployable* Worker: the address
becomes a real `fetch` into the public route table, `by User("bob")` is signed
into a JWT the real auth seam verifies, and the `HttpResult` is decoded from the
`Response`. The developer writes no auth — the framework signs a valid credential
from the `by` clause; *proper* auth (real IdPs, expired/forged tokens) is an
end-to-end concern, not the system tier's. A single-context `from http` service
qualifies for `as system` (it has a real serialisation edge); a `cron`-only target
does not (`bynk.tier.system_needs_wire`).

### Driving a rejection with `Wire`

A typed argument is valid by construction, so the boundary can never reject it. To
test the *rejection* path — the part the boundary exists for — a `system`-tier case
passes a raw `Wire(<String>)` argument: the string reaches the router
**unvalidated**, exactly as an over-the-wire request would, so a refinement
violation or malformed JSON is refused *before the handler runs*.

```bynk
case "an empty sku is rejected at the boundary" as system {
  let r <- api.POST("/cart", Wire("{\"sku\": \"\"}")) by User("alice")
  expect r is Rejected(_)
}
```

A `Wire`-carrying call yields `Rejected(kind) | Handled(_)` instead of an
`HttpResult`: `Rejected` when the router refused the input before the handler,
`Handled` when it ran (a valid raw body promotes to the handler, so
`expect r is Handled(_)`). The rejection's *kind* is discriminable — the nested
pattern tests it:

```bynk
expect r is Rejected(_)                       -- any boundary rejection
expect r is Rejected(RefinementViolation(_))  -- specifically a refinement violation
expect r is Rejected(MalformedJson(_))        -- specifically malformed JSON
```

The kinds are `RefinementViolation`, `MalformedJson`, and `StructuralMismatch`.
The outcome is checked at runtime, not statically typed, so a mistyped kind
name is a case that *fails* (the pattern never matches) rather than a compile
error. `Wire` is legal **only** as a service-address argument in a `system`-tier
case — there is no wire to be raw about at `unit`
(`bynk.test.wire_needs_system`) — and no refined value is ever built from it: the
router validates the raw string, which is the whole point.

### Testing the auth seam with `by Nobody`

`by User("bob")` presents a valid, framework-signed credential the real seam
verifies. To test the *rejection* — an unauthenticated request — a `system`-tier
case drives the route as **`by Nobody`**: the request carries no `Authorization`
header, so the seam refuses it before the handler runs.

```bynk
case "no credential is rejected at the seam" as system {
  let r <- api.POST("/cart", Item { sku: "widget" }) by Nobody
  expect r is Rejected(Unauthorized)
}
```

`by Nobody` yields the same `Rejected(_) | Handled(_)` outcome as a `Wire` call:
a `401` from the seam is `Rejected(Unauthorized)`. It presents no identity
(`by Nobody(...)` is an error), and is meaningful only at `system`, where the
real seam exists (`bynk.test.credential_needs_system`). A validly-signed but
expired or forged credential is an end-to-end concern, not the system tier's.

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

## Tiers — the `as <tier>` clause {#tiers-the-as-tier-clause}

A `case` runs at one of three **tiers**, declared with an `as <tier>` clause in its
header. The three names are the testing pyramid — `unit`, `integration`, `system` —
and a tier is **one body promoted, not a distinct kind of test**:

| Tier | Collaborators | Wire crossed? |
|------|---------------|---------------|
| `unit` (default, elided) | in process; a seam may be stubbed with `stub` | no |
| `integration` | real, **within one context** | no |
| `system` | contexts stood up as the Workers they deploy as | **yes** — the real serialise → JSON → deserialise edge |

```bynk
suite money {
  case "never negative"                   { … }  -- as unit, by default
  case "used in a payment" as integration { … }  -- real collaborators, one context, no wire
  case "checkout end to end" as system    { … }  -- contexts wired across the real edge
}
```

`unit` is the default and is **never written**. `as` also sits on the **`suite`**
header, setting a default every `case` inherits and may override (case wins):

```bynk
suite checkout as integration {          -- every case defaults to integration…
  case "small order authorises"       { … }  -- as integration (inherited)
  case "a unit-level edge"    as unit { … }  -- case overrides the suite default
}
```

A case's effective tier is `case.tier ?? suite.tier ?? unit`. Promotion changes
**only** the header — the body is byte-for-byte identical at every tier.

- **Participants are inferred**, not listed: `integration` / `system` derive their
  real/wired collaborator set from the unit under test's transitive `consumes`
  graph. There is no `wires` clause.
- **`system` needs a serialisation edge**: a `system` suite must cross a real
  serialise → JSON → deserialise boundary — either two or more wired contexts, **or**
  a single target that exposes an `http` service (its public boundary). A target
  with neither is `bynk.tier.system_needs_wire`. (A `queue` service serialises its
  message too, but driving a queue over a real wire at `system` is a later slice, so
  a queue-only target does not yet qualify.) `integration` carries no such rule — it
  is real collaborators within one context, no wire.
- Tiers are **`case`-only**: a `property` generates and does not promote, so a
  suite-level `as` binds its `case` members only; an `as` on a `property` header is
  `bynk.tier.property_has_tier`.
- The **agent-state lifecycle is fixed across tiers**: the unit under test is
  always a real in-memory instance, keyed normally, fresh per case; only its
  collaborators' realness and whether sends cross a serialisation boundary change.

See [Test tiers](/book/guides/testing/integration/) for the full guide, and
[`bynk.tier.*` errors](/book/troubleshooting/integration-errors/).

## `stub` — per-seam test doubles {#stub}

`as <tier>` sets the *default* provision of every seam; a **`stub`** clause
overrides *one* method's provision under test — an explicit call pattern on the
left, a value or `fails` on the right, **never a computed body**:

```bynk
suite pricing {
  stub Rates.lookup("GBP") returns 1.25        -- suite-scoped: applies to every case
  stub Rates.lookup(_)     returns 1.0         -- fallback; first matching clause wins

  case "a fault surfaces as an error" {
    stub Kv.get(_) fails                        -- case-scoped: overrides for this case
    let r <- Prices(Val[AcctId]).quote("GBP")
    expect r is Err(_)
  }
}
```

This substitutes a consumed seam of the unit under test — the third of the seam
triad: `consumes` *declares* a seam, `given` *requires* it, and `stub`
*substitutes* it under test (its own keyword since #548; formerly a pun on the
production `provides`).

- **The left is `Cap.method(<pattern>, …)`** — the capability, the method, and an
  **argument pattern** per parameter. A pattern is the [one predicate
  surface](/book/reference/testing/#expect): a literal (`"GBP"`, `1000`), `_`
  (any), or an `is` narrowing. Clauses for the same method are tried **top to
  bottom, first match wins**, so put specific before fallback.
- **The right is `returns <value>` or `fails`** — a *value* or a *fault* (an `Err`
  is an in-band outcome asserted in the case; `fails` injects a capability
  *fault*). It is never a block: a double that needs logic is the signal to promote
  the tier.
- **`stub` is capability-only.** An agent's realness is the tier's job, not a
  provider's, so `stub` targets a *capability* seam only. Overriding a
  capability the unit does not `consumes` is `bynk.stub.not_a_seam`; naming an
  operation the capability does not declare is `bynk.stub.unknown_op`; a
  `returns` value whose type disagrees with the operation's result type is
  `bynk.stub.rhs_type`.
- **Precedence:** case `stub` > suite `stub` > the tier default.

Bare [observation](#observation) needs **no `stub`** — the recording proxy
records calls at the seam regardless. You reach for `stub` only when the case
depends on a collaborator's *return*.

### Sequenced `stub` — `returns each`

A single value cannot express a collaborator whose successive calls differ — an
advancing clock, a `Kv.get` that returns `None` then `Some`, a network that fails
twice then succeeds. The **return-sequence** form supplies one outcome per call, in
order:

```bynk
stub Clock.now()  returns each [1000, 2000, 3000]      -- three successive successes
stub Kv.get(_)     returns each [None, Some(row)]        -- None, then Some…
stub Net.fetch(_)  returns each [fails, fails, ok(resp)] -- fails twice, then succeeds
```

- `returns each [<outcome>, …]` — the `each` distinguishes "one outcome per call"
  from a single call that returns a *list value* (`returns [a, b]` still returns the
  two-element list once). Each `<outcome>` is a value (a success), the atom `fails`
  (a fault), or `ok(v)` when a fault and a success value must sit in one sequence.
- **Exhaustion: the last outcome repeats** (steady state). `[fails, fails, ok(resp)]`
  is "fails twice, then succeeds forever"; `[1000, 2000, 3000]` holds at `3000`
  after the third call. A malformed sequence (e.g. empty) is
  `bynk.stub.bad_sequence`.
- A collaborator that must *compute* its next return (a delta-advancing clock, a
  threaded cursor) exceeds a fixed sequence and is a reserved **virtual fixture** —
  a named follow-on, not shipped in v0.118.

See [`bynk.stub.*` errors](/book/troubleshooting/integration-errors/).

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

## History properties — `for all run: History[Agent]` {#history-properties}

A `property` generates *values*; a **history property** generates a *run* of an
agent. `for all run: History[Wallet]` binds `run` to a generated, driven
call-history of the `Wallet` agent — the generative sibling of `property` now spans
values *and* whole behaviours:

```bynk
suite demo.wallet {
  property "no accepted spend without a prior accepted top-up" {
    for all run: History[Wallet] {
      expect run.all((s) =>
        (s.call is Spend && s.accepted)
          implies run.upTo(s).any((p) => p.call is TopUp && p.accepted))
    }
  }
}
```

The runner:

1. **generates** a bounded random sequence of `Wallet`'s handler calls — each
   handler chosen uniformly, each argument drawn from its parameter's refinement
   domain (the same generator, seed, and shrinker a value `for all` uses);
2. **drives** the sequence against a **fresh** `Wallet` from its initial state,
   invoking the **real** handlers and their real invariants — so every state in
   `run` is one a handler actually reached (reachability by construction, never a
   fabricated state);
3. **binds** `run` and evaluates the predicate; on failure it reports the seed and a
   **shrunk minimal failing sequence**.

### A history is a `List[Step]`

`run` is an ordinary `List` — assert it with the surface you already know (`.all` /
`.any` / indexing / `.length()`), exactly as [`trace(Cap.op)`](#observation) is a
`List`. There is no temporal vocabulary: "always P" is `run.all((s) => P)`,
"eventually P" is `run.any((s) => P)`, and "P before Q" is a quantified prefix check
via `run.upTo(s)` (the history strictly before step `s`). Each `Step` carries:

| Field | Type | Meaning |
|---|---|---|
| `.call` | a sum over the agent's handlers (`Spend { amount }`, `TopUp { amount }`) | which handler ran, with its generated arguments — matched with `is` / `match`. The variant is the handler name with its first letter upper-cased. |
| `.accepted` | `Bool` | whether the handler **committed** a new state (vs. rejected — an `invariant` / `transition` refusal leaves the state uncommitted) |
| `.old` / `.new` | the agent's state | the committed old→new pair (the same `old`/`new` a `transition` sees), so a step is a reached edge of the state graph |

### Rules

- `History[T]` requires `T` to be an **agent** — only an agent has handlers to
  sequence and reachable states to observe (`bynk.history.not_an_agent`). It is
  legal **only** in `for all` position inside a `property`; anywhere else it is
  `bynk.history.outside_property` (it is a generator, not a value type).
- The agent must be **drivable**: every handler parameter must be
  refinement-generable, else `bynk.history.not_generable`.
- A history property carries **no** `as` — it runs in-process against the real
  handlers, on the generative, flake-free tier ([tiers](#tiers-the-as-tier-clause)
  are `case`-only). Capability seams a driven handler calls are still recordable and
  still [`stub`](#stub)-stubbable, so observation and test doubles compose
  inside a driven run.
- A history property that merely restates a declared `invariant` / `transition`
  (e.g. `run.all((s) => s.new.balance >= 0)` when the agent carries `invariant
  nonneg: balance >= 0`) re-checks a guarantee every reached state already has, and
  is flagged `bynk.history.restates_invariant` (a conservative, syntactic check).

**The bounded-reach ceiling.** A history property is a runner *sample* over
bounded, generated runs — never a proof. **Unbounded** liveness ("eventually P" over
an infinite run) is deliberately *not expressible*: a bounded run can only witness
bounded reach. This is the design choice that keeps history properties cheap and
flake-free and out of model-checking territory. An always-on-every-path guarantee, if
you need one, belongs to a policy / `system`-tier guarantee, not a history property.

On failure a history property reports the run count, the run's root seed, and a
**shrunk minimal sequence** with a reproduce line — see
[`bynk.history.*` errors](/book/troubleshooting/history-errors/). Single-agent
histories are the v1 surface; multi-agent / cross-context protocols are a named
follow-on.

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
pure-observation `case` needs no `stub` or setup at all:

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

## System tests — a flow across Workers {#system-tests}

A `case as system` exercises a **flow across several contexts**, each stood up as
the Worker it actually deploys as — so the real cross-context wire (serialise →
JSON → deserialise → structural projection) is under test, which the in-process
tiers never touch. Its participants are **inferred** from the unit under test's
`consumes` graph, so there is nothing to wire by hand:

```bynk
suite checkout as system {
  case "small order authorises across the wire" {
    let r <- shop.orders.place(100)
    expect r is Ok(_)
  }
}
```

- A case calls into a participant by **qualified name** —
  `shop.orders.place(100)` (a service) — exactly as a cross-context caller would.
  The call travels a simulated Service Binding into the target Worker; any further
  cross-context calls it makes (e.g. `orders → payment`) cross the wire too.
- The inferred set must span **at least two contexts** (the target and a consumed
  one); otherwise `bynk.tier.system_needs_wire`. The closure under `consumes` is
  derived automatically, so a consumed context can never be left unwired.
- A `stub` clause is legal at every tier, `system` included (it overrides one
  seam); what a tier controls is the *default* provision.

Cross-context capabilities (`given B.Cap`) are wired as in production: the
provider is instantiated locally in the consumer Worker (v0.15 model A1).
**Agents** (Durable Objects) work too: a participant's agents are backed by
in-memory Durable Object instances — same key, same instance **within a case**;
state starts empty and is **fresh per case**. See
[Test tiers](/book/guides/testing/integration/).

`bynkc test` runs `system` cases in plain Node alongside the in-process tiers — it
compiles the inferred participants in workers mode under `out/workers/`, stands
them up in-process, and routes the real wire between them. No
`wrangler`/`miniflare` needed.

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
