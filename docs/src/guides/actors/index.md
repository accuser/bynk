# Actors & access control

An **actor** is a *boundary contract*: it tells Bynk what to expect of the party
on the other side of a request, and the compiler generates the verification a
service would otherwise hand-write. A handler names its actor with a **`by`
clause**, and the body runs **only if the contract is satisfied** — the payload
already parsed, the caller's identity available as a typed value.

```bynk,ignore
actor User { auth = Bearer(secret = "AUTH_JWT_SECRET"), identity = UserId }

service api from http {
  on GET("/me") by u: User () -> Effect[HttpResult[Profile]] {
    // runs only for a verified User; u.identity : UserId
  }
}
```

## What an actor declares

An `actor` is a contract type, not a runnable entity. It captures up to four
things about a party:

1. **Authentication scheme** — how the party proves who it is. A closed,
   compiler-known set: `None` (anonymous), `Bearer` (a JWT), `Signature` (a
   webhook HMAC), and `Internal` (an in-system caller over a Service Binding).
2. **Identity** — the typed value a verified party yields, read as
   `binder.identity` and a **sealed** value: minted at the boundary, never
   forged or re-checked downstream.
3. **Authorisation invariant** — an extra property the party must satisfy (an
   `Admin` is a `User` who carries an `admin` claim), written as a refinement.
4. **Replay / ordering** — what the runtime should expect (a webhook's signed
   timestamp bounds replay).

## The rules that always hold

- **Fail-closed.** If verification does not succeed, the body does not run. A
  failed authentication is `401`; a verified party that fails an authorisation
  invariant is `403`; a webhook with a bad signature is `401`.
- **Verify, then run.** Verification is a distinct phase that completes — and
  parses the body — before your code executes.
- **No ambient identity.** The identity threads in as the named `by` binding; it
  is never read from hidden state. A handler that omits the binder
  (`by User`) verifies the contract but captures nothing.
- **HTTP has no safe default.** Every HTTP handler must declare a `by` clause —
  a public route writes `by v: Visitor` (the anonymous actor). The internal
  protocols default sensibly: `on call` → `Caller`, cron → `Scheduler`,
  queue → `Producer`.

## Recipes

**Do**
- [Serve public and authenticated routes](public-and-authenticated.md) — `Visitor` and `Bearer`.
- [Verify an inbound webhook](verify-webhooks.md) — `Signature`, with a replay window.
- [Serve several kinds of caller from one route](multiple-callers.md) — a multi-actor sum.
- [Add an authorisation invariant](authorisation.md) — refinement actors and the `401`/`403` split.
- [Know which context called you](cross-context-callers.md) — the `Caller` identity.

**See also:** [Reference — Actors](../../reference/actors.md),
[Specification §5.7a](../../spec/static-semantics.md),
[Diagnostic index (`bynk.actor.*`)](../../reference/diagnostics.md).
