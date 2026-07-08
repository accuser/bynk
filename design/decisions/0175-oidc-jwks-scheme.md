# 0175 — `Oidc` is a compiler-generated JWKS/RS256+ES256 verifier; the scheme set opens by widening, not by a user `Verifier`

- **Status:** Accepted (v0.151)
- **Provenance:** design-review finding [#553](https://github.com/accuser/bynk/issues/553)
  — "Open the auth-scheme set via a `Verifier` capability (OIDC/JWKS first)". The
  actor model's one strategic weakness was a closed scheme set
  ([ADR 0080](0080-actor-schemes-closed-nominal.md)) with no user-defined verifier;
  real systems hit the OIDC wall almost immediately. The design notes' own top
  open decision (§21) sketched the `Verifier[T]`-capability route.
- **Realises:** `actor User { auth = Oidc(issuer = "…", audience = "…", jwks =
  "…"), identity = UserId }` consumed on a `from http` handler's `by` clause —
  the compiler verifies the RS256/ES256 JWT against the provider's published
  JWKS, checks `iss`/`aud`/`exp`/`nbf`, and mints the identity from `sub`, all at
  the boundary before the body runs, fail-closed → 401.
- **Spec:** `syntactic-grammar.md` (§4.4.10 scheme), `static-semantics.md`
  (§5.7a `Oidc` rules), `emission.md` (§7.3.4a the `Oidc` seam),
  `runtime-library.md` (§7.4.8 `verifyOidcJwt`).
- **Relates:** ADR 0080 (the closed nominal scheme set — "open later = widen the
  enum"), ADR 0085 (Bearer JWT/HS256 — the seam this extends, whose "RS256/ES256
  + JWKS" deferral this cashes in), ADR 0089 (Signature), ADR 0091 (refinement
  authorisation), ADR 0087 (security CI posture), ADR 0156 (the editor surface).

## Context

The actor model is the language's best feature and the piece least tied to
Cloudflare, but the auth-scheme set was closed to `None | Internal | Bearer |
Signature` with no route to OIDC/JWKS, session cookies, or mTLS. ADR 0085 built
Bearer as **compiler-generated** HS256 verification and explicitly **rejected a
user-supplied verifier** — it reintroduces the hand-written-crypto footgun the
feature exists to remove — deferring "RS256/ES256 + JWKS" to a later slice. This
is that slice. It is also the first test of whether opening the set needs a new
mechanism.

## Decisions

**A — Open the set by widening the scheme enum with a compiler-known `Oidc`
scheme, NOT a user-supplied `Verifier[T]` capability.** The finding's framing is
a `Verifier` *capability* (the §21 sketch: custom actors plug in their own
verification). We take the narrower, safer route ADR 0080 prescribes — "opened
later = widen the enum" — for the concrete reason ADR 0085 already established:
verification owns secret sourcing, algorithm pinning, failure shaping, and the
trust assertion, and a user-supplied verifier hands all four to app code (the
exact footgun Bearer removed). OIDC is a **standard**, so the compiler can own a
total, reviewable verifier for it without asking the author to write crypto —
preserving the `emission.md` invariant that *an actor emits no TypeScript; the
compiler generates the boundary verification*. A genuinely user-pluggable
`Verifier[T]` remains open for schemes the compiler cannot standardise (bespoke
session cookies, opaque-token introspection); OIDC does not need it.

**B — The trust declaration carries public parameters and NO secret.** The
finding's second item flags that secrets are bound by env-var-name strings
*inside* the trust declaration (`Bearer(secret = "<ENV>")`) — configuration baked
into the contract. `Oidc` is the answer's first instance: its trust root is the
provider's **public** key set, so the declaration names `issuer`, `audience`, and
the `jwks` URL — all non-secret, public trust parameters that legitimately belong
in the contract (they are exactly what a reviewer must see to know *who* is
admitted) — and **no secret at all**. The distinction the finding wanted drawn:
public trust parameters belong in the contract; secrets do not, and OIDC shows a
scheme that needs none. (Reworking Bearer/Signature secret sourcing is out of
scope here; `Oidc` establishes the shape.)

**C — RS256 and ES256 against an explicit `jwks` URL; discovery and more
algorithms are later slices.** The verifier accepts the two dominant OIDC signing
algorithms (RS256, ES256) and rejects `alg: none` and symmetric `HS*` (the
classic algorithm-confusion forgery against a public key). The JWKS endpoint is
named explicitly (`jwks = "…"`); full OIDC discovery
(`/.well-known/openid-configuration` → `jwks_uri`) is a mechanical follow-on left
out to keep this slice one network dependency, not two. Keys are cached (~10 min)
and refetched once on a `kid` miss so key rotation heals without a redeploy; a
refetch is **not** triggered by a mere bad signature, so a flood of forged tokens
cannot amplify into JWKS fetches.

**D — HTTP-only, single-actor; not a sum member, not a refinement base, this
slice.** Like Bearer, `Oidc` reads an `Authorization: Bearer <jwt>` header, so it
is admissible only on `from http` (`bynk.actor.scheme_not_admissible` elsewhere).
It is rejected as a peer in a multi-actor sum (`bynk.actor.oidc_not_in_sum`) — the
sum wrapper reads the body once and tries members synchronously, a shape the
async JWKS-fetch seam does not yet fit — and an `Oidc` actor is not a refinement
base this slice (a refinement's base must still be `Bearer`). Both are additive
follow-ons, not re-architectures.

**E — The identity is minted from `sub`, through the declared type, exactly as
Bearer.** An `Oidc` actor must declare a string-constructible, context-owned
`identity` (`bynk.actor.oidc_identity_not_string_constructible`); the verified
`sub` is constructed through its `.of` constructor (fail-closed on refinement
violation), and threads through `deps` so `<binder>.identity` reads the sealed
value — the same identity seam Bearer uses, so no new identity machinery.

**F — Document "who, not whose."** The finding's third item: state explicitly
that the model covers *who* is at the boundary (authentication + a sealed
identity), not *whose* a given object is. Object-level authorisation — may *this*
user read *this* record? — is domain logic in the handler body, by design; the
`where`-clause invariants (ADR 0091) narrow *who*, never *whose*. Recorded as a
note in `static-semantics.md` §5.7a.

## Consequences

- The scheme set is **open in practice**: a real, standards-based, asymmetric-key
  authentication scheme ships, and the "widen the enum" route (ADR 0080) is
  proven to carry a scheme with a network dependency and public-key crypto — no
  new language mechanism required.
- All security-bearing codegen stays in one compose-wrapper block and one runtime
  helper (`verifyOidcJwt`), keeping the `/security-review` tight (ADR 0087); a
  standing behavioural guard (`bynkc/tests/oidc_auth.rs`) drives the emitted
  verifier against every bypass class with real RSA/ES256 keys and a mocked JWKS.
- New diagnostics `oidc_missing_issuer` / `oidc_missing_audience` /
  `oidc_missing_jwks` / `oidc_identity_not_string_constructible` /
  `oidc_not_in_sum`; one new runtime export; tree-sitter `scheme` choice widened
  (`parser.c` regenerated at generate time, ADR 0156). No change to Bearer,
  Signature, sums, or the Caller value.
- OIDC discovery, a user-pluggable `Verifier[T]`, `Oidc` in sums, refinement over
  `Oidc`, and reworking Bearer/Signature secret sourcing are all **explicitly
  deferred**, each an additive follow-on.

## Tooling (ADR 0156)

- **Hover / Completion / Signature help / Semantic tokens:** unchanged — `Oidc`
  is a scheme keyword in the same `scheme` production as `Bearer`/`Signature`,
  and its config uses the existing `scheme_arg` surface; no new tooling seam.

## Alternatives considered

- **A user-supplied `Verifier[T]` capability (the §21 sketch).** Deferred — it
  reintroduces the hand-written-crypto footgun ADR 0085 removed, for a capability
  OIDC does not need. Kept open for schemes the compiler cannot standardise.
- **Full OIDC discovery instead of an explicit `jwks` URL.** Deferred — a
  mechanical follow-on; naming the JWKS URL keeps this slice to one network
  dependency and a smaller trust surface.
- **Supporting `Oidc` in multi-actor sums now.** Deferred — the first-wins sum
  wrapper's read-body-once, synchronous-try shape does not fit an async
  JWKS-fetch member without reworking it; single-actor OIDC is the common case.
