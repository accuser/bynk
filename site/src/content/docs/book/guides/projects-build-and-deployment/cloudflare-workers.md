---
title: Compile and target Cloudflare Workers
---
**Goal:** understand the two emission targets and build a deployable Worker.

`bynkc compile` takes a `--target`:

| Target | Flag | What it emits | Cross-context calls |
|---|---|---|---|
| Bundle | `--target bundle` (default) | A flat TypeScript tree mirroring your source | Direct in-process calls |
| Workers | `--target workers` | One Cloudflare Worker per context | JSON calls over Service Bindings, validated at the boundary |

## Bundle (default)

```sh
bynkc compile . --output out
```

Each source unit becomes a `.ts` file, and contexts call each other directly.
Use this for a single deployable unit or for running the output yourself.

## Workers

```sh
bynkc compile . --output out --target workers
```

Each context becomes a directory under `out/workers/<context>/`:

```text
out/workers/notes/
├── handlers.ts     # your handler logic
├── index.ts        # the Worker entry point + router
├── compose.ts      # dependency wiring
└── wrangler.toml    # Cloudflare config
```

The emitted directory is a standard Worker. Run it locally with
[Wrangler](https://developers.cloudflare.com/workers/wrangler/):

```sh
cd out/workers/notes
npx wrangler dev
```

> **Or, in one step:** [`bynk dev`](/book/guides/projects-build-and-deployment/run-locally/) does this whole recipe for
> you — compile, pick the worker, and `wrangler dev` — from anywhere inside the
> project, with nothing to provision. The manual flow above is what it runs
> under the hood.

> An `from http` service only produces a runnable Worker on the `workers` target.
> A stateful agent compiles to a Durable Object there; on `bundle` the same agent
> uses an in-process state registry instead.

## Related

- Tutorial: [Build a small HTTP service](/book/tutorials/02-http-service/).
- [Consume another context's services with `consumes`](/book/guides/program-structure/consume-services/).
- Reference: [emission](/docs/emission/).
