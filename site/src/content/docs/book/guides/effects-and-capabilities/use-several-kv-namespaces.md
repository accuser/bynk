---
title: Use more than one KV namespace
---
**Goal:** keep two kinds of data apart in one context, such as live sessions and a
price cache, each in its own Workers KV namespace.

## Declare one capability per namespace

Consume `Kv`, then derive a capability from it for each namespace:

```bynk
context shop.cart

consumes bynk.cloudflare { Kv }

capability Sessions = Kv
capability PriceCache = Kv

service remember {
  on call(k: String, v: String) -> Effect[Option[String]] given Sessions {
    let _ <- Sessions.put(k, v)
    Sessions.get(k)
  }
}

service peek {
  on call(k: String) -> Effect[Option[String]] given PriceCache {
    PriceCache.get(k)
  }
}
```

`capability Sessions = Kv` is a distinct instance of `Kv`, not another name for
it. It has `Kv`'s operations and its own namespace, so `peek` never sees what
`remember` wrote. There is no `provides` to write: each instance is served by
`Kv`'s own provider, bound to the instance's namespace.

## What the build generates

Each instance gets a binding named from the kind and the capability, plus a
`[[kv_namespaces]]` stanza with its own deploy-time id:

```toml
[[kv_namespaces]]
binding = "KV_PRICE_CACHE"
id = "<KV_PRICE_CACHE_ID>" # set at deploy time

[[kv_namespaces]]
binding = "KV_SESSIONS"
id = "<KV_SESSIONS_ID>" # set at deploy time
```

The context above never uses the bare `Kv`, so it gets no `KV` stanza. Rename an
instance, an agent or a consumed context if two of its Worker's bindings would
share a name (`bynk.context.duplicate_binding`).

## Test each instance on its own

Under `bynkc test` every instance has its own fresh in-memory double, and
`stub` and `expect` name the instance:

```bynk,fragment
case "instances are separate" {
  let _ <- remember.call("a", "1")
  let r <- peek.call("a")
  expect r is None
  expect PriceCache.get called once
}
```

## Deploying

`bynk dev` runs instances locally. `bynk deploy` and `bynk dev --remote` do not
yet create a namespace per instance: they stop with a message naming the
binding (tracked in #1967). Until then, create the namespaces with
`wrangler kv namespace create` and put their ids in the generated
`wrangler.toml` by hand.

**See also:** [Derived capabilities](/book/reference/capabilities/#derived-capabilities-capability-x--k),
[The Cloudflare surface](/book/reference/bynk-capabilities/#the-cloudflare-surface--bynkcloudflare).
