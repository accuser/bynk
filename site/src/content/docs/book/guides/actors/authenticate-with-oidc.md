---
title: Authenticate with OIDC / JWKS
---
**Goal:** accept tokens issued by an OpenID Connect provider (Auth0, Okta,
Cognito, Entra ID, Google, …) — verifying them against the provider's public
keys, with no shared secret to manage.

## The `Oidc` scheme

An `Oidc` actor verifies an asymmetrically-signed (RS256/ES256) JWT from the
`Authorization: Bearer …` header against the provider's published **JWKS**. It
names three **public** trust parameters — the `issuer`, the `audience` this API
is registered as, and the `jwks` endpoint URL — and the identity type to mint
from the token's `sub` claim:

```bynk
context api

type UserId = String where NonEmpty

type Profile = { id: UserId }

actor User { auth = Oidc(issuer = "https://issuer.example.com", audience = "my-api", jwks = "https://issuer.example.com/.well-known/jwks.json"), identity = UserId }

service api from http {
  on GET("/me") () -> Effect[HttpResult[Profile]] by u: User {
    Ok(Profile { id: u.identity })
  }
}
```

At the boundary, before the body runs, the compiler emits verification that:

1. fetches the JWKS (cached, and refetched on a key-id miss so provider
   **key rotation** heals without a redeploy — the refetch rate-limited so a
   forged token with a made-up key id cannot hammer the provider);
2. verifies the RS256/ES256 signature against the matching public key
   (`crypto.subtle.verify`), rejecting `alg: none` and symmetric `HS*`
   algorithms — the classic algorithm-confusion forgery;
3. enforces the trust contract — `iss` equals your `issuer`, `aud` contains your
   `audience`, `exp` is in the future, `nbf` (if present) has passed;
4. mints `u.identity : UserId` from the `sub` claim.

Any failure **fails closed with `401`**, and your body sees only a verified user.

## No secret to manage

Unlike `Bearer` (a shared HS256 secret) or `Signature` (a shared HMAC key), an
`Oidc` actor names **no secret**. Its trust root is the provider's *public* key
set, fetched over the network — so there is no signing key to store, rotate, or
leak. The `issuer`, `audience`, and `jwks` values are public identifiers that
belong in the contract: they are exactly what a reviewer reads to know which
provider and audience a route admits.

## Who, not whose

An actor authenticates **who** is calling and seals their identity. It does not
decide **whose** a given record is — that object-level authorisation ("may *this*
user read *this* document?") is ordinary logic in your handler body, by design.
To require a *claim* the token carries (an `admin` role, say), add an
[authorisation invariant](/book/guides/actors/authorisation/) — that narrows
*who*, still never *whose*.

## Scope this slice

`Oidc` is HTTP-only and used as a **single** actor: it is not yet a member of a
[multi-actor sum](/book/guides/actors/multiple-callers/), nor a refinement base
(use `Bearer` for `where`-clause authorisation invariants for now). Full OIDC
discovery (deriving the JWKS URL from `issuer`) is a planned follow-on; name the
`jwks` URL explicitly today.

**Next:** [Add an authorisation invariant](/book/guides/actors/authorisation/), or
[serve several kinds of caller](/book/guides/actors/multiple-callers/) from one
route.
