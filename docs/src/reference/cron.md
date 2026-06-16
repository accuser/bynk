# Cron

Cron handlers run on a schedule. Like HTTP handlers, they are declared in a
`service` inside a `context`; the protocol sits on the service header
(`from cron`), and each handler is an `on schedule("…")`.

## Handler form

```karn
service <Name> from cron {
  on schedule("<schedule>") (at: Int) -> Effect[Result[(), E]] {
    …
  }
}
```

- **Schedule:** a standard five-field cron expression
  (`minute hour day-of-month month day-of-week`) as a string literal in
  `schedule("…")`. It must have exactly five whitespace-separated fields, or
  `karn.cron.invalid_schedule`.
- **Parameter:** *optional*, at most one. If present it must be `Int` and
  receives the scheduled fire time as Unix epoch **milliseconds**; otherwise
  `karn.cron.bad_params`. Cron has no built-in clock, so this parameter is how a
  handler learns the time — and unlike "now", it is the exact, schedule-aligned
  instant (useful for bucketing and idempotency). Wrap it in your own time type
  inside the body if you want stronger typing.
- **Return type:** must be `Effect[Result[(), E]]` for some error type `E`
  (`karn.cron.return_not_effect_result`).
- **Placement:** only inside a `service`, never an `agent`
  (`karn.parse.handler_in_agent`).

No two cron handlers in a context may declare the same schedule
(`karn.cron.duplicate_schedule`).

## Failure

A cron run has no retry channel. Returning `Ok(())` completes the run silently;
returning `Err(e)` logs `e` and completes. Map a domain failure to `Err` the
same way an HTTP handler maps it to an `HttpResult` error variant — the mapping
stays visible in the handler body.

## Example

```karn
context reaper

service sweeper from cron {
  on schedule("*/5 * * * *") (at: Int) -> Effect[Result[(), String]] {
    Ok(())
  }
}
```

## Emission

cron handlers compile to the Worker's `scheduled` entry point on the
`--target workers` target (dispatching on `event.cron`, passing
`event.scheduledTime` to handlers that declare the parameter), and every
schedule is aggregated into the `[triggers]` table of the generated
`wrangler.toml`. See [emission](emission.md) and
[Target Cloudflare Workers](../guides/projects-build-and-deployment/cloudflare-workers.md).
