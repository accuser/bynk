#import "../template.typ": code-listing, compiler-message

= A new language should not require a new universe <a-new-language-should-not-require-a-new-universe>

A new language does not arrive alone.

It brings a compiler and formatter, editor support, a debugger story, a test
runner, a package strategy, a build pipeline, a deployment target, and a way to
investigate production failures. It asks maintainers to learn its idioms,
operators to trust its output, and an organisation to believe it will still be
supported after the person who introduced it has moved on.

Syntax is a small part of the adoption decision.

This is awkward for Bynk because its argument depends on adding language
constructs. Contexts, capabilities, agents, actors, entry protocols, contracts,
test tiers, and compiler refusals are useful only if a team accepts another
language between its design and its running service. Stronger constraints have
no value when the surrounding cost keeps the language out of production.

#quote(block: true)[
  A language can narrow the set of programs without trying to replace the
  whole world in which those programs must run.
]

Bynk's pragmatic answer is to be new at the front and conservative underneath.
It compiles to TypeScript, uses the JavaScript runtime, and maps its deployment
model onto Cloudflare Workers. That decision buys reach. It also inherits
limitations and creates a seam between two languages that cannot be wished
away.

== The surrounding system is part of the language

Imagine that Bynk instead emitted bespoke bytecode for a new virtual machine.
The language could choose exact integer semantics, design its own concurrency
model, and make every runtime representation match its type system. It would
also need a garbage collector, package distribution, profilers, debuggers,
deployment images, security updates, observability integrations, and years of
operational experience.

None of those needs is secondary to a service team. A beautiful type system
running in an immature environment can be a worse engineering choice than a
less expressive language on infrastructure everyone already understands.

Compiling to an established target changes that equation. JavaScript engines,
TypeScript tooling, npm libraries, source-map-aware debuggers, and serverless
platforms already exist. Bynk can concentrate on the architectural information
it wants to preserve rather than implementing an entire computing substrate.

This is not the same as obtaining an ecosystem for free. Someone still has to
maintain the translation, runtime helpers, editor integration, and platform
bindings. The generated program must remain intelligible to tools that know
nothing about Bynk. Dependency vulnerabilities and platform changes still
arrive from below. Reuse reduces the universe Bynk must create; it also makes
Bynk accountable to the universe it reuses.

== Meaning by translation

Bynk's dynamic meaning is defined by the TypeScript it emits and the runtime
library that output calls. There is no separate Bynk virtual machine whose
behaviour the TypeScript is expected to approximate. If the documented
translation and the emitter disagree, the emitter has a defect.

That is a strong design choice. Generated code is not merely a deployment
format hidden after compilation. It is the operational account of the program.
The default artefact remains typed TypeScript; a build can also emit the same
modules with their types stripped when deployable JavaScript is more useful
than running `tsc`.

Consider a dashboard context that normalises a label and calls a counter
service:

#code-listing(
  [The source names one library seam and one architectural dependency],
  read("../snippets/chapter-11/declared/src/support/dashboard.bynk"),
  lang: "bynk",
)

Several pieces lower to ordinary TypeScript shapes:

#figure(
  block(width: 100%)[
    #set text(size: 8.5pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (0.9fr, 1.25fr, 1.45fr),
      inset: (x: 0.5em, y: 0.5em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Bynk construct],
        text(weight: "semibold")[Typed TypeScript shape],
        text(weight: "semibold")[Runtime significance],
      ),
      [`Effect[T]`], [`Promise<T>`], [Uses the host's asynchronous execution],
      [`Result[T, E]`], [A discriminated union], [Failure remains an ordinary value],
      [Capability], [An interface plus provider class], [External work crosses a named seam],
      [Context], [A module or Worker directory], [The build target selects topology],
      [Agent], [A state interface and class], [Storage is a registry or Durable Object],
    )
  ],
  caption: [The translation preserves Bynk's vocabulary using forms existing tools can inspect.],
)

The correspondence is deliberately recognisable. `<-` becomes `await`. A sum
becomes a discriminated union. A `match` becomes a switch over its tag. A
refined value uses a branded TypeScript type and a constructor that validates
at runtime. These are not the only possible lowerings, but they allow the
generated program to participate in an ecosystem built around TypeScript and
JavaScript.

Recognisable does not mean hand-maintained. Every emitted file says that it is
generated. Editing it would create a third version of the program: the Bynk
source, the compiler's intended output, and the local patch. The value of
readability is inspection, diagnosis, and integration, not an invitation to
fork the build artefact.

== Topology is a build choice

The same source can be emitted for two topologies. The default `bundle` target
places the contexts in one TypeScript tree. `support.dashboard` reaches
`support.counters` through an ordinary in-process dependency, and the `Counter`
agent uses an in-memory state registry.

The `workers` target produces this shape:

#code-listing(
  [Contexts become Worker directories; the adapter remains an in-process module],
  read("../snippets/chapter-11/workers-tree.txt"),
  lang: "text",
)

The dashboard's generated environment contains a `SUPPORT_COUNTERS` Service
Binding. Its call to `increment` becomes a JSON request across that binding,
with generated serialisation and structural validation on the other side. The
counter context's generated `wrangler.toml` declares a Durable Object class for
`Counter`; the dashboard configuration declares the service binding.

#figure(
  block(width: 100%)[
    #set text(size: 8.5pt, hyphenate: false)
    #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    #table(
      columns: (0.82fr, 1.45fr, 1.45fr),
      inset: (x: 0.5em, y: 0.5em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Concern],
        text(weight: "semibold")[Bundle],
        text(weight: "semibold")[Workers],
      ),
      [Context call], [Direct function call], [Validated JSON over a Service Binding],
      [Agent state], [In-process registry], [Durable Object storage],
      [Deployment], [One unit chosen by the integrator], [One Worker per context],
      [Best use], [Fast local or single-unit execution], [Production boundary fidelity],
    )
  ],
  caption: [The source-level architecture is stable while its operational realisation changes.],
)

This alignment is one of the strongest reasons for the Cloudflare target. A
context is already a deployable boundary, so a Worker is a natural runtime
unit. An agent is already keyed state with serialised access, so a Durable
Object is a natural owner. `consumes` already forms an acyclic dependency
graph, so deployment order and Service Bindings can be derived rather than
maintained in a second manifest.

The word *natural* should not be mistaken for *identical*. A direct call and a
Service Binding have different latency, failure, and observability. A state
registry does not reproduce eviction, persistence, or platform limits. JSON
validation at a boundary costs work that a direct call avoids. The two builds
preserve the same declared relationships, but they do not have the same
operational characteristics.

That is why target choice remains visible and why Chapter 9 did not treat a
bundle test as proof of a deployed system. The mapping removes duplicated
wiring; it does not make distribution transparent.

== TypeScript is also a checking boundary

Typed output is more than readable JavaScript with annotations. A successful
Bynk build emits a strict `tsconfig.json`, and the generated tree must pass
`tsc --strict`. This gives the compiler a second line of defence. If a lowering
produces an impossible interface implementation, a mismatched dependency, or
an inconsistent runtime call, the target language can expose the defect.

The same boundary admits existing code deliberately. The dashboard uses a
`Slug` capability declared in a Bynk adapter. Its implementation is ordinary
TypeScript:

#code-listing(
  [A TypeScript class satisfies the capability emitted from the adapter],
  read("../snippets/chapter-11/declared/src/text/normalise.binding.ts"),
  lang: "typescript",
)

`implements Slug` is the contract between the two languages. The generated
composition root constructs `JavaScriptSlug` and injects it under the `Slug`
name required by the handler. A real adapter could import an npm package,
declare a pinned dependency, or call a remote API through a supplied fetch
capability. The adapter, its binding, and the consuming Bynk code all remain
separate and greppable.

This is narrower than allowing arbitrary TypeScript inside any Bynk function.
The restriction is intentional. An unrestricted escape hatch would let ambient
I/O, unvalidated construction, and undeclared dependencies re-enter wherever
they were convenient. Adapters make the host boundary privileged, but located.
Their TypeScript can still be wrong; it simply has to be wrong behind an
explicit contract that strict checking can see.

There is a practical debugging consequence too. Local builds emit source maps
and Bynk debug metadata beside the TypeScript. Tests run under Node and
development Workers run under a JavaScript inspector, while breakpoints and
stack frames resolve back to `.bynk` source. Generated temporaries can be hidden
for ordinary stepping and inspected when the translation itself is under
suspicion.

This is a layer of tooling, not magic. A stale source map can mislead. A runtime
fault may still require reading generated code or the shared runtime library.
The advantage is that the fallback artefact is typed source in a familiar
language rather than opaque bytecode.

== Portability should fail honestly

The emission topology and the host platform are separate choices. Code that
consumes only the portable `bynk` surface can receive platform-specific
implementations of clocks, randomness, logging, fetch, and secrets. Library
adapters remain in process on either topology.

Platform-native capabilities are different. This cache context asks for
Cloudflare KV:

#code-listing(
  [The dependency names the vendor whose semantics the program requires],
  read("../snippets/chapter-11/platform-lock/src/support/cache.bynk"),
  lang: "bynk",
)

Building it for Node is refused:

#compiler-message[
[bynk.target.vendor_required]
context `support.cache` uses the platform-native capabilities of
`bynk.cloudflare`, which run only on the `cloudflare` platform,
but the build selects `--platform node`

Note: build with the matching `--platform`, or remove the
platform-native dependency to stay portable
]

This refusal is preferable to a portability claim made true by reducing every
host to its weakest common denominator. KV is allowed to mean Cloudflare KV.
The cost is a visible lock. A remote vendor reached through an ordinary HTTP
adapter can remain portable; a native storage primitive cannot honestly do so
without another implementation and a decision about semantic differences.

Cloudflare is therefore both leverage and constraint. Workers, Service
Bindings, and Durable Objects supply a coherent deployment target without a
bespoke runtime, but also define its envelope. A host with different deployment
units or state semantics requires more than another command-line flag.

== The inherited universe is not neutral

Bynk inherits JavaScript's values and execution model. `Int` occupies a
JavaScript number, so generated checks must defend a distinction the host does
not possess. Boundary data is JSON until validators admit it. Promises, the
event loop, and platform lifetime rules shape effect execution.

It also inherits ecosystem risk. Dependencies can be compromised; platforms
can change limits or pricing; TypeScript can tighten a rule the emitter relied
on. Separately deployed contexts can run incompatible contracts. Bynk detects
that skew, but does not negotiate versions.

A bespoke runtime would trade those dependencies for different ones, not
abolish them. The question is whether the inherited constraints fit the
service. Bynk bets that JavaScript's reach and a Workers-shaped deployment
model are worth more than complete control below the language.

TypeScript can use all of this without Bynk. It has direct access to every
framework and no translation layer. A team with disciplined boundaries,
explicit effects, well-owned state, strong lint rules, and good deployment
tooling may gain too little to justify the new front end.

Bynk's narrower claim is that such a team should not have to discard its runtime
and operational world to make more architecture compile-visible. The language
adds a model and translates it into familiar machinery. Whether the additional
constraints earn their place remains an empirical question.

That completes Part III. Tests can preserve the declared architecture without
claiming proof. Diagnostics can explain contradictions without choosing the
design. Translation can reuse an established universe without pretending that
it is costless or neutral.

Part IV puts the argument under pressure. First, we will read a whole system and
ask how much of its architecture can actually be recovered from source. Then we
will account for what the stronger constraints take away.
