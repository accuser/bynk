---
level: minor
changelog: A `match` arm or `if` branch may now produce a refined type where a sibling produces its base (or another refined type over the same base) — the branches join to their least upper bound instead of being rejected for not being byte-identical, so `match r { Ok(e: Email) => e, Err(m: String) => m }` type-checks at `String`
---

## ADR: join-match-if-branch-types
title: `match` arms and `if` branches agree up to a least upper bound, not exact type equality
summary: Why branch agreement widens a refined type to its base (and two refined siblings to their shared base) instead of demanding identical `Ty`, and why the join is the sound result type

**Context.** A `match` expression's arms and an `if`/`else`'s two branches must
*agree* on a type — the whole expression has one type, threaded on to the
enclosing context. The checker enforced agreement with exact `Ty` equality:
`match` compared every arm body against the first (`*t != first`), and `if`
compared the two branches (`t == e`). Exact equality is too strict once refined
types are in play. A refined type widens to its base everywhere else (ADR 0001):
an `Email` (`String where NonEmpty`) is usable wherever a `String` is expected.
But two branches that produced `Email` and `String` — both perfectly usable at
`String` — were rejected with `bynk.types.match_arm_mismatch` /
`bynk.types.if_branch_mismatch`, because `Email` is not the *same* `Ty` as
`String`. The canonical shape is a `Result[Email, String]` folded to a
`String`:

```bynk
type Email = String where NonEmpty
fn pick(r: Result[Email, String]) -> String {
  match r { Ok(e) => e, Err(m) => m }   // was rejected; both arms are usable at String
}
```

Both arm bodies already type-checked against the expected `String` individually;
only the final all-arms-agree check, comparing the arms to *each other* by
identity, rejected the program. Valid programs were turned away.

**Decision.** **Branch agreement computes the least upper bound (join) of the
arm/branch body types along the refined→base widening and structural-covariance
lattice that `compatible` already encodes, rather than requiring an identical
`Ty`.** The join of two types is:

- the type itself when the two are equal;
- the *supertype* when one branch is usable where the other is expected
  (`compatible(a, b)` ⇒ `b`), so a refined type and its base join to the base,
  and covariant constructors compose (`Result[Email, String]` and
  `Result[String, String]` join to the latter);
- otherwise, both refined types widened to their shared base, retried — so two
  distinct refined types over one base (`Email` and `Username`, both `String`)
  join to that base **independently of arm order**;
- absent — still a `match_arm_mismatch` / `if_branch_mismatch` error — when the
  types have no common supertype (`String` and `Int`, an enum variant and a
  `String`). The existing negative diagnostics are unchanged for genuinely
  disagreeing branches.

The join, not the first arm's type, becomes the expression's result type. This
is the sound choice: when arms disagree, the value the expression yields could at
runtime be *any* arm's type, so reporting the widest (the least upper bound) is
the only type every possible value inhabits. Reporting a narrower arm's type
(the old `first`) would have been unsound had it ever been reachable — it was not,
because unequal arms were rejected outright; the join makes the previously-rejected
mixes compile *and* types them soundly.

**Consequences.** Programs that fold a refined type and its base through a
`match` or `if` — the common `Result`/`Option` unwrap-to-base shape — now
type-check, at the joined (base) type. Nothing that was accepted changes type:
when all arms are identical the join is that identical type. Nothing that was
rejected for a real mismatch is now accepted: unrelated types have no join and
still error. The result is order-independent, so a hand-written multi-arm `match`
mixing several refined siblings behaves the same regardless of arm order. Guarded
by unit tests over the `join_ty` helper (refined↔base both orders, two refined
siblings both orders, identical-preserved, unrelated-rejected) and by the
unchanged negative fixtures for genuinely disagreeing branches
(`17_if_branch_type_mismatch`, `34_match_branch_type_mismatch`).
