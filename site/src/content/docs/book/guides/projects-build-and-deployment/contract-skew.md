---
title: Contract skew across independently deployed contexts
---
**Goal:** understand what happens when two contexts are deployed from different
versions of your source, why the compiler can see it, and what to do when a call
fails with `ContractMismatch`.

## The problem independent deployment creates

Contexts are independently deployable. That is the point — `shop.orders` and
`shop.payment` become separate Workers, and you can push one without the other.

It is also the hazard. When `orders` calls `payment`, the compiler type-checks
that call by reading `payment`'s source **at the time `orders` was built**. It
knows the parameter types, the return type, every field of every record that
crosses. It bakes that knowledge into `orders`.

Then time passes, and you deploy `payment` alone:

```bash
bynk deploy --context shop.payment
```

`payment` is now live with a contract that `orders` has never seen. `orders`
still believes the old one — and nothing in the two Workers knows they disagree.
The call still connects (a Service Binding resolves by name), the JSON still
parses, and the response is decoded by `orders` against a shape that is no longer
true. That is **contract skew**, and its natural failure mode is the worst kind:
not an error, but a wrong answer.

## What Bynk does about it

The compiler is the only party that ever sees both contracts, so it is the only
party that can record what it knew. Since v0.177 it does.

When it compiles `orders`, it computes a **contract hash** — a fingerprint of
`payment`'s contract as `orders` understands it — and stamps it into every call
site. When it compiles `payment`, it computes the same fingerprint from
`payment`'s own declarations and stamps that into `payment`'s Worker. Neither
Worker computes anything at runtime; both carry a constant fixed when they were
built.

At the boundary, `payment` compares the two before reading the request body. If
they differ, it refuses:

```json
{
  "kind": "ContractMismatch",
  "service": "authorise",
  "expected": "9f2c41ab73e05d18",
  "actual": "317bdd3de84d2176"
}
```

The caller sees a `ContractMismatch`, not a mangled result. The skew became a
loud, nameable failure instead of a silent one.

### Why it refuses before reading the body

Once the two sides disagree about what the contract *is*, the body's meaning is
exactly what is in doubt. Validating it first would report something like
"expected string, got number" — true, but a description of the symptom. The
contract check runs first so the error names the actual fault.

## Better: `deploy` refuses first

A 409 in production is better than corruption, but it is still a failure your
users can see. `bynk deploy` catches it earlier.

Each Worker's build emits `bynk-contracts.json`, recording what the context
provides and what it expects of its dependencies. `deploy` remembers what it
pushed, so when you later run:

```bash
bynk deploy --context shop.orders
```

it compares `orders`'s expectations against what its dependencies actually have
live, and stops:

```
bynk: `shop-orders` was compiled against a contract its live dependencies no
      longer provide (bynk.deploy.contract_skew):
  shop.payment.authorise — compiled against 317bdd3de84d2176, live is 9f2c41ab73e05d18
  Deploying this would ship a caller its callee rejects (409 ContractMismatch) on every call.
  Deploy the whole project (`bynk deploy`) so both sides move together.
```

The runtime check stays regardless — it is the backstop for a `wrangler` push
that bypassed the driver, or a deploy record that has drifted. But in normal use
you should meet this at the command line, not in your logs.

## What to do when you hit it

**Deploy the whole project.** `bynk deploy` (no `--context`) pushes everything in
dependency order, so both sides move together and the hashes agree again. A
contract change is a coordinated deploy; Bynk does not yet run two contract
versions side by side.

**If you did not think you changed a contract**, check what the hash actually
covers. It changes when the *wire* changes:

- a parameter renamed, retyped, added, removed, or reordered;
- a record field renamed, retyped, added, or removed;
- a sum variant added, removed, or its payload changed;
- a refinement predicate changed (`MaxLength(10)` → `MaxLength(11)`);
- the return type changed.

It deliberately does **not** change when nothing on the wire does:

- **reordering** a record's fields, or a sum's variants — a JSON object is
  unordered, and a sum carries a `kind` tag;
- **reordering** refinement predicates — `String where NonEmpty && MaxLength(10)`
  and `String where MaxLength(10) && NonEmpty` are the same type;
- reformatting, comments, or renaming anything the wire never carries;
- **tightening an `opaque` type's predicate.** A consumer cannot see inside an
  opaque type, so nothing it does can depend on that predicate — breaking it over
  a change it cannot observe would be noise.

If you get a mismatch you cannot explain from that list, that is a bug worth
reporting: a hash that fires when nothing changed is worse than no hash at all.

## The one-time rebuild

The first deploy after upgrading to v0.177 must rebuild **every** context. A
Worker built by an older compiler stamps no hash, and an absent hash fails closed
— deliberately, because a caller that predates the check is exactly the kind most
likely to be skewed. Run a full `bynk deploy` once and it settles.

## What this is not

It is **detection, not evolution**. Bynk tells you the two sides disagree; it does
not yet let them disagree safely by running two contract versions at once. And the
hash is not a security control — it is a compile-time constant on a trusted
internal channel, an accident detector, not a defence against a forged header.
