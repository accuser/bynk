# The type-system philosophy

Karn's type system is built around one goal: **make illegal states
unrepresentable**. If a value cannot be expressed, no code — yours or anyone
else's — can produce it, and a whole class of bug simply cannot occur. Three
ideas do most of the work.

## Refinement: types describe values, not just shapes

A conventional type says "this is an integer". A *refined* type says "this is an
integer between 0 and 150". The predicate is part of the type, so an out-of-range
`Age` is not a value you forgot to check — it is a value that cannot exist.

This has a sharp consequence at the boundary between trusted and untrusted data.
A literal you write is checked at compile time and admitted directly; input from
the outside world must pass through `.of`, which returns a `Result`. The type
system thereby forces validation to happen exactly once, at the edge, and
everything inside the edge can assume validity. See
[The refined-literal admission model](refined-literal-admission.md).

## Opacity: identity matters

Two values can have the same representation and yet mean entirely different
things. An order id and a customer id might both be strings, but swapping them is
a serious bug. An **opaque** type gives a value a distinct identity: `OrderId` is
backed by a `String` but is not a `String`, and the compiler refuses to mix them.

Opacity also enforces *boundaries*. A type owned by a context can be constructed
and inspected only within that context; from outside, it is an opaque token. The
data-hiding you would normally enforce by convention becomes a checked property.

## Errors as values: no hidden control flow

Karn has no exceptions and no `null`. An operation that can fail returns a
`Result[T, E]`; a value that might be absent is an `Option[T]`. Because the
failure is *in the type*, the caller cannot ignore it — to get at the success
value they must acknowledge the error case, whether by `match` or by propagating
with `?`.

The payoff is that control flow is visible. There is no invisible path by which a
function might abruptly unwind; every way a call can end is written in its return
type.

## The throughline

Refinement narrows *which values* exist; opacity controls *what a value means and
who can touch it*; errors-as-values makes *failure explicit*. Together they push
correctness from runtime checks and discipline into the type system, where the
compiler enforces it for free. That is the same bet [Karn makes
everywhere](why-karn-exists.md): the correct way should be the structurally
enforced way.
