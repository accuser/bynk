# 0194 — The deploy ledger owns the ids it mints, and defers for state another tool owns

- **Status:** Accepted (v0.171)
- **Provenance:** #600, deploy track slice 1 (spine #558). Builds on slice 0
  (#583, v0.154) and slice 2 (#601, v0.170).
- **Relates:** [[0179]] (provisioning state — the ledger this extends), [[0180]]
  (deploy plans before provisioning and is idempotent), [[0193]] (multi-context
  deploy ordering — the loop these steps drop into), [[0017]] (platform lock per
  deployment unit — what commits a context to these resources), [[0083]] (the
  driver is a thin orchestrator over wrangler).

## Context

Slice 0 provisioned exactly one resource kind: a KV namespace, whose
Cloudflare-generated id it stored and injected. Two of the three resource kinds a
single context's closure can declare were left to the manual ritual. A context
with an `agent` emits `[[durable_objects.bindings]]` and a `[[migrations]]` block;
a context with a queue-protocol service emits `[[queues.consumers]]`
(`bynk-emit/src/emitter/wrangler.rs`). `deploy` pushed both stanzas and
provisioned neither — so a deploy against an unprovisioned queue failed at the
wire, because a consumer binding whose queue does not exist fails the upload.

[[0179]] said the ledger's shape "admits the other resource kinds" and left *how*
to a later slice, foreshadowing that they would not all reconcile the same way.
This slice is where that claim is tested, because the two kinds it adds sit at
opposite ends of it: **DO migrations are the one resource whose truth `deploy`
does not own**, and **queues are the deliberately easy counterpart**. Pairing
them in one ADR keeps the hard case honest against a simple one.

## Decision

**(D1) DO-migration state is delegated to wrangler entirely; the ledger records
nothing.** Wrangler and Cloudflare already track applied migration tags
server-side. A second ledger of applied tags would be a *rival source of truth*
that silently disagrees with the account after any out-of-band change — the
ledger says `v2`; a reset left the account at `v1` — and the disagreement would
surface as a confusing deploy failure rather than as the drift it is. So
`deploy` applies migrations by **letting `wrangler deploy` do it**: the
`[[migrations]]` block already rides the config slice 0 pushes, so this is not a
step the driver adds but one it declines to duplicate.

[[0179]] left room for an *advisory last-applied tag*. It is **not recorded**.
The test is whether anything would read it, and nothing does: reconciliation for
DO does not consult the ledger, the plan's tag is read from the emitted config
(which is where the truth for "what will be asked for" lives anyway), and a
recorded tag would only invite a future reader to mistake it for state. A field
load-bearing for nothing is not free — it is a claim waiting to be believed. So
DO adds **no schema at all**, which is the strongest possible form of "the ledger
defers here".

The plan still shows the migration, because a user deploying an agent should see
that a migration rides along. It is flagged advisory in both surfaces:
`migration v1 (advisory — wrangler deploy applies it)` in `--format short`, and
`{"tag": "v1", "applied_by": "wrangler deploy"}` in `--format json`. The JSON
names an owner rather than carrying an `advisory: true` flag, because the owner
*is* the content of the advisory and a constant-true boolean says less.

**Consequence, stated plainly:** `deploy` cannot detect DO-migration drift in v1.
It trusts wrangler. This is the cost of not keeping a rival ledger, and it is the
right side of the trade: a tool that cannot tell you about drift is better than
one that tells you about drift that isn't there.

**(D2) Queues reconcile by their user-given name, create-if-absent, with no
stored id — and the ledger's queue set is authoritative for nothing.**
`from queue("n")` names the queue and Cloudflare addresses queues by that name,
so there is no minted identity to lose and nothing an id would add.

The ledger gains `environments.<env>.queues`, a set of names this project has
created, **environment-wide rather than per-worker** — a queue is an account
resource, so two contexts consuming `"jobs"` mean one queue, and a per-worker
table would imply otherwise. Its only reader is the plan, which uses it to say
`create` or `reuse` without a `wrangler queues list` call.

The provision step does **not** consult it. Every declared queue is reconciled
against **the account** on every deploy — `wrangler queues info <name>`, and a
create only where that says absent. Skipping on the ledger's word would be the
bug: a queue deleted out-of-band would stay deleted, and the push would fail
against a consumer binding with nothing behind it. Reconciling live makes the
step self-healing, and is what lets the set be honestly described as advisory — a
plan aid, not a source of truth. This is the same posture as DO, one notch weaker
than KV's stored id, and it is why `bynk.deploy.lock` is not a uniform logical→id
map ([[0179]] [DECISION B]).

The check is not optional politeness: `wrangler deploy` **will not** create a
queue its config binds. Its `ensureQueuesExist` looks the names up and throws
`Queue "n" does not exist. To create it, run: wrangler queues create n`. So a
queue-consuming context is undeployable until something creates the queue, and
that something is this step. (Dead-letter queues *are* auto-created by
Cloudflare; consumer queues are not. The asymmetry is theirs, not ours.)

The cost is one wrangler invocation per declared queue per deploy — the `info`
lookup — plus a create for each genuinely absent one, so a first deploy pays two
and a steady-state re-deploy pays one. That is accepted: deploys are infrequent,
the calls are cheap next to the push they precede, and the alternative buys speed
with the one property that makes the step worth having.

Per *deploy*, not per consuming context: provisioning runs per context, but a
queue two contexts consume is one queue, and the emitter's duplicate-consumer
check is scoped to a single context — so a shared queue is a legal project, not a
hypothetical. The run therefore tracks which names it has already attempted. That
set is per-run and deliberately not persisted: every queue is still attempted on
every fresh deploy, which is exactly the property the self-healing rests on.

**The idempotency seam is an exit code, not a message.** `wrangler queues create`
has no `--if-not-exists` (verified against wrangler 4.103). An earlier draft of
this decision concluded that matching the create's "already exists" complaint was
therefore the only seam available, dismissing a live pre-check as "the same race
one call later". That reasoning was wrong — not in its facts but in what it
inferred from them. A pre-check does not remove the race, but the race is a
*concurrent deploy*; the common case is a queue that is simply there, and a
pre-check answers that case without reading prose at all. Dismissing the check
for not being a total solution left an unverifiable string load-bearing for every
re-deploy of every queue project.

So `deploy` asks `wrangler queues info <name>` — a lookup by the same name the
config binds, answering with an **exit code** — and creates only where that says
absent. This is how Cloudflare's own deploy path reconciles queues (`getQueue`),
which is the better argument for it than any of ours.

The message match survives, narrowed to what it can honestly carry: the
race-loser's path, where a concurrent deploy created the queue between the check
and the create. It reads **both** output streams, wrangler being inconsistent
about which carries a complaint.

This remains **the slice's one claim about another tool's prose that no test can
pin**, and it cannot be validated short of a live account: the wording is
Cloudflare's API text, which wrangler renders verbatim as
`{message} [code: {code}]` and has no queue-specific handling for; Cloudflare's
published Queues error codes cover the data plane only, and document no duplicate
-queue code. The honest response to an assertion that cannot be tested is to make
it carry less. Being wrong about it now costs a spurious failure on a rare race
that a re-run fixes, rather than a hard failure on every re-deploy. A non-zero
exit reading as "absent" is safe in the same way: an auth or network failure sends
us to the create, which fails too and surfaces wrangler's real complaint instead
of a diagnosis of our own.

**(D3) Queue creation runs in `provision`, before the push; migration application
stays inside `push`.** The pipeline is unchanged —
`plan → provision(KV, queues) → wire → push(deploy, which applies migrations) →
record` — and no phase is added. The rule that places each step is not taste but
Cloudflare's: a `[[queues.consumers]]` binding fails the upload if its queue does
not exist, so the queue must precede the push; a migration is applied *by* the
upload, so it cannot precede it. Slice 2's per-context loop ([[0193]]) absorbs
both without touching the ordering, exactly as it anticipated.

**(D4) The `queues` set is additive over `version = 1`; the version does not
move.** [[0179]] [DECISION B] already declared the format admits the other kinds,
so a `serde`-defaulted field needs no bump and no migration: a KV-only slice-0
ledger and a slice-2 ledger both load unchanged, with the set empty. It is
skipped on serialise when empty, so a project with no queues never grows a line
for a slice it does not use.

**(D5) The CI provisioning refusal stays KV's alone.** [[0179]] made CI refuse an
*unrecorded* KV namespace: a job that mints an id and cannot commit the result
leaves an orphan nobody can find again. That reasoning does not reach queues. A
queue's name comes from the source, so a CI-created queue loses nothing — the next
run derives the same name and finds the same queue. The asymmetry is not an
oversight in either direction; it is the D1/D2 principle applied to the CI rule:
**the ledger owns the ids it alone mints, and the refusal protects exactly
those.**

## Consequences

`bynk deploy` on a single context now provisions **every** v1 resource that
context's closure locks it to — KV, DO migrations, and queues — so the manual
ritual is retired for the whole of one context's surface, and slice 2's loop
extends that to every context for free.

The ledger's boundary is now fixed by decision rather than discovered per
resource: **it owns the ids it mints (KV), it records names as a planning aid it
never trusts (queues), and it records nothing at all where another tool already
owns the state (DO migrations).** A future resource kind is placed by asking which
of the three it is, and secrets (slice 3, #602) already have their answer —
presence-only, no id, no value (track §4.4) — which is a fourth point on the same
scale, not a new question.

Deliberately still open: environments beyond `default`, secrets (slice 3, #602),
and drift detection for anything (track §4.5, Q6). DO *schema* evolution —
`renamed_classes` / `deleted_classes` — is not deploy's: v1 emits only
`new_classes`, and durable-state migration is its own track (#539). Slice 1
applies whatever the emitter wrote, no more.
