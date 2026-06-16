# `karn.queue.*` errors

`on message` handlers consume one message at a time and have a fixed shape: a
non-empty queue name, exactly one message parameter, and an
`Effect[Result[(), E]]` return. These are the common errors when that shape is
broken.

## `karn.queue.bad_params`

```text
[karn.queue.bad_params] `on message` handlers take exactly one parameter (the message)
```

**Cause:** a queue handler declared zero, or more than one, parameter. A queue
consumer processes exactly one message per invocation.

**Fix:** declare a single parameter (conventionally `message`) of the message's
type.

## `karn.queue.invalid_name`

```text
[karn.queue.invalid_name] `from queue` requires a non-empty queue name
```

**Cause:** the queue name string is empty.

**Fix:** give the queue a name matching the Cloudflare queue you are binding to.

## `karn.queue.return_not_effect_result`

```text
[karn.queue.return_not_effect_result] `on message` handler must return `Effect[Result[(), E]]`
```

**Cause:** the return type isn't `Effect[Result[(), E]]` — the `Ok` payload must
be unit `()`.

**Fix:** return `Effect[Result[(), E]]`; `Ok(())` acknowledges the message and
`Err(e)` retries it.

## Other queue errors

- `karn.queue.duplicate_consumer` — two queue handlers in the context consume
  the same queue. Give each a distinct queue name.
- `karn.parse.queue_in_agent` — `from queue` was placed in an `agent`. Queue
  consumers belong in a `service`.

## Related

- [Process a queued message](../guides/entry-points/queue.md)
- Reference: [Queue](../reference/queue.md)
