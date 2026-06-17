# 0089 — Signature is compiler-generated HMAC-SHA256 over the raw body; configurable header; a timestamp-tolerance replay window; HTTP-only, body-required, identity `()`

- **Status:** Accepted (v0.51)
- **Spec:** `syntactic-grammar.md` (`scheme_config` keyed args), `static-semantics.md` (Signature rules), `emission.md` (the entry-dispatch verification seam), `runtime-library.md` (`verifySignatureHmacSha256`)
- **Realises:** the actors track (`design/tracks/actors.md`), slice 3. Parallels [0085](0085-bearer-token-jwt-hs256.md) (Bearer); leans on [0088](0088-optional-by-binder.md) (the optional binder).

## Context

v0.47 (ADR 0085) cashed the track's throughline — **the compiler generates the
boundary verification a service would otherwise hand-write** — for a caller
presenting a JWT. The other common external boundary is the **inbound webhook**:
a provider (Stripe, GitHub, a sibling system) POSTs an event and proves
authenticity by signing the **raw request body** with a shared secret — an HMAC
the receiver recomputes and compares. There is usually **no per-user identity**;
the signature attests *this came from the trusted sender*, and the event is the
(normal, typed) request body. Webhooks also **retry**, so a verified request can
be *replayed*; the standard guard is a signed **timestamp** within a tolerance.

## Decision

**Signature is compiler-generated HMAC-SHA256 verification over the raw request
body.** An `actor Webhook { auth = Signature(secret = "<ENV>", header =
"<Header>", (timestamp = "<Header>", tolerance = <seconds>)?) }` consumed on a
handler's `by` clause emits, at the boundary and **before the body runs**:
recompute HMAC-SHA256 over the raw body, constant-time-compare against the
configured header, optionally check the signed timestamp is within tolerance,
then deserialise the body from the same bytes. Any failure is **fail-closed →
401** (`HttpResult.Unauthorized`); the body never runs.

- **Verification model.** The compiler emits the verifier (WebCrypto
  `crypto.subtle.verify`, constant-time HMAC-SHA256). It accepts a bare hex
  digest or a `sha256=<hex>` prefix (the GitHub shape). A timing-unsafe `===`
  compare, verifying a *re-serialised* body, and a verifier with no replay guard
  were all rejected — they are the three places webhook integrations most often
  go wrong. Scope is **canonical HMAC + a configurable header**; **provider
  presets** (Stripe's compound `t=…,v1=…` format) are a thin later layer over
  this verifier, and asymmetric/Ed25519 signatures are out.
- **Raw-body ordering.** The signature is over the exact received bytes, so the
  seam sits in the **entry dispatch** (where the body is read), not the compose
  wrapper: it reads the body **once** as text (`await request.text()`), verifies
  the HMAC over those bytes, then the body-param deserialisation parses from the
  **same** text — never a re-read (which throws) or a re-serialisation (which is
  byte-fragile and breaks on whitespace/key order).
- **Replay window.** When a `timestamp` header is configured, the seam verifies
  the timestamp is a finite number within `tolerance` seconds of now (reject
  stale → fail-closed) and binds `<timestamp>.<body>` as the signed string. A
  `tolerance` without a `timestamp` is a static error
  (`karn.actor.signature_tolerance_without_timestamp`). Full replay **dedup** (by
  event id) is the `Idempotency` capability's job — out of scope; the declaration
  shape leaves room.
- **Secret sourcing.** The secret env name is named on the scheme (`secret =
  "<ENV>"`, required — `karn.actor.signature_missing_secret`) and sourced from the
  same env the `Secrets` capability reads. The `header` is required too
  (`karn.actor.signature_missing_header`).
- **No identity.** A `Signature` actor attests authenticity, not a principal — its
  identity is `()`, so **no `identity =` is permitted**
  (`karn.actor.signature_identity_unsupported`) and the binder-less `by Webhook
  (body: T)` (ADR 0088) is the canonical form.
- **Body-required, HTTP-only.** The signature is over the body, so a `Signature`
  handler MUST take a `body` param (`karn.actor.signature_requires_body`); a
  request signature is an HTTP-body concept, so `Signature` is admissible only on
  `from http` (`karn.actor.scheme_not_admissible` otherwise).
- **Keyed-args config.** The scheme config generalises from v0.47's single
  `(secret = …)` to `Scheme(key = value, …)` with string- or integer-valued args
  (the `scheme_config`/`scheme_arg` grammar productions); the checker validates
  which keys each scheme admits.

## Consequences

The second authenticated scheme, with no app-written HMAC and the byte-fragile
raw-body handling done correctly by construction. Unlike Bearer, the seam lives in
the entry dispatch (the body-read site), establishing that a scheme contributes
its seam wherever the data it verifies is available — a precedent later
body-bearing schemes reuse. Adding Signature is new surface against the v0.45
scheme descriptor (0080), not a re-architecture; provider presets and replay
dedup extend it additively. A standing behavioral bypass-class test
(`signature_auth.rs`, the ADR 0087 posture) guards the emitted verifier.
