#import "../template.typ": code-listing, compiler-message

#let source-lines(path, start, end) = {
  read(path).split("\n").slice(start, end).join("\n")
}

= State changes are contracts <state-changes-are-contracts>

The basket in Chapter 5 owns its state. Its key selects one logical instance,
its handlers are the only public route to its fields, and its writes commit
together. Those properties tell us where a change happens.

They do not tell us whether the change makes sense.

An order can own a `status` whose type contains `Draft`, `Placed`, and `Paid`.
Every one of those values is valid. Assigning `Placed` to an order that is
already `Paid` therefore passes an ordinary type check. So does clearing the
payment reference while leaving the status as `Paid`. Each field still contains
a member of its declared type; the contradiction exists between fields or
between moments.

This is the difficulty of stateful rules. Some facts concern one value. A
quantity must be positive. Some concern one snapshot. A paid order must have a
payment reference. Others concern a change over time. Once paid, an order must
not become unpaid.

The first kind fits a type. The second and third need contracts over state.

== A state type is not a state machine

A TypeScript implementation can make the current status and expected outcomes
explicit:

#code-listing(
  [An exhaustive operation coexists with a type-correct escape hatch],
  read("../snippets/chapter-06/conventional.ts"),
  lang: "typescript",
)

`pay` handles every status. The exhaustive `switch` makes it difficult to add a
new status without revisiting payment. Its `UpdateResult` also separates an
expected rejection from a successful update.

Then `restoreStatus` accepts any `OrderStatus`. It may have been added for an
administrator, an import, or recovery from an earlier defect. Nothing in its
types prevents a paid order being restored to `Placed`. Nor does
`OrderState` prevent `{ status: "Paid", paymentRef: null }`.

We could remove the generic operation and make every constructor private. We
could encode `OrderState` as a discriminated union in which the `Paid` variant
always carries a non-null payment reference. Both are strong improvements.
The union can make the invalid snapshot unrepresentable.

It still does not remember the previous snapshot. `Paid` and `Placed` can each
be valid values while the move from the first to the second is forbidden. That
rule lives in the operations, a reducer, a state-machine library, a database
constraint with history, or a convention around who may construct the next
value.

#quote(block: true)[
  A valid state does not imply a valid transition.
]

Calling a record with a status field a state machine is therefore premature. A
machine has a set of states, a starting state, and rules for the steps between
them.

== Make the lifecycle finite

The Bynk order begins by naming its finite vocabulary:

#code-listing(
  [The lifecycle, failures, and observable snapshot have distinct types],
  read("../snippets/chapter-06/declared/src/commerce/orders/types.bynk"),
  lang: "bynk",
)

`OrderStatus` is a sum. It says that the current lifecycle state is exactly one
of `Draft`, `Placed`, or `Paid`. The agent gives that sum an explicit initial
value and places two contracts beside its stored fields:

#code-listing(
  [The agent declares a snapshot invariant and a step invariant],
  source-lines(
    "../snippets/chapter-06/declared/src/commerce/orders/order.bynk",
    0,
    13,
  ),
  lang: "bynk",
)

The starting state is `Draft`, so a fresh order has a real lifecycle state
rather than an invented null. `paymentRef` starts as `None`, which is honest for
an order that has not been paid.

The two predicates answer different questions.

`paid_has_payment_ref` is an `invariant`. It looks at one proposed committed
state. If the status is `Paid`, the payment reference must be present. In
logical terms, `implies` means that the right side is required whenever the
left side is true. For a draft or placed order the predicate says nothing about
the reference.

`paid_is_terminal` is a `transition`. It sees `old`, the last committed state,
and `new`, the state about to be committed. If the old status was `Paid`, the
new status must also be `Paid`. The predicate constrains the step, not either
snapshot in isolation.

Both predicates are pure, local to the agent, and boolean. They cannot consult
a capability or another agent. A contract that changed while being checked, or
depended on a remote answer, would not be a stable claim about this owner's
state.

== Handlers still make the decisions

Contracts do not replace transition logic. The order's ordinary handlers still
decide what each request means in every current state:

#code-listing(
  [Exhaustive handlers accept or reject the expected business requests],
  source-lines(
    "../snippets/chapter-06/declared/src/commerce/orders/order.bynk",
    14,
    36,
  ),
  lang: "bynk",
)

`place` accepts only a draft. `pay` accepts only a placed order and updates the
status and payment reference together. Every other case returns an
`OrderError`.

These rejections are part of normal operation. A repeated payment request is
not evidence that the program has broken its own rules; `AlreadyPaid` is an
outcome a caller can inspect. Adding a fourth status would make both matches
non-exhaustive, forcing the author to decide what placement and payment mean in
that new state.

The contracts serve a different purpose. They protect the owner against every
handler, including one added later that forgets the established protocol. The
matches make the intended paths explicit. The invariant and transition keep a
new path from silently violating the promises made by the agent.

This division matters. If insufficient stock, an already-paid order, or an
expired offer were expressed as invariant violations, ordinary business
conditions would become internal faults. Expected refusals belong in `Result`.
An invariant violation means the implementation attempted to commit a state it
had promised could never exist.

== Two ways to be wrong

The compiler accepts the following maintenance handlers because their
assignments are well typed:

#code-listing(
  [Each maintenance operation breaks a different state contract],
  source-lines(
    "../snippets/chapter-06/declared/src/commerce/orders/order.bynk",
    37,
    50,
  ),
  lang: "bynk",
)

Suppose a placed order is paid successfully. `reopenForReview` then proposes a
state with status `Placed` and the existing payment reference. The snapshot is
internally consistent: the `paid_has_payment_ref` implication does not apply
when the new status is `Placed`. The step is illegal because the old status was
`Paid`. `paid_is_terminal` catches what the snapshot invariant cannot.

`forgetPaymentReference` does the opposite. It leaves the order `Paid`, so the
terminal-state transition is satisfied. The proposed snapshot has no payment
reference, so `paid_has_payment_ref` fails.

At runtime the two attempts report the contract that refused the commit:

#compiler-message[
InvariantViolation: Order.paid_is_terminal \
InvariantViolation: Order.paid_has_payment_ref
]

Neither offending state is persisted. The previously paid order remains paid
with its payment reference intact.

These examples are intentionally obvious. Real violations tend to arrive
through less conspicuous changes: a privacy cleanup that clears too much, a
recovery path that restores an old status, a second handler that updates only
one of two related fields, or a new variant whose meaning was not incorporated
into an existing rule. The value of the contract is that the later author does
not need to remember every earlier handler in order to preserve the owner's
declared promises.

== The commit is the checking point

Invariants and transitions are checked against the state a handler proposes to
commit, not after each assignment.

That timing allows `pay` to set `status := Paid` and then
`paymentRef := Some(ref)`. Between those lines, its working state would fail
`paid_has_payment_ref`. No other handler can observe that intermediate value,
and it is never offered as a committed snapshot. When the handler returns, the
complete proposed state satisfies the invariant and is written atomically.

If a predicate fails, the state write is abandoned before persistence. As in
Chapter 5, that does not rewind unrelated effects the handler has already
performed. A message sent or another agent call completed earlier in the
handler still stands. The contract protects the local commit, not the whole
world.

A transition also needs a real previous commit. On an agent's first commit
there is no `old` snapshot, so step predicates are skipped. Snapshot invariants
still apply to that first state. This makes the starting-state declaration and
snapshot contracts responsible for genesis; transitions govern the history
that follows.

The runtime check is a deliberate trade. Bynk does not currently prove that a
handler must violate an invariant merely by inspecting its source. The invalid
maintenance handlers compile. Enforcement happens wherever the commit runs, in
production as well as tests.

== The guarantee is weaker here, and worth admitting

This book's recurring claim is that Bynk moves architectural facts into the
program, where the compiler can refuse a contradiction before the code runs. The
state contract is the one place where that claim must be qualified.

An invariant or transition is not tested when the program is compiled. It is
tested when a handler tries to commit, wherever that commit happens to run. A
violation is not a compile error a reviewer meets in a diff; it is a runtime
fault in production, on a channel a caller cannot currently pattern-match. That
is close to the situation Chapter 3 objected to. There, a domain failure hidden
in a thrown exception was a promise the contract did not make. Here, an
invariant breach surfaces as an internal fault that the type system did not
force any caller to anticipate.

The discomfort is real, and smoothing it over would be dishonest. So why does
the book still count this as progress?

Because the alternative in a conventional service is not a compile-time proof.
It is the same rule living in a reducer, a database trigger, a review comment,
or nobody's memory, upheld only by the discipline of whoever writes the next
handler. Placing it on the agent that owns the state changes three things even
without a static proof. The rule is written once, beside the state it governs,
instead of being repeated at every mutation site. It is enforced against every
handler, including the one added next year by someone who never read the
others. And when it fails, the report names the specific contract that refused
the commit, at the owner where the state lives, rather than appearing as corrupt
data discovered three systems downstream.

That is a weaker guarantee than an unrepresentable state or an exhaustive match,
and a stronger one than a convention. Where a rule can be encoded so that a bad
state cannot be constructed at all---the discriminated identities of Chapter 2,
the closed sums of Chapter 3---that remains the better tool, because it fails at
compile time and needs no runtime check. The invariant is for the rules that
genuinely resist that treatment: relationships between fields, and relationships
between one commit and the next. For those, a checked contract on the owner is
the most the current language can honestly offer, and more than most systems
keep anywhere at all.

Whether a future version could prove some of these statically, rather than
checking them at the commit, is an open question the language has not yet
answered.

== A useful refusal

The difference between a snapshot and a step also appears in the declaration
rules. Consider a `transition` that mentions only the current field name:

#code-listing(
  [This predicate describes one state, despite being labelled a transition],
  read("../snippets/chapter-06/misclassified/src/commerce/orders.bynk"),
  lang: "bynk",
)

The predicate may be true, but it says nothing about a move. It mentions
neither `old` nor `new`, so Bynk rejects the classification:

#compiler-message[
[bynk.transition.no_step_reference] Error:
transition `has_known_status` references neither `old` nor `new`,
so it constrains a single state, not a step
]

The repair is to declare it as an `invariant`, or to express a genuine relation
between the previous and proposed states. The refusal prevents `transition`
from becoming a decorative synonym that obscures what kind of claim the author
intended.

Other declaration rules preserve the same character. A predicate must produce
`Bool`, must be pure, and must remain inside one agent. Names are unique because
the failing name appears in the runtime report. These checks do not establish
that the rule is wise, but they keep its subject and enforcement model
unambiguous.

== Contracts do not design the machine

An invariant can be too weak. `status == status` is universally true and
protects nothing. A transition can prohibit one bad move while overlooking
five others. `paid_is_terminal` does not by itself say that `Draft` may move
only to `Placed`, or that payment must immediately follow placement. The
handlers still carry those choices.

Contracts can also be too strong. If an administrator genuinely needs a
controlled reopening process, declaring paid as terminal forbids the feature
at the state boundary. The team must either reject the feature, revise the
contract and its consequences, or model a richer state such as
`PaymentUnderReview`. The friction is useful only when the declaration reflects
a decision the system should preserve.

Nor can an agent-local predicate establish a global truth. “Every captured
payment has exactly one matching settlement” spans owners and often external
systems. Querying another agent during a commit check would introduce failure,
latency, and circularity into the local atomic boundary. Such a rule needs a
protocol, reconciliation, or monitoring rather than an invariant pretending
the distributed world is one snapshot.

The predicate language is therefore deliberately constrained. It is suited to
stable, local relationships over stored values and to relations between two
local commits. Broader temporal claims may need tests over histories, and
cross-owner claims need distributed coordination. Later chapters will return
to both.

== Could TypeScript do this?

Yes. A discriminated union can encode snapshot consistency more strongly than
the opening record. A reducer can accept only events and centralise legal
steps. A class can hide constructors and mutation. State-machine libraries can
generate transition tables and visualisations. Database checks and transactions
can enforce important properties at the final persistence boundary.

A disciplined combination of those techniques can be excellent. In some
systems a database constraint is the strongest possible home for a rule because
many applications write the same data. In others, making invalid states
unrepresentable in a union removes the need for a runtime invariant entirely.

Bynk provides a common source-level place for the rules that remain: the agent
that owns the state. Exhaustive matches, snapshot invariants, step invariants,
and the atomic commit boundary share one model. The cost is runtime checking,
restricted predicates, and a fault channel that callers cannot currently
pattern-match. An invariant breach is observable as an internal failure, not a
typed domain result.

The language still relies on judgement to choose the right mechanism. Use a
type when one value can carry the truth. Use a `Result` when a caller is expected
to encounter and handle a refusal. Use an invariant when every committed
snapshot must satisfy a local property. Use a transition when the legality lies
in the move from one snapshot to the next.

With those choices, the order can say where its state lives, what states mean,
which expected requests it rejects, and which histories it refuses to persist.

One question remains outside the machine.

The operation `pay` may be legal from `Placed`, and `reopenForReview` may be
illegal from `Paid`. Neither statement identifies who is permitted to request
the operation in the first place. The next boundary is not state but the caller
whose authority gives a call its meaning.
