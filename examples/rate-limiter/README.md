# Rate limiter

A fixed-window rate limiter: at most N requests per client per window. Each client
is an **agent** — on Cloudflare, one Durable Object per key — so limits are
strongly consistent and never bleed across clients.

What it shows:

- **The agent model** — `agent Limiter { key client: ClientId; state { … } }`.
  Two calls with the same `ClientId` address the same instance; different ids are
  independent. Compiles to a Durable Object on the `workers` target.
- **Zeroable state** — `windowStart` and `count` are `Int`s, so a never-seen
  client starts clean at `0/0` with no constructor to call.
- **A single `commit` per call** — the handler computes the next window and count
  once and commits one replacement; an over-budget request is *not* counted, so a
  client can't deepen its own hole.
- **An honest clock** — the agent has no ambient time; the HTTP handler reads
  `Clock.now()` and passes it in, which keeps the agent a pure function of its
  inputs.

> Note: `HttpResult` has no `429` variant yet, so the verdict is returned in the
> body (`allowed`, `remaining`, `resetAt`) with a `200`.

## Layout

```text
rate-limiter/
├── bynk.toml
└── src/
    └── ratelimit.bynk      # context ratelimit — agent + HTTP service
```

## Run it

```sh
bynkc check src
bynkc compile src --output out --target workers
cd out/workers/ratelimit
npx wrangler dev
```

```sh
# 10 requests / 60s per client (the :client segment is the key)
curl localhost:8787/check/acme
# {"allowed":true,"remaining":9,"resetAt":...}

# hammer it past the limit and `allowed` flips to false, `remaining` to 0
for i in $(seq 1 12); do curl -s localhost:8787/check/acme; echo; done
```

Deploy with `npx wrangler deploy`. The Durable Object migration is already in the
generated `wrangler.toml`.
