# Version compatibility & changelog

Karn is pre-1.0 and developed in small, spec-first increments (see
[Versioning & roadmap](../explanation/versioning-and-roadmap.md)). This book is
written against **v0.19**.

This page is a high-level summary of notable increments, not an exhaustive
per-commit history. While Karn is pre-1.0, increments may change behaviour.

## Recent increments

| Version | Highlights |
|---|---|
| **v0.19** | The first platform adapter and live platform locking — `karn.cloudflare` exporting a minimal `Kv` (get/put/delete, collection-free), injected like the `karn` surface and named inside the reserved prefix. Consuming it types `env.KV` into the Worker `Env`, emits the `[[kv_namespaces]]` wrangler stanza, and (bundle) threads an optional `env` through `composeApp`. Platform-lock enforcement goes live: `karn.target.vendor_required` / `vendor_conflict` over the in-process given-closure, per deployment unit. |
| **v0.18** | Adapter dependencies & the ambient surface — adapters gain `consumes U { Cap, … }` (adapter-to-adapter), external providers' `given` is wired (compose passes a by-name deps object to the binding constructor, transitively), `karn.Fetch` + `karn.Secrets` join the first-party surface, and `--platform node` makes the platform axis observable. Config-as-capability: the `tokens`/`weather` exemplars drop their secret/URL parameters. |
| **v0.17** | Adapters — the host boundary. The `adapter` declaration kind: capability contracts beside a named TypeScript `binding` (external, bodiless providers), `consumes U { Cap, … }` bare-name flattening for consumers, the reserved `karn` namespace and first-party `karn` surface (`Clock`, `Random`, `Logger`), npm `requires` pinning, and a minimal `--platform` axis. |
| **v0.16** | Multi-Worker integration testing (`test integration "…" { wires … }`) — stand several contexts up as in-process Workers and exercise a flow across the real cross-context wire (serialise/deserialise), no mocks. Covers cross-context service calls, cross-context capabilities, and cross-Worker agents (Durable Objects, backed in-memory with state fresh per case). The MVP's final increment. |
| **v0.15** | Cross-context capability resolution — a context `exports capability { … }`; a consumer depends on it via a qualified `given B.Cap` and its provider is instantiated locally (in-process). The platform/framework-context pattern. |
| **v0.13** | Refinement narrowing — `value is RefinedType` checks the refinement at runtime and narrows the value to that type in the branch (flow-sensitive counterpart to `.of`). |
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
