---
level: minor
changelog: In a test case, a route guarded by a refinement actor takes `by Admin("root")` with the base actor's identity, and a `system` case signs the claims the refinement checks; a route guarded by an actor sum (`by who: User | Visitor`) takes any member, `by Visitor` or `by User("ada")`, and runs with that member's arm
---

## ADR: test-principal-refinement-and-sum
title: A test acts as a refinement actor with its base identity, and as any member of an actor sum
summary: Refinements resolve through their base's identity and sign a witness of their predicate at `system`; a sum `by` takes any member and builds `deps.who`

**Context.** ADR 0205 checks a case's call-site `by <Actor>(<identity>)`
against the addressed handler. It resolved an actor's identity from its own
`identity =` entry, and a handler's actor from its `by` clause's first name.
Both broke on actor forms that came later. A refinement (`actor Admin = User
where hasClaim("admin")`, ADR 0091) declares no identity of its own, so `by
Admin("root")` was refused as `bynk.test.actor_no_identity`, and a bare `by
Admin` passed check and then failed `tsc`, because the handler takes the base's
`identity` (#2019). A sum (`by who: User | Visitor`, v0.52) was checked against
its first member only, so `by Visitor` was refused as
`bynk.test.principal_identity_mismatch`, and `by User("ada")` emitted
`{ identity }` where the handler takes `{ who }` (#2020).

**Decision.** A refinement actor's identity is its base actor's, as the
Worker's Bearer seam already treats it. So a case writes `by Admin("root")`
with a value of the base's identity type, and a bare `by Admin` is
`bynk.test.actor_identity_required`. At the `unit` and `integration` tiers the
refinement's claim is given, not checked: the predicate lives in the Worker's
entry wrapper, not the handler. At the `system` tier the test driver for a
refinement-guarded route signs, next to `sub`, a claim set that satisfies the
acting refinement's predicate (`hasClaim("admin")` → `{ "admin": true }`,
`claimEquals("role", "ops")` → `{ "role": "ops" }`, with `&&`, `||` and `!`
solved over the closed vocabulary). The real seam then checks it as it would
any token, and a case acting as the base actor signs no claims and gets the
`403`.

A handler whose `by` clause is a sum runs as whichever member resolves, so a
case may name any member, with exactly the identity that member carries
(`by User("ada")`, `by Visitor`, `by Caller("shop")` on a `call` sum). The
emitted call builds the handler's `deps.who` arm (`{ tag: "User", identity }`
or `{ tag: "Visitor" }`). The emitted handler names that deps key `who` for
every sum, whatever the binder is called. A non-member is
`bynk.test.principal_identity_mismatch`, and a case with no `by` is
`bynk.test.principal_required`, since neither can build an arm. This applies to
the `call` sums of I3 (#1971) too: before, a case with no `by` on one passed
check and emitted no `who`. At the `system` tier a sum needs no change: a
signed `User` token resolves the `User` arm, and `by Visitor` presents a token
with no subject, which the verifier rejects, so resolution falls through to
`Visitor`.

**Consequences.** Every route a refinement or a sum guards can now be driven
from a test at every tier, and each shape the checker accepts compiles under
`tsc`. A refinement-guarded route's `system` drivers take a trailing claims
argument, and other routes' drivers are unchanged. A predicate no claim set
satisfies signs no claims, so its route answers `403` to every case. A sum's
`Signature` member still has no `system`-tier signer, as a single-actor
`Signature` route has none.
