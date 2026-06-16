# First-party `karn` capabilities

Karn ships a small set of capabilities with the compiler, under the reserved
`karn` namespace. You consume them like any capability — `consumes` the unit,
then `given` the capability in the handlers that need it — but you never declare
or provide them: the toolchain injects the implementation for the target
platform. The `karn` root namespace is reserved, so your own code can never
collide with it.

There are two units:

| Unit | Consume with | Portability |
|---|---|---|
| **`karn`** | `consumes karn { … }` | Portable — the same source runs on the `cloudflare` and `node` platforms. |
| **`karn.cloudflare`** | `consumes karn.cloudflare { … }` | Platform-locked — consuming it pins the deployment unit to Cloudflare. |

For *why* a capability is the unit of outside-world access, see
[Understand the capability model](../guides/effects-and-capabilities/understand-the-capability-model.md);
for the general `capability` / `provides` / `given` rules, see
[Capabilities & providers](capabilities.md).

## The portable surface — `karn`

`consumes karn { … }` brings these into scope. They are implemented identically
on every platform (the host `Date.now`, `crypto`, `fetch`, `console`,
environment), so code that stays on this surface is portable.

| Capability | Operations |
|---|---|
| **`Clock`** | `now() -> Effect[Int]` — milliseconds since the Unix epoch. |
| **`Random`** | `uuid() -> Effect[Uuid]` · `int(lo: Int, hi: Int) -> Effect[Int]` (lo-inclusive, hi-exclusive). |
| **`Logger`** | `info(msg: String) -> Effect[()]` · `error(msg: String) -> Effect[()]`. |
| **`Fetch`** | `send(req: Request) -> Effect[Result[Response, FetchError]]` — an outbound HTTP request. |
| **`Secrets`** | `get(name: String) -> Effect[Option[String]]` — read configuration/secrets; `None` if unset. |

The `karn` unit also exports the transparent types these operations use:

```karn,ignore
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

## The Cloudflare surface — `karn.cloudflare`

`consumes karn.cloudflare { … }` exposes the platform's own infrastructure.
Consuming it **locks the deployment unit to Cloudflare** (it cannot then target
`node`), and the generated `wrangler.toml` gains the matching binding stanza.

| Capability | Operations |
|---|---|
| **`Kv`** | `get(key: String) -> Effect[Option[String]]` · `put(key: String, value: String) -> Effect[()]` · `putTtl(key: String, value: String, ttlSeconds: Int) -> Effect[()]` · `delete(key: String) -> Effect[()]` · `list(prefix: Option[String]) -> Effect[List[String]]`. |

`Kv` is backed by a single Worker KV namespace bound as `env.KV`; the
`[[kv_namespaces]]` stanza is derived for you.

> **Note** — *producing* queue messages (`send`/`sendBatch`) is not yet a
> first-party capability. *Consuming* a queue is a separate entry-point feature:
> see [Process a queued message](../guides/entry-points/queue.md) and the
> [Queue reference](queue.md).

## Consuming a first-party capability

Consume the unit, then grant the capability with `given` in each handler that
calls it — exactly as you would a capability you declared yourself:

```karn
context greeter

consumes karn { Clock, Logger }

service api from http {
  on GET("/now") () -> Effect[HttpResult[Int]] given Clock, Logger {
    let t <- Clock.now()
    let _ <- Logger.info("checked the clock")
    Ok(t)
  }
}
```

The Cloudflare surface is consumed the same way — `consumes karn.cloudflare { Kv }`
and `given Kv` — with the portability trade-off noted above. To configure a
first-party capability (an API key for `Fetch`, say), read it from `Secrets`
rather than passing it as an argument; [Wrap a library as an adapter](../guides/effects-and-capabilities/wrap-a-library.md)
shows the pattern.

## Related first-party modules

Beyond capabilities, the `karn` namespace also ships pure **commons** you bring
in with `uses` (not `consumes`) — `karn.list` and `karn.map` (combinators over
the `List`/`Map` kernels) and `karn.string` (string helpers). These are ordinary
functions with no effects; see the [type system reference](types.md).

**See also:** [Capabilities & providers](capabilities.md),
[Adapters](adapters.md),
[Understand the capability model](../guides/effects-and-capabilities/understand-the-capability-model.md).
