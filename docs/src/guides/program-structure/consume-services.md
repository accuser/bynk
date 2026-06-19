# Consume another context's services with `consumes`

**Goal:** call a service that belongs to another context.

A context declares the contexts it depends on with `consumes`, then calls their
services. Suppose a `payment` context offers an `authorise` service:

```bynk
context payment

service authorise {
  on call(amount: Int) -> Effect[Result[Int, Int]] {
    Ok(amount)
  }
}
```

## Declare the dependency and call the service

In the consuming context, declare `consumes <context>` and call the service by
its qualified name. Service calls are effectful, so bind the result with `<-`:

```bynk,ignore
context orders

consumes payment

service placeOrder {
  on call(total: Int) -> Effect[Result[Int, Int]] {
    let _ <- payment.authorise(total)
    Ok(total)
  }
}
```

## Use an alias

Add `as <Alias>` to call through a shorter name:

```bynk,ignore
context orders

consumes payment as Pay

service placeOrder {
  on call(total: Int) -> Effect[Result[Int, Int]] {
    let _ <- Pay.authorise(total)
    Ok(total)
  }
}
```

## How it compiles

The call is the same Bynk code on both targets, but the emitted wiring differs:

- **`bundle`** (default) — a direct in-process function call through a composed
  surface.
- **`workers`** — a JSON call over a Cloudflare Service Binding, with the
  arguments and result validated as they cross the boundary.

See [Target Cloudflare Workers](../projects-build-and-deployment/cloudflare-workers.md) for the
target details.

## Related

- Reference: [type system](../../reference/types.md).
- Explanation: [How a Bynk program is shaped](how-a-program-is-shaped.md).
