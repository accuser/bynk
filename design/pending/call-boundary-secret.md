---
level: minor
changelog: Every cross-context call and event delivery now carries the deployment's call secret `BYNK_CALL_SECRET` in an `X-Bynk-Call-Secret` header, and a Worker refuses a `/_bynk/call/` or `/_bynk/event/` request without it (`401`, before anything else), because those paths answered on the Worker's public `workers.dev` address and anyone could call an `on call` service claiming any caller. `bynk deploy` mints the secret and sets it on every context, `bynk dev` and `bynk test` use a fixed local value. **Breaking for deployed projects:** redeploy the whole project once (`bynk deploy`, not `--context`), since until every Worker holds the secret, calls between old and new Workers fail
---

## ADR: deployment-call-secret
title: A shared per-deployment secret admits every internal request, amending ADR 0092
summary: `BYNK_CALL_SECRET` is minted by `bynk deploy` and checked first on every `/_bynk/call/` and `/_bynk/event/` request

**Context.** ADR 0092 trusts the `X-Bynk-Caller` header because
`/_bynk/call/` "is platform-dispatched and not externally routable", so only
the compiler writes it and every context in a deployment is one trust domain.
That premise is false. A Worker has one `fetch` entry, which dispatches
`/_bynk/call/` and `/_bynk/event/` on the path alone, on every route the
Worker serves. The generated `wrangler.toml` sets neither `routes` nor
`workers_dev`. Wrangler 4.120.0 then turns `workers.dev` on
(`getSubdomainValues`: `const defaultWorkersDev = routes.length === 0`), so
every deployed context has a public address. Anyone could call any `on call`
service from the internet, claim any context as the caller, and inject events
into any subscriber. The contract hash is no credential: it is in
`bynk-contracts.json`, and a 409 echoes it back. The interop track
(`design/tracks/interop.md` §3.3, §11 Q1, §12) settled a fix outside the
track: its D2 option (d), a shared per-deployment secret, until per-edge
signatures (its slice I2) replace it.

**Decision.** Every Worker in a deployment holds one secret, bound as
`BYNK_CALL_SECRET`. Senders stamp it in an `X-Bynk-Call-Secret` header:
`callService` and the events fan-out's `deliverEvent`, reading it from their
own `env`. Each throws, naming the binding, rather than send without it. On
the receiving side, the first statement of the `/_bynk/call/` and
`/_bynk/event/` dispatches checks it. The check comes before the service
lookup, the contract check and the body read. It runs for every handler,
whether or not it binds `by c: Caller` (track §11 Q5). A failure answers `401`
with the body `Unauthorized`, so a sender without the secret cannot tell
services apart (no 404 against 409), never sees a contract hash, and cannot
write the skew log line. The comparison runs over SHA-256 digests of both
values with no early exit, so it takes the same time whatever arrives.
`crypto.subtle.timingSafeEqual` would do on workerd, but the generated test
harness runs on Node. A Worker whose own secret is unset or empty refuses
every internal request (fail closed) and logs that once per isolate. The
secret is read from `env` like an actor's auth secret, so no `Env` interface
or `wrangler.toml` names it.

`bynk deploy` provisions it as a `managed` secret, listed on every context in
the plan. Cloudflare never returns a secret's value, and the deploy ledger is
committed, so it cannot hold one (ADR 0195 D1 stands: the ledger records no
secret). So one rule, decided once per run over every Worker in it:

- If the user supplies the value (`--secrets-file`, or `BYNK_CALL_SECRET` in
  the environment), it is set where absent, and everywhere under `--force`.
- If every Worker in the run already holds one, and `--force` is off, it is
  kept. A Worker whose `secret list` could not be read counts as holding it,
  as `tolerate_unresolvable` does for other secrets.
- Otherwise a whole-project run mints 32 random bytes (hex) and sets the value
  on every Worker, replacing any it had. That covers a first deploy, a project
  that predates the secret, a context added to a live deployment, and
  `--force` (a rotation).
- A `--context` run that needs a value it has not been given is refused
  before anything is pushed, because the Workers outside the run would keep
  the old one.

The value is never printed or written anywhere. A user who wants to deploy a
single new context later supplies their own value instead.

`bynk dev` passes a fixed local value (`bynk-local-call-secret`) to every
`wrangler dev` as `--var BYNK_CALL_SECRET:…`. It is fixed so that two `bynk
dev --context` sessions still admit each other's calls, and it is passed on
the command line so it never lands in a file a deploy could read. The
generated test harnesses put the same value in every participant's `env` and
in the root `env` that test cases call through.

We considered three alternatives. `workers_dev = false` alone was rejected: it
does not cover a context that serves `from http` routes, and its public
address must stay. A gitignored local file holding the minted value was
rejected because CI deploys from a fresh checkout would not have it. Printing
the value once was rejected because it lands in CI logs.

**Consequences.** ADR 0092's trust paragraph now reads "one trust domain per
deployment secret". The secret proves a request came from inside the
deployment, not which context sent it: any holder can still claim any
`X-Bynk-Caller`, which is the gap the interop track's per-edge signatures (I2)
close. A Worker pushed with plain `wrangler deploy` needs the secret set by
hand. A Worker served by plain `wrangler dev` refuses internal requests until
it is given one. A rotation leaves a short window in which calls between a
Worker that already holds the new value and one that does not yet are
refused; there is no two-key overlap. Already-deployed projects must redeploy
the whole project once. `workers_dev = false` for contexts with no public
surface remains worth doing as defence in depth, but it is left to its own
change, since deciding "no public surface" means weighing `from http`,
`from websocket` and static assets.
