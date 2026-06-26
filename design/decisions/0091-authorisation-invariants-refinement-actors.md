# 0091 — Authorisation invariants are refinement actors over a Bearer base; the predicate is a closed claim-predicate set checked against the verified claims at the boundary; a failed invariant is 403, distinct from the 401 authentication channel; an `Admin` is-a `User`

- **Status:** Accepted (v0.53)
- **Spec:** `syntactic-grammar.md` (the refinement form), `static-semantics.md` (§5.7a refinement actors), `emission.md` (the refinement seam / 403), `runtime-library.md` (`verifyBearerJwtHs256` surfaces claims)
- **Realises:** the actors track, slice 5 (Q3) — and closes the 401/403 split (Q6) the multi-actor slice deferred. Builds on [0085](0085-bearer-token-jwt-hs256.md) (Bearer) and the refinement-not-a-sum-member rule of [0090](0090-multi-actor-sum-dispatch.md).

## Context

Authentication (`None`/Bearer/Signature) answers *who you are*; it does not answer
*whether you may*. A verified `User` may still lack the right to an admin route —
the classic place an app forgets the guard, or conflates "not logged in" (401)
with "logged in but not allowed" (403). The track reserved the refinement form
`actor Admin = User where <predicate>` since v0.45 (parsed, rejected) precisely
for this; the grammar already carries it.

## Decision

**An authorisation invariant is a refinement actor: `actor Admin = User where
<claim predicate>`.** A handler `by a: Admin` emits, at the boundary: verify the
base scheme (failure → **401**), check the predicate against the *verified* claims
(failure → **403**), then mint the base identity and run the body.

- **Desugar as refinement (Q3).** `actor Admin = User where p` is "an `Admin` is a
  `User` who additionally satisfies `p`" — it *carves a subset*, it does not add a
  member, so the closed scheme set (0080) and exhaustiveness stay intact. By
  refinement elimination an `Admin` is usable wherever its base `User` is: a `by a:
  Admin` binder yields the base `User` identity, and the invariant is discharged at
  the boundary.
- **Closed claim-predicate vocabulary.** The predicate is a closed set —
  `hasClaim("name")` (present and truthy) and `claimEquals("name", "value")`
  (string equality), composed with `&&`/`||`/`!`. Claims are untyped JSON, so an
  arbitrary typed expression has no surface to bind against; the closed set covers
  the common RBAC shape and is the substrate a later typed-claims layer sits on.
  An out-of-set predicate is `bynk.actor.refinement_predicate_unsupported`.
- **Bearer base only (for now).** A refinement tests claims, which only a `Bearer`
  actor carries; refining `None`/`Internal`/`Signature` has nothing to test
  (`bynk.actor.refinement_base_unsupported`). Internal-channel authorisation is a
  later, separate question.
- **Claims surface at the boundary, not the body.** `verifyBearerJwtHs256` now
  returns the verified `claims` alongside `sub`; the seam evaluates the predicate
  (lowered inline — no new runtime export) against them. The body still sees only
  the sealed `sub`-minted identity: claims are an authorisation-time input, not a
  body-time value, so they are not threaded into `deps` (keeping the sealed-identity
  surface of [0081](0081-verified-identity-context-sealed.md) intact).
- **403 fail-closed, distinct from 401 (closes Q6).** A verified party that fails
  the invariant → `HttpResult.Forbidden` (403); an unverified party → 401. The two
  are structurally separate response channels — the runtime can never answer "who
  are you" when it means "you may not." The check is **after** scheme verification
  and **before** identity mint / body, and fails closed.
- **Never a sum member.** A refinement is a handler's sole `by` contract; the
  `refinement_in_sum` rule (0090) bars it from a sum, which is what keeps sum
  reachability decidable and the sum-failure response unambiguously 401.

## Consequences

The authorisation half of the boundary, completing the 401/403 split. Built by
*activating* a reserved grammar form and *extending* the Bearer seam — new surface,
not a re-architecture. The closed claim-predicate set keeps the 403 decision total
and reviewable; a standing behavioral guard (`bynkc/tests/refinement_auth.rs`, the
ADR 0087 posture) drives the emitted trichotomy (401 / 403 / allow). Typed claims
schemas and general predicate expressions, non-Bearer authorisation, nominal actor
extension (adding structure, not restricting), and RBAC hierarchies extend this
additively. The `bynk.actor.refinement_unsupported` blanket rejection is retired.
