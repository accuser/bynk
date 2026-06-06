# Version compatibility & changelog

Karn is pre-1.0 and developed in small, spec-first increments (see
[Versioning & roadmap](../explanation/versioning-and-roadmap.md)). This book is
written against **v0.12**.

This page is a high-level summary of notable increments, not an exhaustive
per-commit history. While Karn is pre-1.0, increments may change behaviour.

## Recent increments

| Version | Highlights |
|---|---|
| **v0.12** | Provider composition (`provides … given`) — a provider may depend on other capabilities; the composition root wires the dependency graph in topological order. |
| **v0.11** | Agent state-field initialisers (`state { status: OrderStatus = Pending }`), enabling sum-typed **state machines** (and opaque/refined state) — no more `Option`-wrapping. |
| **v0.10b** | Queue consumers (`on queue`) — message deserialisation, the Worker `queue` entry point with `Ok`/`Err` ack/retry, and `wrangler.toml` `[[queues.consumers]]`. |
| **v0.10a** | Cron handlers (`on cron`) — scheduled tasks compiling to the Worker `scheduled` entry point and `wrangler.toml` `[triggers]`. |
| **v0.9.4** | Refined-literal admission (write a literal where a refined type is expected); `Mock[T]` value fabrication for tests. |
| **v0.9.1** | `assert` as an expression; project-mode hardening; a `tsc` verification stage. |
| **v0.9** | HTTP handlers (`on http`), `HttpResult`, and the Cloudflare Workers target. |
| **v0.7.1** | Tail-position auto-lift of plain values into `Effect`. |
| **v0.6** | Cross-context service calls (`consumes`) and composition roots. |
| **v0.5** | The effect system (`Effect[T]`, `<-`) and the generated runtime. |

Earlier increments established the core: `commons`/`context` units, the type
system (opaque, sum, record, refined types), `match`/`is`, `Result`/`Option`,
agents, capabilities, and testing.

## Deferred to v1

Events, sagas, and storage kinds are designed but not yet shipped — see
[Versioning & roadmap](../explanation/versioning-and-roadmap.md#what-is-deferred-to-v1).

> This summary will become a precise per-increment changelog as the docs-delta
> discipline (docs shipped with each increment) takes hold.
