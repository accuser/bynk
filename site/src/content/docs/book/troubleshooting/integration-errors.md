---
title: "`bynk.tier.*` and `bynk.stub.*` errors"
---
These diagnostics come from the **tier dial** (the `as <tier>` clause) and from
**`stub`** test doubles (v0.118). See [Test tiers](/book/guides/testing/integration/),
the [`stub` reference](/book/reference/testing/#stub), and the
[tiers reference](/book/reference/testing/#tiers-the-as-tier-clause).

## `bynk.tier.property_has_tier`

```text
[bynk.tier.property_has_tier] a `property` cannot carry a tier; `as <tier>` is a `case`-only clause
```

**Cause:** an `as <tier>` clause is attached to a `property` header (or a `property`
sits under a tiered `suite` and tried to inherit it). A `property` *generates* its
subjects and does not promote — promoting it would multiply generation by
real-collaborator cost and re-admit the ambient nondeterminism a tier removes.

**Fix:** remove the tier from the `property`. A suite-level `as` binds its `case`
members only, so a `property` under a tiered suite is fine as long as it carries no
tier of its own. To check a generated input end to end, promote *that witness* as a
concrete `case … as integration`.

## `bynk.tier.system_needs_wire`

```text
[bynk.tier.system_needs_wire] a `system` case must span at least two contexts, but only `shop.orders` is reached
```

**Cause:** a `case as system`'s inferred participant set — the unit under test plus
its transitive `consumes` closure — is fewer than two contexts. `system` describes
the *cross-context, wired* tier, so it needs a wire to cross.

**Fix:** if the flow genuinely stays within one context, use `as integration` (real
collaborators, one context, no wire) instead. If it should cross a boundary, make
sure the unit under test actually `consumes` the other context — participants are
inferred from that graph, never listed.

## `bynk.stub.not_a_seam`

```text
[bynk.stub.not_a_seam] `Rates` is not a capability the unit under test consumes; only a consumed capability can be provided
```

**Cause:** a `stub` clause targets something that is not a capability seam the
unit under test consumes / has in scope via `given` — for example an agent, a type,
or a capability the unit does not depend on. `stub` is **capability-only**: an
agent's realness is the tier's job, not a provider's.

**Fix:** provide a capability the unit actually consumes. To change the realness of
an agent or a whole context, promote the tier (`as integration` / `as system`)
instead.

## `bynk.stub.unknown_op`

```text
[bynk.stub.unknown_op] capability `Rates` has no operation named `looup`
```

**Cause:** the `Cap.method(…)` left-hand side names an operation the capability does
not declare (typically a typo).

**Fix:** use one of the capability's declared operations (check the `capability`
block).

## `bynk.stub.rhs_type`

```text
[bynk.stub.rhs_type] `returns "1.25"` has type `String`, but `Rates.lookup` returns `Float`
```

**Cause:** a `returns <value>` supplies a value whose type disagrees with the
operation's declared return type.

**Fix:** return a value of the operation's result type. To inject a *fault* rather
than a value, write `fails`; an in-band `Err` outcome is an ordinary value you
assert directly in the case.

## `bynk.stub.bad_sequence`

```text
[bynk.stub.bad_sequence] `returns each []` is empty; a sequence needs at least one outcome
```

**Cause:** a `returns each [<outcome>, …]` sequence is malformed — most commonly
empty, so there is no outcome to serve on the first call.

**Fix:** give the sequence at least one outcome. Each outcome is a value (a
success), the atom `fails` (a fault), or `ok(v)`; the **last outcome repeats** once
the sequence is exhausted, so `[fails, fails, ok(resp)]` is "fails twice, then
succeeds forever".
