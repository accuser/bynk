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
| An `actor` with `auth = Bearer/Signature` | Sets that secret, and refuses to deploy without it |

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

## Secrets

`bynk deploy` sets your secrets before it pushes, and forgets them. Values move
from wherever you keep them straight to `wrangler secret put` — they are never
written to `bynk.deploy.lock`, to generated config, or to the plan.

Every line of the plan is marked with how Bynk came to know the name:

```
secret set AUTH_JWT_SECRET (declared)
secret set STRIPE_KEY (read)
secret set LEGACY_TOKEN (supplied)
deploy api
```

**Declared** — an actor's `auth` secret:

```bynk
actor User { auth = Bearer(secret = "AUTH_JWT_SECRET"), identity = UserId }
```

The name is right there in your source, so `bynk deploy` reads it out of your
compiled project. You supply the value; you never name it.

**Read** — a `bynk.Secrets` lookup:

```bynk
let key <- Secrets.get("STRIPE_KEY")
```

Bynk sees this name too, as long as you pass a literal.

**Supplied** — anything you name yourself with `--secrets-file` or `--secret`,
that Bynk didn't find on its own.

The difference between `declared` and `read` is **not** how much Bynk knows about
them — it saw both names in your source. It's what happens when the value is
missing, and that follows from their types:

| | If you don't supply a value |
|---|---|
| `declared` — an actor's `auth` secret | **The deploy fails**, naming it. Unset, the Worker 401s every request. |
| `read` — a `Secrets.get("…")` name | **A warning.** `get` returns `Option`, so your code already handles `None` — Bynk won't refuse to ship a program that's correct. |

> **Understand — a `read` line is a warning, not a promise.** `Secrets.get` takes
> an ordinary `String`, so `Secrets.get(pickAName())` is legal Bynk and no
> compiler pass can say what it will ask for. When Bynk sees one it tells you at
> compile time:
>
> ```
> warning: bynk.secrets.computed_name
>   `Secrets.get` is called with a computed name, so `bynk deploy` cannot know
>   which secret this context reads
> ```
>
> …and the plan admits it:
>
> ```
> secrets incomplete api (computes at least one name)
> ```
>
> **That line is the important one.** Without it, a short `read` list would be
> the most dangerous thing Bynk could show you: a list that's usually right gets
> trusted, and the one computed name it misses becomes a `None` in production
> that nobody thought to check for. So Bynk lists what it saw, and says plainly
> when that isn't everything.
>
> It would have been easy to forbid computed names instead — every `Secrets.get`
> in Bynk's own tree passes a literal, so the rule would cost nothing today. It
> isn't done because choosing a secret at runtime is a reasonable thing to want,
> and a language shouldn't take that away to make a deploy tool's list tidier.

### Supplying values

Names and values are separate questions. Names come from your actors (declared),
your `--secrets-file`'s keys, and each `--secret NAME`. Values are looked up per
name:

1. `--secrets-file` — a dotenv-style `NAME=value` file. Don't commit it.
2. The environment — checked for names Bynk already knows.
3. A prompt, if you're at a terminal.

```sh
bynk deploy --secrets-file .secrets.env          # names and values
STRIPE_KEY=sk_live_… bynk deploy --secret STRIPE_KEY   # value from the env
```

The environment is a **value** source only. `bynk deploy` never scans it for
names — deciding "these look like secrets" over your whole shell and uploading
them is not something a deploy tool should do to you.

In CI, where there is no terminal to prompt at, a missing value is a hard error
naming the secret. It is never a blank.

### Re-deploying

Cloudflare doesn't give secret values back, so `bynk deploy` can't compare yours
to what's up there — it can only see which names are **set**. So the default is
set-if-absent:

```
bynk: secret `AUTH_JWT_SECRET` is already set on `api`, skipping — use --force to overwrite
```

Pass `--force` when you've rotated a value. The default skips deliberately:
setting every secret on every deploy would cut a fresh Cloudflare secret version
each time, for nothing.

One consequence worth knowing: `--dry-run` never authenticates, which is what
lets you plan offline — so a plan says `set` for everything, and the skip shows
up when you actually run.

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

## Environments

By default `bynk deploy` ships to a single, unqualified environment. Pass
`--env` to ship the same project to more than one — `staging` and `production`
provision, record, and reconcile entirely independently, even on the same
Cloudflare account:

```sh
bynk deploy --env staging
bynk deploy --env production
```

Omit `--env` and nothing changes from before — it is the same as
`--env default`.

> **Understand — why `bynk deploy` writes more than `--env` to Wrangler.**
> Cloudflare does not carry your bindings into a named environment
> automatically: KV namespaces, queues, and Service Bindings all have to be
> declared again under that environment, or Wrangler deploys with none of
> them. `bynk deploy` does this for you — it writes an environment-scoped
> section into the generated config, alongside the plain one, every time you
> deploy to something other than the default. You never write it yourself,
> and it never survives a rebuild you didn't ask `bynk deploy` to make.

Queue names and Service Binding targets pick up a `-<env>` suffix under a
non-default environment, so `staging` and `production` never collide on the
same physical queue or bind to each other's Workers:

```
queue create job-intake-staging
deploy commerce-orders-staging
```

KV needs no such suffix — its identity is the namespace id Cloudflare hands
back, already kept apart per environment in `bynk.deploy.lock`.

`--env` is the driver's own flag; putting `--env` or `--environment` after
`--` as well is rejected rather than silently resolved one way or the other:

```sh
bynk deploy --env staging -- --env production
# bynk: `--env staging` conflicts with `--env` after `--` — pass one or the other, not both
```

## Provisioning state

The first deploy writes `bynk.deploy.lock` beside `bynk.toml`. Commit it. It is
intentionally not written into the generated `wrangler.toml`, because each build
replaces that file. It holds no secrets. Every environment gets its own section
of the file, so `staging` and `production` never share a KV id, a deployed-state
flag, or a queue record. Per environment, it records three different things, and
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
the KV binding — pass the matching `--env` if you deployed to a named one
(`bynk dev --env staging -- --remote`). Normal local `bynk dev` remains
entirely local and needs no Cloudflare account.

## Orphans

Delete a context, or the last `on queue` handler that used it, and the ledger
still remembers what it provisioned — `bynk deploy` says so before doing
anything else:

```
orphan kv payment
orphan worker payment
orphan queue old-jobs
deploy commerce-orders
```

A removed context with a KV namespace shows up as **two** lines — the
namespace and the Worker are separate resources, tracked separately, and
reported separately. This is a plain diff against your current source; it
never calls Cloudflare, so it works under `--dry-run` too.

Pass `--prune` to delete what's reported — KV namespaces and queues only,
never a Worker:

```sh
bynk deploy --prune
```

```
bynk: will delete KV namespace for `payment`
bynk: will delete queue `old-jobs`
Delete 2 resource(s)? [y/N]
```

That confirmation is separate from the one `--yes` already skips for
creation — a script that only meant to authorise provisioning must not also,
silently, authorise deletion. Pass `--yes` **and** `--prune` together to
prune non-interactively.

`--prune` is safe to re-run: deleting something already gone (a resource
someone else removed between your plan and your prune) is treated the same
as deleting it now — the ledger entry is stripped either way, so a
half-finished prune never gets stuck re-reporting the same orphan forever.

An orphaned Worker is reported but never deleted — `wrangler delete` also
removes routes, custom domains, and cron triggers, a larger blast radius than
a namespace or a queue. Clean one up by hand with `wrangler delete` if you're
sure it's unused.

Orphans are project-wide, not scoped to `--context`: `bynk deploy --context
orders --prune` can delete a queue orphaned by a *different*, unrelated
context — the report (and the confirmation naming exactly what will be
deleted) is what keeps this visible rather than surprising.

Separately: a KV namespace deleted outside Bynk (in the Cloudflare
dashboard, say) is noticed the next time you deploy and re-provisioned
automatically — the same self-healing a deleted queue already had.
