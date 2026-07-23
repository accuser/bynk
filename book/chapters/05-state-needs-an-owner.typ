#import "../template.typ": code-listing, compiler-message

= State needs an owner <state-needs-an-owner>

A storage capability can tell us that a handler may read and write. It can make
the dependency replaceable, keep it out of pure functions, and expose its place
in the effect graph. It cannot tell us what the state belongs to.

Consider a basket. Its lines, note, and revision may occupy one database row,
several rows, a document, or an entry in memory. Those are representations. The
domain fact is that they are the lines, note, and revision of _this basket_. Two
requests carrying the same basket identity should meet the same remembered
state. A request carrying a different identity should not.

That fact influences more than lookup. It determines which updates must be
committed together, where a lifecycle begins, which operations are allowed to
change the data, and what a concurrent caller means by “the current basket”. If
ownership is absent from the program, each function has to reconstruct it from
keys, repository calls, and naming conventions.

State is therefore not merely data that happens to persist. It is memory with a
subject.

== A database is a place, not an owner

A conventional TypeScript design can be explicit about the stored shape and
still leave ownership distributed through the application:

#code-listing(
  [The repository stores baskets, while each function reconstructs ownership],
  read("../snippets/chapter-05/conventional.ts"),
  lang: "typescript",
)

This is not careless code. `BasketState` names the fields, absence from storage
is handled, and callers cannot mutate the returned values through these types.
The store is abstract, so an implementation can use a database in production
and memory in a test.

The problem is not the repository. It is what the repository contract permits
us to forget.

Every operation receives a `CustomerId`, loads whatever state is stored under
it, invents an empty basket when none exists, builds a replacement value, and saves
it. The repetition is architectural work disguised as plumbing. Each new
operation must remember the same lifecycle rule and use the same key. A generic
`save` also permits code elsewhere to write a basket shape without going
through either of the operations shown here.

The load and save are separated in time. Two calls can load revision 4, make
different changes, and both save revision 5. Whether that loses an update
depends on facilities outside the functions: a transaction, an optimistic
version check, row locking, or a storage API with stronger operations. The
source shown here does not say.

Changing the repository interface can address each weakness. We can add a
compare-and-swap operation, hide `save`, put basket operations on a class, or
introduce an aggregate repository that runs a callback inside a transaction.
Well-designed systems do exactly this. But the answer is no longer simply
“state is in the database”. We have had to decide what owns the state and make
that decision part of the programming model.

#quote(block: true)[
  Storage answers where values are kept. Ownership answers which identity and
  operations give those values meaning.
]

== Give memory an identity

Bynk calls its state-owning unit an _agent_. An agent is not one object created
once at program startup. Its declaration describes a family of logical
instances, one for each value of its key type.

The basket vocabulary gives the key and stored values their domain identities:

#code-listing(
  [The key is distinct from the values stored by the basket],
  read("../snippets/chapter-05/declared/src/commerce/baskets/types.bynk"),
  lang: "bynk",
)

`CustomerId` and `Sku` have the same runtime representation, but their opaque
types stop one being passed where the other is required. `Quantity` carries the
`InRange` rule from Chapter 2. State ownership begins with identity, so
letting unrelated strings select an instance would weaken the boundary before
any state was read.

The agent places the state and the operations that govern access beside that
identity:

#code-listing(
  [One keyed basket owns three related pieces of state],
  read("../snippets/chapter-05/declared/src/commerce/baskets/basket.bynk"),
  lang: "bynk",
)

The `key` line answers whose state this is. `store` fields answer what is
remembered. Handlers answer how code outside the agent may interact with it.
There is no public operation that replaces the complete basket state, so a
caller cannot bypass the vocabulary of `setLine`, `leaveNote`, and `snapshot`
through a generic save.

The storage kinds describe different shapes of memory. `lines` is a `Map`, so it
owns entries addressed by SKU. `note` and `revision` are `Cell`s, each holding
one value. A `Cell` is read by its field name. A replacement uses `:=`; a change
that depends on the prior value uses `update` so the read-modify-write is
visible.

`setLine` changes the line map and increments the revision. Those writes are
staged during the handler. A read later in the same handler would see the staged
values, and the state is committed together when the handler returns. If the
handler faults before that point, neither state change is persisted.

That last guarantee is specifically about the agent's state. It does not rewind
an arbitrary network request already sent by a capability, and a successful
call to another agent is not rolled back if this agent later faults. Ownership
defines a useful atomic boundary, not a distributed transaction around every
effect.

== The key selects the owner

Code reaches an agent by constructing a reference with its key, then calling a
handler:

#code-listing(
  [Addressing the same key selects the same logical basket],
  read("../snippets/chapter-05/declared/src/commerce/baskets/service.bynk"),
  lang: "bynk",
)

`Basket(owner)` does not mean “allocate a new empty basket every time”. It addresses
the logical `Basket` instance selected by `owner`. Calls that use the same key meet
the same remembered state. Calls using different keys address independent
instances.

This turns a parameter that was repeated through every repository operation
into the identity of the stateful boundary. Once `basket` has been obtained,
the call to `snapshot` does not need the identifier again. The handler runs
against the state belonging to the addressed instance.

The placement depends on the compilation target. On the Workers target, a key
selects a Cloudflare Durable Object. In a bundle or a test, the runtime uses an
in-process registry keyed by the same logical value. The source-level model is
the stable part: one declared owner per key, regardless of the mechanism used
to locate it.

That independence is also a modelling choice. A basket per `CustomerId` gives
each customer's basket its own state boundary. A single basket book keyed by a
storefront, holding every customer's basket in one map, would create a different
boundary. The declarations
might hold similar data, but contention, atomicity, querying, and failure would
be grouped differently.

The key is not a persistence detail. It is an architectural decision about
which facts live and change together.

== A fresh key needs an honest beginning

Addressing is also creation. There is no separate constructor that every caller
must invoke before using a key. If a key has never been seen, the runtime must
still produce valid state for it.

That is why every agent store field needs a defined starting value. It may have
an explicit constant initialiser, or its type must have an implicit zero. An
`Int` begins at `0`, a `Bool` at `false`, a `String` at `""`, and an `Option[T]`
at `None`. A record is zeroable when all of its fields are. Empty storage
collections provide the natural beginning for their kinds.

The accepted basket uses all three ideas. Its map begins empty, its revision is
zero, and its optional note is `None`. These are not placeholder bit patterns.
They are honest statements about a basket that has not yet been changed: it has
no lines, no revisions, and no note.

Now make the revision type positive while leaving it without an initialiser:

#code-listing(
  [A positive revision has no valid implicit zero],
  read("../snippets/chapter-05/non-zeroable/src/commerce/baskets.bynk"),
  lang: "bynk",
)

The type says every `Revision` must be greater than zero. Fresh-state
initialisation would require a value of zero. Bynk refuses to pretend that both
claims can be true:

#compiler-message[
[bynk.agents.non_zeroable_state_field] Error:
agent `Basket` store cell `revision` has no defined zero value,
so a fresh key cannot be initialised
]

There are two honest repairs. If revision 1 is a meaningful initial state, the
field can declare `= 1`. If the value genuinely does not exist until a later
operation, its type can be `Option[Revision]`, whose initial `None` makes that
absence visible to every reader.

Inventing `-1`, an empty identifier, or another sentinel would restore a
machine-level starting value while damaging the domain model. Zeroability is
not a demand that every type accept the number zero. It is a demand that every
fresh owner begin in a state its own types recognise as valid.

This rule has a design consequence. Not every domain entity is naturally an
agent addressed into existence. A basket can reasonably begin empty. An
approved loan may require an applicant, terms, and an approval decision before
it can exist. Those facts need explicit initialisers, honest optionality, or a
different lifecycle boundary. The compiler can reject an impossible default;
it cannot choose the right lifecycle.

== Ownership is the commit boundary

Grouping fields under one agent makes more than their names local. It defines
the unit in which state writes commit.

Within one handler invocation, writes to that agent's fields are staged. Reads
observe earlier writes from the same invocation. At a successful return, the
write set is persisted as one state commit. A fault persists none of it. The
revision and map update in `setLine` therefore do not become independently
visible merely because they use different storage kinds.

The guarantee stops at the agent boundary. If one operation changes two baskets,
each remote handler commits its own state. There is no automatic two-phase
commit joining the two owners, and a failure after the first call does not undo
it. Cross-owner consistency needs a protocol such as idempotent operations,
compensation, or a saga.

This is not a missing annotation. It follows from choosing independent keyed
owners, especially when those owners may live on distributed infrastructure.
Making the boundary visible helps the author see where a local invariant ends
and a distributed process begins.

Atomicity also does not imply exactly-once execution. A queue or scheduled call
may be retried. Replacing a map value with the same value may tolerate that;
incrementing a revision or appending a log entry may not. The state commit
prevents partial local writes. It does not decide whether repeating the whole
handler is safe.

== State outlives the code that wrote it

An owner persists across calls and may persist across deployments. That makes
type evolution part of the state model.

When Bynk loads stored agent state, it validates the values against the current
definitions, including refinements. A value written under an older definition
does not acquire validity merely because it came from the program's own
storage. If a deployment tightens `Quantity` and old data no longer satisfies
the rule, loading that state faults rather than silently coercing or dropping
it.

Adding a new field is less disruptive when it has a defined beginning. The
loaded state is combined with the agent's current zero values and initialisers,
so a newly added zeroable field receives its declared default instead of
appearing as `undefined`.

This is useful protection, not a migration system. Bynk does not currently
provide a versioned state schema or an automatic repair path for a breaking
change. Tightening a refinement can orphan persisted data until the team plans
and performs a migration. State ownership tells us where the problem lives; it
does not make long-lived data effortless.

== Could TypeScript do this?

Yes. An aggregate class can keep its state private and expose domain methods. A
repository can load one aggregate, execute a callback under optimistic locking,
and save it atomically. Actor frameworks and Durable Objects already organise
work around keyed instances. Branded identifiers, immutable state, and careful
constructors can express most of the same discipline.

Bynk's contribution is to make the combination a standard language shape. The
agent key, store fields, effectful handlers, fresh-state rule, and commit boundary
are declared together and checked together. The generated implementation can
then use the target's state mechanism without making its API the architecture
of the domain.

The cost is constraint. State must fit the available storage kinds. Every
instance must have a meaningful fresh state. Cross-agent transactions are not
provided. A poorly chosen key can create a hot owner, scatter data that needs
to change together, or gather unrelated data into one oversized instance.
Queries across many owners may require a separate index or reporting model.

Nor does an agent establish who is entitled to name a key. If an HTTP handler
accepts an arbitrary `CustomerId`, the fact that `Basket(owner)` has well-defined
state does not authorise the caller to see it. Ownership of state and authority
of callers are separate questions. Chapter 7 will join them.

For now, the important advance is that remembered values no longer float behind
a general storage interface. Their identity, lifecycle, access operations, and
local commit boundary have a home in the program.

The agent still permits any assignment whose type is correct. A basket line may
remain positive while a wider business rule is broken. An order status may move
from `Cancelled` back to `Pending` if both values belong to the status type.
Ownership tells us where state changes. It does not yet say which changes are
valid.

That is the next problem.
