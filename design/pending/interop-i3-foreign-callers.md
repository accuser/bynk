---
level: minor
probes: emit_diagnostics, diagnostic_coverage, ts_writes
changelog: A context can admit `on call` callers that are not Bynk by declaring each as an actor with the new `Foreign` scheme, `actor Ledger { auth = Foreign(secret = "LEDGER_CALL_KEY", worker = "ledger-svc") }`, and naming it in a handler's `by` clause, alone or in a sum such as `by who: Caller | Ledger`. A foreign caller signs its calls as the actor under that secret, and is admitted only where its actor is named; `Caller` admits Bynk contexts only, and a caller a handler does not admit gets `403`. `bynk deploy` mints each foreign caller's secret, sets it on the context, and prints the `wrangler secret put` for the foreign Worker's operator. `bynk dev` and `bynk test` use a fixed local value, and a system test can call `by Ledger`. See the [wire format](/book/reference/wire-format/#foreign-callers)
---

## ADR: foreign-caller-actors
title: Foreign callers are declared actors with their own `call` scheme
summary: A non-Bynk caller is a `Foreign` actor the callee declares; it signs as the actor under a deploy-minted secret, resolves to that actor and never to `Caller`, and `bynk deploy` prints the hand-off

**Context.** The interop track's D2 (`design/tracks/interop.md` §8, §11 Q3,
Q4, Q5) settled how code that is not Bynk calls a context. ADR 0485
(I2) proves every caller by a per-edge signature, so
the mechanism exists. What a handler needs is to tell a foreign caller from a
Bynk context in the type of its `by` binder. `CallerId` must stay `String` and
`Caller` must keep its meaning, because both are Foundations-layer
(`bynk-1.0-definition.md` Gate 1).

**Decision.**

- **A new actor scheme, `Foreign`,** admissible on `call` only:
  `actor Ledger { auth = Foreign(secret = "LEDGER_CALL_KEY", worker = "ledger-svc") }`.
  - The `secret` names the env binding the callee verifies under. It may not
    begin with an edge key's prefix (`bynk.actor.foreign_reserved_secret`).
  - The optional `worker` names the foreign Worker, for the hand-off below.
  - A `Foreign` actor takes no `identity` (`bynk.actor.foreign_identity_unsupported`):
    the actor is the identity.
  - It needs no new declaration form. Actors already carry schemes, secrets and
    `by` sums with a `match` on the resolved actor.
- **The wire name is the actor's name.** A foreign caller signs exactly as a
  Bynk caller does, sending its actor's name as `X-Bynk-Caller`, under the
  declared secret. The context's entry holds a table of the foreign callers its
  handlers admit (actor name → secret) and passes it to `__verifyCall`. A caller
  named in the table is verified under its secret, anyone else under an edge key.
- **The invariant that keeps the two apart.** A foreign actor's name is never an
  edge name, and both sides enforce it:
  - The checker refuses a `Foreign` actor whose name could be a context's
    (`bynk.actor.foreign_name_is_context_name`).
  - The runtime's `edgeSuffix` refuses any name with a capital letter.
  - The table is read with `Object.hasOwn`.

  So no signature under a foreign secret can pass as a Bynk context, and no edge
  key can pass as a foreign caller. Tests pin both halves.
- **Admission is per handler, and published.** Authentication says who called.
  The service's handler then says whom it admits:
  - a Bynk context by `Caller` (the default with no `by`), or by a local
    `Internal` actor;
  - a foreign caller only by its own `Foreign` actor, never by `Caller`.

  Anyone else gets a bare `403`, after the service lookup and before the
  contract check. It is published as wire-format check 6 (`NotAdmitted`). A
  context with no `Foreign` actor emits neither the table nor the check, so its
  output is unchanged.
- **Sums on `call`.** `by who: Caller | Legacy | Ledger` binds the resolved
  actor:
  - `Caller` carries the calling context's name; a foreign caller is its tag
    alone.
  - The body `match`es it, through the same `ActorSum` typing an HTTP sum uses.
  - `Foreign` peers are exempt from the one-peer-per-scheme rule, since each is
    told apart by name. No two may share a secret
    (`bynk.actor.foreign_duplicate_secret`).
  - `caller_binder_for` no longer resolves for a sum, whose binder is the
    resolved actor, not a `CallerId`.
- **The secret's hand-off (§11 Q4).** The compiler lists each context's foreign
  callers in `bynk-secrets.json` as `foreign` (`actor`, `secret`, `worker`).
  This is additive within manifest version 2: an absent list means none.
  - `bynk deploy` lists each secret as `managed` on that context.
  - A value the user supplies is set, and nothing is printed.
  - Otherwise a held one is kept, unless `--force`.
  - Otherwise one is minted and set. Once the context is deployed, the deploy
    prints a ready-to-run `printf '%s' '<value>' | npx wrangler secret put
    BYNK_CALL_TO_<CALLEE> --name <worker>` for the foreign Worker's operator.
    That is the one place a value is ever shown.
  - A run that stops after minting prints the hand-off anyway, saying it may
    not be set.
  - `bynk deploy` never writes to a Worker it does not own. A `--context` run
    may mint these, since only the callee's end is ours.
- **Off the deployed path.** `bynk dev` passes each foreign caller's secret as
  the local key. The integration harness gives every participant its foreign
  callers' secrets. A system-tier case that calls `by <ForeignActor>` signs as
  that actor (the consumed service records the foreign callers it admits, which
  are not part of its contract). At the unit tier, a call-site principal builds
  the sum's `who`. On the bundle target every caller is a context: a sum
  resolves to `Caller`, and a handler admitting only foreign callers throws.

**Consequences.**

- "A Bynk context" and "the foreign caller `Ledger`" are distinct in the type
  of `c`. `CallerId`, `Caller` and every Foundations program are unchanged; the
  scheme is additive.
- Granting a Service Binding to a non-Bynk Worker no longer grants any
  context's identity. That Worker can call only handlers that name its actor,
  and only once its operator holds the secret.
- A foreign caller's replay window and body cap are the signature's, from ADR
  0485.
- A lost minted value cannot be recovered. `bynk deploy --force` mints and
  prints a new one, and the foreign Worker must take it.
- A generated TypeScript client (D3, #1970) and recording foreign callers in
  `bynk-contracts.json` and the ledger are the track's I4.
