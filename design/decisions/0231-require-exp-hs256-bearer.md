# 0231 — HS256 bearer verification requires `exp`

- **Status:** Accepted (v0.208)

**Context.** The HS256 bearer seam (`verifyBearerJwtHs256`) treated `exp` as
optional: `exp` was checked only `if (payload.exp !== undefined)`. A token minted
with no `exp` claim therefore passed verification and never expired, so a leaked
bearer could not be aged out — the sole time-based revocation the seam offers.
The OIDC path (`verifyOidcJwt`) was already strict, rejecting a token with no
numeric `exp` as "missing exp". The two credential seams disagreed on whether a
non-expiring token is admissible (defect [#725](https://github.com/accuser/bynk/issues/725)).

**Decision.** Require `exp` on the HS256 bearer seam. A payload with no `exp`
returns `Err("missing exp")`; a present-but-non-number `exp` remains
`Err("malformed exp")`; an `exp` in the past remains `Err("token expired")`.
This brings the HS256 seam to parity with the OIDC seam and with RFC 7519's
intent that a bearer credential carry a bounded lifetime. `nbf` stays optional
(it constrains the start, not the end, of validity).

**Consequences.** A caller that mints exp-less HS256 tokens must now include an
`exp` claim; such tokens were a standing revocation hole, so the stricter check
is the safer default. The bearer seam keeps a distinct `missing exp` vs
`malformed exp` diagnostic (finer than OIDC's single collapsed check), so the
rejection reason stays legible at the boundary.
