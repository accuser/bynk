#import "../template.typ": code-listing

#let source-lines(path, start, end) = {
  read(path).split("\n").slice(start, end).join("\n")
}

= Reading a whole system <reading-a-whole-system>

So far, this book has made its case one pressure point at a time. A boundary
disappeared into imports. A string acquired a domain identity. Failure became a
sum, an effect acquired a capability, and state found an owner. Each chapter
held most of the system still while one architectural decision came into focus.

Real programs do not offer that courtesy. Their meaning lives in the
arrangement of all those decisions, including the decisions nobody made.

This chapter is a code-reading exercise. The system is an order
endpoint small enough to print and complete enough to compile. It accepts an
authenticated request, reserves stock, charges a payment provider, and records
the order. It has shared domain values, three contexts, two agents, one
capability, explicit failures, and a Workers build.

It also has problems.

The point is not to correct them before anyone can see them. The point is to ask
what a reader can recover from the source, what the compiler can preserve, and
where both must stop.

#quote(block: true)[
  Architecture is not the list of constructs in a program. It is the set of
  consequences a reader can recover from their arrangement.
]

== Start with the map

The project contains one shared vocabulary and three deployable contexts:

#code-listing(
  [The complete source tree is small enough to hold in working memory],
  read("../snippets/chapter-12/system-tree.txt"),
  lang: "text",
)

The first pass does not read handler bodies. It reads unit headers:

#code-listing(
  [The orders header names its vocabulary, outgoing edges, exports, and caller],
  source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/orders.bynk",
    0,
    14,
  ),
  lang: "bynk",
)

`uses commerce.values` shares types but creates no runtime edge.
`consumes commerce.inventory as Inventory` and `consumes commerce.payments as Payments`
do. Before reading an expression, we know that orders may call inventory and
payments, that neither callee calls back, and that the dependency graph is
acyclic.

On the Workers target, that graph becomes three Workers and two Service
Bindings. Inventory and payments can be deployed independently; orders must
bind to both. On the bundle target, the same edges become direct calls. The
source does not tell us the latency of either build, but it does tell us where
latency and remote failure can enter.

#figure(
  block(width: 100%)[
    #set text(size: 8.5pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (0.82fr, 1.35fr, 1.35fr),
      inset: (x: 0.5em, y: 0.5em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Unit],
        text(weight: "semibold")[Owns or declares],
        text(weight: "semibold")[Depends on],
      ),
      [`commerce.orders`], [`Order`, HTTP API, `Customer`], [`commerce.inventory`, `commerce.payments`],
      [`commerce.inventory`], [`Stock`, reservation service], [Shared values only],
      [`commerce.payments`], [`Bank`, charge service], [Shared values only],
      [`commerce.values`], [Domain vocabulary], [Nothing],
    )
  ],
  caption: [A first reading recovers topology without following control flow.],
)

The aliases are worth noticing. The body will say `Inventory.reserve` and
`Payments.charge`, not import a file or construct a client. Those names are the
only legal routes across the two context boundaries. A later implementation
cannot quietly reach into `Stock` state from orders without changing the
declared architecture.

That is already more useful than a folder diagram, but it is not yet an account
of the system.

== Read the edge as a contract

The HTTP service accepts `POST /orders/:id`. The path value is an `OrderId`; the
body contains an opaque `Sku`, a bounded `Quantity`, and a positive `Cents`. Invalid
input is rejected during admission rather than discovered halfway through
reservation.

The handler also says `by customer: Customer`. `Customer` is a bearer-authenticated
actor whose sealed identity is an opaque `CustomerId`. The body does not parse a
user ID and trust it. The edge establishes the caller, and the handler passes
that verified identity into the order owner.

This gives a reviewer several concrete questions:

- Is bearer authentication the intended scheme for creating an order?
- Is `AUTH_JWT_SECRET` the right operational dependency?
- Should the order ID come from the client?
- Are positive cents sufficient, or should currency and an upper bound be part
  of admission?

Bynk does not answer those questions. It makes the places that require answers
compact and visible.

The second HTTP handler is more revealing:

#code-listing(
  [The read authenticates a customer but does not use customer identity],
  source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/orders.bynk",
    93,
    97,
  ),
  lang: "bynk",
)

Any authenticated `Customer` who knows an `OrderId` can read that order. The
agent stores an owner, but this handler never retrieves or compares it.

That may be the intended policy. It may be a security defect. The compiler
cannot infer which because the missing fact is not syntactic; it is the
organisation's authorisation rule. What the language has done is make the
evidence unusually direct. The route is not accidentally public, and caller
identity is not hidden in middleware. A reviewer can see both the protection
that exists and the check that does not.

== Follow the irreversible work

Only now do we read the main handler:

#code-listing(
  [Order submission crosses two contexts and commits state between them],
  source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/orders.bynk",
    62,
    92,
  ),
  lang: "bynk",
)

The surface story is straightforward:

1. Begin the order and record the authenticated customer.
2. Reserve stock.
3. Mark the order as reserved.
4. Charge the customer.
5. Mark the order paid, or rejected if either dependency refuses.

The architectural story is less comfortable. Each agent call is an independent
state transaction. The call to `Order(id).begin` commits before inventory is
contacted. `Inventory.reserve` commits its own stock mutation before
`Order(id).markReserved`. Payment happens after both.

If the payment provider returns `Err`, the order becomes `Rejected`, but the
stock remains reserved. There is no release operation and no compensating
call. If the payment call fails below its declared `Result`, the order may
remain reserved without even reaching the rejection branch.

Nothing about this is obscured by a framework transaction helper. The sequence
is visible in the handler, the state owners are named, and the network edges are
declared. The system has not become correct. It has become reviewable.

This distinction matters. A dependency graph can tell us that orders talks to
inventory and payments. It cannot tell us that reservation precedes charging,
that a commit lies between them, or that one failure path lacks compensation.
Architecture includes temporal arrangement as well as boxes and arrows.

== Open the owners

Inventory owns one `Stock` agent per `Sku`:

#code-listing(
  [A stock reservation is atomic inside one owner],
  source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/inventory.bynk",
    9,
    25,
  ),
  lang: "bynk",
)

The key answers *which state?* The `Cell` fields answer *what persists?* The
handler answers *who may mutate it?* The invariant answers *what must hold after
every commit?*

Within a `Stock` instance, checking availability and updating both counts is one
serialised operation. Two concurrent orders for the same SKU cannot both read
the same available count and commit incompatible reservations. The compiler
also prevents either count from committing below zero.

Those guarantees stop at the owner boundary. The invariant says nothing about
an order, a payment, or a later release. It cannot express "every reserved unit
belongs to an order that will eventually be paid or compensated" because that
claim spans agents and time.

The `Order` agent is similarly precise and similarly local. It records an owner,
status, reservation flag, and optional payment reference. Its snapshot
invariants say that a paid order has both a reservation and a payment reference.
Its transition says that paid is terminal.

Notice what remains legal: a rejected order may have `reserved == true`. That
is exactly the state produced by a declined payment. The compiler accepts it
because the declarations accept it. The problem is not that the invariant
engine failed. The model never claimed that rejected orders release stock.

This is a useful reading habit: do not ask only what an invariant forbids. Ask
which suspicious states it still permits.

== Trace effects through both layers

Payments is a context boundary and `Bank` is a capability boundary inside
that context:

#code-listing(
  [The payment service may use only the capability named in its contract],
  source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/payments.bynk",
    0,
    10,
  ) + "\n\n" + source-lines(
    "../snippets/chapter-12/whole-system/src/commerce/payments.bynk",
    15,
    18,
  ),
  lang: "bynk",
)

Orders can call only the exported `charge` service. The service can perform the
external effect only because its handler declares `given Bank`. The
complete source supplies a local, deterministic `DemonstrationBank`; a
production implementation could sit behind an adapter without changing the
orders context.

The two boundaries serve different purposes. The context boundary limits what
another part of the system may ask payments to do. The capability boundary
limits what payment code may ask the outside world to do. One is system
topology; the other is effect authority.

Failure loses information as it crosses those layers. Payments distinguishes
`Declined` from `ProviderUnavailable`. Orders matches `Err(_)`, converts both to
`PaymentFailed`, and returns HTTP 503. That is a design decision visible in a
few adjacent lines. A decline might deserve 422 while an outage deserves 503;
or the product may intentionally reveal neither. Again, the compiler can
enforce exhaustive handling only after the program chooses how much meaning to
preserve.

== Make a recoverability ledger

After one reading, we can separate architectural facts from architectural
questions:

#figure(
  block(width: 100%)[
    #set text(size: 8.1pt, hyphenate: false)
    #set par(justify: false, leading: 0.56em, first-line-indent: 0pt)
    #table(
      columns: (0.78fr, 1.42fr, 1.42fr),
      inset: (x: 0.45em, y: 0.46em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Concern],
        text(weight: "semibold")[Recoverable from source],
        text(weight: "semibold")[Still requires judgement],
      ),
      [Boundary], [Three contexts; two outgoing edges], [Whether they are the right boundaries],
      [Caller], [POST and GET require `Customer`], [Whether GET must enforce ownership],
      [State], [`Order` by ID; `Stock` by SKU], [Retention, deletion, and migration policy],
      [Failure], [Named stock, payment, and HTTP outcomes], [Whether mappings preserve enough meaning],
      [Effects], [Charging requires `Bank`], [Provider trust, timeout, and retry policy],
      [Consistency], [Atomic commits within each agent], [Compensation across owners],
    )
  ],
  caption: [Compile-visible architecture narrows uncertainty; it does not eliminate it.],
)

This ledger is more honest than saying the architecture is "in the code". Some
facts are statically recoverable. Some are recoverable only by following the
ordered body of a handler. Some are absent because the program has not made the
decision. Others belong to requirements, operations, or platform behaviour and
cannot be inferred from declarations alone.

The source declares no application-level idempotency key, timeout, retry,
compensation, audit effect, retention policy, or order-ownership check. That
does not prove none exists elsewhere in the deployed environment. It does tell
us the Bynk program neither names nor enforces one.

Absence becomes useful evidence only when its scope is stated.

== Read the whole at several scales

A productive review moves through the program more than once:

1. Read unit headers for topology and permitted direction.
2. Read entry points for protocol, caller, admission, and public failure.
3. Read service signatures for cross-context contracts.
4. Read agents for identity, persistence, invariants, and commit boundaries.
5. Read capability clauses for external authority.
6. Read effectful bodies for temporal order and partial work.

No single representation replaces the others. A generated graph is excellent
for topology and poor at time. A type signature is excellent for admitted
values and poor at state ownership. An invariant states a local truth and says
nothing about a business obligation nobody encoded. The handler body remains
part of the architecture because sequencing has consequences.

This is the result of putting Bynk's claim under pressure. A reader can recover
more than filenames, imports, route configuration, and convention would reveal.
The compiler can preserve the graph, caller requirements, state boundaries,
effect seams, and declared failure choices across two deployment topologies.

But the language does not produce a correct design merely by making a design
explicit. In this small system it helps us find an authorisation question, a
lossy failure mapping, and a missing compensation path. Those are valuable
findings precisely because they remain the team's responsibility.

The final chapter asks what this explicitness costs: which programs become
awkward, which forms of flexibility disappear, which problems remain outside
the model, and when a team should choose the larger language underneath Bynk
instead.
