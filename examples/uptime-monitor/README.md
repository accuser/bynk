# Uptime monitor

A scheduled job that pings a set of URLs every five minutes, records each one's
status in KV, and serves the latest result over HTTP. It puts **cron**, **`Fetch`**,
**`Kv`**, and **`Logger`** together in one small context.

What it shows:

- **A cron entry point** — `service checks from cron { on schedule("*/5 * * * *") (at: Int) … }`.
  Cron has no ambient clock, so the schedule-aligned instant arrives as the `at`
  parameter (epoch-ms).
- **Outbound HTTP** — `Fetch.send(Request { method: Get, … })` returns a
  `Result[Response, FetchError]`; a network failure is a value, not an exception.
- **Capabilities live on handlers** — the effectful fetch/store work stays in the
  cron handler (a free function can't hold `given`, and `Request` can only be
  built where it's used); only the pure `toStatus` mapping is factored out.
- **A read-side HTTP route** — `GET /status/:name` reads the stored JSON back
  through `Json.decode[Status]`.

## Layout

```text
uptime-monitor/
├── bynk.toml
└── src/
    └── monitor.bynk     # context monitor — cron service + HTTP service
```

## Run it

```sh
bynkc check src
bynkc compile src --output out --target workers
cd out/workers/monitor
#   npx wrangler kv namespace create KV     # paste the id into wrangler.toml
npx wrangler dev
```

Trigger the schedule locally (wrangler exposes a scheduled endpoint in dev):

```sh
curl "localhost:8787/__scheduled?cron=*/5+*+*+*+*"

curl localhost:8787/status/example
# {"name":"example","ok":true,"code":200,"at":...}
```

The generated `wrangler.toml` already carries `crons = ["*/5 * * * *"]`. Deploy
with `npx wrangler deploy`. To watch more sites, add another `Fetch`/`Kv` block in
the cron handler.
