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
└── src/
    └── links.bynk      # context links — the HTTP service
```

## Run it

```sh
bynkc check src
bynkc compile src --output out --target workers
cd out/workers/links
# KV needs a namespace; create one and paste its id into wrangler.toml:
#   npx wrangler kv namespace create KV
npx wrangler dev
```

```sh
curl -XPOST localhost:8787/links -d '{"target":"https://bynk.dev"}'
# {"code":"a1b2c3d4","target":"https://bynk.dev"}  (HTTP 201)

curl localhost:8787/links/a1b2c3d4
# "https://bynk.dev"

curl localhost:8787/links/missing0
# (HTTP 404)
```

Deploy with `npx wrangler deploy` from the same directory.
