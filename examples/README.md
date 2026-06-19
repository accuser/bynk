# Bynk examples

A gallery of small, complete Bynk projects. Each one type-checks, compiles to a
standard Cloudflare Worker, runs locally under `wrangler dev`, and deploys with
`wrangler deploy` — from the same source.

Start with **[`hello-world`](hello-world/)** (a greeter — refined types, a
capability, typed HTTP, a test). The rest each lead with a different part of the
language:

| Example | Leads with | Entry points | Capabilities |
|---|---|---|---|
| [`hello-world`](hello-world/) | refined types, capabilities, typed HTTP | http | `Logger` |
| [`link-shortener`](link-shortener/) | KV persistence with TTL, random ids | http | `Random`, `Kv` |
| [`feature-flags`](feature-flags/) | public vs. authorised routes, KV listing | http | `Kv` |
| [`rate-limiter`](rate-limiter/) | the agent model (one Durable Object per key) | http | `Clock` |
| [`todo`](todo/) | an agent keyed by the caller's verified identity | http | — |
| [`uptime-monitor`](uptime-monitor/) | scheduled work that calls the outside world | cron + http | `Fetch`, `Kv`, `Logger` |
| [`webhook-relay`](webhook-relay/) | verifying a signed webhook, then forwarding it | http | `Fetch`, `Logger`, `Secrets` |

Together they cover every entry point (http, cron, queue is covered in the
[queue guide](../docs/src/guides/entry-points/queue.md)), both state models (KV and
Durable-Object agents), every actor scheme (`Visitor`, `Bearer`, an authorisation
refinement, `Signature`), and the outbound-`Fetch` + JSON-codec + caching story.

## The shared workflow

Every example uses the same three commands. From an example directory:

```sh
bynkc check src                                   # type-check, no output
bynkc compile src --output out --target workers   # emit a Worker
cd out/workers/<name> && npx wrangler dev         # run it locally
```

`bynkc` lives at `target/release/bynkc` after `cargo build --release -p bynkc`
(see the [install page](../docs/src/introduction/install.md)). The generated
`wrangler.toml` carries the bindings each example needs — a `[[kv_namespaces]]`
stanza, `[[durable_objects.bindings]]`, or `crons` — with placeholder ids to fill
in at deploy time.

## Notes on the current language surface

These examples are honest about what compiles *today* (Bynk is pre-1.0):

- **`HttpResult` has no redirect or `429` variant yet.** `link-shortener` returns
  the target URL as JSON rather than a `302`; `rate-limiter` reports the verdict
  in the body rather than a `429`. Both are noted where they occur.
- **Capabilities (`given`) live on handlers, not on free functions.** Effectful
  work stays inside service/agent handlers; only pure helpers are factored out
  (see `uptime-monitor`).
- **`bynkc test` runs against `commons` and agents in capability-free contexts**
  (see `todo`). Testing handlers in a context that `consumes` a capability needs
  harness support that is still landing, so those examples ship without test
  blocks for now.
