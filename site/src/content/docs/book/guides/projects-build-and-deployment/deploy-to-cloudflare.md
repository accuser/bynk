---
title: Deploy to Cloudflare
---

Deploy a single-context Bynk project to Cloudflare Workers with one command:

```sh
bynk deploy
```

The first deploy checks Node and Wrangler, compiles the project, shows what it
will create, checks your Wrangler authentication, asks for confirmation, then
creates the required KV namespace and deploys the Worker. Authenticate first
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

## Provisioning state

The first deploy writes `bynk.deploy.lock` beside `bynk.toml`. Commit it. It
records the Cloudflare-generated KV namespace id, not a secret, so every
developer and CI job deploys to the same namespace. It is intentionally not
written into the generated `wrangler.toml`, because each build replaces that
file.

CI can deploy a project with a recorded namespace. It does not bootstrap a new
one: run the first deploy locally, commit `bynk.deploy.lock`, and then let CI
push subsequent builds. This avoids creating a namespace whose identity cannot
be committed back to the project.

After a first deploy, `bynk dev -- --remote` reads the same lock file to fill
the KV binding. Normal local `bynk dev` remains entirely local and needs no
Cloudflare account.
