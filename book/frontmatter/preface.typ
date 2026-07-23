= Preface <preface>

This book began with a frustration that will be familiar to anyone who has
worked on a service for long enough: the design can remain sensible while
becoming steadily harder to see.

The first version of a system may place its important decisions close together.
A reader can find the public boundary, the domain operation, the state change,
and the external call in one short path. As the system becomes useful, those
decisions spread across routes, workers, middleware, queues, repositories,
configuration, tests, and deployment files. Nothing needs to have gone badly
wrong. Growth alone can increase the distance between the architecture a team
believes it has and the evidence available in the program.

Bynk is one response to that distance. It asks a programming language to
understand more of the vocabulary in which service architecture is discussed:
domain identity, admissible values, failure, effects, state ownership, caller
authority, entry points, and dependency boundaries. The wager is that decisions
expressed in that vocabulary can participate in compilation rather than survive
only as conventions around the code.

This is a book about that wager.

== What this book is for

The intended reader is an experienced TypeScript or backend developer who has
felt the cost of reconstructing a service before changing it. No prior knowledge
of Bynk is assumed, and adopting the language is not the price of admission.

Each chapter begins with a problem that exists independently of Bynk. It
examines the conventional responses available to capable teams, the
architectural information those responses preserve or lose, and the language
construct Bynk offers in return. Successful examples show what the model can
state. Compiler refusals show which contradictions the model treats as
unacceptable. The later chapters test the argument against a whole system and
account for the flexibility, tooling, and organisational costs of stronger
constraints.

The aim is not to prove that every service should be written in Bynk. It is to
make the trade legible. A reader should finish able to recognise where
architectural knowledge disappears in ordinary service code, understand Bynk's
attempt to retain it, and judge whether that attempt fits the dominant risks of
a particular system.

== The print book and the online Book

This volume is not the Bynk language reference in hard covers.

The online Bynk Book owns exact syntax, compiler options, diagnostic codes,
capability APIs, target configuration, and step-by-step guidance. Those details
need to follow the implementation closely and should be searchable, linkable,
and correctable without waiting for another printing.

The print book has a different responsibility. It develops the reasoning behind
the language through a sustained narrative. Its examples are evidence for an
argument rather than an exhaustive catalogue of features. When a precise rule
or command matters, the online Book remains the authority.

That division is deliberate. A durable hardcopy should explain why a design is
worth considering even after individual spellings and compiler switches have
changed. A living reference should tell a developer exactly how the current
toolchain behaves.

== How to read the examples

The examples use one evolving family of service problems: orders, payments,
inventory, customer identity, messages, and scheduled work. They are small
enough to print but are compiled as projects rather than treated as
pseudocode. Rejected examples are retained intentionally when a diagnostic is
the evidence under discussion.

Most chapters can be read without following every expression. Unit headers,
contracts, and compiler messages often carry the architectural point. Readers
who want to run or extend an example can use the manuscript sources alongside
the online reference.

The book's recurring contrast with TypeScript is not a claim that TypeScript is
careless or architecturally weak. It is the host language Bynk emits, the
ecosystem many intended readers know, and a useful way to expose which facts a
general-purpose type system sees directly and which facts a team supplies
through libraries and conventions.

== A note on versions

Bynk is approaching 1.0 while this manuscript is being developed. The working
edition therefore describes a moving implementation and is explicit where a
constraint is fundamental to the language model rather than merely a present
feature boundary.

Before publication, the copyright page and companion material will name the
compiler release against which every example was checked. The online Bynk Book
will remain the source for later changes. The source map maintained with this
manuscript records the documentation and implementation areas consulted for
each chapter; it is an editorial audit trail, not a substitute for the reader's
reference.

The larger questions are less version-sensitive. What should a value mean after
admission? Where should an effect receive its authority? Who owns a state
transition? Which caller is allowed to request it? What failure has the public
contract promised to handle? How much of a deployed system can a reader recover
from source?

Those questions are the route through the book. Bynk's answers are specific.
The pressure that produced them is not.
