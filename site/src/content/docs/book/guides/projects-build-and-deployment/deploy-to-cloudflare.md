---
title: Deploy to Cloudflare
---

Deploy a Bynk project to Cloudflare Workers with one command — every context,
in the right order:

```sh
bynk deploy
```

The first deploy checks Node and Wrangler, compiles the project, shows what it
will create, checks your Wrangler authentication, asks for confirmation, then
provisions each context's resources and deploys its Worker. Authenticate first
with `wrangler login` or `CLOUDFLARE_API_TOKEN`.

Use `--yes` in an automated, non-interactive invocation:

```sh
bynk deploy --yes
```

## Review a plan

No Cloudflare calls or project-state changes happen with `--dry-run` (or its
`--plan` alias):

```sh
bynk deploy --dry-run
bynk deploy --dry-run --format json
```

Arguments after `--` go straight to `wrangler deploy`, for example:

```sh
bynk deploy -- --compatibility-date 2025-01-01
```

## What gets provisioned

`bynk deploy` creates exactly what your code already commits each context to —
nothing speculative. If it isn't in the generated `wrangler.toml`, it isn't
`bynk deploy`'s to create.

| In your context | What `bynk deploy` does |
|---|---|
| `consumes bynk.cloudflare { Kv }` | Creates a KV namespace and records its id |
| An `agent` | Applies the Durable Object migration that registers its class |
| `service … from queue("n")` | Creates the queue `n` before pushing |
| An `on cron` handler | Nothing to create — the schedule rides the config |
| `consumes` another context | Nothing to create — it sets the deploy order |

A context using all three of the first kinds plans like this:

```
kv create ops-hub
queue create job-intake
migration v1 (advisory — wrangler deploy applies it)
deploy ops-hub
```

### Queues

Queues are created **by name** — the name you wrote in `from queue("n")`. Before
each deploy, `bynk deploy` asks Cloudflare whether the queue is there and creates
it only if it isn't. So a queue you deleted outside Bynk comes back on the next
deploy, and a `reuse` line never means "and if it's gone, too bad".

Queues are created *before* the Worker is pushed, and this step is doing real
work: `wrangler deploy` does **not** create a queue for you — it checks, and
fails with `Queue "n" does not exist. To create it, run: wrangler queues create n`.
Creating them is what makes a queue-consuming context deployable in one command.

### Durable Object migrations

The migration line is **advisory**, and worth understanding:

> **Understand — Cloudflare owns your migration state, not Bynk.** The migration
> is applied by `wrangler deploy` itself, from the same config it is already
> reading, and Cloudflare records which tags have been applied. `bynk.deploy.lock`
> deliberately keeps **no** record of it.
>
> The reason is that a second record could disagree with the account — the lock
> file says `v2`, but a reset left Cloudflare at `v1` — and you'd be debugging
> Bynk's memory instead of your deployment. So the plan tells you which tag the
> push will *ask for*, and never claims to know what is already applied.
>
> The trade-off is real and deliberate: `bynk deploy` cannot warn you that your
> migrations have drifted. A tool that can't tell you about drift beats one that
> invents it.

## Multi-context projects

A project with several contexts ships every one of them, in the order their
Service Bindings require. The plan shows that order before anything is touched:

```
kv create commerce-payment
deploy commerce-payment
deploy commerce-orders
order commerce-payment → commerce-orders
```

`commerce-payment` goes first because `commerce-orders` binds to it.

> **Understand — why the order is not just tidiness.** Cloudflare resolves a
> Service Binding when the Worker is **uploaded**, not when a request arrives.
> Deploying `commerce-orders` before `commerce-payment` exists doesn't merely
> leave a window where calls fail — the *upload itself is rejected*. So the
> order is a correctness requirement, and `bynk deploy` derives it from your
> `consumes` declarations rather than asking you to remember it.
>
> You never have to untangle a cycle to make this work: `consumes` cycles are
> already a compile error (`bynk.context.consumes_cycle`), so a project that
> compiles always has an order.

### Deploying one context

`--context` re-pushes a single context, which is what you want when iterating on
one service in a topology that is already live:

```sh
bynk deploy --context commerce.orders
```

It does **not** deploy that context's dependencies. If it binds to a context that
has never been deployed, `bynk deploy` says so and stops, rather than sending an
upload Cloudflare would reject:

```
bynk: `commerce-orders` binds to `commerce-payment`, which has never been
deployed — a Service Binding to a Worker that does not exist fails at upload.
  Deploy the whole project once (`bynk deploy`) to bring the topology up.
```

Bring a fresh topology up with a whole-project `bynk deploy` first; use
`--context` for iteration after that.

### When a deploy fails part-way

A multi-context deploy is **resumable, not transactional**. If the third context
fails after two succeeded, the two that landed stay deployed and recorded — there
is no rollback — and the run stops rather than pushing on into a topology that
isn't what the plan described. Fix the problem and re-run `bynk deploy`: it
re-pushes in the same order, reusing the KV namespaces it already created.

A half-deployed project is a real state, not a corrupt one, and the next plan
describes it honestly — contexts already live read `redeploy` rather than
`deploy`.

## Provisioning state

The first deploy writes `bynk.deploy.lock` beside `bynk.toml`. Commit it. It is
intentionally not written into the generated `wrangler.toml`, because each build
replaces that file. It holds no secrets. It records three different things, and
it trusts them to three different degrees:

- **KV namespace ids** — the real state. Cloudflare generates the id, and this
  file is the only place it exists, so every developer and CI job deploys to the
  same namespaces.
- **Which contexts have been deployed** — how `--context` knows whether the
  contexts you bind to are actually live.
- **Which queues have been created** — a note to itself, so the plan can say
  `create` or `reuse` without calling Cloudflare. Nothing depends on it being
  right: a queue is found by its name either way.

CI can deploy a project with a recorded namespace. It does not bootstrap a new
one: run the first deploy locally, commit `bynk.deploy.lock`, and then let CI
push subsequent builds. This avoids creating a namespace whose identity cannot
be committed back to the project.

That restriction is about ids, so it applies to KV alone. CI happily creates a
queue, because a queue's name comes from your source — the next run derives the
same name and finds the same queue, with or without the lock file.

After a first deploy, `bynk dev -- --remote` reads the same lock file to fill
the KV binding. Normal local `bynk dev` remains entirely local and needs no
Cloudflare account.
