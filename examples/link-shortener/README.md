# Link shortener

Create short links and resolve them — backed by **Workers KV**, with codes minted
from a random UUID and expiring on their own.

What it shows:

- **KV persistence with a TTL** — `consumes bynk.cloudflare { Kv }` and
  `Kv.putTtl(key, value, 86400)`. The mapping disappears after a day with no
  sweep to write.
- **A first-party capability** — `Random.uuid()` mints the code; it is injected
  by the platform, not constructed by you.
- **Refined types at the boundary** — `Slug` and `Url` carry their constraints,
  so an invalid code can never be stored and an over-long URL is rejected with
  `400` before the handler runs.

> Note: `HttpResult` has no redirect variant yet, so `GET /links/:code` returns
> the target URL as JSON rather than issuing a `302`.

## Layout

```text
link-shortener/
├── bynk.toml
├── src/
│   ├── codes.bynk     # commons codes — Slug + Url + key helper
│   └── links.bynk     # context links — the HTTP service
└── tests/
    └── codes.bynk     # unit tests for the boundary + key helper
```

## Check and test

```sh
bynkc check src
bynkc test .
```

```text
codes:
  ✓ keyOf namespaces a slug
  ✓ a slug must be 6–12 characters
  ✓ a url must be non-empty and bounded

3 passed, 0 failed.
```

The `Slug`/`Url` boundary types and the `keyOf` helper live in `commons codes`,
so they are unit-tested without a KV or `Random` binding. The handlers consume
those platform capabilities, which keeps them out of the test surface
([#291](https://github.com/accuser/bynk/issues/291)); exercise them end to end
under `bynk dev`, below.

## Run it

```sh
bynk dev
```

From anywhere inside the project, `bynk dev` compiles, picks the `links` worker,
and serves it on `http://localhost:8787` in local mode — Workers KV is
simulated, so there's nothing to provision first. Then:

```sh
curl -XPOST localhost:8787/links -d '{"target":"https://bynk.dev"}'
# {"code":"a1b2c3d4","target":"https://bynk.dev"}  (HTTP 201)

curl localhost:8787/links/a1b2c3d4
# "https://bynk.dev"

curl localhost:8787/links/missing0
# (HTTP 404)
```

*Under the hood,* `bynk dev` runs the manual recipe:

```sh
bynkc compile src --output out --target workers
cd out/workers/links
npx wrangler dev
```

To **deploy** for real, KV needs a namespace — create one and paste its id into
`wrangler.toml`, then `npx wrangler deploy` from the worker directory:

```sh
npx wrangler kv namespace create KV
```
