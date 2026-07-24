#import "../template.typ": code-listing, compiler-message

#let source-lines(path, start, end) = {
  read(path).split("\n").slice(start, end).join("\n")
}

= Tests should preserve the architecture <tests-should-preserve-the-architecture>

Part II made a series of architectural claims executable. Capabilities name
effects. Agents own state. Contracts constrain commits. Actors identify callers.
Entry protocols retain the agency of requests, messages, schedules, and
connections.

A compiler can preserve all of those declarations while a test quietly avoids
them.

The test may call an inner function instead of the service boundary, construct
state instead of addressing its owner, replace a capability with an object that
implements a second version of the policy, or connect components in a way the
production graph never permits. Every assertion can pass. The deployed system
can still fail at the first boundary the test removed.

This is the peculiar danger of test architecture. Isolation makes a difficult
system easier to control, but every simplification changes the thing from which
confidence is being inferred.

#quote(block: true)[
  A test does not stand outside the architecture. It constructs an architecture
  in which its claim is evaluated.
]

== A green test can describe another system

Consider an ordinary TypeScript test around order placement:

#code-listing(
  [A direct test replaces both collaborators with local objects],
  read("../snippets/chapter-09/conventional.ts"),
  lang: "typescript",
)

This is a useful test. It establishes that `placeOrder` returns `rejected` and
does not save when the supplied bank declines. The collaborators are explicit,
the result is deterministic, and the failure message is adequate.

Its name can easily acquire more authority than its evidence. The test does not
show that the production bank adapter declines the same amount. It does not
cross the orders-to-payment boundary, serialise a request, invoke an actor, or
address an agent. The bank rule `amount <= 10_000` has been copied into the test,
so the test and the implementation may agree with each other while both disagree
with the real provider.

None of this makes the test bad. It makes its claim local: *given these
collaborators, this function responds correctly*. False confidence begins when
that result is read as evidence about wiring, boundaries, or providers the test
did not exercise.

The common repair is to add integration and end-to-end tests. That helps, but it
often produces three suites with three setup languages and three slightly
different scenarios. The unit test calls a function with mocks, the integration
test assembles containers, and the end-to-end test drives HTTP through a separate
client. As the bodies diverge, promotion stops being a comparison of the same
claim under more realistic conditions.

== Substitute at the declared seam

Bynk starts from the effect boundaries the production program already names.
The pricing context requires rates and auditing as capabilities:

#code-listing(
  [The production service exposes exactly two substitution seams],
  source-lines(
    "../snippets/chapter-09/declared/src/commerce/pricing.bynk",
    2,
    9,
  ) + "\n\n" + source-lines(
    "../snippets/chapter-09/declared/src/commerce/pricing.bynk",
    18,
    25,
  ),
  lang: "bynk",
)

The test substitutes those methods at those seams:

#code-listing(
  [A stub supplies outcomes; observation reuses the same capability boundary],
  read("../snippets/chapter-09/declared/tests/pricing.test.bynk"),
  lang: "bynk",
)

`stub Rates.lookup("GBP") returns 2` is deliberately smaller than a fake rate
service. The left side identifies a capability operation and an argument
pattern. The right side is a value. A stub may instead say `fails`, or supply a
fixed sequence of outcomes, but it cannot contain a computed body.

That restriction prevents the double from growing its own currency rules,
cache, fallback policy, and error handling. When the test needs that much
behaviour, the answer is to run more of the real system rather than maintain a
parallel implementation under the test directory.

The next two expectations observe interaction without installing spies.
Capability calls are already routed through known seams, so the test build can
record them. The deploy build does not carry that recording. The test asserts
that one rate lookup used `GBP` and that one audit note carried the expected
message.

Interaction checks should still be used with restraint. A test that specifies
every internal call order can make harmless refactoring expensive. The useful
observations are architectural ones: an external charge occurs once, a rejected
operation does not persist, or an audit effect follows an accepted decision.
The seam makes observation possible; judgement decides whether the interaction
belongs in the contract of the scenario.

== Realism should be a setting

Bynk treats unit, integration, and system tests as one case form with a tier:

#figure(
  block(width: 100%)[
    #set text(size: 8.5pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (0.7fr, 1.65fr, 0.9fr),
      inset: (x: 0.5em, y: 0.5em),
      stroke: (x, y) => if y == 0 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Tier],
        text(weight: "semibold")[What becomes real],
        text(weight: "semibold")[Wire crossed],
      ),
      [`unit`], [The target; selected capability seams may be stubbed], [No],
      [`integration`], [Real collaborators within one context], [No],
      [`system`], [Contexts stood up as their deployable Workers], [Yes],
    )
  ],
  caption: [The tier changes the environment around a case, not its assertion language.],
)

`unit` is the default. Adding `as integration` asks for real collaborators
inside the context. Adding `as system` asks the runner to exercise the
serialise-to-JSON-to-deserialise boundary between contexts. A suite may set the
default and an individual case may override it.

The checkout test uses the highest tier:

#code-listing(
  [The case crosses the context edge declared by the production program],
  read("../snippets/chapter-09/declared/src/commerce/orders.bynk")
    + "\n\n"
    + read("../snippets/chapter-09/declared/tests/checkout.test.bynk"),
  lang: "bynk",
)

The test contains no participant list. The compiler follows
`consumes commerce.payment as Payment`, stands up the inferred contexts, and
routes the call across the same serialisation shape the Workers use. Adding a
new consumed context changes the graph the test runner sees; there is no
separate wiring manifest to remember.

Promotion does not make every test end to end, nor should it. Most scenarios
need the speed and control of a unit test. Some need real collaborators without
a wire. A few need the cross-context boundary. The value of the tier is that
the amount of reality is visible on the case rather than encoded in which test
framework, folder, or base class happened to run it.

There is an important current limitation. The `unit` tier does not yet
synthesise a return for every unstubbed collaborator. A seam without an explicit
stub keeps its real provider. Unit therefore states an isolation discipline
that the author must complete seam by seam; it is not presently proof that
everything outside the target was doubled. A fast green case may still include
more reality than its reader assumes.

Nor is `system` the final word. The runner stands Workers up in process under
Node and exercises their emitted boundary. It does not prove that production
secrets, bindings, network policy, or a particular deployment are correct.
Those concerns still require deployment checks and, where warranted, tests
against the deployed system.

== The seam a test can't invent

The seam rule has force. Suppose a test attempts to introduce an exchange-rate
collaborator that the target context neither declares nor reaches:

#code-listing(
  [The test tries to invent a dependency absent from the production graph],
  read("../snippets/chapter-09/not-a-seam/tests/pricing.test.bynk"),
  lang: "bynk",
)

The test compiler refuses:

#compiler-message[
[bynk.stub.not_a_seam]
`Exchange` is not a capability seam of `commerce.pricing`

Note: a `stub` clause names a capability the target context
declares or reaches through a consumed context
]

The body of `fixedPrice` does not use `Exchange`, so the stub cannot influence
this particular assertion anyway. The refusal matters for the larger habit it
prevents: tests may not acquire a hidden dependency graph merely because a
mocking tool can replace any import or property.

If the context should depend on exchange rates, declare the capability and
require it with `given`. If it should not, the test must not pretend that it
does. The production design decides where substitution is legal.

== Generate histories by driving the owner

Cases supply chosen scenarios. Some stateful claims concern many sequences that
an author will not think to enumerate.

The stock agent has two operations and one snapshot invariant:

#code-listing(
  [The invariant prevents a committed negative stock level],
  read("../snippets/chapter-09/declared/src/commerce/inventory.bynk"),
  lang: "bynk",
)

The invariant says that every committed snapshot has non-negative availability.
It does not say how an accepted shipment became possible. That is a claim about
history:

#code-listing(
  [A generated run is made only from states reached by real handlers],
  read("../snippets/chapter-09/declared/tests/stock.test.bynk"),
  lang: "bynk",
)

`History[Stock]` does not fabricate a stock record and ask whether it looks
valid. The runner creates a fresh agent, generates a bounded sequence of
`receive` and `ship` calls with generated quantities, and drives the real
handlers. Each step records the call, whether its commit was accepted, and the
old and new committed states.

The property says that every accepted `Ship` has an accepted `Receive` earlier
in the run. `run.upTo(step)` is the prefix before the shipment. There is no
separate temporal assertion language: the run is a finite list and the claim
uses ordinary `all`, `any`, and `implies`.

Driving is the important choice. A non-negative stock level is valid, but a
particular valid level may be unreachable from the initial state under the real
operations. Fabricating arbitrary states can produce counterexamples to a
system that no execution can enter. Generated histories search reachable
behaviour because the handlers construct every step.

This is still testing, not model checking. Runs are bounded samples, failures
are shrunk to a smaller reproducible sequence, and a passing property is not a
proof of unbounded liveness. Histories currently cover one agent, not a protocol
across several owners or contexts. Their value is adversarial exploration of
reachable paths, with an honest ceiling.

== Could existing tooling do this?

Yes. Dependency injection can make production seams available to test doubles.
Mock libraries can restrict replacement to interfaces. Spies can observe calls.
Property-testing libraries can generate values and command sequences. Contract
tests, containers, and browser-driven tests can increase realism around the same
business scenario.

A disciplined TypeScript codebase can preserve its architecture in tests. It
can also choose more sophisticated doubles than Bynk permits and integrate with
a much wider testing ecosystem.

Bynk's contribution is to connect these choices to constructs the program
already uses. A stub targets a capability. Participants come from `consumes`.
Agent state remains real and fresh at every tier. Histories call real handlers.
Test-only constructs are removed from the deploy build. The language narrows
the routes by which a convenient test can become a second program.

It cannot prevent a weak assertion, a misleading scenario name, an insufficient
sample, or a test suite aimed at the wrong risks. Confidence remains an
engineering judgement.

The compiler can, however, identify when a test asks for something structurally
incoherent: a double without a seam, a system tier without a wire, a fabricated
value with no honest generator, or a history that merely restates an invariant.
Those refusals are more than error codes. Properly written, they can explain
which design fact the attempted program has contradicted.

The next chapter examines that possibility: a compiler refusal as a form of
design feedback.
