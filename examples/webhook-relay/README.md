# Webhook relay

Accept a signed webhook only when its signature proves it came from the trusted
sender, then forward the event to a configured upstream URL. The HMAC check is
generated for you — there is no app-written crypto.

What it shows:

- **A `Signature` actor** — `auth = Signature(secret = "WEBHOOK_SECRET", header = "X-Signature", timestamp = "X-Timestamp", tolerance = 300)`.
  Before the handler runs, the boundary recomputes HMAC-SHA256 over the raw body
  (constant-time, WebCrypto), rejects a mismatch or a stale timestamp with `401`,
  and only then parses the body from the same bytes.
- **Authenticity, not a principal** — a `Signature` actor has no identity, so the
  binder is omitted (`by Webhook`).
- **Outbound HTTP from a handler** — the verified event is re-encoded with
  `Json.encode` and POSTed onward with `Fetch.send`.
- **Configuration via `Secrets`** — the upstream URL is read from
  `Secrets.get("RELAY_TARGET_URL")` rather than hard-coded.

## Layout

```text
webhook-relay/
├── bynk.toml
└── src/
    └── relay.bynk       # context relay — the HTTP service
```

> **No unit tests, by design.** Unlike the other examples, this one has no pure
> kernel to factor into a `commons`: every step is effectful at the boundary —
> the HMAC verification is the actor's, then `Secrets.get` → `Fetch.send`. The
> handler consumes platform capabilities throughout, which keeps it out of the
> test surface ([#291](https://github.com/accuser/bynk/issues/291)). Exercise it
> end to end under `bynk dev`, below.

## Run it

```sh
# this service reads two values — supply local ones through the passthrough
bynk dev \
  -- --var WEBHOOK_SECRET:dev-secret \
     --var RELAY_TARGET_URL:https://httpbin.org/post
```

From anywhere inside the project, `bynk dev` compiles, picks the `relay` worker,
and serves it on `http://localhost:8787` in local mode. Then:

```sh
# a request with no / wrong X-Signature is rejected at the boundary
curl -XPOST localhost:8787/hooks/event -d '{"id":"evt_1","kind":"order.created"}'
# (HTTP 401)

# with a valid HMAC-SHA256 of the body (and a fresh X-Timestamp) it forwards
curl -XPOST localhost:8787/hooks/event \
  -H "X-Timestamp: $(date +%s)" -H "X-Signature: sha256=<hmac>" \
  -d '{"id":"evt_1","kind":"order.created"}'
# "relayed"
```

*Under the hood,* `bynk dev` compiles to `out/workers/relay/` and runs `wrangler
dev` there. **Deploy** with `npx wrangler deploy`; set the real secrets with
`npx wrangler secret put WEBHOOK_SECRET` and `… RELAY_TARGET_URL`.
