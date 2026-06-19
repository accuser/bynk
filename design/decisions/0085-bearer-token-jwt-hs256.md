# 0085 — BearerToken is compiler-generated JWT/HS256; identity is a claim through the identity type; HTTP-only

- **Status:** Accepted (v0.47)
- **Spec:** `syntactic-grammar.md` (`actor_decl` scheme config), `static-semantics.md` (Bearer rules), `emission.md` (the verification seam), `runtime-library.md` (`verifyBearerJwtHs256`)
- **Realises:** the actors track (`design/tracks/actors.md`), slice 2.

## Context

v0.45 (ADRs 0080–0082) built the whole boundary-contract machine against the two
zero-crypto schemes (`None`/`Internal`), which mint a unit identity and emit no
verification. The track's throughline is that **the compiler generates the
boundary verification a service would otherwise hand-write**. Slice 2 cashes that
in for the most common external case — a Bearer token on an HTTP route — and
mints the first real (non-unit) identity.

## Decision

**Bearer is compiler-generated JWT verification with HS256.** An
`actor User { auth = Bearer(secret = "<ENV>"), identity = UserId }` consumed on a
handler's `by` clause emits, at the boundary and **before the body runs**: extract
`Authorization: Bearer <token>`, HS256-verify the JWT against the secret, enforce
`exp`/`nbf`, and mint the identity from the `sub` claim. Any failure is **fail-
closed → 401** (`HttpResult.Unauthorized`); the raw token never reaches the body.

- **Verification model.** The compiler emits the verifier (WebCrypto
  `crypto.subtle.verify`, constant-time; `alg` must be `HS256` — `none`/algorithm
  confusion rejected). A user-supplied verifier was rejected (it reintroduces the
  hand-written-crypto footgun the feature removes); an opaque shared-secret
  compare was rejected (it yields no per-party identity). Scope is HS256 only;
  RS256/ES256 + JWKS and opaque-token lookup are later slices.
- **Identity source.** The minted identity is the JWT `sub` claim, **constructed
  through the declared `identity = T` type's string-constructor** (`.of` +
  refinement), so an absent/ill-formed `sub` fails closed → 401. A Bearer actor's
  identity must therefore be a string-constructible, context-owned type
  (`bynk.actor.bearer_identity_not_string_constructible`).
- **Secret sourcing.** The secret env name is named on the scheme
  (`Bearer(secret = "<ENV>")`, required — `bynk.actor.bearer_missing_secret`) and
  sourced from the same env the `Secrets` capability reads (explicit env first,
  then a `process.env` probe). The seam runs in the compose wrapper, which owns
  `env` and `deps`, so the whole verification is one cohesive, reviewable block.
- **HTTP-only.** An `Authorization` header is an HTTP concept; `Bearer` is
  admissible only on `from http` (`bynk.actor.scheme_not_admissible` otherwise).
- **Identity threading.** The minted identity threads through the handler's
  `deps`; `<binder>.identity` lowers to `deps.identity` — resolving the v0.45
  note that the blanket `.identity → undefined` lowering was only sound for unit
  identities.

## Consequences

The first authenticated, typed, sealed identity flows service→agent with no app
crypto. The seam concentrates all security-bearing codegen in one wrapper block,
keeping its `/security-review` tight. Adding Bearer is new surface against the
v0.45 scheme descriptor (0080), not a re-architecture. The non-unit identity now
has a real runtime value; later schemes (Signature, RS256) and the calling-context
`CallerId` value extend the same seam.
