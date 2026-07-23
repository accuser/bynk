#import "../template.typ": code-listing, compiler-message

= Failure is part of the contract <failure-is-part-of-the-contract>

The order identifier now means order identifier. Quantity has crossed a checked
boundary. The ordering context declares its dependency on payment. Much of what
the system knows has survived into the program.

Then payment declines.

This possibility is not a surprise. Decline is part of accepting payments, just
as a missing order is part of looking one up and an unavailable provider is part
of relying on a remote service. The team has designed responses for each case.
A decline is shown to the customer. A provider outage may be retried. A missing
order becomes a not-found response. None is an implementation accident.

Yet these outcomes are often absent from the type of the operation that can
produce them. The function returns an authorisation when it succeeds. When it
does not, it throws, rejects a promise, returns `undefined`, writes a log, or
uses some combination agreed by the team. The success path is machine-readable;
the other paths are convention.

This asymmetry becomes dangerous when the system changes. A new
`ProviderUnavailable` error is added below the order service. A caller written
when decline was the only known failure catches every exception and tells the
customer that the payment was rejected. A temporary operational failure has
quietly acquired business meaning.

The bug is not that the program can fail. The bug is that the program's account
of the operation does not say how.

== The success type tells half the truth

A TypeScript version of the order path can be carefully typed and still leave
part of its contract outside the signature:

#code-listing(
  [Absence is typed; rejected promises are described in a comment],
  read("../snippets/chapter-03/conventional.ts"),
  lang: "typescript",
)

The return type acknowledges one alternative. `findOrder` may produce no order,
so `placeOrder` may produce `undefined`. A caller must at least test for that
possibility before treating the result as a `PlacedOrder`.

Payment failure is different. `authorise` returns `Promise<string>`. Its comment
says that the promise may reject with either `PaymentDeclined` or
`ProviderUnavailable`, but neither appears in the type. TypeScript permits a
promise to reject with any value. The caller can catch the rejection, but the
compiler cannot require the catch to distinguish the cases or notice when the
set changes.

This is not a criticism of asynchronous exceptions as an implementation
mechanism. Exceptions provide concise propagation and integrate with platform
APIs. The problem appears when an expected outcome that should change caller
behaviour is represented in a channel the function's contract does not name.

#quote(block: true)[
  If a failure changes what a caller should do, it belongs in the contract the
  caller can see.
]

Documentation helps only at the point when someone reads and remembers it.
Tests can demonstrate selected cases but cannot make every new caller handle
them. Telemetry reveals the outcome after the operation has run. A typed
contract acts earlier: it makes the alternatives part of constructing a valid
caller.

== Absence is not failure

Before putting failures into types, we need to separate two ideas that are
often collapsed.

An order lookup may find nothing. That can be an ordinary answer to the
question, not evidence that the lookup malfunctioned. A database connection
may also fail while performing the same lookup. That is not absence. The first
means “there is no value”; the second means “the operation could not determine
whether there is a value”.

A nullable return can express the first distinction, but `null` and
`undefined` tend to accumulate meanings: not found, not loaded, not applicable,
not initialised, or failed without detail. Bynk instead uses `Option[T]`, whose
two cases are `Some(value)` and `None`. There is no `null` value that can appear
inside an unrelated type.

The order example keeps the `OrderId` vocabulary from Chapter 2 and adds `Cents`
for a money amount. Its small lookup stands in for persistence so we can
concentrate on the outcomes:

#code-listing(
  [An optional lookup becomes an error only when the operation requires it],
  read("../snippets/chapter-03/declared/src/commerce/orders/lookup.bynk"),
  lang: "bynk",
)

`findTotal` answers a lookup question with `Option[Cents]`. `None` says no total
was found. It does not invent a reason and does not yet declare the absence to
be an error.

`requireTotal` asks a different question: can order placement continue? At that
point absence has domain significance, so the function translates `None` into
`Err(OrderMissing(id))`. The same fact moves from an ordinary result of a lookup
to a named failure of an operation because the meaning of the question has
changed.

If persistence itself could fail, its honest return would need another layer,
such as `Result[Option[Cents], StorageError]`. That shape has three meaningful
outcomes: a total was found, no total exists, or the lookup failed. Flattening
the last two into `None` would tell callers that an outage and a missing order
are interchangeable.

The nested type is more verbose. It is also a more accurate description of the
choices the caller actually faces.

== Put the alternatives in the operation

Payment has no useful absent outcome. Authorisation either succeeds or fails
for a reason the caller may need. Its context therefore declares a small error
vocabulary:

#code-listing(
  [Payment names the failures it presents to consumers],
  read("../snippets/chapter-03/declared/src/commerce/payment/types.bynk"),
  lang: "bynk",
)

The service places that vocabulary in its return type:

#code-listing(
  [Authorisation returns success or a payment error],
  read("../snippets/chapter-03/declared/src/commerce/payment/authorise.bynk"),
  lang: "bynk",
)

`Result[String, PaymentError]` has exactly two outer cases. `Ok` carries the
authorisation; `Err` carries one of the declared payment failures. A caller that
has a `Result` does not yet have a `String`. It must inspect, transform, or
propagate the result before it can use the success value.

The surrounding `Effect` is a separate part of the signature. It says that the
operation participates in effectful execution; the next chapter will examine
what that requires. For the present argument, the significant part is that
effectful work does not erase its possible domain outcome. The complete type is
`Effect[Result[String, PaymentError]]`.

The error variants are deliberately more stable than a provider's raw response.
The payment context may use several processors, each with its own status codes,
timeouts and SDK exceptions. Consumers need the distinction between a business
decline and temporary unavailability. They do not necessarily need the provider
taxonomy that produced it.

This is the same boundary principle we applied to services themselves. Payment
owns the translation from implementation detail into the contract it presents.
Exporting `PaymentError` transparently lets a consumer see the alternatives
without giving that consumer responsibility for interpreting the provider.

== Propagation is not disappearance

The ordering context has its own error vocabulary. A missing order belongs to
ordering. A payment failure originates elsewhere but must remain visible in the
outcome of placing an order:

#code-listing(
  [The order error records where payment failure enters],
  read("../snippets/chapter-03/declared/src/commerce/orders/types.bynk"),
  lang: "bynk",
)

`OrderError.Payment` wraps a `PaymentError`. The trailing `embeds` declaration
says that this is the route by which that subordinate error enters the local
error type. It is a declared conversion, not a catch-all and not a guess made
from matching names.

The service can then keep its successful path readable:

#code-listing(
  [Question marks shorten propagation without hiding the outcome],
  read("../snippets/chapter-03/declared/src/commerce/orders/place.bynk"),
  lang: "bynk",
)

The first `?` receives a `Result[Cents, OrderError]`. If it is `Ok`, `cents` is
the enclosed value. If it is `Err`, the service returns that error immediately.

The payment call produces an effectful result. The `<-` bind performs the
effect and leaves `decision` as `Result[String, PaymentError]`. The second `?`
does the same success-or-propagate operation, using the declared embedding to
wrap a payment error as `OrderError.Payment`.

This resembles exception propagation in its brevity, but it differs at the
boundary that matters. Remove `Result` from the service return type and the
propagation no longer type-checks. Try to propagate an unrelated error without
a declared embedding and the compiler refuses it. The control flow is concise;
the possibility remains in the signature.

An embedding is intentionally local and direct. It does not turn every lower
level error into every higher level error through an invisible conversion
chain. Ordering chooses to carry `PaymentError`, and that choice appears beside
the definition of `OrderError`. Mapping an order error to an HTTP response will
still be a separate decision at the HTTP boundary.

== Exhaustiveness makes change visible

Some callers propagate a failure. Others must decide what it means.

Payment retry policy, for example, treats a decline as final and provider
unavailability as temporary:

#code-listing(
  [A caller distinguishes every payment outcome],
  read("../snippets/chapter-03/declared/src/commerce/payment/policy.bynk"),
  lang: "bynk",
)

The `match` covers success and both error variants. Its arms all produce a
`Bool`, so the match itself produces the retry decision.

Now omit the decline arm. The remaining code has a perfectly reasonable answer
for every case it mentions, but not for every value of the input type. Bynk
rejects it:

#code-listing(
  [A retry policy with an unhandled decline],
  read("../snippets/chapter-03/non-exhaustive/src/commerce/payment.bynk"),
  lang: "bynk",
)

#compiler-message[
[bynk.types.non_exhaustive_match] Error:
non-exhaustive `match` - variant `Err(Declined)`
of `Result[String, PaymentError]` is not covered
]

This refusal matters most when the error type evolves. Add a new payment
variant and every match that enumerates the old set becomes a place requiring a
decision. The compiler turns a change in the contract into a list of affected
policies.

A wildcard arm can make a match exhaustive without naming every variant. That
is sometimes appropriate: metrics may count every error the same way, and a
boundary may have a safe generic fallback. The wildcard is an explicit choice
to stop distinguishing at that point. It trades the compiler's assistance on
future variants for a deliberately uniform policy.

Exhaustiveness therefore does not guarantee wise error handling. It guarantees
that a caller either accounts for the declared alternatives or visibly chooses
not to distinguish them.

== Designing a useful failure vocabulary

Moving failure into the type system does not decide which failures deserve a
name. Poor error types can be explicit and still be unhelpful.

At one extreme, `Result[T, String]` tells a caller that failure is possible but
provides no stable alternatives. The caller may parse prose, compare fragile
messages, or treat every error alike. At the other extreme, exposing every
network code, database driver exception and provider response couples the
caller to implementation details the boundary was meant to hide.

A useful error vocabulary is drawn around caller decisions. If two causes lead
to the same response at every relevant boundary, separating them may add noise.
If one should be retried and the other shown to a customer, collapsing them
removes information the caller needs. Payloads should carry the data required
to act or explain, without leaking secrets or making transient diagnostic text
part of the public contract.

The same underlying event can also acquire different meaning at different
boundaries. `PaymentError.Declined` is part of payment's service contract.
`OrderError.Payment(Declined)` is part of ordering's operation. An HTTP handler
may map it to a status and public response; a scheduled process may record it
for manual review. Those mappings should be explicit because transport policy,
domain outcome and operational policy are not the same thing.

There is a maintenance cost. Adding a variant can create work across callers.
Large functions can accumulate wide error sums. Careless wrapping can produce
a tower of types that preserves origin but clarifies nothing. The point is not
to expose every possible mishap. It is to retain the outcomes that form the
meaningful contract between components.

== Could TypeScript do this?

Again, yes.

A TypeScript codebase can replace thrown domain errors with discriminated
unions, define `Option` and `Result` libraries, use exhaustive `never` checks,
and standardise propagation helpers. Several mature libraries provide exactly
these tools. Teams using them consistently can make the opening function's
failure contract as explicit as Bynk's.

Bynk makes the choice a language-wide baseline. There is no unchecked `null`
flow to combine with optional values and no source-level exception channel for
ordinary domain failure. `Option`, `Result`, exhaustive matching, `?`, and
declared error embedding share one model across contexts.

That does not mean a Bynk program cannot fault. Generated TypeScript runs on a
real platform. A defect, failed assertion, exhausted resource, broken adapter,
or unexpected host exception can still terminate work. It would be dishonest
to claim that a `Result` enumerates every physical way execution might not
complete.

The distinction is between _expected operational outcomes_ and faults for which
the program has no meaningful continuation. Payment decline, ordinary absence,
and a provider state the caller is expected to handle belong in the contract.
Memory exhaustion does not become more manageable merely because it has been
added to every error sum. Judgement is still required at that boundary.

Nor does an explicit error type make the implementation truthful by itself. A
payment adapter can misclassify an outage as a decline. A programmer can use a
wildcard too broadly or invent an `Unknown` variant that absorbs every future
decision. As with explicit contexts and domain types, the compiler preserves
the distinctions the author declares; it cannot supply the missing judgement.

== What Part I has established

The three chapters in this part have made one argument at three scales.

At the component scale, a context and its declared dependencies preserve which
parts of the system may interact. At the value scale, opaque and refined types
preserve identity and validity after admission. At the operation scale,
`Option`, `Result`, and named error sums preserve the ways a call may conclude.

In each case, the conventional program can contain the same knowledge. It can
place modules in careful directories, validate values at the edge, document
exceptions, and test the important paths. Bynk's move is to let selected facts
participate in compilation. A violation then becomes more than a disagreement
with documentation; it becomes a program the compiler will not accept.

This does not make the source a complete description of the system. Part I has
said what the boxes mean, what values may cross their boundaries, and what
outcomes their operations declare. It has not yet said what those operations
are allowed to require from the world.

An order service may have an explicit failure type while reaching invisibly for
a database, a clock, a network client, or a logger. Those dependencies affect
how the service can run, test, and compose just as surely as its return type
does.

The next question is not only how a call can end, but what it must be allowed to
do before it ends.
