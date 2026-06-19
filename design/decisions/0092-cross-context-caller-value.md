# 0092 — The cross-context `CallerId` value is the calling context's qualified name, stamped by the compiler into a reserved `X-Bynk-Caller` header beside the args body, read at the callee's `/_bynk/call/` boundary and threaded into `deps`; an absent caller on a `by c: Caller` handler is fail-closed; trust is static / channel-based (no crypto), first-party

- **Status:** Accepted (v0.54)
- **Spec:** `static-semantics.md` (§5.7a the live `Caller`), `emission.md` (§7.3.4a the cross-context caller seam), `runtime-library.md` (`callService` caller param)
- **Realises:** the actors track (`design/tracks/actors.md`), slice 6 — the deferred **value** half of Q7. Completes the track's Q1–Q7 scope. Builds on the v0.6 cross-context Service-Binding pipeline, the typed `Caller`/`CallerId` from Foundations, and the Bearer deps-threading pattern ([0085](0085-bearer-token-jwt-hs256.md)) under the sealed-identity model ([0081](0081-verified-identity-context-sealed.md)) and channel-trust `Internal` scheme ([0080](0080-actor-schemes-closed-nominal.md)).

## Context

Cross-context calls already work (v0.6): context A calls B's `on call` over a
Service Binding, dispatched at `/_bynk/call/<service>` with a JSON args body. The
`Caller` prelude `Internal` actor and the `CallerId` type (resolved to `String`)
landed in Foundations — but `c.identity` lowered to the placeholder `undefined`
and the wire format carried only the arguments: nothing identified the caller. A
callee knew *that* it was called, not *who* called it.

## Decision

**A cross-context `on call … by c: Caller (…)` handler reads a live `CallerId` —
the calling context's qualified name — established at the boundary.**

- **The value is the calling context's qualified name** (e.g. `"shop.orders"`),
  stamped by the compiler as a compile-time constant. It answers exactly "which
  context called me," the natural sealed identity for an `Internal` channel.
  `CallerId` stays `String`.
- **Transmitted via a reserved `X-Bynk-Caller` header**, not an args envelope.
  `callService` gains a `callerContext` argument and sets the header; the args
  body — and its typed (de)serialisation — is **untouched**. The identity rides
  beside the payload as metadata, not inside it.
- **Read and threaded at the callee boundary**, mirroring Bearer. The
  `/_bynk/call/<service>` dispatch reads the header and threads the name into the
  handler's `deps` as its `CallerId` identity; `<binder>.identity` lowers to
  `deps.identity` (the same `deps_identity_binder` path Bearer uses). A
  binder-less `on call` reads no header and is byte-unchanged.
- **Absent caller → fail-closed.** A Bynk caller always stamps the header; its
  absence (or empty value) on a `by c: Caller` handler means a malformed /
  non-Bynk internal call — rejected before the body (the internal analogue of
  401). This holds the sealed-identity guarantee: no body runs with a placeholder
  identity.
- **Trust is static / channel-based, first-party.** The `Internal` scheme trusts
  the binding: `/_bynk/call/` is platform-dispatched and not externally routable,
  and the compiler stamps the *real* name (no app-controlled forging path). A
  malicious *first-party* context forging the header is out of the threat model —
  all contexts in a deployment are one trust domain. This mints **identity**, not
  **authorisation**: no inter-context permission check is added or implied.

## Consequences

Closes Q7's value half with no new crypto and no churn to the args wire format —
the header is additive, and the deps-threading reuses the Bearer path (the
`bearer_identity_binder` field generalises to `deps_identity_binder`). With this
slice the actors track's planned **Q1–Q7 scope is complete**; the only remaining
item, Q8 (replay/ordering), is cross-track (Events) and may not be an actors
slice at all. A standing behavioral guard (`bynkc/tests/cross_context_caller.rs`)
drives the callee dispatch with and without the header (live id vs fail-closed
401). Signed caller headers (multi-trust-domain integrity), inter-context
**authorisation**, and a structured `CallerId` extend this additively if a
multi-trust-domain model ever arrives.
