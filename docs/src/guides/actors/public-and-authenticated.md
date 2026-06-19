# Serve public and authenticated routes

**Goal:** expose a public endpoint, and a second endpoint that requires a
verified user — reading the user's identity in the body.

## A public route: `Visitor`

HTTP has no safe default actor, so even an anonymous route declares one. The
prelude actor `Visitor` (scheme `None`) accepts everyone and yields no identity:

```bynk
context api

service api from http {
  on GET("/health") by Visitor () -> Effect[HttpResult[String]] {
    Ok("ok")
  }
}
```

The binder is optional — `by Visitor` (no name) verifies the contract and
captures nothing, which is all an anonymous route needs.

## An authenticated route: `Bearer`

A `Bearer` actor verifies a JWT from the `Authorization: Bearer …` header. It
names the env var holding the signing secret, and the identity type to mint from
the token's `sub` claim:

```bynk
context api

type UserId = String where NonEmpty

type Profile = { id: UserId }

actor User { auth = Bearer(secret = "AUTH_JWT_SECRET"), identity = UserId }

service api from http {
  on GET("/me") by u: User () -> Effect[HttpResult[Profile]] {
    Ok(Profile { id: u.identity })
  }
}
```

At the boundary, before the body runs, the compiler emits HS256 verification
(constant-time, with `exp`/`nbf` checks), mints `u.identity : UserId` from the
`sub` claim, and **fails closed with `401`** on any problem — a missing or
malformed token, a bad signature, an expired token, or a `sub` that does not
satisfy `UserId`'s refinement. Your body sees only a verified user.

- The **secret** is the name of an environment variable (the same source the
  `Secrets` capability reads), not the key itself.
- The **identity type** must be a context-owned, string-constructible type (here
  `UserId`), so the minted value is sealed to this context.
- `u.identity` is read-only. You cannot construct a `User`, pass it around, or
  reach any field other than `.identity`.

## Verify a token without capturing the identity

Drop the binder when you only need to *gate* a route, not read who it was:

```bynk
context api

type UserId = String where NonEmpty

actor User { auth = Bearer(secret = "AUTH_JWT_SECRET"), identity = UserId }

service api from http {
  on POST("/ping") by User () -> Effect[HttpResult[String]] {
    Ok("pong")
  }
}
```

The token is still verified fail-closed; no identity is minted.

**Next:** [Add an authorisation invariant](authorisation.md) to require a claim
(an admin-only route), or [serve several kinds of caller](multiple-callers.md)
from one route.
