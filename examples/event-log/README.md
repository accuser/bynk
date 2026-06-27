# Event log

An append-only activity stream backed by a **`Log`** — a time-indexed sequence
with built-in retention and time-window queries.

What it shows:

- **A `Log` store with `@retain`** — `history: Log[Event] @retain(30.days)`. Each
  append drops entries past the horizon, so the log stays bounded with no separate
  sweep to write.
- **The one non-idempotent write** — `append` stamps `Clock.now()`, so the
  recording handler declares `given Clock`. The `Event` carries an `id` as a
  dedup key, since an at-least-once retry can append twice.
- **Time-window reads, clock-free** — `since`/`recent` build a `Query[T]` over the
  entry values; the caller passes the cutoff instant, derived with `Instant` /
  `Duration` arithmetic (`now - 1.hours`). A reading handler needs no clock.
- **One shared query vocabulary** — a windowed `breakdown` collects the window and
  tallies it with `summarise` from `commons digest`, whose `groupBy` runs eagerly
  over a `List` — the same vocabulary the agent runs lazily over storage.

> **No `bynkc test` here.** Like [`webhook-relay`](../webhook-relay/), every write
> path is platform-effectful at the boundary — `append` stamps the platform
> `Clock`, which has no in-test substitute
> ([#291](https://github.com/accuser/bynk/issues/291)). The pure `summarise` in
> `commons digest` is still type-checked by `bynkc check`; the behaviour is
> exercised end to end under `bynk dev`.

## Layout

```text
event-log/
├── bynk.toml
└── src/
    ├── digest.bynk     # commons digest — Event + the pure `summarise` query
    └── events.bynk     # context events — the Log-backed agent + HTTP service
```

## Check

```sh
bynkc check src      # type-check, no output
```

## Run it

```sh
bynk dev
```

`bynk dev` compiles, picks the `events` worker, and serves it on
`http://localhost:8787` in local mode — the Durable Object is simulated. Then:

```sh
curl -XPOST localhost:8787/events -d '{"id":"e1","kind":"login","who":"alice"}'
# {"id":"e1","kind":"login","who":"alice"}  (HTTP 201)

curl localhost:8787/events/recent
# [{"id":"e1","kind":"login","who":"alice"}, ...]   (newest first, up to 20)

curl localhost:8787/events/last-hour
# [{"kind":"login","count":1}, ...]                 (a groupBy tally of the last hour)

curl localhost:8787/events/last-day
# 1                                                 (count over the last 24h)
```

*Under the hood,* `bynk dev` compiles to `out/workers/events/` and runs `wrangler
dev` there. **Deploy** with `npx wrangler deploy` from that directory.
