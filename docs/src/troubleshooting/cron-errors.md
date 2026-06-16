# `karn.cron.*` errors

cron handlers run on a schedule and have a fixed shape: at most one `Int`
parameter (the scheduled time), a five-field schedule, and an
`Effect[Result[(), E]]` return. These are the common errors when that shape is
broken.

## `karn.cron.bad_params`

```text
[karn.cron.bad_params] an `from cron` parameter must be `Int` (the scheduled time in epoch milliseconds)
```

**Cause:** a cron handler declared more than one parameter, or a single
parameter that isn't `Int`. A scheduled trigger's only input is the time it
fired.

**Fix:** take either no parameter or a single `Int` (the scheduled time, Unix
epoch milliseconds). Wrap it in your own time type inside the body if you want
stronger typing.

## `karn.cron.invalid_schedule`

```text
[karn.cron.invalid_schedule] cron expression `every day` must have exactly five whitespace-separated fields
```

**Cause:** the schedule string is not a five-field cron expression.

**Fix:** write five fields — `minute hour day-of-month month day-of-week`. For
example, `"0 0 * * *"` (midnight daily) or `"*/15 * * * *"` (every 15 minutes).

## `karn.cron.return_not_effect_result`

```text
[karn.cron.return_not_effect_result] cron handler must return `Effect[Result[(), E]]`
```

**Cause:** the return type isn't `Effect[Result[(), E]]` — the `Ok` payload must
be unit `()`.

**Fix:** return `Effect[Result[(), E]]`; use `Ok(())` on success and `Err(e)` on
a domain failure.

## Other cron errors

- `karn.cron.duplicate_schedule` — two cron handlers in the context declare the
  same schedule. Give each a distinct expression.
- `karn.parse.cron_in_agent` — `from cron` was placed in an `agent`. Scheduled
  tasks belong in a `service`.

## Related

- [Run a task on a schedule](../guides/entry-points/cron.md)
- Reference: [Cron](../reference/cron.md)
