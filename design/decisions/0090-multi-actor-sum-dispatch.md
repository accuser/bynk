# 0090 — Multi-actor handlers are an ordered sum of peer actors, resolved first-wins, keyed by scheme; a sum requires a binder; the body matches the resolved nominal actor; HTTP-only; total failure → 401

- **Status:** Accepted (v0.52)
- **Spec:** `syntactic-grammar.md` (`by_clause` actor list), `static-semantics.md` (§5.7a.1 multi-actor sum), `emission.md` (the first-wins boundary wrapper)
- **Realises:** the actors track (`design/tracks/actors.md`), slice 4 (Q4). Composes the three landed scheme seams — [0085](0085-bearer-token-jwt-hs256.md) (Bearer), [0089](0089-signature-hmac-sha256-webhooks.md) (Signature), and the zero-crypto `None` — under the sealed-identity model ([0081](0081-verified-identity-context-sealed.md)) and verify-then-body fail-closed ([0082](0082-by-clause-verify-then-body-defaults.md)); leans on the optional binder ([0088](0088-optional-by-binder.md)).

## Context

The three landed slices each verify **one** party at a boundary. Real routes
often serve more than one kind of caller: a richer view to a signed-in `User`
and a public view to an anonymous `Visitor`; an ingest endpoint taking either an
internal `User` token or a partner `Webhook` signature. Without language support
that forces splitting the route or hand-rolling the "try auth A, else B"
branching — the verification logic the track exists to generate. No surveyed
framework offers a closed, discriminated actor sum (Servant collapses multiple
schemes to one shared principal and loses *which* verified).

## Decision

**A `by` clause may name an ordered sum of peer actors** (`by who: A | B`),
resolved **first-wins**, the body `match`ing on the resolved actor.

- **Peer keying is by scheme.** Peers are distinguished by their authentication
  scheme; **two peers may not share a scheme** (a second same-scheme member is
  unreachable — `bynk.actor.duplicate_sum_scheme`). Keying by concrete
  *verification* (same-scheme multi-provider — two `Signature` senders with
  different secrets) was considered and **deferred**: it buys expressiveness at
  the cost of a softer reachability guarantee. The declaration shape (`A | B | …`)
  does not foreclose widening "distinct scheme" to "distinct verifier" later.
- **A sum requires a binder.** The body learns *which* peer verified by matching
  the binder, so a binder-less sum is rejected (`bynk.actor.sum_requires_binder`).
  Single-actor handlers keep the optional binder of ADR 0088.
- **The binder is a compiler-formed nominal actor sum**, matched exhaustively by
  the existing sum-type checker. Each arm binds the resolved actor's **identity
  directly** (`User(u)` ⇒ `u` is the identity; the arm already names the actor, so
  no `.identity` indirection); a unit-identity peer (`Visitor`, a `Signature`
  webhook) binds nothing. A non-exhaustive match is the ordinary
  `bynk.types.non_exhaustive_match`.
- **Refinements are never members.** `User | Admin` is rejected — every `Admin`
  is a `User`, so the arm is dead (`bynk.actor.refinement_in_sum`, Q3). Narrowing
  is a guard inside the resolved arm. Keeping refinements off the sum axis is what
  keeps reachability decidable and the failure response unambiguous.
- **Reachability is decidable and scheme-level.** A `None`-scheme catch-all
  (`Visitor`) accepts everyone, so it must come last; any member after it is
  unreachable (`bynk.actor.unreachable_sum_arm`). The compiler does not reason
  about predicate-level disjointness.
- **HTTP-only.** A sum is admissible iff every member is admissible on the
  protocol; in practice HTTP is the only protocol with more than one admissible
  non-internal scheme (`bynk.actor.scheme_not_admissible` for a non-admissible
  member).
- **Emission: a single boundary wrapper owns the whole boundary.** It reads the
  raw body **once** when any member verifies over it (Signature) or the handler
  takes a `body`; tries each member's scheme in declared order (Bearer against the
  `Authorization` header, Signature against the held bytes, `None`
  unconditionally); binds the first success into a tagged `{ tag, identity? }`
  threaded through `deps.who`; **fail-closed → 401** if none verifies; then parses
  the body from the same bytes and dispatches. No member re-reads the request —
  composing a header member with a body member never re-reads or re-serialises
  (the body-fragility ADR 0089 closed stays closed).
- **Total failure → 401.** A sum's members are peer base actors with no
  invariants, so total failure is *no party authenticated* → 401. There is no 403
  path (authorisation invariants live only on refinement actors, which cannot be
  members — Q3). Verification is side-effect-free and idempotent; audit/logging
  belongs *after* resolution.

## Consequences

The track's one genuinely novel construct, built by **composing** the three
landed scheme seams rather than adding a new scheme — new surface against the
sealed scheme descriptor (0080), not a re-architecture. Modelling the binder as a
new `Ty::ActorSum` whose variants `variants_of` reports means the entire `match`
machinery (arm resolution, payload binding, exhaustiveness) works unchanged. The
emission crux — first-wins over a header member and a body member with a single
body read — is solved by the wrapper owning the boundary. A standing behavioral
guard (`bynkc/tests/multi_actor_sum.rs`, the ADR 0087 posture) drives the emitted
resolution: first-wins, fall-through on an invalid earlier member, and
fail-closed-total. Concrete-verification keying (same-scheme multi-provider),
refinement members + the 403 path (Q3), and internal-channel sums extend this
additively.
