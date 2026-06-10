# 0002 — v0.10 ships as two slices under one number; queues are consumer-only

- **Status:** Accepted (v0.10)
- **Spec:** §5.7, §7.3.4

## Context
`on cron` and `on queue` are siblings of `on http` sharing the handler-kind
plumbing, but are independent of each other. Splitting into two version numbers
would churn the downstream roadmap.

## Decision
Ship as **v0.10a (cron)** then **v0.10b (queue)** under the single v0.10 number.
The queue increment covers the **consumer** side only (`on queue` with a single
message parameter, returning `Effect[Result[(), E]]` — `Ok` acks, `Err`
retries); the producer side waits for a capability home (it arrived with the
platform-adapter work).

## Consequences
Increments stay small without renumbering. The Result-ack convention made the
later `cloudflare.Queue` producer capability purely additive.
