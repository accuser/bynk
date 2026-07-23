#import "../template.typ": code-listing, compiler-message

#let source-lines(path, start, end) = {
  read(path).split("\n").slice(start, end).join("\n")
}

= Who is calling is part of the operation <who-is-calling-is-part-of-the-operation>

The order at the end of Chapter 6 can distinguish a legal payment from an
illegal transition. It still cannot distinguish a customer paying for their
order from a stranger attempting the same operation.

The method name is identical. The arguments may be identical. The current
state may permit the transition in both cases. The difference lies outside the
state machine: who made the request, how the system knows, and what that
identity is allowed to mean here.

Most applications answer these questions. They verify a cookie or token in
middleware, attach a user to a request, check a role, and pass an identifier to
the domain code. The difficulty is not that authentication is forgotten
altogether. It is that the answer often becomes ambient on the journey from
the network to the operation.

A route appears to accept an order identifier and a payment reference. Whether
it also requires a customer, an administrator, an internal service, or nobody
at all is left to a wrapper, a router group, a framework extension, or the
reader's memory.

#quote(block: true)[
  The caller is not metadata about an operation. The caller helps determine
  what operation is being performed.
]

== Authenticated is not authorised

Consider a conventional TypeScript route written with some care:

#code-listing(
  [Middleware authenticates the request and a claim admits basket readers],
  read("../snippets/chapter-07/conventional.ts"),
  lang: "typescript",
)

`requireCustomer` prevents the body from receiving an unauthenticated request. The
route also checks a `basket:read` claim. Its handler type makes the principal
available without an optional-value dance. This is substantially better than
casting an augmented request or assuming that earlier middleware ran.

The route is still wrong.

It loads `request.params.owner`, not `request.principal.id`. Any authenticated
principal with the claim can ask for another owner's basket by changing the
path. TypeScript accepts the program because `CustomerId` is only an alias for
`string`, and because neither authentication nor a general permission answers
the object-level question.

There are three distinct decisions here:

- *Authentication:* did the credential establish a principal?
- *Boundary authorisation:* does that principal meet the broad condition for
  entering this operation?
- *Object-level authorisation:* may this principal act on this particular
  basket, order, document, or account?

Frameworks often place all three under the word “auth”. That shorthand is
convenient until one check is taken as evidence that another has happened.
A valid token does not imply access to every object. A role does not necessarily
imply ownership. An internal caller is not automatically trusted to perform
every internal operation.

The repair in TypeScript is not mysterious. Load by the authenticated identity,
compare the principal with the resource owner, or use an authorisation policy
whose inputs include both. The important point is where the evidence appears.
If the route's signature says nothing about a caller, reviewers must reconstruct
that evidence from the surrounding framework.

== Declare the boundary contract

In Bynk, an actor names the contract under which a handler may be entered. The
basket context declares two:

#code-listing(
  [A verified identity is also the key of the state it may address],
  source-lines(
    "../snippets/chapter-07/declared/src/commerce/baskets/basket.bynk",
    0,
    21,
  ),
  lang: "bynk",
)

`Customer` says how an incoming HTTP request establishes a customer and which type that
identity has. The `Bearer` scheme verifies the token with the secret named by
`AUTH_JWT_SECRET`; a successful token's subject is admitted as `CustomerId`. Failure
stops at the boundary with an unauthorised response. The handler body does not
run.

This does more than put a helpful type annotation on a string. `CustomerId` belongs
to the context, and the value is sealed at the boundary. Downstream code
receives the identity that the actor contract minted; it cannot construct an
actor value and claim to be somebody else.

`Admin` refines `Customer`. It uses the same authenticated identity but additionally
requires the `admin` claim. A missing or invalid bearer credential fails
authentication. A valid customer without the claim fails authorisation. The
distinction between an unauthenticated caller and a forbidden one is part of
the boundary behaviour rather than a branch the handler must remember to
write.

The agent below those declarations is keyed by `CustomerId`. Identity therefore
connects two architectural decisions: who crossed the boundary and which
stateful owner receives the call.

== Put the caller beside the operation

The HTTP service makes that connection at each handler:

#code-listing(
  [Public, customer, and administrator entry contracts are visible at the routes],
  source-lines(
    "../snippets/chapter-07/declared/src/commerce/baskets/basket.bynk",
    22,
    37,
  ),
  lang: "bynk",
)

The health route says `by Visitor`. Public access is not the absence of a
security decision; it is the explicit decision that no principal is required.

The ordinary basket route says `by u: Customer`. Before its body begins, the runtime
must have discharged the `Customer` contract. Inside the body, `u.identity` is the
sealed `CustomerId` produced by that check. The route has no owner parameter to
confuse with it. `Basket(u.identity)` addresses the state owned by the caller.

This is a small piece of code, but it closes the gap in the TypeScript example.
The identity that grants entry is the identity that selects the agent. A caller
cannot edit a URL to substitute a second `CustomerId`, because the operation does
not accept one.

The administrator route is deliberately different. An admin is allowed to name
an owner in the path, so the handler addresses `Basket(owner)`. The actor
contract proves an authenticated customer with the required claim; it does not
prove that the selected basket is appropriate for the administrative task.

That is not an accidental hole in the actor model. It is its boundary.

An actor answers *who is calling under what entry condition*. It does not answer
*whose object is this* or *does this action satisfy today's business policy*.
If administrators may see baskets only for their region, or support agents need
an active case number, that relationship belongs in domain logic or a declared
policy the handler invokes. Calling every such relationship an actor would
hide the object and circumstances on which the decision depends.

The benefit is precision about what has and has not been established. At the
first route, choosing the agent by `u.identity` removes the object-level
question. At the second, the explicit `owner` keeps that question visible in
review.

== Absence is also a security decision

Many frameworks give a route whatever authentication state earlier middleware
happened to attach. If none is present, the handler may be public, broken, or
dependent on a router configuration elsewhere. The function alone cannot say
which.

Bynk refuses to infer that choice for HTTP. This route omits its caller:

#code-listing(
  [An HTTP operation with no actor contract],
  read("../snippets/chapter-07/missing-actor/src/commerce/baskets.bynk"),
  lang: "bynk",
)

The compiler rejects it:

#compiler-message[
[bynk.actor.missing_by_on_http] Error:
an HTTP handler must declare its actor with a `by` clause

Note: HTTP has no safe default actor - a public route writes
`by Visitor`; an authenticated route names its actor
]

The repair is not necessarily to secure the route. It is to decide. Write
`by Visitor` if the operation is intentionally public, or name the actor whose
contract must be verified. The compiler cannot determine the desired policy,
but it can prevent silence from masquerading as one.

This rule also improves change review. Moving a route between router groups or
reordering middleware cannot silently change the caller contract, because the
contract is attached to the handler. A diff that changes `by Visitor` to
`by u: Customer` is visibly a change in the operation, not an incidental deployment
edit.

== Identity crosses internal boundaries too

The caller problem does not stop at the public edge. Suppose the orders context
calls an audit context. A private service binding prevents arbitrary internet
traffic from reaching the handler, but “internal” is not one undifferentiated
authority. Orders, support, imports, and reconciliation may deserve different
treatment.

A cross-context handler can capture the calling context:

#code-listing(
  [The callee can observe the context that made an internal call],
  read("../snippets/chapter-07/declared/src/commerce/audit/audit.bynk"),
  lang: "bynk",
)

`Caller` is a built-in actor for `on call` handlers. Its identity is the
qualified name of the calling context. The compiler stamps that name at the
call site and the internal channel carries it to the callee; application input
does not supply it. If the handler needs only verification and not the name,
`on call` has `Caller` as its default and can omit the binder.

Again, identity is not permission. Learning that `commerce.orders` made the
call does not prove that orders may erase an audit record. The callee can use
the identity in its own decision, record it as provenance, or reject unexpected
contexts. What changes is that “which internal component called?” is no longer
lost as soon as the service boundary is crossed.

This is particularly valuable after a system grows. A service that originally
had one internal caller often acquires several. Without caller identity, its
operations tend to inherit the authority of the most privileged caller or
grow ad hoc flags such as `fromAdmin`. A boundary identity lets the callee make
the distinction without trusting a caller-supplied story about itself.

== Could a framework do this?

Yes. A well-designed web framework can make authenticated requests a distinct
type. Route builders can require a security policy. Branded identifiers can
separate a verified subject from a path string. Authorisation libraries can
combine principal, action, resource, and environment. Service meshes can
authenticate workloads, and generated clients can attach an internal caller
identity.

Those are sound techniques. For an application already built around them,
recreating the same guarantees in a new language may not be worthwhile.

Their weakness is usually not capability but composition. The route type, token
verifier, middleware order, policy call, branded identity, service
configuration, and state lookup can live in different systems maintained by
different people. The architecture is present, but no single construct requires
the pieces to remain aligned.

Bynk narrows the arrangement. A handler declares `by`; a closed authentication
scheme verifies it; a successful boundary mints a typed identity; the body can
use that identity to address an owner. The compiler checks the declaration and
refuses an unclassified HTTP route. This removes flexibility: authentication
schemes are not arbitrary user code, claim refinements are intentionally
limited, and object-level policy still needs modelling.

The constraint is worthwhile when the caller is part of what the operation
means. `Basket(u.identity).view()` is not merely a shorter form of a repository
lookup. It states that this operation views the authenticated caller's basket.
The public route, the admin route, and the internal call make different promises
even when their bodies could return the same shape.

The system can now say where state lives, which changes are legal, and who may
cross a boundary to request them. It still has another source of architectural
meaning to recover.

A payment requested over HTTP, a retry arriving from a queue, a reconciliation
started by a schedule, and a message on a WebSocket are not interchangeable
calls. They occur under different guarantees about time, delivery, response,
and connection. The next chapter turns from who entered an operation to the
kind of boundary through which it arrived.
