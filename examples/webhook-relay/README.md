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

## Run it

```sh
bynkc check src
bynkc compile src --output out --target workers
cd out/workers/relay
#   cat > .dev.vars <<EOF
#   WEBHOOK_SECRET = "dev-secret"
#   RELAY_TARGET_URL = "https://httpbin.org/post"
#   EOF
npx wrangler dev
```

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

Deploy with `npx wrangler deploy`; set the real secrets with
`npx wrangler secret put WEBHOOK_SECRET` and `… RELAY_TARGET_URL`.
