= Prologue: The diagram is already wrong <prologue>

The service began as four boxes on a whiteboard.

One received HTTP requests. One contained the business logic. One stored data.
The last sent messages to an external provider. Three arrows connected them.
The boundaries were clear, the responsibilities obvious, and the whole system
could be explained in less than a minute.

For a while, the implementation looked much the same.

There was a route handler, a small set of domain types, a repository, and a
client for the external API. The names in the code resembled the names on the
whiteboard. A new developer could place the two side by side and recognise the
same design.

Then the service became useful.

A second kind of caller needed access, but with slightly different permissions.
A webhook arrived from a partner. Retries were added because the partner was
not always available. A scheduled task found operations that had become stuck
between attempts. One customer needed a stricter validation rule. Another
needed an exception to it. A cache appeared. A queue followed. Someone moved a
check out of a handler so that two endpoints could share it.

None of these decisions was obviously wrong. Most were sensible responses to
real requirements. Each change was reviewed, tested, and deployed.

The diagram still had four boxes.

It was not entirely false. The HTTP service still existed, as did the business
logic, the storage, and the external provider. But the important questions
could no longer be answered by looking at it.

Which component owned the state of an operation? Which callers could cause that
state to change? Could the scheduled task perform the same actions as an
authenticated user? Was the webhook signature verified before or after its
payload was decoded? Which pieces of code were allowed to contact the external
provider? What happened when that provider failed? Could two retries update the
same record concurrently? Which tests exercised the real boundary, and which
replaced it with a mock?

The answers existed, but they were scattered.

Some lived in code. Some lived in configuration. Some lived in tests. Some
lived in comments that had once been accurate. A few lived only in the memories
of the people who had built the system.

The architecture had not disappeared. It had become implicit.

That distinction matters. An implicit architecture can still be a good
architecture, but maintaining it requires people to continually reconstruct
it. Every change begins with an archaeological exercise: trace the imports,
inspect the middleware, find the dependency-injection bindings, search for
writes to the table, read the tests, and ask someone why the retry worker is
allowed to call a function that the public API is not.

Modern programming languages can help with parts of this investigation. A type
checker can tell us that a function expects a string and returns a promise. It
may know the shape of a request or the fields in a database record. It can stop
us passing a number where a string belongs.

But it generally does not know what the string means.

It does not know that one string is an order identifier and another is a
customer identifier. It does not know that a percentage must be between zero
and one hundred, or that an operation may move from `Pending` to `Accepted` but
never back again. It does not know that a function performs an external effect,
that a particular component owns a piece of state, or that one category of
caller may observe a value but must never change it.

We compensate by building conventions around the language.

We create branded types and validation schemas. We arrange files into layers
and agree which layers may import which others. We use dependency-injection
containers to describe services and their implementations. We encode
permissions in middleware. We wrap storage access in repositories. We represent
state machines through enums, conditional statements, and care. We write
architecture tests to check that the other tests have not bypassed the
architecture.

These techniques can work. In well-run teams, they often work very well.

But they share a weakness: the language does not understand why they exist.

To the compiler, a folder boundary is usually just a path. A naming convention
is just a name. A validation schema is a value in a library. A
dependency-injection binding is runtime configuration. An architectural rule is
something enforced elsewhere---by a linter, a test, a review checklist, or the
attention of the next developer.

The system's most important facts are present only by implication.

Bynk begins with a question:

#quote(block: true)[
  What if those facts belonged to the language?
]

What if a deployable boundary were not inferred from a directory, but declared
as a context? What if an HTTP interface, a queued message, and a scheduled task
were different kinds of entry point that the compiler understood? What if state
had a named owner? What if external effects were visible in a function's
contract? What if the identity of the caller were part of the operation being
described? What if a domain type could define not only the shape of its values,
but which values were admissible in the first place?

This changes what a program can say.

A Bynk program does not consist solely of functions and data structures. It can
speak in the vocabulary of a service architecture: contexts, services, agents,
actors, capabilities, handlers, effects, and invariants. These are not
annotations added for the benefit of a documentation generator. They are
constructs that participate in checking and compilation.

The distinction is subtle but consequential.

If architecture is a convention, the compiler can preserve the implementation
while the design quietly decays around it. If architecture is part of the
program, some kinds of drift become compile errors.

This is the central bet behind Bynk: that many failures we accept as inevitable
runtime or organisational problems are really failures of representation. The
language in which we expressed the system did not have the vocabulary to state
the rule we cared about.

Consider a customer identifier and an order identifier. In many services, both
are strings. We know they are different because of their variable names, their
database columns, and the surrounding code. The type checker knows only that
both are strings. Accidentally passing one in place of the other is therefore a
valid program whose meaning is wrong.

Or consider a percentage that must fall between zero and one hundred. A runtime
validator can reject `240` when it arrives at an API boundary. But once the
value has passed validation, does the rest of the program know that it is valid?
Or does it continue to carry an ordinary number, accompanied by an assumption?

The same pattern appears at larger scales. A team knows that only one component
should write a piece of state, but the implementation exposes a database client
that can be imported anywhere. It knows that a handler may send email, but that
fact is absent from the handler's signature. It knows that a transition requires
an authenticated caller, but the function containing the transition accepts no
representation of that authority.

In each case, knowledge possessed by the designers has been erased from the
program.

Bynk tries to retain more of it.

That does not make the difficult questions disappear. A language cannot choose
the right service boundaries, discover the correct domain model, or decide who
should be authorised to do what. Those are design decisions, and no syntax can
relieve us of making them.

What a language can do is remember the decisions once made.

It can make the boundary visible. It can distinguish identities that share an
underlying representation. It can require failure to appear in a contract. It
can make effects explicit. It can recognise the owner of state and check rules
governing its transitions. It can refuse a program that contradicts the
architecture the program itself declares.

This means Bynk is opinionated in a deeper sense than a framework.

A framework offers a way to organise a system. Bynk limits the systems that can
be expressed. Some of those limits are there because the language is young.
Others are deliberate. They remove forms of flexibility that are easy to use
locally but difficult to reason about globally.

That comparison invites the sharpest objection to the whole enterprise, and it
is better stated now than avoided. If the aim is only to make architectural
facts explicit and checkable, why a new language at all? A capable team could
brand its types, lint its import boundaries, generate its wiring, and forbid
ambient effects using tools it already owns. Much of what follows can indeed be
approximated that way, and later chapters will admit as much at each step. The
open question is whether an assembly of conventions, libraries, and lint rules
holds the line as reliably as a language whose compiler treats these facts as
the definition of a valid program---and whether that reliability is worth
another compiler standing between a team and its running service. The trade is
real, not rhetorical. Part IV faces it directly.

That trade deserves scrutiny. Constraints are not valuable merely because they
are constraints. An inflexible model can prevent bad designs, but it can also
prevent necessary ones. A compiler can enforce a declared architecture while
the architecture itself remains mistaken. Making an effect explicit does not
make the effect wise. Making invalid states unrepresentable depends on correctly
identifying which states are invalid.

Nor does Bynk attempt to replace the world beneath it. It compiles to typed
TypeScript and targets Cloudflare Workers. Contexts become deployable components
on an existing platform; agents map to an existing model for keyed, stateful
computation. The emitted code is intended to be read and debugged.

This is another bet: that a language can be ambitious about how programs are
described while remaining pragmatic about how they run.

The result sits somewhere between a programming language, an architectural
model, and a set of arguments about service design. To understand Bynk is
therefore not merely to learn its syntax. It is to examine those arguments.

Should invalid input be rejected at a boundary, or should invalid values be
impossible to carry beyond it? Should a dependency be visible because a function
imports it, or because the function's contract declares the capability it
requires? Is state merely data in storage, or does it have an owner and a
lifecycle? Is authorisation middleware surrounding the program, or part of the
meaning of an operation? Do tests verify the architecture, or quietly substitute
another one?

These are the questions this book explores.

It is not a language reference. Exact syntax, compiler options, diagnostic
codes, and capability APIs belong in the online Bynk Book, where they can remain
complete and current. Nor is this a sequence of instructions for building a
particular application.

Instead, we will begin with the things that make service software hard: lost
domain meaning, invisible effects, ambiguous ownership, porous boundaries,
architectural drift, and tests that provide confidence in a system different
from the one we deploy.

For each, we will look at the conventional solutions and why they sometimes
fail. We will examine Bynk's response, the reasoning behind it, and the
restrictions it introduces. Successful programs will show what the language
permits. Rejected programs will often tell us more, because a compiler's
refusals reveal what the language believes a correct system to be.

By the end, the four boxes on the whiteboard will not have become unnecessary.
Diagrams remain useful, as do design documents, conversations, and judgement.

But they need no longer be the only place where the architecture is visible.

The diagram may still become outdated.

The program should not be able to forget.
