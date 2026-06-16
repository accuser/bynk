# Run a task on a schedule

**Goal:** run some work on a fixed schedule — a nightly sweep, an hourly
refresh — without an incoming request.

Cron handlers go in a `service` inside a `context`. Each names a schedule (bare,
after `cron`), takes at most one parameter, and returns `Effect[Result[(), E]]`.

## A minimal scheduled task

```karn
context reaper

service sweeper from cron {
  on schedule("*/5 * * * *") () -> Effect[Result[(), String]] {
    Ok(())
  }
}
```

The schedule is a standard five-field cron expression
(`minute hour day-of-month month day-of-week`). `*/5 * * * *` runs every five
minutes; `0 0 * * *` runs at midnight.

## Get the scheduled time

Cron has no built-in clock. To learn when the run fired, declare a single `Int`
parameter — it receives the scheduled time as Unix epoch milliseconds. It is the
exact, schedule-aligned instant (better than "now" for bucketing or idempotency
keys):

```karn
context reaper

service sweeper from cron {
  on schedule("0 * * * *") (at: Int) -> Effect[Result[(), String]] {
    Ok(())
  }
}
```

## Signal success or failure

Return `Ok(())` when the run succeeds. A cron run has no caller to answer and no
retry, so a failure is returned as `Err(e)` — it is logged and the run
completes. Map a domain error to `Err` explicitly:

```karn
type SweepError = enum { StorageUnavailable }

service sweeper from cron {
  on schedule("0 0 * * *") () -> Effect[Result[(), SweepError]] {
    Err(StorageUnavailable)
  }
}
```

## Build and run

Cron services compile to a Cloudflare Worker with `--target workers`; each
schedule lands in the `[triggers]` table of the generated `wrangler.toml`. See
[Target Cloudflare Workers](../projects-build-and-deployment/cloudflare-workers.md).

## Related

- Reference: [Cron](../../reference/cron.md).
- Reference: [HTTP](../../reference/http.md) — the sibling request-driven handler.
