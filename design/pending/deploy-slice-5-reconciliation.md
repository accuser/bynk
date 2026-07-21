---
level: patch
changelog: "`bynk deploy` reports orphaned resources and prunes them with `--prune`; a deleted KV namespace self-heals like a deleted queue already does"
---

## ADR: deploy-reconciliation
title: Reconciliation maturity — per-kind orphan reporting, once-per-run KV drift, and --prune scoped to KV/queues
summary: The ledger-vs-source diff is reported offline before any mutation; KV drift is checked once per deploy, not once per context; --prune deletes KV namespaces and queues idempotently but never a whole Worker

**Context.** `bynk deploy` never told a user about a resource the ledger
remembered that the current source no longer declared — a deleted context, a
removed `on queue` handler — it silently kept recreating what was still there
and said nothing about what wasn't. Separately, KV was the one resource kind
that did not self-heal: queues reconcile by attempting the create on every
run and reading "already exists" as success (ADR 0194 D2), but
`recorded_kv` trusted a recorded id unconditionally, so a namespace deleted
out-of-band got injected dead into the generated config and `wrangler
deploy` failed with Cloudflare's own opaque complaint rather than the driver
noticing and re-provisioning.

**Decision.**

**D1 — orphan detection is a pure, offline diff, per resource kind,
independently.** `find_orphans` compares the ledger's `kv`/`workers`/`queues`
sets for one environment against the current build's full declared resource
set (`resources`, already read over every worker `dev::discover_workers`
found, regardless of `--context`) — no live Cloudflare call, so `--dry-run`'s
"never authenticates" promise is unaffected. `kv` and `workers` are both
keyed by worker name and checked independently: a context removed from
source that had KV is reported as **two** orphans, one per map, not merged
into a single line.

**D2 — KV drift is checked once per `bynk deploy` run, not once per
context.** `wrangler kv namespace list` is an account-wide list; fetching it
per context would pay one identical round-trip per KV-bearing context on
every redeploy. `run()` fetches it once, only when at least one selected
context both needs KV and already has a recorded id worth checking (a first
deploy has nothing to check drift against), and threads the result into
`deploy_one`. A recorded id absent from the live set is treated exactly as
unrecorded: re-provision, record the new id. Still provision-time, not
plan-time, so the fetch only ever runs on a real `bynk deploy`, past the
confirm gate.

**D3 — `--prune` deletes KV namespaces and queues only, never a whole
Worker.** `wrangler delete` removes routes, custom domains, and cron
triggers along with the script — a materially larger blast radius than a
namespace or a queue. An orphaned Worker is reported, exactly as KV/queues
are; pruning one is explicitly out of scope. For a removed context with KV,
`--prune` deletes the `kv` orphan and leaves the paired `workers` orphan
report-only — the two are pruned independently, matching D1's independent
reporting.

**D4 — `--prune` needs its own confirmation, separate from `--yes`'s
creation gate.** `confirm_prune` lists every resource about to be deleted and
asks once for the whole batch; `--yes` alone does not imply pruning is
authorised — a script that only meant to authorise *creation* must not
accidentally also authorise deletion. Non-interactive pruning requires
`--yes` and `--prune` together, the same shape `confirm` already uses for
creation.

**D5 — a not-found response from either delete verb counts as success, and
the ledger entry is stripped regardless.** Confirmed empirically against a
real Cloudflare account: neither `wrangler queues delete <name>` nor
`wrangler kv namespace delete --namespace-id <id> --skip-confirmation`
treats deleting an already-gone resource as silent success — both return a
hard error (`Queue "<name>" does not exist...`; `namespace not found [code:
10013]`). Without this driver-side match, a crash between a successful
Cloudflare delete and the ledger write — or a resource deleted out-of-band
between the plan and the prune — would wedge every subsequent run: the same
orphan re-reported, the same delete re-attempted, now rejected as not-found
and (absent this rule) read as failure, so the ledger entry would never be
stripped. `prune_orphans` matches both error shapes (`kv_namespace_already_deleted`,
`queue_already_deleted`) and treats them identically to a clean delete —
mirroring `create_queue`'s existing "already exists ⇒ success" idempotency,
for the inverse (delete) direction.

Also confirmed empirically: `wrangler queues delete` has no confirmation
prompt or force flag of its own (`--help` shows only global flags) — nothing
for the driver to skip. `wrangler kv namespace delete --skip-confirmation`
does, and is used so wrangler's own prompt (redundant with D4's batch
confirmation) never blocks a non-interactive `--prune --yes`.

**Consequences.** A namespace deleted out-of-band now self-heals on the next
deploy, exactly as a deleted queue already did — closing the one asymmetry
between the two resource kinds. A project with a removed context sees it
named in the plan before anything is pushed, rather than the ledger silently
carrying a stale entry forever. `--prune`'s worst case is re-creatable state
(a fresh namespace, a fresh queue) — never a live endpoint disappearing out
from under production traffic, since Worker deletion stays out of scope.

- **S1 (deferred).** `--prune`-ing an orphaned Worker (`wrangler delete`) —
  named as future work, not this slice's, given the larger blast radius
  (routes, custom domains, cron triggers).
- **S2 (deferred).** A race between `--prune`'s report and its delete call —
  a concurrent deploy could re-add a resource in the window between the two.
  No distributed lock exists anywhere in this track; named rather than
  silently ignored.
- **S3.** This is the `bynk deploy` track's last slice. Retirement follows in
  its own PR per the track lifecycle (`design/tracks/README.md` §5).
