# 0078 — `QueueResult`: a typed queue verdict, and the agency rule

- **Status:** Accepted (v0.44)
- **Spec:** `runtime-library.md` (`QueueResult`), `static-semantics.md` (queue return), `emission.md` (queue routing)

## Context

Through v0.43 queue handlers returned `Effect[Result[(), E]]` — the *same type*
as cron — yet `Err` meant **retry** for queue and **log-and-drop** for cron. The
two protocols' verdicts were distinguished only by the handler keyword, not by
the type a reader sees, against the `HttpResult` precedent where the wire outcome
is a named sum.

## Decision

Queue handlers return `Effect[QueueResult]`, where `QueueResult` is a built-in,
**non-generic** sum `Ack | Retry`. `Ack` confirms the message; `Retry` carries a
`String` reason for the log path. The runtime routes on the verdict
(`Ack`→`msg.ack()`, `Retry`→log + `msg.retry()`) instead of on the keyword.
Cron keeps `Effect[Result[(), E]]`.

The rule is cut around **agency**: *a protocol earns a verdict type when the
handler makes a dispatch decision the type should name.* A queue handler decides
ack-vs-retry, a decision independent of success/failure (you may `Ack` a logical
failure to drop a poison message, or `Retry` despite partial success), so the
verdict must be named. HTTP's status is the analogous decision. Cron makes no
dispatch decision — success/failure is the whole story and the platform's
logging of `Err` is transport behaviour, not a handler choice — so
`Result[(), E]` is exactly right and a `CronResult` would name a decision that
does not exist.

## Consequences

`QueueResult` is non-generic (no payload beyond `Retry`'s reason), so nobody
mirrors `HttpResult[T]`'s type parameter by reflex. `Ack`/`Retry` resolve as
built-in variants the way `HttpResult`'s do, and inherit the
`bynk.types.ambiguous_constructor` disambiguation. The rule generalises: a future
protocol gets a verdict type iff its handler dispatches.
