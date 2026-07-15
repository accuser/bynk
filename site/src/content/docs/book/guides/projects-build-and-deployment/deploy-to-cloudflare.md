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
creates each context's required KV namespace and deploys its Worker.
Authenticate first with `wrangler login` or `CLOUDFLARE_API_TOKEN`.

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

The first deploy writes `bynk.deploy.lock` beside `bynk.toml`. Commit it. It
records, per context, the Cloudflare-generated KV namespace id — not a secret —
so every developer and CI job deploys to the same namespaces. It also records
which contexts have been deployed, which is how `--context` knows whether the
contexts you bind to are actually live. It is intentionally not written into the
generated `wrangler.toml`, because each build replaces that file.

CI can deploy a project with a recorded namespace. It does not bootstrap a new
one: run the first deploy locally, commit `bynk.deploy.lock`, and then let CI
push subsequent builds. This avoids creating a namespace whose identity cannot
be committed back to the project.

After a first deploy, `bynk dev -- --remote` reads the same lock file to fill
the KV binding. Normal local `bynk dev` remains entirely local and needs no
Cloudflare account.
