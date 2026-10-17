# 0485 — Caller identity is proved by a per-edge signature, amending ADR 0092 and superseding ADR 0479

- **Status:** Accepted (v0.340)

**Context.** ADR 0092 mints a `by c: Caller` handler's `CallerId` from the
`X-Bynk-Caller` header and trusts it because "every context in a deployment is
one trust domain". ADR 0479 (#2004) showed `/_bynk/call/` and `/_bynk/event/`
answer on every Worker's public address, and contained that with one shared
`BYNK_CALL_SECRET`. A shared secret admits members of the deployment but cannot
tell them apart: any holder can claim any caller, and the interop track
(`design/archive/retired-tracks.md`, interop track D2) cannot let a non-Bynk caller in on those
terms. D2 settled option (c), a signed caller header under a per-edge secret,
as the mechanism, with this slice (I2) replacing the shared secret for every
Bynk edge.

**Decision.**

- **Edges and keys.** An edge is a Service Binding between two of a project's
  Workers: a caller to a context it calls, or a publisher to a subscriber its
  events fan out to. Each has its own key, 32 random bytes (hex). The receiver
  holds it as `BYNK_CALL_FROM_<SENDER>` and the sender as
  `BYNK_CALL_TO_<RECEIVER>`, where the suffix is the qualified context name
  upper-cased with each dot written `__`. That encoding is injective on context
  names whose dot-separated segments are lowercase letters and digits,
  starting with a letter, with single underscores between runs; a context
  outside that domain is refused at deploy, and a callee refuses a caller
  header outside it. So one caller's key can never verify another caller's
  name, as a plain `replace('.', "_")` would let `a.b` and `a_b` share one.
- **Signing.** `callService` and the events fan-out's `deliverEvent` sign
  HMAC-SHA256 over a canonical input, one field per line: the scheme tag
  `BYNK-CALL-1`, the caller, the callee's qualified name, the method, the
  pathname as sent, `X-Bynk-Contract` (empty when absent), `X-Bynk-Wire` (`1`
  when absent), the timestamp, and the SHA-256 of the body. Every header the
  callee acts on is in it, and the callee's name binds a signature to one
  callee even where two hold the same key. They send `X-Bynk-Caller`,
  `X-Bynk-Timestamp` (Unix seconds) and `X-Bynk-Signature` (hex), and refuse to
  send without the edge's key, naming it. `bynk_check::wire_format` holds the
  constants and `signing_input`; the runtime mirrors them, and
  `bynkc/tests/wire_reference.rs` signs from the published description, not
  the runtime's signer.
- **Verifying.** The first step of every internal dispatch, after only the
  call dispatch's method check, is `__verifyCall(request, env, "<context>")`.
  It reads the body under a **1 MiB** cap (`413` past it: a declared length is
  refused before reading, a streamed body is cancelled when it crosses), picks
  the key for the edge from the claimed caller, and refuses with a bare `401`
  when there is none, when the timestamp is more than **300 seconds** from the
  callee's clock either way, or when the signature does not verify
  (`crypto.subtle.verify`, constant time). Otherwise it returns the verified
  body as text, which the dispatch parses. The cap and the window are
  requirements, not tuning knobs: the cap is what stops a sender without a key
  making the callee buffer and hash an unbounded body, and the window is the
  only bound on replay. The published check order (wire format v1) becomes
  method, body size, signature, wire version, service, contract, JSON,
  arguments. The separate missing-caller `401` goes: a signature is checked
  under the key the caller selects, so a call without one never gets past
  authentication.
- **Wire format v1 is amended in place, once.** Adding the body cap's `413`,
  dropping the missing-caller `401` and making `X-Bynk-Caller` required would
  each be a new wire version under ADR 0483's stability rule. They are made
  within version 1 because ADR 0483 left authentication explicitly open until
  this slice, version 1 has only just shipped, and no client outside a
  deployment could have held the shared secret it replaces. This is the last
  in-place change to version 1: from here on, the signature, its headers and
  the check order are frozen with the version like everything else.
- **Every request, every handler.** A binder-less `on call` and every event
  subscriber verify too (interop track Q5). An undeclared caller has no key, so it
  cannot reach any internal route.
- **Provisioning.** `bynk deploy` derives the edges from the build's own
  `[[services]]` bindings between the project's Workers, lists each key as a
  `managed` secret on the context that holds it, and decides each edge once
  per run over both ends: both ends hold a key (an unreadable `secret list`
  about a live Worker counts as holding it) and `--force` is off → keep it;
  otherwise mint one and set it on both ends. Cloudflare never returns a value
  and the ledger is committed, so a key cannot be copied from one end to the
  other: a `--context` run that would have to re-key an edge to a Worker it does
  not deploy is refused before anything is pushed, and under `--force` such an
  edge keeps its key. A run that stops between the two ends of a rotating edge
  names it and points at `bynk deploy --force`. An edge key in
  `--secrets-file` is ignored rather than spread to every context. The ledger
  records neither the keys nor the edges.
- **Off the deployed path.** `bynk dev` passes every edge a Worker is on, over
  the whole build (so separate `bynk dev --context` sessions admit each other),
  as `--var <key>:bynk-local-call-key`. The generated test harnesses do the
  same, and call their participants as `integration`. The local key is never
  written into a generated `wrangler.toml`.

This amends ADR 0092's trust paragraph: one trust domain per signing key, not
per deployment. `CallerId` stays `String` and `Caller` keeps its meaning, so the
Foundations layer is unchanged; for a Bynk caller the value a handler reads is
the same name, now proved. It supersedes ADR 0479: `BYNK_CALL_SECRET` and
`X-Bynk-Call-Secret` are no longer sent or read.

**Consequences.**

- A key's holder can claim only its own name, and a request cannot be altered
  in flight or replayed against another callee. This is the mechanism a
  foreign caller (I3) will be admitted by.
- A captured request can be replayed inside the 300-second window. Full replay
  protection needs a nonce store, which this does not build (ADR 0092 left
  replay to the events track). An `on call` handler with side effects that
  must happen once should use the `Idempotency` capability.
- Each call costs a SHA-256 of the body and an HMAC on both sides.
- **Breaking for deployed projects.** Workers built now refuse requests from
  Workers built before, and the other way round, so a project deployed with
  the shared secret must be redeployed whole once. Every edge then lacks its
  key, so that deploy mints them all. `BYNK_CALL_SECRET` is left on the
  Workers and can be deleted.
- Rotation is per edge and not yet windowed: between one end taking a new key
  and the other, calls along that edge fail. A two-key window (accept the old
  key and the new) is left for later.
- A context name outside the edge-name domain (a segment starting with or
  ending in `_`, or holding `__`, or upper case) cannot take part in an edge;
  `bynk deploy` and `bynk dev` refuse it with the rule.
