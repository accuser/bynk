---
title: "First-party `bynk` capabilities"
---
Bynk ships a small set of capabilities with the compiler, under the reserved
`bynk` namespace. You consume them like any capability — `consumes` the unit,
then `given` the capability in the handlers that need it — but you never declare
or provide them: the toolchain injects the implementation for the target
platform. The `bynk` root namespace is reserved, so your own code can never
collide with it.

There are two units:

| Unit | Consume with | Portability |
|---|---|---|
| **`bynk`** | `consumes bynk { … }` | Portable — the same source runs on the `cloudflare` and `node` platforms. |
| **`bynk.cloudflare`** | `consumes bynk.cloudflare { … }` | Platform-locked — consuming it pins the deployment unit to Cloudflare. |

For *why* a capability is the unit of outside-world access, see
[Understand the capability model](/book/guides/effects-and-capabilities/understand-the-capability-model/);
for the general `capability` / `provides` / `given` rules, see
[Capabilities & providers](/book/reference/capabilities/).

## The portable surface — `bynk`

`consumes bynk { … }` brings these into scope. They are implemented identically
on every platform (the host `Date.now`, `crypto`, `fetch`, `console`,
environment), so code that stays on this surface is portable.

| Capability | Operations |
|---|---|
| **`Clock`** | `now() -> Effect[Instant]` — the current instant (an absolute point in time; epoch-millis under the hood). |
| **`Random`** | `uuid() -> Effect[Uuid]` · `int(lo: Int, hi: Int) -> Effect[Int]` (lo-inclusive, hi-exclusive). |
| **`Logger`** | `info(msg: String) -> Effect[()]` · `error(msg: String) -> Effect[()]`. |
| **`Fetch`** | `send(req: Request) -> Effect[Result[Response, FetchError]]` — an outbound HTTP request. |
| **`Secrets`** | `get(name: String) -> Effect[Option[String]]` — read configuration/secrets; `None` if unset. |

The `bynk` unit also exports the transparent types these operations use:

```bynk,ignore
type Uuid       = String where Matches("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
type Method     = enum { Get, Post, Put, Delete }
type FetchError = enum { Network, Timeout }

type Request = {
  method: Method,
  url: String,
  contentType: Option[String],
  authorization: Option[String],
  body: Option[String],
}

type Response = {
  status: Int,
  body: String,
}
```

## The Cloudflare surface — `bynk.cloudflare`

`consumes bynk.cloudflare { … }` exposes the platform's own infrastructure.
Consuming it **locks the deployment unit to Cloudflare** (it cannot then target
`node`), and the generated `wrangler.toml` gains the matching binding stanza.

| Capability | Operations |
|---|---|
| **`Kv`** | `get(key: String) -> Effect[Option[String]]` · `put(key: String, value: String) -> Effect[()]` · `putTtl(key: String, value: String, ttlSeconds: Int) -> Effect[()]` · `delete(key: String) -> Effect[()]` · `list(prefix: Option[String]) -> Effect[List[String]]`. |

`Kv` is backed by a single Worker KV namespace bound as `env.KV`; the
`[[kv_namespaces]]` stanza is derived for you.

> **Note** — *producing* queue messages (`send`/`sendBatch`) is not yet a
> first-party capability. *Consuming* a queue is a separate entry-point feature:
> see [Process a queued message](/book/guides/entry-points/queue/) and the
> [Queue reference](/book/reference/queue/).

## Consuming a first-party capability

Consume the unit, then grant the capability with `given` in each handler that
calls it — exactly as you would a capability you declared yourself:

```bynk
context greeter

consumes bynk { Clock, Logger }

service api from http {
  on GET("/now") () -> Effect[HttpResult[Int]] by Visitor given Clock, Logger {
    let t <- Clock.now()
    let _ <- Logger.info("checked the clock")
    Ok(t.toEpochMillis())
  }
}
```

The Cloudflare surface is consumed the same way — `consumes bynk.cloudflare { Kv }`
and `given Kv` — with the portability trade-off noted above. To configure a
first-party capability (an API key for `Fetch`, say), read it from `Secrets`
rather than passing it as an argument; [Wrap a library as an adapter](/book/guides/effects-and-capabilities/wrap-a-library/)
shows the pattern.

## Related first-party modules

Beyond capabilities, the `bynk` namespace also ships pure **commons** you bring
in with `uses` (not `consumes`) — `bynk.list` and `bynk.map` (combinators over
the `List`/`Map` kernels) and `bynk.string` (string helpers). These are ordinary
functions with no effects; see the [type system reference](/book/reference/types/).

> **Deprecated (v0.91, ADR 0116 D6):** the `bynk.list` free functions whose method
> forms now exist — `map`, `filter`, `find`, `any`, `all` — emit a non-failing
> `bynk.list.deprecated_function` warning at each call site (the build still
> succeeds) with a machine-applicable fix to the method form: `map(xs, f)` →
> `xs.map(f)`, `find(xs, p)` → `xs.filter(p).first()`, and so on. Prefer the
> [`List` methods](/book/reference/types/#list-methods). `reverse` and `traverse` keep their
> free-function form (no method equivalent yet).

**See also:** [Capabilities & providers](/book/reference/capabilities/),
[Adapters](/book/reference/adapters/),
[Understand the capability model](/book/guides/effects-and-capabilities/understand-the-capability-model/).
