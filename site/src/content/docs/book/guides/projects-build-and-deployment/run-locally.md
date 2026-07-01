---
title: "Run your project locally with `bynk dev`"
---
**Goal:** build your project and serve it on a local URL in one step — no
compile flags to remember, no `cd` into a generated directory, no Cloudflare
account.

```sh
bynk dev          # from anywhere inside the project
```

That's the whole thing. `bynk dev` finds your project root (the nearest
`bynk.toml`), compiles it to Workers, picks the worker to serve, and runs
[`wrangler dev`](https://developers.cloudflare.com/workers/wrangler/) for you.
Your service comes up on `http://localhost:8787`.

> **Understand — local dev needs no provisioning.** `wrangler dev` runs in
> *local mode* (Miniflare), which simulates KV, Durable Objects, and queues
> keyed by **binding name**. The `id = "<KV_NAMESPACE_ID>"` placeholders in the
> generated `wrangler.toml` are only read when you deploy for real — local mode
> ignores them. So a KV- or agent-backed project runs locally against the
> generated config untouched; there is no namespace to create first.

## What it does

`bynk dev` collapses the manual recipe (compile → `cd` → `wrangler dev`) into
one command. In order, it:

1. **Locates the project** — walks up for `bynk.toml` and reads your `[paths]
   src`, so you can run it from any subdirectory.
2. **Pre-flights** — checks that `bynkc`, Node, and `wrangler` are usable, with
   the same report (and fix-it lines) as [`bynk
   doctor`](/docs/editor-and-tooling/doctor/). A missing tool fails here, before
   anything is built.
3. **Compiles** — runs `bynkc compile … --target workers` into a managed build
   directory (see [The build directory](#the-build-directory) below).
4. **Selects the worker** — one context is served automatically; see
   [Multi-context projects](#multi-context-projects).
5. **Serves** — runs `wrangler dev` from inside the worker directory.

`bynkc` type-checks as part of compiling, so a type error stops you here with
the usual diagnostics — there is no separate `check` step to run.

## The build directory

`bynk dev` compiles into a driver-managed **`.bynk/dev/`** under your project
root — the same relationship `cargo`'s `target/` has to your source. It is
created and gitignored automatically (a `.bynk/.gitignore` containing `*` is
written on first build), so a `dev` run never dirties `git status` and you never
edit your own ignore file. Your own `bynkc compile --output out` builds, if you
keep any, are left alone — `out/` stays yours.

## Multi-context projects

A project with several contexts compiles to several workers, and `wrangler dev`
serves one at a time. `bynk dev` picks for you when there's no ambiguity:

- **One context** → served automatically.
- **Several contexts** → `bynk dev` lists them and asks you to choose:

  ```sh
  bynk dev --context payments
  ```

`--context` accepts the context name in either form (`commerce.payments` or its
worker-directory form `commerce-payments`).

> Serving several service-bound workers at once — with live cross-context calls
> between them — is not yet supported locally; `bynk dev` runs one context.

## Passing options through to wrangler

`bynk dev` owns one flag of its own (`--context`) and forwards everything after
`--` to `wrangler dev` verbatim, so it stays stable as wrangler evolves:

```sh
bynk dev -- --port 8788                       # serve on a different port
bynk dev -- --var AUTH_JWT_SECRET:dev-secret  # supply a local secret
bynk dev -- --persist-to .wrangler-state      # control where local state lives
```

If your service reads secrets (a `Bearer` actor's `AUTH_JWT_SECRET`, a webhook
`WEBHOOK_SECRET`, …), pass them with `-- --var KEY:VALUE` for local runs — you
don't need real Cloudflare secrets to develop.

> Local KV / Durable Object state persists under `.wrangler/` between runs.
> That's usually what you want; clear that directory (or point `--persist-to`
> elsewhere) for a clean slate.

## Debugging the worker (`--inspect`)

`bynk dev --inspect` serves with the V8 inspector enabled, so you can attach a
JavaScript debugger and set breakpoints **in your `.bynk` source**:

```sh
bynk dev --inspect                 # inspector on port 9229
bynk dev --inspect --inspect-port 9300
```

It prints an inspector URL on start. Attach any CDP client — VS Code's built-in
JavaScript debugger, Chrome DevTools — and breakpoints set in `.bynk` bind and
pause on real requests: the compiler emits source maps (since v0.68, per-statement
in handler bodies since v0.70), and `wrangler`/esbuild composes them into the
worker bundle, so the debugger resolves the running code back to your `.bynk`
lines.

> One wrinkle: `wrangler`'s inspector requires an `Origin` header on the
> WebSocket connection. VS Code's debugger sends one automatically; a hand-rolled
> CDP client must set it (`Origin: http://localhost`), or the connection is
> rejected with `400 Bad Request`.

## When `wrangler` isn't installed

`bynk dev` resolves `wrangler` the same way `doctor` does: a project-local
`node_modules/.bin/wrangler` wins, then a global install, then `npx`. If it can
only be reached through `npx`, `bynk dev` says so — `npx` *downloads* wrangler on
first use, so it's a one-time pause, not a missing tool. Run [`bynk doctor
--only deploy`](/docs/editor-and-tooling/doctor/) to see exactly what you have.

## Deploying

`bynk dev` is for local development only and provisions nothing. Creating real
KV namespaces and deploying to Cloudflare is a separate step — see [Target
Cloudflare Workers](/book/guides/projects-build-and-deployment/cloudflare-workers/) for the manual `wrangler deploy`
flow.

## Related

- [Target Cloudflare Workers](/book/guides/projects-build-and-deployment/cloudflare-workers/) — the two emission targets
  and the manual recipe `bynk dev` runs for you.
- [Check your environment with `bynk doctor`](/docs/editor-and-tooling/doctor/) —
  the same capability check `bynk dev` pre-flights.
- Reference: [the `bynk` driver CLI](/docs/bynk-cli/).
