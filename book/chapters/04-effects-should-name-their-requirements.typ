#import "../template.typ": code-listing, compiler-message

= Effects should name their requirements <effects-should-name-their-requirements>

The payment operation now tells the truth about its outcomes. It can return an
authorisation reference, a decline, or an unavailable provider. A caller can no
longer mistake one of those cases for another without making an explicit
choice.

But an operation has a middle as well as an ending.

To reach its result, payment may open a network connection. It may read a
credential, consult a clock, write an audit event, increment a metric, or wait
on a rate limiter. These actions change how the operation can be run. They
determine what must exist in production, what must be substituted in a test,
what can fail independently, and which parts of the world the code is trusted
to reach.

None of that necessarily appears in the operation's return type. A function can
offer an exact `Result` while obtaining the objects it needs from module
globals, a container, ambient platform state, or a helper several calls below.
The destination is explicit; the route remains hidden.

Part I asked whether the program preserves the meaning of its components,
values, and outcomes. Part II turns to authority: what an operation may ask the
world to do, who owns the state it changes, and whose identity gives a call its
meaning.

We begin with the invisible middle.

== A typed outcome can still hide the work

Consider a TypeScript payment function whose domain result is carefully
modelled:

#code-listing(
  [The outcome is explicit, but the operation's requirements are ambient],
  read("../snippets/chapter-04/conventional.ts"),
  lang: "typescript",
)

`PaymentResult` makes decline and provider unavailability visible to the
caller. That is an improvement over a rejected promise with an undocumented
exception. Yet the signature of `authorise` says only that it needs an amount
and will eventually produce a result. It says nothing about `bank` or `audit`.

A reader discovers those dependencies by opening the body. The bank object may
itself reach for configuration and a network client. The audit object may write
to a queue through another imported singleton. To know what the operation
requires, the reader follows the call graph until it reaches the edge of the
program, then hopes no dynamic lookup changes the answer.

This is often a reasonable way to build software. Module imports are concise,
platform clients are convenient, and a small application may have only one
sensible implementation of each dependency. The problem appears under
variation.

A unit test wants a bank that never performs network I/O. A command-line tool
wants to reuse the calculation without configuring the audit service. A worker
must use a different credential from the HTTP process. A retry wrapper needs to
know which calls can repeat external work. Each variation exposes another fact
that the signature left implicit.

The hidden dependency also weakens review. Adding an audit call appears to be a
small line in a function body, but architecturally it gives the operation a new
reason to fail and the deployment a new service to supply. Nothing at the
boundary announces that its requirements have changed.

#quote(block: true)[
  A return type says how a call may finish. A requirement list says what it may
  ask the world to do.
]

Both belong to the contract of effectful work.

== Effect marks the boundary

Bynk distinguishes pure computation from work that participates in effects.
The payment operation carries that distinction in its type:

#code-listing(
  [Authorisation names both its outcome and its direct requirements],
  read("../snippets/chapter-04/declared/src/commerce/payment/authorise.bynk"),
  lang: "bynk",
)

The outer `Effect` says that evaluating the operation involves effectful
execution. The inner `Result` says that the domain outcome may be an
authorisation reference or a `PaymentError`. These are independent claims.

`Effect[Result[String, PaymentError]]` does not mean that failure has been
hidden inside an effect. The effect describes how the operation is performed;
the result describes a value it produces. A pure calculation could return the
same `Result` without contacting anything. An effectful logger might return
`Effect[()]` because its useful outcome is only completion.

The body also separates two forms of sequencing. `<-` performs an effect and
binds its produced value. `do` performs an effect whose value is not needed.
After the bank call, `outcome?` still propagates a declared payment error just
as it did in Chapter 3. Effect sequencing and outcome propagation cooperate,
but neither substitutes for the other.

`Effect` alone is not a permission list. It tells us that an effect boundary
exists, not what can cross it. That information comes from capabilities.

== Name what the world can do

A capability is a contract for a related set of effectful operations. The
payment context declares three:

#code-listing(
  [Capabilities describe effects without choosing their implementation],
  read("../snippets/chapter-04/declared/src/commerce/payment/capabilities.bynk"),
  lang: "bynk",
)

`Bank` can request a charge. `Audit` can record an authorisation. `Log` can
write an informational event. Their declarations have no bodies, and every
operation returns an `Effect`. They say what the surrounding program may be
asked to provide without saying how the work reaches a particular bank,
logging system, or storage service.

This separation matters because a capability is not merely an interface
attached to a concrete client. It is part of the vocabulary with which the
source describes authority. A handler that is given `Bank` may request the
bank operations in that contract. It has not thereby received a database,
clock, filesystem, or general network handle.

The `given Bank, Audit` clause on `authorise` is its direct requirement list.
Before reading the implementation, we know that this handler can charge through
the bank and record through audit. A new call to `Log`, for example, requires a
visible change to the handler's declaration.

This is deliberately different from a service dependency between contexts.
Chapter 1 used `consumes` to say that one domain context may call a service
presented by another. A capability describes an effectful facility supplied to
code within that architecture. Both make dependencies explicit, but they
operate at different boundaries.

Capabilities can also be exported and used across contexts when that is the
right design. The important constraint remains: availability is declared. A
capability does not become ambient merely because more than one context needs
it.

== A useful refusal

Suppose the authorisation handler calls `Bank.charge` but omits `Bank` from its
`given` clause:

#code-listing(
  [The bank exists, but this handler has not declared that it requires it],
  read("../snippets/chapter-04/undeclared/src/commerce/payment.bynk"),
  lang: "bynk",
)

The operation and its types are otherwise straightforward. Bynk refuses the
call for an architectural reason:

#compiler-message[
[bynk.given.undeclared_capability] Error:
capability `Bank` is used but not listed in the handler's `given` clause
]

The remedy is not to hide the call behind a helper. It is to add `Bank` to the
requirement list or to redesign the operation so it does not need the bank. In
either case, the source and the implementation must agree.

This refusal gives `given` more force than documentation. A comment saying
“requires bank access” can fall out of date. A list maintained only for a
dependency-injection framework can be bypassed by importing another singleton.
Here, capability use is checked against the handler boundary where the
requirement is declared.

The compiler also warns when a declared capability is unused. That warning is
less fundamental than rejecting undeclared use, but it keeps the list from
becoming ceremonial. A capability should describe a requirement the
implementation actually has, not a set copied from a neighbouring handler in
case one becomes useful later.

== Providers make the requirements concrete

A contract does not contact a bank. Something must implement it.

Bynk calls that implementation a provider. For the example, we can supply
deterministic providers that need no external systems:

#code-listing(
  [Providers implement capabilities and declare requirements of their own],
  read("../snippets/chapter-04/declared/src/commerce/payment/providers.bynk"),
  lang: "bynk",
)

`SandboxBank` implements every operation in `Bank` with the required type.
`SilentLog` implements `Log`. These examples are intentionally small so the
shape remains visible. A real bank provider would usually delegate to an
adapter at the generated TypeScript boundary, where a host library or SDK
performs the physical I/O.

`LoggedAudit` shows the more interesting case. It provides `Audit`, but needs
`Log` to do so. Its own `given Log` clause declares that dependency. Supplying
an audit facility therefore introduces another node in the effect graph rather
than silently importing a logger.

The distinction between direct and transitive requirements is important. The
`authorise` handler says `given Bank, Audit` because those are the facilities
its implementation uses. It does not say `given Log`. Logging is a requirement
of this particular audit provider, not an inherent requirement of authorising
a payment.

Choose a different audit provider and the transitive graph may change while
the handler remains valid. Replace `LoggedAudit` with an in-memory recorder and
no logger may be needed. Replace `SandboxBank` with a production bank provider
and configuration, clock, or transport capabilities may enter below it. The
handler is coupled to the capability contract; composition chooses the
implementation and satisfies what that implementation requires.

This is where providers do more than make testing convenient. Their declared
dependencies form a graph the compiler can check and order. If `Audit` requires
`Log`, composition must make `Log` available before constructing the audit
provider. If providers form a dependency cycle, there is no valid order to
discover. Composition is constrained by the same information the source uses
to describe effectful authority.

The graph is still only as informative as its capability boundaries. A
provider called `World` with operations for HTTP, storage, time, randomness,
secrets, and logging would technically make its dependency explicit. It would
also recreate a service locator under a more respectable name. Almost any
handler could ask almost anything of it, and changes to authority would once
again disappear inside implementations.

At the other extreme, one capability per function can fragment a coherent
facility into bookkeeping. Good capability design groups operations that
belong to the same useful authority and tend to vary together. The language can
enforce a boundary once declared; it cannot decide where that boundary should
be.

== Could dependency injection do this?

Yes. A disciplined TypeScript program could make the opening function accept
an explicit dependency object:

```typescript
type PaymentDependencies = {
  bank: Bank;
  audit: Audit;
};
```

Passing that object to `authorise` would reveal its direct requirements, permit
test doubles, and make composition a deliberate activity. Constructor
injection, functional environment values, and typed effect libraries can carry
the same idea across a large application. These are established techniques,
not approximations of serious design.

The difficulty is consistency and enforcement. Nothing in ordinary TypeScript
prevents the injected function from also importing a global clock or opening a
client through another module. A team can prohibit that by convention, linting,
review, module boundaries, or a library discipline. Bynk gives the pattern one
language-wide representation and rejects capability use that escapes it.

That choice has a cost. `Effect` and `given` add ceremony to operations that
would otherwise be plain functions. Capability contracts and providers add
names. A handler with a long requirement list may be awkward to read and
compose. Some teams will reasonably prefer a smaller language and strong local
conventions, especially where effects are few and the codebase is compact.

The long list can also be valuable evidence. It may reveal that an apparently
simple handler performs payment, storage, messaging, logging, clock access, and
feature evaluation. The compiler cannot say whether to split that handler, but
it can keep the breadth of its authority from being invisible.

== What the list does not promise

A declared capability is a limit on source-level access, not a proof that the
world will behave.

`Bank.charge` may be slow, unavailable, non-idempotent, or incorrectly
implemented. Its provider may send sensitive data to the wrong endpoint. An
adapter executing in the host environment can possess physical privileges
beyond the capability its Bynk-facing contract exposes. Capability checking
does not replace operational controls, security review, timeouts, observability,
or careful error translation.

Nor does the list describe every property a caller might need. Two providers
can implement the same signature while offering different latency, consistency,
privacy, or delivery guarantees. If one of those differences changes the
meaning of the program, it needs representation in the contract, the
composition policy, or the deployment environment. The word `capability` does
not make an underspecified interface complete.

What the model preserves is narrower and still useful. Effectful work is marked.
Its direct requirements are named. Implementations are separated from those
requirements, and their own dependencies remain visible to composition. A new
kind of effect cannot enter a handler through the capability mechanism without
changing a declaration the compiler checks.

That makes the invisible middle inspectable. Given an operation, we can ask not
only what values it accepts and what outcomes it returns, but what authority it
requires in order to get from one to the other.

The question becomes more difficult when the effect is state.

A `Storage` capability could tell us that an operation may read and write. It
would not tell us whose state is being changed, how long that state lives,
which operations preserve its invariants, or whether two concurrent calls are
acting on the same thing. Permission to touch state is not ownership of state.

That is the boundary the next chapter must make explicit.
