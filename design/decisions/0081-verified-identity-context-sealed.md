# 0081 — A verified actor identity is a context-sealed value

- **Status:** Accepted (v0.45)
- **Spec:** `static-semantics.md` (`bynk.actor.identity_not_sealed`, the sealed-identity rule), `emission.md` (identity at the seam)
- **Realises:** the actors track, question Q2.

## Context

A verified party yields a typed identity, available in the handler and passed
downstream to agents (design notes §6). What is that value — a plain record, a
linear capability, or something else — and what stops downstream code forging or
re-checking it?

## Decision

A verified identity is a **context-sealed value**: minted only at the
verification seam, flowing service → agent unchanged, and **never re-checked
downstream**. It is accessed as `name.identity` on the `by`-bound actor (the
binding is the *verified actor*, leaving room for `name.scheme`/claim accessors
later without a surface change).

Sealed by reusing Bynk's existing rule that context-owned types are minted only
inside the owning context (cf. refined boundary IDs, 0014): a declared
`identity = T` must be a context-ownable type (`bynk.actor.identity_not_sealed`
otherwise), so unforgeability is by construction, not convention. This is the
object-capability *introduction* rule — re-checking downstream would reintroduce
ambient authority and the confused-deputy footgun. Authority is made visible in
signatures (an agent's typed parameters say "operates under identity X") without
full linearity, which is the wrong ergonomic grain for a read-many, fanned-out,
logged, tested value.

Foundations is **machinery-first**: identities are typed and threaded, but the
zero-crypto schemes carry no payload, so `name.identity` is `()` for trivial
actors. The one non-trivial Foundations identity is the prelude `Caller`'s
calling-context value (Q7, folded in): typed as a sealed value and threaded; its
concrete runtime value lands with the authenticated-scheme slices.

## Consequences

Identity is unforgeable and authoritative downstream with no re-validation. The
constraint — identity types must be context-ownable — is the price of by-
construction sealing; the `identity_not_sealed` diagnostic explains it. The
`name.identity` surface leaves headroom for actor-level accessors (scheme,
claims) the later slices add.
