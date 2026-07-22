#import "../template.typ": architecture-flow, code-listing, compiler-message

= When architecture becomes convention <when-architecture-becomes-convention>

The first version of the ordering service had no architecture problem. It had
an order handler, a payment function, and a database. The handler called the
function and wrote the result. Anyone reading the code could hold the whole
system in mind.

The team still made architectural decisions. Payment authorisation was kept
apart from order placement. Orders owned the process of accepting an order.
Payments owned the conversation with the bank. Only a successful authorisation
could lead to a stored order. These were real decisions, even if they did not
yet feel like architecture.

They did not feel like architecture because the code was small enough to make
their consequences obvious.

This is an important advantage of small systems, and one that is easily
misdiagnosed. We sometimes say that a small system has no architecture, when
what we mean is that its architecture requires no special effort to recover.
The distance between a decision and the code expressing it is short. The
person reading one function can see the relevant callers, dependencies, data,
and effects at once.

Then the system succeeds.

Payments gain retries and more than one provider. Orders acquire discounts,
fraud checks, stock reservations, and a second entry point for customer-service
staff. A scheduled process revisits orders abandoned halfway through checkout.
The database is split. Different teams take responsibility for the two areas.
Nothing about this development is pathological. It is what useful software
does.

The original decisions remain important, but they are no longer visible in one
place. They have been distributed through the implementation. Architecture has
become convention.

#block(breakable: false)[
== The reasonable beginning

Imagine that the first order path looks much like this:

#code-listing(
  [A conventional order-placement function],
  read("../snippets/chapter-01/conventional.ts"),
  lang: "typescript",
)
]

There is nothing scandalous here. The function is short. Its control flow is
plain. Payment failure is handled before the order is inserted. The imports
even provide a rough sketch of the design: orders depend on payment and
storage.

For the first few months, this may be all the sketch the team needs.

But look more closely at what the program says and what its authors know. The
program says that one module imports two others. Its authors know that payment
is a separate responsibility; that all payment authorisation should pass
through a controlled boundary; that order placement may depend on payment, but
payment must not reach back into orders; and that the ordering component is the
only owner of the decision to place an order.

Those are not the same description.

The imports show how this function happens to work. The architectural facts
describe which implementations should be allowed to exist.

That difference is initially harmless. Everyone knows the rule. A pull request
that imports the bank client directly into the order handler will be corrected
in review. A new teammate will be shown the `payment` directory and told that
all authorisations go through it. Perhaps a short design document records the
reason.

The system therefore has two languages. The programming language describes
values, functions, promises, and modules. The team’s language describes
responsibilities, owners, boundaries, and permitted dependencies. The second
language governs the first, but the compiler understands only one of them.

#quote(block: true)[
  A convention is a decision the compiler has been asked not to remember.
]

This is not an argument against conventions. Software development would be
impossible without them. Not every design preference deserves syntax, and no
language can encode every fact a team cares about. The question is narrower:
what happens when a decision is important enough to shape the system, yet is
represented only through arrangements the language treats as incidental?

== Where the boundary went

A boundary represented by a directory can be seen by a person. A boundary
represented by an import rule can be checked by a linter. A boundary
represented by a repository interface can be exercised by tests. Each
technique adds useful friction.

None necessarily tells the programming language what the boundary _means_.

A module system can make a symbol private, but it does not usually know that a
set of modules forms a deployable component. An interface can hide a concrete
payment provider, but it does not know that payment is an organisational
responsibility with a public service surface. A dependency-injection container
can connect an implementation at runtime, but the connection may be data from
the compiler’s point of view. A package can forbid an import in one direction,
but the reason for the prohibition remains elsewhere.

#architecture-flow()

As the implementation grows, the representation and the decision drift apart.
The payment boundary may now be implied by a package name, an interface, a
container registration, a network route, and several tests. Each fragment is
reasonable. Together they make the architecture expensive to reconstruct.

This expense is often paid in small instalments. A developer traces imports to
learn whether a helper is safe to call. A reviewer searches for other writes to
a table. A production incident reveals that a background worker used a code
path with different authentication assumptions. A migration requires a list of
all callers, assembled partly by search and partly by asking around.

No single instalment looks large. The aggregate is one of the defining costs of
a mature service: before changing it, we must rediscover what it is.

There is a deeper cost too. If the architecture is inferred from the current
implementation, then any implementation that compiles can appear to redefine
the architecture. The new direct import is not merely a violation of the old
design; once merged, it becomes evidence about the design. Enough local
exceptions turn a rule into a historical note.

Architecture has not vanished. Its authority has weakened.

== What counts as architectural information

The word _architecture_ is easy to inflate until it covers every decision in a
codebase. That would make it useless here. We are concerned with information
that constrains the system beyond one implementation detail: where a
responsibility begins and ends, which component owns a decision or piece of
state, which interactions are permitted across those boundaries, and which
effects must be made visible to a caller.#footnote[
  This is a working definition for the argument of this book, not a claim that
  all software architecture can be reduced to four categories.
]

These facts answer questions such as:

- May ordering initiate a payment?
- May payment read or modify an order?
- Is payment authorisation an internal helper or a service offered across a
  boundary?
- Which component is deployed when payment changes?
- What evidence will reveal a new dependency between the two?

The TypeScript function answers the first question descriptively: ordering
_does_ initiate a payment. It does not answer it normatively: ordering _may_
initiate a payment, and this is an intended dependency. Nor does it make the
inverse dependency impossible.

This distinction between _does_ and _may_ is where architecture begins to pull
away from ordinary implementation.

Dependency graphs extracted from source are valuable because they tell us what
the program does today. An architectural model should also tell us what the
program is permitted to do tomorrow. It is a constraint on the set of valid
programs, not merely a picture of the current one.

== Naming the boundary

Bynk makes the deployable boundary a language construct called a _context_. A
context contains the services and other architectural elements that belong
together. Its name is not inferred from where the source file happens to sit.
It is declared in the program.

#block(breakable: false)[
The payment side of the example can begin like this:

#code-listing(
  [The payment context exposes one service],
  read("../snippets/chapter-01/declared/src/commerce/payment.bynk"),
  lang: "bynk",
)
]

There is more here than we need for the present argument. The service is
effectful, failure is represented in its result, and the error type is exported
transparently so another context can handle its variants. Later chapters will
examine those choices. For now, the significant lines are `context
commerce.payment` and `service authorise`.

They state two architectural facts. This code belongs to the payment context,
and authorisation is a service that context presents. The boundary and the
opening in the boundary are both part of the program.

This does not prove that payment is the correct boundary. Calling something a
context cannot rescue a confused decomposition. The author still has to decide
whether authorisation, settlement, refunds, and fraud belong together. Bynk’s
claim is not that the compiler can discover the architecture. It is that, once
chosen, the architecture should not have to survive solely as an inference.

#block(breakable: false)[
The order side then declares its dependency:

#code-listing(
  [The ordering context declares and uses payment],
  read("../snippets/chapter-01/declared/src/commerce/orders.bynk"),
  lang: "bynk",
)
]

The `consumes` clause is intentionally conspicuous. It says that
`commerce.orders` is allowed to call services offered by `commerce.payment`,
and gives the dependency the local name `Payment`. The call inside `place` is
therefore not the first place at which the relationship appears. Before reading
the service body, we already know that this context depends on payment.

The body still describes the implementation. It authorises the amount, accepts
the order when authorisation succeeds, and maps payment failure into an order
error. But the context header describes the allowed shape of that
implementation.

This is a small addition to the source and a large change in status. The
dependency is no longer only a fact we can discover. It is a declaration the
compiler can require.

== A useful refusal

Remove the `consumes` clause and call the payment service by its full name. The
reader can still understand the intention. The target exists, the operation
exists, and the types could be made to line up. Bynk nevertheless rejects the
program:

#compiler-message[
[bynk.resolve.unconsumed_context] Error:
`commerce.payment.authorise` looks like a cross-context service call,
but `commerce.payment` is not in this context's `consumes` clauses
]

The refusal does not say that orders must never call payments. It says that a
cross-context call cannot be smuggled into an implementation without changing
the context’s declared dependencies. The author must either make the
relationship explicit or choose a different design.

That requirement changes the conversation around a code review. Without the
declaration, the architectural question is hidden inside the diff of a function
body: is this new call consistent with the intended dependency graph? With it,
the graph itself changes in the diff. The reviewer may still approve a poor
dependency, but the change is harder to make accidentally.

Compiler errors are often treated as obstacles between an author and a working
program. This one is better understood as an architectural checkpoint. The
implementation may be locally coherent and still contradict the program’s
declared shape.

The compiler is not substituting its judgement for the team’s. It is returning
the team’s earlier judgement at the moment it matters.

== Convention and declaration

The difference is easiest to see by placing the two approaches side by side.

#figure(
  block(width: 100%)[
    #set text(size: 8.5pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (0.82fr, 1.12fr, 1.12fr),
      inset: (x: 0.55em, y: 0.5em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Question],
        text(weight: "semibold")[Convention],
        text(weight: "semibold")[Declaration],
      ),
      [Where is the boundary?], [In folders, packages, or deployment configuration.], [In a named context.],
      [Where is the dependency?], [In imports, bindings, routes, and calls.], [In a `consumes` clause.],
      [Who detects a new edge?], [A reviewer, linter, test, or incident.], [The compiler, before code is emitted.],
      [Can the rule be wrong?], [Yes.], [Yes. Explicitness is not correctness.],
    )
  ],
  caption: [Two ways of carrying the same architectural decision.],
)

The final row matters. It would be easy to turn declaration into a morality
tale: conventions are fragile, declarations are rigorous, therefore more
declaration is always better. Real systems are less obliging.

A declared dependency can legitimise coupling that should not exist. Contexts
can be drawn at the wrong scale. A team can spend so much effort maintaining an
idealised model that the model obstructs ordinary work. Explicit architecture
creates its own migration cost because a structural change must now change the
program’s declarations as well as its implementation.

These are not accidental drawbacks. They are the price of giving architectural
decisions force.

When a rule lives only in convention, a developer can bend it locally and
explain the exception later. Sometimes that flexibility is exactly what a
young system needs. When the rule is declared in the language, the exception
requires a visible change to the model. That is slower at the point of change
and cheaper at the point of understanding.

The useful question is therefore not whether declarations are superior in the
abstract. It is whether a particular decision is valuable and stable enough
that violations should stop compilation. Bynk answers _yes_ for a selected set
of service-architecture facts. That selection is one of the language’s central
opinions, and it deserves to be judged by the programs it makes clearer as well
as the programs it refuses.

== The architecture in the diff

Suppose the ordering team later decides that fraud assessment should become a
separate context. In a conventional service, the change may introduce a client,
register an implementation, add configuration, and call the client from the
order path. A reviewer can assemble those edits into the higher-level statement
“orders now depend on fraud”. The repository may even contain tooling that
generates a dependency diagram after the fact.

In Bynk, the statement also has a source-level form. The header of the ordering
context acquires another `consumes` clause. The diff contains a direct change to
the allowed architecture.

This has effects beyond review.

Search becomes more reliable because architectural dependencies have a
distinguished representation. Tooling does not have to decide which imports
are significant and which are implementation detail. A compiler can reject
references that bypass the declared graph. Deployment tooling can treat a
context as a context because that concept survives into compilation. A reader
can begin with the declarations before entering the service bodies.

Most importantly, the representation is prospective. It does not merely report
that an edge exists; it establishes the conditions under which code may create
one.

This is what it means for architecture to be part of the program. The source is
not a perfect design document, and the design is not complete merely because
the source compiles. Rather, selected design decisions participate in the same
feedback loop as types and names. They can be inspected, changed, and
contradicted. Contradiction has a mechanical consequence.

== The limit of the promise

There is a temptation, especially when introducing a language, to let a useful
mechanism absorb every virtue around it. Explicit boundaries become good
boundaries. Checked dependencies become loose coupling. A compiling program
becomes a well-designed system.

None follows.

If `commerce.orders` consumes six other contexts, Bynk can make all six edges
visible. It cannot tell us whether ordering has accumulated too much
responsibility. If payment exports a service that exposes the bank provider’s
model directly, the compiler can check calls to it. It cannot decide whether
the abstraction leaks. If two contexts exchange a long chain of synchronous
calls, the dependency declarations reveal the chain. They do not make its
latency acceptable.

The gain is more modest and, for that reason, more credible: the program retains
information that conventional implementations often discard.

Retained information improves the conditions under which judgement is applied.
The team can see the dependency it is evaluating. A new colleague can discover
the intended boundary without first learning every repository convention. A
tool can operate on a declaration rather than a guess. A compiler can prevent
an implementation from quietly outrunning its model.

There will always be knowledge outside the program. Business motives, team
ownership, operational history, and the reasons behind a boundary still belong
in conversation and prose. The goal is not to turn source code into the only
truth. It is to stop erasing facts the language is capable of carrying.

The four-box diagram from the prologue will still simplify. It should. A useful
diagram omits detail. The problem begins when the implementation can violate
the diagram’s essential relationships without having to say that the
architecture changed.

Bynk makes that statement necessary.

The next problem appears inside the boxes. Even when the boundary is visible,
ordinary values can lose their meaning: two identifiers become strings, a
validated amount becomes an integer, and an impossible state becomes one more
combination of fields. Architecture is not only the arrangement of components.
It also depends on whether the program preserves the distinctions the domain
requires.

That is where we turn next.
