# Compile and target Cloudflare Workers

**Goal:** understand the two emission targets and build a deployable Worker.

`karnc compile` takes a `--target`:

| Target | Flag | What it emits | Cross-context calls |
|---|---|---|---|
| Bundle | `--target bundle` (default) | A flat TypeScript tree mirroring your source | Direct in-process calls |
| Workers | `--target workers` | One Cloudflare Worker per context | JSON calls over Service Bindings, validated at the boundary |

## Bundle (default)

```sh
karnc compile . --output out
```

Each source unit becomes a `.ts` file, and contexts call each other directly.
Use this for a single deployable unit or for running the output yourself.

## Workers

```sh
karnc compile . --output out --target workers
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

> An `from http` service only produces a runnable Worker on the `workers` target.
> A stateful agent compiles to a Durable Object there; on `bundle` the same agent
> uses an in-process state registry instead.

## Related

- Tutorial: [Build a small HTTP service](../../tutorials/02-http-service.md).
- [Consume another context's services with `consumes`](../program-structure/consume-services.md).
- Reference: [emission](../../reference/emission.md).
