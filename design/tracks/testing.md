# Testing — one predicate surface

Persistent design doc for a **far-reaching, multi-increment** rethink of how Bynk
expresses tests. The surface is **not yet settled**; this doc is the living map the
per-slice proposals are cut from. It **settles direction, not build** — each slice
is still an ordinary `vX.Y-<slug>.md` proposal that cites this doc and the
foundational ADRs.

- **Status:** Settled — active. Direction merged; the load-bearing organising ADR
  ([0144, one predicate surface](../decisions/0144-one-predicate-surface.md)) landed up
  front. Remaining ADRs are numbered per-slice at authoring time (no range reserved).
  Open decisions have settle-time dispositions (see [Settle dispositions](#settle-dispositions)).
- **Sharpens:** the testing philosophy (`site/src/content/docs/book/guides/testing/philosophy.md`)
  and reference (`site/src/content/docs/book/reference/testing.md`), and extends the
  agent-invariant model (`site/src/content/docs/book/reference/agent-invariants.md`) —
  which already states the thesis
  this track generalises: *"Invariants are the contract half of validation; tests
  are the behaviour half."*
- **Posture:** clean-slate. We design the testing experience we want and let it
  **replace** today's `assert` / `Mock` / `mocks` surface where it improves on it.
  Migration is a per-slice concern, deliberately out of scope here.

## The thesis

Today's testing surface treats tests as a separate sublanguage — `assert <bool>`,
a `Mock[T]` fabricator, a `mocks` re-implementation block. Meanwhile the language
already has a richer way to state a checked claim: an **invariant** is a pure
`Bool` predicate (`implies`, `is`, operators, pure methods) universally quantified
over an agent's reachable states, enforced at the commit boundary.

That predicate is the thing to build on. The redesign is one sentence:

> **One predicate surface, over a ladder of subjects, sourced by supply-or-generation,
> checked at one of three checkpoints, run at one of three tiers.**

Testing stops being a framework bolted to the language. It becomes the predicate
Bynk already has — aimed at more *subjects* (values, type-domains, function calls,
state snapshots, transitions, histories) and checked in more *places*
(commit boundary, dev-build call sites, the test runner). Examples, properties,
contracts, invariants, and interaction checks are not five features; they are
rungs and checkpoints of one idea.

This buys three things at once: **one predicate grammar to learn, not a matcher zoo**
(the framing surface — `suite`/`case`/`property`/`provides`/… — is broad; what goes
*inside* every claim is a single grammar), higher-quality code (durable behavioural
facts become declared guarantees, not test-only assertions), and a developer
experience where failures explain themselves and — at the unit and integration tiers
— ambient nondeterminism cannot cause flakes (the `system` tier reintroduces it by
design; see Pillar IV).

---

## The spine

There is exactly one assertion language in Bynk: a pure `Bool` predicate built from
`is`, `implies`, the operators, and pure value methods. It already exists — it is
the invariant predicate. Production code and tests share it verbatim:

```bynk
expect    balance >= 0          -- in a test
invariant nonneg: balance >= 0  -- on an agent
ensures   nonneg: result >= 0   -- on a function
```

Same grammar, same diagnostics. (Today the predicate pair exists only as
`bynk.invariant.not_bool` / `bynk.invariant.impure_predicate`, while `assert` uses
the inconsistent spelling `bynk.assert.non_bool`. The `expect` slice (1) normalises
`non_bool` → `not_bool` so a single family — `bynk.<position>.not_bool` /
`.impure_predicate` — covers every predicate position.) Moving from writing code to
verifying it introduces no new vocabulary. Everything below is a *facet* of this
surface, not a separate feature.

## Pillar I — the subject ladder

The same predicate, aimed at a widening **subject**. This is the heart of the
track: examples, properties, contracts, and invariants are rungs distinguished
only by *what the predicate is about*.

```bynk
expect balance >= 0                          -- a value
for all a: Account { expect debit(a, a.balance).balance == 0 } -- a type's whole domain
fn debit(a: Account, amt: Money) -> Account
  ensures still_solvent: result.balance >= 0 -- a call (input ⇒ result)
invariant nonneg: balance >= 0               -- a committed snapshot   (exists today)
transition no_overdraft: new.balance >= 0    -- a step (old → new)
property "no spend without a prior top-up" { ... } -- a history (a sequence)
```

`value → domain → call → snapshot → step → history`. The matcher / spy / assertion
distinctions dissolve: there is nothing to *match*, only a subject to *name*. The
lower rungs (snapshot/step) are **invariants** — declared on an *agent*, carried by
the code; the upper rungs (value/domain/call/history) are written in tests, here over
a *value* `Account`. The `balance >= 0` name is shared across both purely for the
motif: the point is one predicate recurring across different subjects, not one entity
— a value type cannot carry invariants, nor can an agent be `for all`-generated
(DECISION P).

## Pillar II — subjects are supplied or generated

A predicate needs a subject to judge. You get one of two ways:

- **supply** it — pin a value: `example clamp(15, 0, 10) == 10`, `Val[T](50)`;
- **generate** it — let the type produce subjects: `for all q: Quantity { … }`.

`Val[T]` fabricates a valid inhabitant; the refinement is its bound; shrinking and a
repro **seed** come along. A `case` (supplied subjects) and a `property` (generated)
are two framings of one claim over one subject — given distinct keywords precisely so
a reader sees at a glance whether generation, with its seed and cost, is in play.

Freedom from **ambient** nondeterminism falls out **here**, not as its own pillar.
A subject is always a pinned value, a generated value, or an **injected capability**
— never something ambient. Clock, randomness, and network are capabilities, so a
predicate cannot secretly depend on them; a test that does not inject a clock cannot
read one. *That* much is structural — not a mode you enable, but a consequence of
subjects having exactly three honest sources. Determinism of the **generator and
stubs themselves** (a fixed seed, fixed stub returns) is a separate matter: a
*discipline* the runner enforces (see Risks), not a structural guarantee.

## Pillar III — three checkpoints

*Where* a predicate is evaluated is orthogonal to *what* it says. The same claim
can sit at any of three checkpoints, and promoting it between them never rewrites
the predicate:

- **commit boundary** — state and transition invariants. Cheap: the runtime already
  holds the old state and the proposed new state.
- **every call, dev builds** — function `requires`/`ensures` as runtime guards.
- **the test runner, adversarial** — the runner generates subjects (values, or
  whole call-histories) and hunts for one that violates the predicate.

This is what makes the invariant↔test duality real. `ensures still_solvent` is one
predicate a dev build evaluates per call and the test runner attacks with generated
accounts. You author it once; the checkpoint is a build/run choice.

## Pillar IV — tier is a dial, not a kind

The one axis that is genuinely *not* about the predicate: how much of the real
world is present when it runs. It is a property of the test case, declared with an
`as <tier>` clause in the case header — and **`unit` is the default, so it is
elided**:

```bynk
suite money {
  case "never negative"                 { … }  -- as unit, by default
  case "used in payment" as integration { … }
}
```

The three tiers borrow the testing pyramid verbatim — `unit` (collaborators
stubbed), `integration` (real collaborators within one context, no wire), `system`
(contexts wired across the real serialise → JSON → deserialise boundary). A
newcomer arrives already holding the model, *and* the pyramid's "many unit, some
integration, few system" proportion advice rides along for free.

One honesty about the flake-free claim (Pillar II): it is a **unit/integration**
property. The `system` tier runs deployed Workers over the real wire — real I/O,
timing, and network — so it *reintroduces ambient nondeterminism by design*. System
tests trade the structural guarantee for realism; their nondeterminism is a
first-class operational concern (bounded and seeded where possible), not something
the language rules out.

The load-bearing commitment is the framing: these are **not three kinds of test,
but one body promoted**. `as integration` reads as "run this *as* an integration
test", not "this is a different artefact". Promoting a case means **removing stubs,
not rewriting it** — the body is untouched; only what stands behind its seams
changes. And because state and transition invariants are checked at the commit
boundary, they fire at *every* tier for free — so promoting a case can surface a
collaborator's invariant a stub was hiding, with no new test code. The state
lifecycle is fixed across tiers: the unit under test is always a real in-memory
instance, keyed normally, fresh per case; only the realness of its *collaborators*
and whether sends cross a serialisation boundary change.

`as` sits on the **suite** header too, setting a default its cases inherit and
override (`suite checkout as integration { … }`) — the "set once" ergonomics of the
old `test integration` block, without a separate construct. Tiers are a **`case`
affordance only**: a `property` generates, and does not promote (Principle 5).

---

## Tests in practice

A small `commerce` domain runs through the examples: a pure `money` module, an
`Order` agent (the one from the invariant reference), and a cross-context checkout.

### A pure function — example, property, and contract together

The contract lives on the function. The suite adds an illustrative `case` and a
`property`; the contract is checked at *both* the call site (dev builds) and by the
runner (generation).

```bynk
-- commerce/money.bynk
fn discount(p: Price, pct: Percent) -> Price
  ensures never_above_input: result <= p
  ensures zero_pct_is_identity: pct == 0 implies result == p
{ ... }
```

```bynk
-- tests/commerce/money.bynk
suite money {
  case "a tenth off a tenner" {
    example discount(1000, 10) == 900   -- the documented witness
  }

  property "more discount, never a higher price" {
    for all p: Price, a: Percent, b: Percent where a <= b {
      expect discount(p, b) <= discount(p, a)  -- relational: no single `ensures` can say this
    }
  }
}
```

The runner generates `Price`/`Percent` from their refinements, includes the
boundary values, and reports a **shrunk counterexample with a seed** on failure.
The `ensures` clauses need no test at all — they are checked everywhere `discount`
is reachable.

**The division of labour** — and the rule that stops properties duplicating
contracts: a claim about *one* result belongs in `ensures`, checked everywhere for
free; a property earns its keep only when the claim is **relational or spans calls**
— monotonicity, round-trips, commutativity — which no per-call postcondition can
express. Restating an `ensures` as a `for all` is redundant; the property above
says something the contract structurally cannot, because it relates *two* calls.

### An agent — invariants the unit test inherits

The agent declares its durable behavioural facts as the three invariant rungs:

```bynk
-- commerce/order.bynk
type OrderStatus = enum { Pending, Placed, Paid }

agent Order {
  key id: OrderId

  store status:     Cell[OrderStatus] = Pending
  store user:       Cell[Option[UserId]]
  store cart:       Cell[Option[Cart]]
  store paymentRef: Cell[Option[AuthId]]

  -- snapshot: a committed state is internally consistent
  invariant placed_has_user_and_cart:
    status == Placed implies (user.isSome() && cart.isSome())
  invariant paid_has_payment_ref:
    status == Paid implies paymentRef.isSome()

  -- step: a paid order can never become unpaid (uses `is`/`implies`; needs no enum Ord)
  transition paid_is_terminal: old.status is Paid implies new.status is Paid

  on call place(u: UserId, c: Cart) -> Effect[()] {
    status := Placed
    user   := Some(u)
    cart   := Some(c)
    ()
  }

  on call pay(ref: AuthId) -> Effect[()] {
    paymentRef := Some(ref)
    status     := Paid
    ()
  }
}
```

A unit test exercises behaviour the invariants *don't* state, and gets the three
invariant rungs enforced underneath it for free:

```bynk
-- tests/commerce/order.bynk
suite order {
  case "placing then paying records the reference" {   -- as unit, by default
    let o = Order(Val[OrderId])
    let _ <- o.place(Val[UserId], Val[Cart])
    let _ <- o.pay(Val[AuthId]("auth_123"))
    expect o.status() is Paid
    expect o.paymentRef() is Some("auth_123")
    -- placed_has_user_and_cart, paid_has_payment_ref, paid_is_terminal:
    -- all checked at each commit, no lines spent on them
  }
}
```

### Interaction — observation, not spies

Because `Logger` is injected at a known seam, the runtime records its calls
automatically — so observation needs no setup at all. You override a collaborator's
*return* only when the test depends on it (`provides`, see DECISION R); a
pure-observation case supplies nothing:

```bynk
case "an oversized order is rejected and logged" {
  let r <- Orders(Val[AcctId]).place(50000)
  expect r is Err(_)
  expect Logger.log called once with msg is "rejected: amount too large"
  expect Store.put never called                  -- a rejected order writes nothing
}
```

For anything the sugar (`called` / `never called` / counts / `with <predicate>` /
`before`) doesn't cover, bind the trace and assert with ordinary Bynk — so the
observation vocabulary never has to grow to stay expressive:

```bynk
let calls = trace(Logger.log)
expect calls.count == 2
for all c in calls { expect c.msg.length > 0 }
```

Observation covers the *scenario-specific* facts a test cares about. A **universal**
emission guarantee — "every payment audits, on every path" — is not a per-case
assertion at all: it belongs to a cross-cutting policy or a use-case minimal
guarantee (DECISION E, U), the one emission claim a test cannot give.

### The tier dial — one body, three tiers

The same checkout case, promoted by changing only the header — the body is
identical at every tier:

```bynk
case "a small order authorises end to end"                { … }  -- as unit (default)
case "a small order authorises end to end" as integration { … }  -- real Payment, no wire
case "a small order authorises end to end" as system      { … }  -- deployed Workers, real wire
```

At `unit`, `Payment`'s own invariants don't run (it's stubbed). At `integration`
and `system` they do — so a green `unit` that fails at `integration` means a
collaborator's invariant caught a defect the stub had been hiding. The dial turns
"did I mock this faithfully?" into a checkable question — and the body never moved.

### What a failure looks like

Failures report against the *structure* of the predicate, not a bare location —
plus, for an interaction or agent test, the recorded effect trace:

```text
commerce.money › more discount, never a higher price
  property failed after 41 cases (seed 0x5f3a)
  shrunk counterexample:  p = 100, a = 10, b = 20
  expected:  discount(p, b) <= discount(p, a)
  actual:    discount(100, 20) == 95  >  discount(100, 10) == 90
  reproduce: bynkc test commerce/money.bynk --seed 0x5f3a

commerce.order › an oversized order is rejected and logged
  expect Store.put never called — but it was called once
    Store.put(OrderRow{ id: …, amount: 50000 })   at order.bynk:41
  effect trace for this case:
    1. Logger.log("rejected: amount too large")
    2. Store.put(OrderRow{ … })                    ← unexpected
```

---

## The surface, construct by construct

Indicative only — the normative grammar is written once, in the spec, per slice.

| Construct | Subject rung | Checkpoint |
|---|---|---|
| `suite <name> { … }` | — (container) | — |
| `case "…" [as <tier>] { … }` | example-based test | test runner |
| `property "…" { for all … }` | generative test (domain / history) | test runner (generated) |
| `example <expr>` | value (pinned) | test runner |
| `expect <pred>` | value | test runner |
| `for all x: T where <pred> { … expect … }` | type domain | test runner (generated) |
| `Val[T]` / `Val[T](pin)` | value fabrication | — |
| `provides Cap.method(<args>) returns <v> \| fails` | provider override (stub / fault) | test runner |
| `expect Cap.op called …` / `trace(Cap.op)` | recorded emissions | test runner |
| `requires <name>: <pred>` / `ensures <name>: <pred>` | function call | dev call site **and** runner |
| `invariant <name>: <pred>` | committed snapshot | commit boundary *(exists today)* |
| `transition <name>: <pred over old/new>` | step | commit boundary |

All predicate positions share the invariant predicate surface: `implies`, `is`
(with bindings in scope across the predicate), operators, pure methods; no effects,
capabilities, or test-only constructs inside a predicate.

`example` vs `expect`, since both are boolean predicates over pinned values: use
`example` for a self-contained input→output **witness** that documents behaviour and
can be lifted into the reference docs (`example discount(1000, 10) == 900`); use
`expect` for an assertion over a value the case **computed or bound** (`expect bal ==
300`). When in doubt, `expect`.

## Principles

Five rules make the keywords cohere — the invariants of the *design*, several
compiler-enforced:

1. **A contract is a property that is always on.** `ensures` / `invariant` /
   `transition` are the generative check scoped to a function or agent *forever*; a
   `property` in a suite is the same mechanism scoped to that suite. A `case` should
   not restate a contract already declared at the source — flagged by a *conservative*
   check (`bynk.test.redundant_contract`: syntactic / α-equivalent, not full semantic
   equivalence — see Risks).
2. **Emission is observed or guaranteed, never declared per-handler.** What a handler
   emits is derivable from its (small, explicit) body — Bynk already rejects a
   source-level `@requires` for being derivable (v0.99, ADR 0127) — so there is no
   per-handler `emits` clause to restate it. A scenario-specific emission is a test
   observation (`expect Cap.op called` / `never called`); a *universal* "always emits"
   guarantee is a cross-cutting policy / use-case minimal guarantee (DECISION E, U).
3. **Outcomes and faults are different kinds of failure.** A `Result` / `Err(…)` is
   an expected, typed, in-band *outcome*, asserted directly in a `case`. A contract
   or invariant violation is a *fault* — caught by the runner, never something a
   `case` asserts against.
4. **Only a `consumes`-declared capability can be `provides`-overridden.** A test
   substitutes a provider at a seam the unit already declares; overriding a capability
   the unit never `consumes` is an error (`bynk.provides.not_a_seam`). So `consumes`
   (declare) and `provides` (substitute) stay in step — and `given` reverts to its one
   production meaning, *require* a capability in scope (see DECISION R).
5. **Promotion changes substitution, not assertion.** `as integration` swaps what
   stands behind a seam; the `case` body's `expect`s are unchanged. That is what
   makes a tier promotion cheap to read — and why tiers are a `case` affordance only:
   a `property` generates, so promoting it would multiply generation by
   real-collaborator cost and re-admit the ambient nondeterminism the tier removes.
   To check a generated input end-to-end, promote *that witness* as a concrete
   `case as integration`.

## Decisions

**[DECISION A] One predicate surface is the organising commitment.** Every checked
claim — `expect`, `ensures`, `invariant`, `transition`, observation predicate — is
the existing invariant predicate (`implies`/`is`/pure `Bool`).
No second assertion grammar, no matcher library. *Load-bearing ADR
[0144](../decisions/0144-one-predicate-surface.md) — settled up front.*

**[DECISION B] `assert` is replaced by `expect`.** `assert` reduces a claim to one
bit and reports only a location; `expect` retains operator/operand structure and
reports expected-vs-actual for free. Clean-slate: we remove `assert` rather than
keep both. *ADR.*

**[DECISION C] Value fabrication is `Val[T]`; the word "mock" is retired.** `Val[T]`
fabricates a *valid inhabitant* of `T` (DECISION P); `Val[T](v)` pins one. Bare, it
is the boundary-inclusive seed the runner draws from under `for all`; pinned, it is
the supplied subject a `case` uses. This **retires `Mock[T]`**, whose name collided
with mocking a *collaborator* — and, with the old `mocks` re-implementation block
already replaced by `provides` (DECISION R), removes the word "mock" from the language
entirely: collaborators are `provides`d, values are `Val`. A custom generator (a distribution the
refinement cannot express) is a reserved future `Gen[T]` / `Arb[T]`, unneeded while a
type is its own inhabitant space. Also retires the "every test silently exercises the
edge value" dishonesty. *ADR.*

**[DECISION D] Contracts are invariants for functions.** `requires`/`ensures` reuse
the invariant predicate verbatim; `requires` filters generation, `ensures` is the
target. Justified by Bynk already having invariants — the concept is familiar by
construction. *Load-bearing ADR.*

**[DECISION E] The invariant subject widens to the *step* (`transition`) — but not to
emission.** `transition` (old→new) is a commit-boundary invariant, checked where
snapshot invariants already are; it is intrinsic to the agent's own state. Emission is
*not* an invariant rung: what a handler emits is derivable from its body, and Bynk
already rejects a source-level `@requires` on exactly that ground ("the requirement is
derivable, so authoring it would restate an internal", v0.99 / ADR 0127). A per-handler
`emits` clause would restate the body — noise in a language of small, explicit
functions — and only ever covered the *unconditional* case. Emission facts split by
purpose instead: "what does this touch" is already at the signature (`given`) and in
tooling; a scenario-specific emission is a **test observation** (DECISION F); and a
*universal* guarantee ("every payment is audited, on every path") is a cross-cutting
**policy** / **use-case minimal guarantee** — the one emission claim a test cannot give,
since a test checks only the paths it runs (DECISION U). *Load-bearing ADR.*

**[DECISION F] Observation has a thin sugar plus a bound-trace escape hatch.**
Sugar: presence (`called`/`never called`), count, `with <predicate>`, ordering
(`before`). Everything else: `trace(Cap.op)` is a value asserted over with ordinary
Bynk. The sugar never has to grow to stay expressive. *ADR.*

**[DECISION G] Tier is an `as <tier>` clause on the case (and suite) header — not a
keyword statement.** Tiers are `unit | integration | system` (the testing pyramid,
for zero-cost schema transfer); `unit` is the default and elided. The clause sits in
the signature where declaration metadata belongs, leaves a clean default, composes at
suite and case level (case wins), and — the load-bearing reason — frames a tier as
**one body promoted, not a distinct kind of test**. `as integration` fills the
missing middle between today's unit and integration tests; the state lifecycle is
fixed across tiers. Subsumes today's `test integration`. Tiers are a **`case`-only**
affordance — a `property` generates, and does not promote (Principle 5); a suite-level
`as` therefore binds its `case` members only, and any `property` under a tiered suite
ignores it. The `as` spelling is **confirmed by DECISION N** (it disambiguates
cleanly from the `consumes … as` alias — resolved at settle).

*Rejected, recorded because they were close:* (a) a **body-statement keyword**
(`realism`/`fidelity`/`scope`/`tier <level>`) — reads as executable but isn't, and
`scope`/`context` re-collide with lexical scope and the `context` unit kind; (b) a
**tier-leading case keyword** (`integration "…"`, `unit "…"`) — terser, but it
re-instates "three kinds" *in the grammar* (sibling productions per tier), removes
the clean default, and reserves the common nouns `unit`/`system` as statement heads
(`unit` also collides with the `()` unit type). The semantic question sits *under*
the syntax: a tier is a promotion of one body, not a separate artefact — and `as`
is the only candidate whose grammar says so. *Load-bearing ADR.*

**[DECISION — open H] How far the temporal/history rung reaches.** Cross-handler
"eventually" protocols (`property "…"` over a generated call-history) open an LTL /
bounded model-checking surface. *Recommend:* treat history-level protocols as
**test-runner-only** universals, deferring any commit-time temporal enforcement.
Flagged for sign-off.

**[DECISION — open I] Keyword surface for the step rung.** `transition <name>:`
over `old`/`new`, versus folding it into `invariant <name>: step old -> new { … }`
to keep a single keyword. *Recommend* the dedicated `transition` for readability;
open.

**[DECISION — open J] Whether `requires`/`ensures` run at runtime by default.**
Always-on dev-build guards (caught early, costs cycles) vs runner-only (no runtime
cost, later feedback) vs per-profile. *Recommend* per-build-profile, on in dev/test,
off in release. Open.

**[DECISION — open K] Do `integration`/`system` infer their participants?** Today
`test integration` requires an explicit `wires` list. The compiler already knows the
`consumes` graph, so `as system` could *derive* the real/wired set and retire `wires`
entirely. *Recommend* inference (one fewer thing to declare); open pending a case
where the participant set is genuinely ambiguous and an explicit `wires` would
disambiguate.

**[DECISION — open L] Constraint regimes split by tier.** With per-case `as`, the
"≥2 contexts, no `mocks`" rules attach to `system` (cross-context, wired) and do
*not* apply to `integration` (real collaborators within one context). This relaxes
today's integration constraints — recorded as a deliberate consequence of the
naming, not an accident.

**[DECISION M] Per-seam override is `provides`, at case or suite scope.** `as <tier>`
sets the *default* provision of every seam (unit → doubles, integration/system →
real); a `provides Cap.method(<args>) returns <v> | fails` clause overrides *one
method's* provision under test. This is **capability-only** — an agent's realness is
the tier's job, not a provider's (correcting the earlier `given Payment …`, which
named an agent). Precedence: case `provides` > suite `provides`, both over the tier
default. Mechanism in DECISION R.

**[DECISION N] `as` disambiguates cleanly from the `consumes … as Alias` keyword —
confirmed at settle.** `as` is today's capability-alias keyword (`consumes b as
Alias`, `site/src/content/docs/book/spec/syntactic-grammar.md:144`), so it is not
lexically free. Verified at settle against the parser: `As` is a reserved token
consumed **only** in `consumes`-declaration position (after `consumes
<qualified_name>`, `bynk-syntax/src/parser/declarations.rs:462`). The tier clause
reuses `as` in **case/suite-header position**, a distinct production where no
`consumes` is in scope, so the parser reaches `As` only where it already dispatches
by leading keyword (`consumes` vs `case`/`suite`) — no ambiguity, DECISION G's
spelling holds. Had the check failed, the pre-agreed fallback was a single short
dedicated keyword (never the rejected forms in DECISION G).

**[DECISION — open O] Enum positional `Ord` is a prerequisite for *ordered*
transitions.** A `transition` like `new.status >= old.status` needs enums to be
ordered — and today they are not: the changelog enumerates the orderable types
(`Instant`, v0.90; `Duration`, v0.86) and enums never appear, while `agents.md`
notes legal-transition tables are a later increment. The track's `transition`
examples are therefore written with `is`/`implies` (`old.status is Paid implies
new.status is Paid`), which need no ordering. Whether to grant enums a
**declaration-positional `Ord`** so `>=` becomes available is a prerequisite to
decide *before* slice 4 ships ordered-status transitions. *Prereq for slice 4.*

**[DECISION P] A generated subject is a *valid* inhabitant of its type — but for
agents, validity is not reachability.** For a value of a refined / record / sum type,
`Val[T]` / `for all` produce only inhabitants that satisfy the refinements: the
generator *is* the type's inhabitant space. Two consequences. First, a property whose
predicate merely **restates a refinement** — `for all q: Quantity { expect q > 0 }`
when `Quantity` is `Int where Positive` — is redundant;
the runner should flag it (a *conservative* check — see Risks), since it re-checks a
guarantee the type already gives. Domain properties earn their keep by asserting
**behaviour over valid inputs** (what a function or flow *does* to them).

**Agents are the exception.** Fabricating an agent *state* that satisfies every
invariant does **not** make it a state any handler sequence can actually produce — a
valid-but-unreachable state fed to a generative `transition` or behavioural property
yields counterexamples production can never hit (false positives that erode trust in
the runner). So agent behavioural generation must generate **handler sequences** (drive
the real handlers from the initial state and observe the states they reach), *not*
fabricate states — which is exactly the history rung (slice 7). Snapshot and step
invariants are unaffected: they run at the commit boundary over the *real* old→new of
an actual handler, never a fabricated state. Bounding and shrinking the sequence
generator is **open** (Risks). *Load-bearing ADR (defines generation).*

**[DECISION Q] Test vocabulary: `suite` / `case` / `property`.** The braced `test`
block splits into three honest words: `suite <name>` is the container; `case "…"` is
an example-based test (a body of `example` / `expect`); `property "…"` is a generative
test (a body of `for all … expect`). Naming the example-vs-generative split
structurally lets a reader see whether generation — with its seed and cost — is in
play, and gives `as <tier>` one unambiguous home (the `case` header; Principle 5).
Replaces today's `test`, which overloaded container *and* case. *ADR.*

**[DECISION R] Test-time provider substitution is `provides`, not `given`.** A test
double is an alternative *provider* of a capability, and Bynk already spells that
`provides` (`provides Clock = SystemClock { … }` in production). So a stub is a
**provider scoped to a test** — the same seam and mechanism production uses, which is
what the track has meant since the philosophy note on `mocks`. This completes a clean
triad: `consumes` *declares* a seam, `given` *requires* it at a use site, `provides`
*supplies* it — with `given` no longer doing double duty (declare vs supply). A suite
runs inside its context, so consumed capabilities are already in scope via `given` as
normal; a test `provides` **changes the provision under test**. The form is *always an
explicit call pattern*, never sugar:

    provides Clock.now()          returns Instant.fromEpochMillis(1000)
    provides Rates.lookup("GBP")  returns 1.25
    provides Rates.lookup(_)      returns 1.0     -- fallback; first match wins
    provides Kv.get(_)            fails           -- inject a fault (Principle 3)

Naming the method with its call pattern keeps the target unambiguous (no "which
method?" inference), makes arg-shape matching uniform, and reuses the predicate
surface for the pattern (`_`, literals, `is`) — no matcher words. The right-hand side
is a *value* or `fails`, **never a computed body**: a stub that needs logic is the
signal to use the real provider or promote the tier, which is what stops `provides`
becoming the `mocks` block reborn. `provides` is **capability-only** and sits at case
and suite scope (DECISION M). Bare observation needs no `provides` — calls are recorded
at the seam (DECISION F). A stateful/sequenced return (an advancing clock,
retry-then-succeed, pagination) strains the value form and is **not a corner case** —
it is its own open item (DECISION V), which slice 6 cannot ship without. *Load-bearing
ADR.*

**[DECISION S] File & build model: test-ness is structural; `[paths]` lists the tree,
not roles.** A `suite` is a *test-only declaration kind*, so test-ness is a property
of the declaration — not the file, the filename, or the directory. The language
already gates `assert`/`expect` at the block, not the path, so this is the honest
extension of what exists. It settles the whole file/build model:

- **One extension, no filename marker.** Everything is `.bynk`; no `.test.bynk`. A
  filename convention would only restate what the `suite` keyword already says.
- **`suite` is legal in any file.** A single *atomic* file may hold `commons` +
  `context` + `suite` together — the shareable / single-file / playground case (the
  in-browser track). Conventionally they are separate files, but that is convention,
  not a rule.
- **The emitter strips test-only declarations from the production build.** `bynkc
  build` skips every `suite` block wherever it sits — never type-checked for the
  build, never emitted to the Worker (`#[cfg(test)]` with the keyword as the cfg);
  `bynkc test` compiles and runs them. So an atomic file ships its `commons` /
  `context` and drops its `suite`; a pure-suite file emits nothing.
- **Discovery scans the whole source tree** for `suite` declarations, not a
  designated folder.
- **`[paths]` enumerates the source tree, not roles.** The role-named `src` / `tests`
  are dropped for a flat `include` / `exclude` pair naming what to compile:

      [paths]
      exclude = ["vendor"]     # `include` defaults to the project root

  `[paths]` is optional: `include` defaults to the project root (minus the tool's own
  build-output cache), so a conventional `src/` + `tests/` layout needs no config;
  `include` / `exclude` are the escape hatch for monorepos, vendored, or generated
  `.bynk`. `tests/` survives as a pure human convention, unmentioned by the build.

A consequence to accept: with roles gone from config, placement is inert in *both*
directions — a `context` misplaced under `tests/` now emits like any other. A team
wanting the old separation enforces it with a lint, not the build config. *Load-bearing
ADR (build model).*

**[DECISION — open T] Visibility on co-location.** A `suite` names its target and
sees that unit's testable surface *because it targets it*, independent of file.
*Recommend* keeping visibility target-driven and identical everywhere, so moving a
suite between files is semantically inert. The alternative — co-location granting
white-box access to a unit's internals (Rust-inline-test style) — is deferred; if
wanted, it should hang off the *target declaration*, not the file. Open.

**[DECISION — open U] Where a universal emission guarantee lives.** Dropping
per-handler `emits` (DECISION E) relocates the one legitimate emission claim — "this
effect happens on *every* path, and stays that way" (audit / compliance) — but *where*
it lands is open: a cross-cutting **policy** ("every state-changing handler emits
`Logger.audit`", declared once over many handlers) or a **use-case minimal guarantee**
(Cockburn — "this interaction guarantees, even on failure, that the attempt is
recorded"), which awaits the still-unsettled use-case / `feature` construct. Both are
declared *once*, never per-handler. *Recommend* deferring until the use-case construct
is settled; a `policy` form is the smaller interim option. Open.

**[DECISION — open V] Stateful / sequenced `provides`.** The value-only rule
(DECISION R) cannot express a collaborator whose successive calls differ — an advancing
`Clock.now()`, a `Kv.get` that returns `None` then `Some`, a network that **fails
exactly twice then succeeds**, pagination. This is routine, not a corner, and the "use
the real provider" escape *fails for fault sequences* (you cannot ask a real network to
fail twice then succeed). Two candidate forms: a **return-sequence** (`provides
Clock.now() returns [1000, 2000, 3000]`, consumed per call, with a defined exhaustion
rule) or a small **virtual fixture** advanced in the test (`provides Clock =
VirtualClock(from: …)`). **Settle lock:** the **return-sequence** is the shipping
form (predicate-surface consistency), with a fixture reserved for genuinely stateful
protocols; the exhaustion rule and the fixture boundary are **finalised at slice 6
authoring**. **Gates slice 6** — `provides` cannot ship complete without it.

## Settle dispositions

The settle step (docs-only, no version bump) landed the one hard-to-reverse
organising ADR ([0144](../decisions/0144-one-predicate-surface.md), DECISION A) and
gave every decision a disposition. The per-slice ADRs (B, C, D, E, F, G, M, P, Q, R,
S) are **resolved in the doc but numbered and written with their slice**, so they
land beside the code that embodies them. The remaining open decisions dispose as:

| Decision | Disposition | When it closes |
|---|---|---|
| **A** | Resolved — ADR 0144, landed up front | Settle |
| **N** | Resolved — `as` disambiguates cleanly; spelling kept | Settle |
| **V** | Form locked (return-sequence); exhaustion/fixture detail finalised with the slice | Settle → slice 6 |
| **I** | Recommend the dedicated `transition`; confirm at authoring | Slice 4 |
| **J** | Recommend per-build-profile (on dev/test, off release) | Slice 3 |
| **K** | Recommend participant inference from the `consumes` graph | Slice 6 |
| **L** | Tier-split constraints (`system` cross-context, `integration` relaxed) | Slice 6 |
| **O** | Ship slice 4 with `is`/`implies`; enum positional `Ord` is a *separate* prereq only if ordered-status transitions are wanted | Before ordered transitions |
| **T** | Defer — recommend target-driven visibility, identical everywhere | Deferred |
| **U** | Defer until the use-case / `policy` construct settles | Deferred |
| **H** | Defer — history protocols are test-runner-only; commit-time temporal enforcement deferred | Slice 7 |

## Slice decomposition (ordered, indicative)

1. **`expect` + `suite`/`case`, and the file/build model** — two *separable* landings,
   cut independently:
   - **(1a) `expect` + `suite`/`case` + structural failure reporting** — replace
     `assert` and the braced `test`; expected-vs-actual output. The smallest standalone
     win. (DECISION A, B, Q)
   - **(1b) File & build model** — `suite` legal anywhere, stripped from the production
     emit, discovered across the tree; role-named `[paths] src`/`tests` → flat
     `include`/`exclude`. Independently riskier (emitter strip, tree-wide discovery,
     config change), so cut on its own. (DECISION S)
2. **`Val[T]` + `property`/`for all` + seeds/shrinking.** Generative tests over
   refinement domains; `case`/`example` the supplied form; `Val[T]` fabricates valid
   inhabitants. (DECISION C, P, Q)
3. **Function contracts.** `requires`/`ensures`, generation filtered by `requires`,
   runner attack + optional dev-build guard. (DECISION D, P; *runtime-default gated
   on open DECISION J*)
4. **Step invariants.** `transition` at the commit boundary. (DECISION E; *gated on
   open DECISION I, and on the enum-`Ord` prerequisite DECISION O for ordered-status
   transitions*)
5. **Observation surface.** Auto-recorded trace; the sugar; `trace()` escape hatch.
   (DECISION F)
6. **The tier dial + `provides`.** `as unit|integration|system` on case and suite
   headers, `unit` default; absorb `test integration`; participant inference;
   per-seam `provides` override (explicit `Cap.method(args) returns | fails`); nail the
   agent-state lifecycle per tier. (DECISION G, K, L, M, R; *`as` disambiguation gated
   on open DECISION N; sequenced `provides` gated on open DECISION V*)
7. **History/protocol properties** *(gated on DECISION H)*. Generated call-histories;
   runner-only.

Slices 1–3 stand alone and deliver value independently. 4–6 build the
invariant↔test duality and are best landed in order. 7 is the visionary tail and
must not block the rest.

## Risks

- **Generation cost & determinism.** Property runs must be fast and reproducible:
  fixed seed by default, bounded case counts, boundary-biased sampling, printed
  repro command. A flaky generator would poison the flake-free claim.
- **Agent-state generation: validity ≠ reachability.** Fabricating an agent state that
  satisfies every invariant does not make it *reachable* by any handler sequence, so a
  generative `transition` / behavioural property fed fabricated states can fail on a
  state production can never reach — a false positive that erodes trust. The sound route
  is **handler-sequence generation** (drive the real handlers, observe reached states;
  the history rung, slice 7), not state fabrication (DECISION P). Bounding and shrinking
  those sequences is open.
- **The flake-free claim is unit/integration only.** The `system` tier runs real I/O,
  timing, and network, so ambient nondeterminism returns by design (Pillar IV). The
  structural guarantee is scoped to the lower tiers; system-tier nondeterminism is an
  operational concern (bounded, seeded), not a language guarantee.
- **Redundancy checks rest on equivalence detection.** `bynk.test.redundant_contract`
  (Principle 1) and the restates-a-refinement flag (DECISION P) must decide that one
  predicate restates another — a hard semantic-equivalence problem where a *false
  rejection blocks a valid test*, worse than a missed one. Scope both to a
  **conservative, syntactic** check (α-equivalent predicate over the same subject),
  never full semantic equivalence; accept that near-duplicates slip through.
- **The universal-emission guarantee has no home yet.** Dropping per-handler `emits`
  (DECISION E) leaves "every payment audits, always" to a policy / use-case minimal
  guarantee that is still open (DECISION U). Until it lands, that guarantee can only be
  a test observation over the paths a suite covers — a universal reduced to samples, and
  a known gap.
- **Tier-dial state semantics.** "Same body, higher tier" is clean for pure
  collaborators; Durable-Object state per tier needs a written, tested contract
  (fresh-per-case, keyed normally, invariants at every tier).
- **Stub-return fidelity at `unit`.** An auto-stub must return *something*, and that
  value is itself test-only behaviour — so a green `unit` can be green precisely
  because the stub handed back a convenient value the real collaborator never would.
  The tier dial mitigates it (promote to `integration` and the real return is
  exercised), but the auto-stub's return contract — derive from the capability's
  result type, and surface it in the trace so it is *visible* — must be stated so the
  convenience is never silent.
- **Clean-slate churn.** Removing `assert`/`mocks` is a real migration; sequenced
  per slice with codemods, out of scope for this direction-setting doc.

## Docs delta (per slice, sketched)

Each slice leaves the book (`site/src/content/docs/book/`) current: a reference page section for its construct,
a guide entry (and an "Understand" on-ramp for the genuinely new concept — the
*one predicate surface* framing belongs in `guides/testing/philosophy.md`, rewritten
around this spine), a `reference/changelog.md` row, and the currency banner advanced.
The philosophy page is the keystone rewrite: it should open with the spine and
present examples/properties/contracts/invariants/observation as rungs, not features.

## Open questions

- Does the history rung (DECISION H) earn a place in v1, or ship as a named
  follow-on once rungs 1–6 are in use?
- Should `example` be its own keyword, or is a single-case `for all` with a pinned
  subject enough? (Leaning: keep `example`; the interim `example`-vs-`expect` rule is
  stated under the construct table.)
- *(Resolved by DECISION P for function inputs and values — one generator over a type's
  valid inhabitants. Agent behavioural generation is **not** the same mechanism: it
  needs handler-sequence generation for reachability, still open — see Risks.)*
- What is the typed shape of `old` / `new` / a bound `trace(...)` such that `is` and
  `implies` apply unchanged and `impure_predicate` still bites?

## Out of scope (for now)

Named so the boundary is deliberate, not an oversight:

- **`flaky` / quarantine — declined.** The flake claim (narrowed to *ambient*
  nondeterminism) makes a recurring failure a broken seed or leaky stub — a bug to
  fix, not a state to annotate. A quarantine keyword would legitimise the very
  nondeterminism the design rules out.
- **Coverage — a runner feature, not a keyword.** The meaningful metric here is
  *contract/refinement coverage* (which `ensures` / `invariant`s / refinements a run
  exercised), not line coverage. Whether the runner reports it is a later call.
- **Snapshot/golden `case`s, Pact-style cross-service `contract` cases, mutation
  testing.** Named and deferred: snapshots if outputs grow large (rendered views,
  generated documents), a first-class `contract` case if services multiply beyond the
  `as system` HTTP-edge check, and mutation testing as a meta-tool (it tests the
  tests), never a language keyword.
