#import "../template.typ": code-listing, compiler-message

= A data shape is not a domain model <a-data-shape-is-not-a-domain-model>

The ordering system now has a visible boundary. Its context declares that it
may call payment, the payment service presents a deliberate surface, and a new
dependency cannot appear without changing the program's account of its
architecture.

Inside that boundary, however, an order still arrives as a small collection of
ordinary values. The order identifier is a string. The customer identifier is
a string. The quantity is a number. They have sensible field names and arrive
inside a record called an order line. At a glance, the model seems perfectly
adequate.

Then a refactoring reverses the two identifiers in a call.

Nothing crashes immediately. Both values are well-formed strings. The database
accepts them. The logs show identifiers in the places where identifiers belong.
Only later, when a reservation is associated with the wrong customer, does the
system reveal that two values with the same representation never had the same
meaning.

In another path, quantity has been checked at the HTTP boundary. It was an
integer between one and one hundred when it entered. Several functions later it
is still a number, but the type no longer records why it is safe. A maintainer
cannot tell whether this value passed the check, came from an older entry point,
or was produced by arithmetic after validation. The program remembers the
shape of the value and forgets the fact established about it.

The component boundary is intact. The meaning inside it has leaked away.

== The record looks convincing

Capable teams do not normally pass anonymous arrays around and hope everyone
remembers what each position means. They introduce records, schemas, validation
functions, and names. A conventional order path might therefore be quite
careful:

#code-listing(
  [A validated record that still erases two distinctions],
  read("../snippets/chapter-02/conventional.ts"),
  lang: "typescript",
)

This code does several useful things. It names the three fields. It checks that
quantity is an integer in the accepted range. It centralises the check in a
function whose name marks the point at which an order line is accepted. There
is no shortage of intent.

Yet the final call compiles. `customerId` and `orderId` are both strings, so
their reversal is structurally valid. The check on `quantity` also leaves no
trace in its type. After `acceptLine` returns, the value is no more specific
than any other number.

The record is a good data shape. It tells us which fields travel together and
which primitive representation each one uses. Those are valuable facts,
especially at storage and transport boundaries. But a domain model has another
job: it must preserve the distinctions on which correct decisions depend.

Structure answers questions such as these:

- Does an order line have an identifier, a customer and a quantity?
- Is the quantity represented by a number?
- Can the value be serialised into the expected wire format?

Meaning asks different questions:

- Is this identifier allowed to identify an order rather than a customer?
- Is this number known to be a permissible quantity?
- Which operation admitted the value into the part of the program that trusts
  it?

A record can answer the first group while remaining silent about the second.
Calling that record a domain type does not close the gap.

#quote(block: true)[
  Validation is an event. A validated type lets the result of that event travel
  with the value.
]

== Three facts, not one

The phrase _stronger type_ can obscure several different requirements. For the
order line, at least three facts are in play.

The first is _shape_: these fields form one value. A record expresses that
directly.

The second is _identity_: an `OrderId` and a `CustomerId` are different kinds
of value even if both are represented by strings and obey the same formatting
rule. Their difference comes from what they refer to, not what their characters
look like.

The third is _validity_: a `Quantity` is an integer for which a predicate is
known to hold. The values zero and one hundred and one are integers, but they
are not quantities in this ordering system.

These facts overlap, but they should not be conflated. A regular expression can
recognise the spelling of an identifier without establishing what it identifies.
A nominal wrapper can distinguish two identifiers without proving that either
exists in a database. A record can put a quantity next to an order identifier
without proving that enough stock exists. Each mechanism carries a particular
kind of information.

Bynk makes the distinction between identity and validity explicit. An opaque
type gives a value nominal identity and controls access to its representation.
A refined type restricts the values of a base type with a predicate. A record
can then compose those types without reducing them back to primitives.

#block(breakable: false)[
The order vocabulary can be declared like this:

#code-listing(
  [Identity and validity inside the order shape],
  read("../snippets/chapter-02/declared/src/commerce/values/types.bynk"),
  lang: "bynk",
)
]

The two identifier types deliberately have the same base and the same
`NonEmpty` predicate. No difference in their representation explains why they
cannot be exchanged. `opaque` supplies that difference: `OrderId` and
`CustomerId` are distinct names whose underlying strings are hidden outside the
commons that owns them.

`Quantity` makes a different promise. Its representation is an `Int`, and code
can use it where the base value is needed for a read. The `InRange` predicate
restricts which integers may enter the type. The distinction is about the set
of permitted values rather than secrecy around the representation.

Finally, `OrderLine` remains a record. Bynk has not replaced data shapes with a
more exalted abstraction; it has made the fields of the shape carry more of
their meaning. The braces say which values belong together. The field types say
which distinctions survive after construction.

This is still not a complete model of ordering. It says nothing about stock,
price, ownership, or whether the customer may buy the product. Its value lies
in being precise about what it _does_ say.

== A refusal about meaning

The difference becomes concrete at the same call that TypeScript accepted. In
a function whose parameters are opaque identifiers, reversing the arguments
produces a compiler error:

#code-listing(
  [The same representation is not the same type],
  read("../snippets/chapter-02/swapped/src/commerce/values.bynk"),
  lang: "bynk",
)

#compiler-message[
[bynk.types.argument_mismatch] Error:
argument 1 to `reserve` has type `CustomerId`,
but parameter `orderId` expects `OrderId`
]

The compiler does not need to recognise the names `order` and `customer` as
special domain language. It only needs to preserve the distinction the author
declared. Even though both types are backed by non-empty strings, neither is a
substitute for the other.

This is a small refusal with an architectural consequence. Identifiers often
cross many layers: handlers, application services, persistence adapters,
messages and scheduled jobs. If they become strings at the first boundary,
every later call depends on names and position to recover their meaning. If
they retain distinct types, the compiler carries that meaning across the path.

The guarantee is narrower than it may first appear. An `OrderId` does not prove
that an order with that identifier exists. A `CustomerId` does not prove that a
customer owns a given order. Opacity prevents accidental interchange; it does
not perform a database lookup or establish a relationship between entities.

That narrowness is useful. Types are strongest when their claim is clear enough
to rely on. Treating an identifier type as proof of existence would make the
model sound more impressive and the program less honest.

== Admission is the boundary that matters

Declaring `Quantity` raises an immediate practical question: how does an
ordinary integer become one?

Most values do not originate inside the domain model. They arrive as JSON,
route parameters, database columns, configuration, queue payloads or user
input. At those edges the program has a base value and no entitlement to trust
it. A useful constrained type must therefore explain not only what it means,
but how values are admitted.

Bynk provides two paths, because the program can know a value in two different
ways.

#figure(
  block(width: 100%)[
    #set text(size: 8.7pt, hyphenate: false)
    #set par(justify: false, leading: 0.6em, first-line-indent: 0pt)
    #table(
      columns: (0.7fr, 1.05fr, 1.2fr),
      inset: (x: 0.55em, y: 0.52em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Source],
        text(weight: "semibold")[Admission],
        text(weight: "semibold")[Outcome],
      ),
      [Known literal], [Predicate checked at compile time], [The refined value, or a compiler refusal],
      [Runtime value], [Checked construction with `.of`], [`Result[T, ValidationError]`],
    )
  ],
  caption: [Two admission paths preserve one rule: an unchecked runtime value is not silently trusted.],
)

A literal written in a position that expects a refined type is already known to
the compiler. It can test the predicate during compilation and admit the value
directly. A value obtained at runtime cannot be proved in advance, so `.of`
checks it and returns either the refined value or a validation error.

The order commons gives those checked constructors names suited to its own API:

#code-listing(
  [Known values and runtime values enter differently],
  read("../snippets/chapter-02/declared/src/commerce/values/admission.bynk"),
  lang: "bynk",
)

The three `parse` methods delegate runtime values to `.of`. Their result types
make the uncertainty visible: parsing may succeed with the requested type or
fail with a `ValidationError`. The caller cannot receive a `Quantity` merely by
asserting that an integer has been checked somewhere else.

`defaultQuantity` follows the other path. Its return type supplies the expected
type, and the literal `1` is checked against `InRange(1, 100)` while the program
is compiled. Since the proof is available then, manufacturing a runtime
`Result` would add a failure path that cannot occur.

Change the literal to zero and compilation stops:

#code-listing(
  [A known-invalid default],
  read("../snippets/chapter-02/invalid-quantity/src/commerce/values.bynk"),
  lang: "bynk",
)

#compiler-message[
[bynk.refine.literal_violates] Error:
literal 0 does not satisfy `InRange`
required by type `Quantity`
]

This is more than compact validation syntax. It establishes a trust boundary.
Outside that boundary, an integer may or may not be a quantity. At the point of
admission, the predicate must be established. Inside, a function that accepts a
`Quantity` can rely on the fact rather than repeat the check or trust a comment.

The phrase _inside_ is logical rather than necessarily physical. Admission
does not have to occur in one special directory or HTTP handler. It occurs
wherever untrusted representation becomes trusted domain value. Good program
structure will usually keep those points close to external boundaries, because
that lets the largest possible part of the program operate on meaningful
types.

== The proof must survive the journey

Many systems already validate input thoroughly. The harder question is what
happens after validation.

If a schema library returns a value typed only as `number`, then it has
protected the boundary but not enriched the rest of the program. A later
function cannot distinguish that value from a number obtained through an
unchecked path. It must either trust the call chain, validate again, or accept
that the proof has become institutional knowledge.

A refined type changes the output of validation. Before admission the value is
an `Int`; afterwards it is a `Quantity`. That change lets functions state the
proofs they require:

```bynk
fn allocate(quantity: Quantity) -> Allocation {
  -- this operation begins after range validation
}
```

The important line is the parameter, not the comment. Every caller must supply
a value that has already crossed an admission path. Moving the check is no
longer a local refactoring if it would allow an ordinary integer through.

The proof is not immortal. Arithmetic on a `Quantity` produces an `Int`, because
adding or multiplying valid quantities can produce a value outside the original
range. If that result must become a quantity again, it must be admitted again.
This may feel inconvenient, but preserving the refined type through an operation
that does not preserve its predicate would be a lie.

Nor should every intermediate value receive a new domain type. A program in
which every integer has a bespoke name can become harder to read than one that
uses primitives thoughtfully. The test is not whether a value can be refined;
it is whether later correctness depends on a distinction that ordinary types
erase.

== Opacity is authority, not decoration

Opaque identifiers introduce a related boundary. Outside their defining
commons, code cannot inspect the raw string or mint an identifier by using the
base representation directly. It must use the API the owner provides. That
makes construction policy part of the type's design rather than a suggestion
attached to a factory function.

The owner retains special authority. Within the defining commons, an opaque
type can be created with its checked `.of` constructor or, deliberately, with
`.unsafe`. The latter is useful when the owner has another reason to know the
value is legitimate—for example, it has just generated the identifier—but it
is also a real escape hatch. Opacity says who is allowed to take that
responsibility; it does not make the responsibility disappear.

Refined types make a different trade. They have checked construction and
compile-time literal admission, but no `.unsafe` constructor. Their central
claim is precisely that the predicate was tested. An unchecked public path
would turn refinement back into convention.

This separation helps prevent a common muddle. Identity and validity sometimes
appear together, as they do in the non-empty opaque identifiers above, but they
are not the same guarantee. Opaque types centralise the authority to interpret
a representation. Refined types establish a predicate about a value. Choosing
one should follow the fact the program needs to retain.

== Could TypeScript do this?

Yes.

TypeScript teams can define branded types, hide constructors in modules, use
schema libraries that infer narrowed output types, and expose smart constructors
returning explicit success and failure values. With care, the final call in the
opening example can be made to fail compilation. Other languages offer newtypes,
refinement libraries, private constructors or richer dependent type systems.

Bynk's case cannot rest on these techniques being impossible elsewhere. It
rests on making them ordinary parts of the language used for service design.
Opaque and refined types have shared syntax, shared construction rules, known
diagnostics and predictable behaviour at context boundaries. A project need not
select a branding idiom and persuade every contributor not to bypass it.

That standardisation has a cost. Every meaningful distinction introduced into
the type system appears at boundaries and during migration. Existing data has
to be admitted. Adapters have to translate representations. A changed predicate
can expose old assumptions across the program. An author can also model at the
wrong resolution, producing a thicket of tiny types that adds ceremony without
preventing a plausible mistake.

The strongest argument for an `OrderId` is not that primitives are inherently
bad. It is that confusing an order with a customer is cheap to do, hard to see,
and consequential enough to prevent. The strongest argument for `Quantity` is
not that every number needs a predicate. It is that a range check is a stable
precondition used by enough later code that the result should survive the
function that performed it.

This is a design choice, not a demand for maximal typing.

== What the model still cannot know

After these changes, the order line carries more meaning, but many invalid
states remain representable.

The quantity may exceed available stock. The customer may not be allowed to
place the order. The order identifier may be well formed and refer to nothing.
A unit price and quantity may each be valid while their product exceeds a
payment limit. These are relationships between values, current state or
external facts. A local predicate on one primitive cannot establish them.

Some such rules belong in constructors for larger values. Some belong in state
transitions. Some require an effectful query. Some should remain policy evaluated
at the moment of an operation. Calling all of them validation would conceal
important differences in when and how they can be known.

Bynk does not turn domain modelling into predicate writing. It offers places to
retain particular facts once the author has identified them. The quality of the
model still depends on choosing distinctions that match the business and
refusing claims the program cannot support.

The useful advance is that those choices no longer evaporate immediately into
strings and integers. The program can carry them from admission, through the
order record, and into the operation that needs them. Its shape and its meaning
can reinforce one another.

One consequence has been waiting in every call to `.of`: admission can fail.
The constructor does not throw and does not pretend that the success value
always exists. It returns a `Result`, making failure part of the type seen by the
caller.

If domain meaning is to survive success, failure must survive too. That is the
next part of the contract.
