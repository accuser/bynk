# Bynk examples

A gallery of small, complete Bynk projects. Each one type-checks, compiles to a
standard Cloudflare Worker, runs locally under `wrangler dev`, and deploys with
`wrangler deploy` — from the same source.

Start with **[`hello-world`](hello-world/)** (a greeter — refined types, a
capability, typed HTTP, a test). The rest each lead with a different part of the
language:

| Example | Leads with | Entry points | Capabilities | Tests |
|---|---|---|---|---|
| [`hello-world`](hello-world/) | refined types, capabilities, typed HTTP | http | `Logger` | greeting + `Subject` boundary |
| [`link-shortener`](link-shortener/) | KV persistence with TTL, random ids | http | `Random`, `Kv` | `Slug`/`Url` boundary + key helper |
| [`feature-flags`](feature-flags/) | public vs. authorised routes, KV listing | http | `Kv` | `FlagKey` boundary + key round-trip |
| [`rate-limiter`](rate-limiter/) | the agent model (one Durable Object per key) | http | `Clock` | the fixed-window policy |
| [`todo`](todo/) | an agent keyed by the caller's verified identity | http | — | the agent (add / complete) |
| [`uptime-monitor`](uptime-monitor/) | scheduled work that calls the outside world | cron + http | `Fetch`, `Kv`, `Logger` | the health policy + key helper |
| [`webhook-relay`](webhook-relay/) | verifying a signed webhook, then forwarding it | http | `Fetch`, `Logger`, `Secrets` | — (all-effectful; see below) |

Together they cover every entry point (http, cron, queue is covered in the
[queue guide](../docs/src/guides/entry-points/queue.md)), both state models (KV and
Durable-Object agents), every actor scheme (`Visitor`, `Bearer`, an authorisation
refinement, `Signature`), and the outbound-`Fetch` + JSON-codec + caching story.

## The shared workflow

From any example directory, one command builds and serves it locally:

```sh
bynk dev          # compile + serve on http://localhost:8787 (local mode)
```

That's the compile-and-run recipe in one step — it runs `wrangler dev` in local
mode, so KV / Durable Objects / queues are simulated and there's nothing to
provision. The manual equivalent it runs under the hood:

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

To run an example's tests:

```sh
bynkc test .      # from the example directory
```

Six of the seven examples ship tests. Each factors its pure logic — a refined
type's boundary, a key helper, a windowing or health policy — into a `commons`
unit that is unit-tested without any platform binding (see *Notes* below for why
the split is drawn where it is).

## Notes on the current language surface

These examples are honest about what compiles *today* (Bynk is pre-1.0):

- **`HttpResult` has no redirect or `429` variant yet.** `link-shortener` returns
  the target URL as JSON rather than a `302`; `rate-limiter` reports the verdict
  in the body rather than a `429`. Both are noted where they occur.
- **Capabilities (`given`) live on handlers, not on free functions.** Effectful
  work stays inside service/agent handlers; only pure helpers are factored out
  (see `uptime-monitor`).
- **A test can target a `commons`, a capability-free agent, a user-declared
  `capability`, or a consumed *context*** — the last two are substitutable in a
  `test` block with `mocks`. What a test **cannot** target today is a context
  that itself `consumes bynk { … }` / `consumes bynk.cloudflare { … }`: a
  *platform* capability has no in-test substitute, and merely declaring one
  breaks the whole context's test emission
  ([#291](https://github.com/accuser/bynk/issues/291)). So each example keeps its
  testable logic — a refined type's boundary, a key helper, a windowing or health
  policy — in a `commons` (or a capability-free agent, as in `todo`), and leaves
  its platform-touching handlers for an end-to-end check under `bynk dev`.
  `webhook-relay` is the one example with no test: every step is effectful at the
  boundary (HMAC verify → `Fetch` → `Secrets`), so there is no pure kernel to
  factor out.
