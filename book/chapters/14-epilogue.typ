= Epilogue: The program should not be able to forget <epilogue>

Return to the four boxes on the whiteboard.

The service has continued to change. The second caller is now important enough
to have its own team. The webhook has acquired a second version. Retries use a
queue, the scheduled task performs reconciliation, and the cache has become a
piece of state whose failure matters. One external provider became three.

The old diagram is still on a page in the design system. It still has four
boxes.

This time, the team does not begin by correcting it from memory. They begin with
the program.

They find the deployable contexts and the edges between them. They find the
actors admitted at each public boundary, the agents that own persistent state,
and the capabilities through which effects may occur. They find which failures
are part of contracts, which entry points may be retried, and which tests
exercise the same seams that production uses.

The resulting diagram is not the program. It omits values, invariants, failure
variants, provider selection, and the order of irreversible work. It will age
as every diagram ages.

But it can be drawn from evidence.

== Draw what the source can support

The questions from the opening service have not become easier.

Someone must still decide which component should own an operation, whether a
scheduled task may exercise the same authority as a customer, when a webhook
has been authenticated, and what should happen after an external provider
accepts work but the next state commit fails.

Bynk cannot make those decisions. What it can do is give each decision a place
in the program.

State ownership can appear as an agent rather than as a repository everyone has
agreed not to bypass. Caller authority can appear on an entry point rather than
being inferred from middleware around it. An external effect can appear as a
capability requirement rather than as an import several calls below the public
operation. A failure can appear in a closed result rather than travelling by an
exception path known only to the runtime.

Contexts preserve the intended dependency graph. Refined and opaque values
preserve distinctions the host representation would erase. Agent invariants
preserve rules across every commit, not merely across the handlers whose
authors remembered them. HTTP, queues, schedules, and WebSockets remain
different architectural boundaries because the language gives each a different
contract.

The compiler can then reject contradictions. It can refuse a dependency the
context did not declare, an effect for which no authority was supplied, a state
change that violates an invariant, a call that omits its actor, or a match that
forgets a failure variant.

These refusals do not prove that the diagram is correct. They mean that the
implementation cannot quietly become a different diagram.

#quote(block: true)[
  Architecture is not preserved by being written down once. It is preserved
  when change is required to confront it.
]

That confrontation is the useful friction running through this book. A new
caller changes more than a route. A new effect changes more than an import. A
new failure changes more than a log message. Moving responsibility changes more
than a folder path. When those facts participate in the program's meaning, a
local edit must account for its architectural consequences.

The cost arrives as declarations, coordinated changes, and compiler refusals.
The return is evidence from which the system can be read again.

== Remembering is not knowing

The whole order system in Part IV compiled. It still reserved stock without
compensating for failed payment. It authenticated the reader of an order
without checking that the reader owned it.

Those defects matter because they mark the boundary of the argument.

A compiler can preserve a stated authorisation rule; it cannot invent the rule
that the team failed to state. It can show that two state owners commit
independently; it cannot decide whether the workflow needs compensation,
co-location, or a shared transaction. It can make an external effect visible;
it cannot promise that the provider is reliable or that invoking it is wise.
It can enforce the declared context graph while every boundary in that graph is
poorly chosen.

The program should remember the architecture. It should not be mistaken for the
architect.

This is why rejected programs have played such a large part in the argument.
A refusal is valuable only after someone has made a decision worth preserving.
The compiler's job is not to replace judgement but to stop later convenience
from silently overruling it.

That separation protects against a tempting kind of confidence. A program rich
in domain types, explicit effects, actors, agents, and exhaustive results can
look unusually deliberate. Deliberation is not correctness. The declarations
make questions easier to locate and contradictions harder to introduce; they
do not absolve a team from reviewing policy, sequencing, operations, and human
consequences.

Nor does every decision deserve to be fixed early. Some domains are still being
discovered. Some systems are open at runtime by design. Some workloads depend
on shared relational transactions, framework conventions, or platform features
that Bynk can reach only through a broad adapter. In those systems, forcing the
architecture into Bynk's model may preserve the wrong things at too high a
price.

The language choice remains conditional. The principle need not be.

== Ask what the language is allowed to forget

Even in a TypeScript system, a team can ask the question behind Bynk.

Which facts exist only in the current developers' memories? Which are implied
by folder names, container bindings, middleware order, database etiquette, or
test discipline? Which would be expensive to reconstruct during an incident?
Which could drift while every type check and test remained green?

Not every answer calls for a new language construct. An import rule, schema,
architecture test, database constraint, or short design record may be the
right representation. A convention can be entirely adequate when its scope is
small, its enforcement is reliable, and its reason remains close to the code.

The harder cases are decisions that are both important and repeatedly erased
by the implementation medium. A domain identity that becomes a string at every
boundary. An effect that disappears into an asynchronous call. An ownership
rule that every module can technically violate. An authenticated caller whose
identity vanishes before the state change it authorises. A queue result treated
as though it were an ordinary function return.

These are not merely missing comments. They are places where the language of
the design and the language of the program have parted company.

Bynk's answer is to make more of the design executable: checked in source,
carried through compilation, visible in tests, and reflected in the generated
TypeScript and deployment topology. That answer is opinionated. It closes
graphs, names effects, gives state owners, distinguishes entry points, and
demands failure vocabulary. It also exposes a host boundary beyond which its
proofs do not reach.

The value of the answer is not measured by how much syntax it introduces. It is
measured by what a reader no longer has to infer and what a change can no
longer contradict unnoticed.

== The next diagram

The team finishes the new whiteboard diagram. It has more than four boxes.
Some arrows name service calls; others name messages. Stateful components are
marked as owners rather than storage shapes. Public edges distinguish
customers, partners, schedules, and internal services.

The diagram is already incomplete. It does not show every failure, invariant,
refinement, capability, test seam, or deployment choice. Tomorrow's requirement
will expose another omission.

That is not a failure of the exercise. Architecture is not a picture held
perfectly still. It is the set of decisions that continue to govern a system as
the system changes.

The whiteboard helps people discuss those decisions. Documentation explains
their history and intent. Reviews apply judgement. Operations reveal where the
model meets reality. Tests provide evidence at chosen boundaries. None becomes
unnecessary because the language can express more.

The language has a narrower responsibility: it should retain the decisions that
must remain true for the program to mean what its authors say it means.

If Bynk succeeds, it will not be because every service diagram stays current or
every design becomes correct. It will be because some of the facts most easily
lost in service software---identity, validity, failure, effect, ownership,
authority, time, and dependency---can no longer disappear without an explicit
change to the program.

The architecture will still evolve.

This time, the program will have to evolve with it.
