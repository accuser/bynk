#import "../template.typ": code-listing

= The cost of stronger constraints <the-cost-of-stronger-constraints>

It is easy to praise a constraint by listing the mistakes it prevents. The
account is incomplete until it lists the useful programs, local shortcuts, and
ways of working that the same constraint makes harder.

Bynk refuses undeclared context calls, ambient effects, fabricated identities,
unowned state, unhandled variants, invalid commits, and dependency cycles. This
book has argued that those refusals preserve architectural information that
ordinary service code tends to lose.

The refusals are not free.

They move work earlier. They reduce the number of locally convenient changes.
They make some designs awkward and others impossible to state without placing
their most dynamic part behind an adapter. They add a compiler, a generated
language, and a smaller community to a system that could otherwise use
TypeScript directly.

#quote(block: true)[
  Every guarantee has a shadow price: the programs it refuses, the declarations
  it demands, and the questions it cannot answer.
]

This chapter is the bill.

== A constraint spends flexibility

Each of Bynk's central restrictions exchanges one kind of freedom for one kind
of evidence:

#figure(
  block(width: 100%)[
    #set text(size: 8.1pt, hyphenate: false)
    #set par(justify: false, leading: 0.56em, first-line-indent: 0pt)
    #table(
      columns: (0.9fr, 1.35fr, 1.35fr),
      inset: (x: 0.45em, y: 0.46em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Constraint],
        text(weight: "semibold")[Evidence gained],
        text(weight: "semibold")[Freedom spent],
      ),
      [Acyclic `consumes`], [Visible topology and deploy order], [Mutual or runtime-installed dependencies],
      [`given` capabilities], [Named effect authority], [Ambient library and platform access],
      [Agent ownership], [One serialised state owner], [Shared writes and cross-owner transactions],
      [Closed sums], [Exhaustive failure handling], [Unknown variants accepted without a boundary],
      [Actors at edges], [Caller requirement in the contract], [Middleware-defined or improvised identity],
      [Validated boundaries], [Admitted values match declared types], [Zero-cost trust in external data],
    )
  ],
  caption: [The same rule creates the evidence and removes the freedom.],
)

The right-hand column is not a catalogue of bad practices. Mutual dependencies
can model a protocol in which peers genuinely call each other. Ambient access
can be appropriate inside a short script. Shared transactions are exactly what
a relational database is good at. Open variants are useful in plugin systems.
Middleware can be the correct owner of authentication.

The architectural question is not whether constraint is virtuous. It is whether
the evidence bought by a particular constraint is worth the flexibility spent
in this system.

== The cost arrives first

The shortest implementation of a new feature is often not the one Bynk permits.

A developer cannot begin with a string and decide later whether it is an order
ID. They must choose its identity and admission rules. An effect cannot start as
an import and become a capability after tests need substitution. State cannot
sit in a module variable until persistence matters. A new call cannot cross a
context boundary while the team postpones deciding whether the dependency is
architecturally acceptable.

Those decisions may eventually save expensive rework. They still slow the first
working path. In an uncertain domain, early declarations can fossilise guesses.
A team may spend time naming boundaries that discovery will invalidate next
week.

The compiler also turns some refactorings into coordinated edits. Adding a
variant requires every exhaustive match to respond. Tightening a refinement
requires every admission point and test value to satisfy it. Moving a service
across a context changes the `consumes` graph, deployment topology, codecs, and
possibly failure characteristics.

That friction is intentional feedback, but intentional friction is still
friction. When rapid exploration is more valuable than architectural stability,
ordinary TypeScript can be the more disciplined choice because it lets the team
learn before it commits the model to a language.

== Some systems are open on purpose

Consider a host whose purpose is to accept new plugins while it is running:

#code-listing(
  [Runtime values change which handlers exist and which events they receive],
  read("../snippets/chapter-13/open-plugin-host.ts"),
  lang: "typescript",
)

This is not architecture accidentally hidden in a map. The open map *is the
architecture*. Installation returns an uninstall operation; each plugin decides
which event kinds it handles; neither the plugin set nor the event vocabulary
must be known when the host is compiled.

That openness is useful for editor extensions, tenant automation, test runners,
and long-lived processes that load integrations independently of the core
release. The price is familiar: a source reader cannot recover the runtime
graph, an event payload is admitted only as `unknown`, and a plugin can introduce
effects the host's type signatures do not enumerate.

Bynk's compile-visible context graph cannot describe this relationship as a set
of statically known `consumes` edges. The program could place the registry
behind a `PluginHost` capability and implement it in a TypeScript adapter. That
is a legitimate design. It also moves the plugin graph, its effects, and its
failure policy outside the part of the system Bynk can inspect.

An escape hatch does not preserve a guarantee while avoiding its cost. It moves
the guarantee boundary.

== Ownership does not compose into a transaction

Chapter 12 exposed a more common cost. One `Stock` agent can reserve inventory
atomically. One `Order` agent can commit an order transition atomically. The
operation that spans both agents and a payment provider is not atomic.

Bynk makes that fact difficult to miss. It does not supply distributed
transactions, automatic compensation, exactly-once effects, or a proof that an
eventually consistent workflow will eventually become consistent.

The agent model can therefore make a shared-database design more laborious.
Data that once changed in one SQL transaction may need commands, results,
idempotency keys, compensating operations, and reconciliation. A report that
once joined arbitrary tables may need a read model or a deliberate co-location
decision. Those changes can improve ownership, but they are not merely clearer
syntax for the same system.

Sometimes the shared transaction is the important guarantee. If the domain is
centred on ad hoc relational queries and multi-record atomic updates, forcing
every concept behind a keyed state owner may weaken the design while making it
more elaborate.

The correct response is not to pretend that distribution is free. It may be to
place the relevant data under one agent, retain a database boundary behind a
capability, or use a language and framework whose primary model matches the
transactional workload.

== Explicitness has volume

Architectural information takes space.

The concise call `charge(order)` becomes a capability operation, a `given`
clause, a provider, perhaps an adapter binding, and a `Result` whose variants
must be mapped at each public boundary. A caller becomes an actor declaration
and a `by` clause. A persistent value becomes an agent key, storage field,
handler, invariant, and lifecycle.

That volume can repay itself in navigation, tests, generated wiring, and safer
change. It can also bury a simple computation beneath declarations whose value
never materialises. Boilerplate is not redeemed merely because the compiler
understands it.

There is a cognitive cost too. A reader must learn which types are refined,
which are opaque, where effects may run, how providers compose, when an agent
commits, how caller identity crosses contexts, and how bundle behaviour differs
from Workers behaviour. The language reduces some uncertainty by introducing a
new model that the team must carry.

A ten-line automation, a pure transformation library, a user interface, or a
small endpoint built mostly from an established framework may not contain
enough architectural pressure to repay that model.

== The host boundary is a proof boundary

Adapters are how Bynk cooperates with the JavaScript ecosystem. They are also
where Bynk stops knowing.

The compiler can check that a binding implements a capability interface. It
cannot see whether a library performs undeclared I/O, invokes callbacks
concurrently, or interprets untyped configuration safely. The TypeScript
implementation can violate domain expectations while satisfying every emitted
type.

This is not unique to Bynk. Every foreign-function boundary narrows what the
host language can prove. The important cost is frequency. If most of a program's
work happens inside adapters because its frameworks and libraries expect
JavaScript-native control, then Bynk has become a typed facade around the real
application.

At that point the architecture may be easier to express directly in
TypeScript, with local conventions and targeted static checks, than to maintain
two languages separated by a boundary too broad to be informative.

== A language is an organisational dependency

Adopting Bynk adds more than syntax. The team must own compiler upgrades,
editor support, generated-output failures, source-map behaviour, platform
changes, documentation, build caching, and the judgement required to decide
which compiler refusals reveal a design problem.

It also creates a staffing constraint. A TypeScript service draws on a large
labour market and tool ecosystem. Bynk asks contributors to learn a smaller
language and the organisation to trust its stewardship.

That cost is sharpest when one advocate introduces the language into a team that
has not chosen it. Guarantees do not compensate for a single-maintainer
toolchain. Credible adoption needs shared ownership, an exit path through
readable TypeScript, version discipline, and a suitable target platform.

The organisation is part of the runtime.

== Know which problem you are buying

Bynk is most likely to earn its place when architectural drift is expensive and
recurrent:

#figure(
  block(width: 100%)[
    #set text(size: 8.35pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (1fr, 1fr),
      inset: (x: 0.52em, y: 0.5em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Bynk is more likely to fit],
        text(weight: "semibold")[TypeScript is more likely to fit],
      ),
      [A long-lived service with several boundaries], [A script, UI, library, or framework-led application],
      [Stable domain identities and failure vocabulary], [Requirements still being discovered through rapid change],
      [Keyed state ownership matches the workload], [Shared relational transactions dominate],
      [The dependency graph should be fixed and reviewable], [Runtime plugins or open extension are the product],
      [Workers and Durable Objects are a natural target], [Another runtime or deployment model is central],
      [The team will jointly own the language choice], [The toolchain would depend on one advocate],
    )
  ],
  caption: [The decision follows the system's dominant risk, not a universal language ranking.],
)

This is not a maturity ladder. A team may choose TypeScript for reflection,
dynamic composition, shared transactions, or framework intimacy. Another may
choose Bynk because caller authority, state ownership, and failure contracts
are already its hardest risks.

The choice need not cover an organisation. A Bynk context can sit beside
TypeScript at an HTTP or queue boundary, and an adapter can contain a
library-heavy integration. The useful boundary is the smallest one that keeps
the important architecture visible.

== The accounting

Part IV has produced a conditional result.

Reading the whole system showed that Bynk makes boundaries, owners, callers,
effects, and failure choices unusually recoverable. It also showed a missing
authorisation decision and an uncompensated reservation that the language could
expose but not repair.

Recoverability is purchased with earlier decisions, closed graphs, explicit
plumbing, local rather than global guarantees, a host boundary where proofs end,
and commitment to another language.

Bynk is justified when the cost of architecture becoming convention is greater
than the cost of keeping architecture in the program. If that inequality does
not hold, the stronger constraint is not stronger engineering. It is ceremony.

That completes the argument tested. The epilogue returns to the diagram that was
already wrong and asks a narrower question: after all these constraints and
costs, what should the program no longer be able to forget?
