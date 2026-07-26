# 0287 — Event replay and backfill split to a separate, future, currently-unfiled track

- **Status:** Accepted (v0.237.1)

**Context.** `design/bynk-design-notes.md` §7 describes a new subscriber
"backfilling from log history" and the runtime upgrading old wire events to
the current schema on read (lines 243, 276). The Events track (spine #936)
also inherits the actors track's deferred **Q8 (replay/ordering)**
([#260](https://github.com/accuser/bynk/issues/260)).

**Decision.** Replay/backfill and actors' Q8 move to a separate, future,
currently-unfiled track. Three reasons, the same discipline the `Idempotency`
capability track applied to its own durable-provider split
([ADR 0282](../decisions/0282-idempotency-capability-slice0.md) ships the
in-memory provider only): first, a replay-safe subscriber needs to dedup
`env.eventId` across a crash and a re-delivery window measured in the log's
retention, which wants the **durable** `Idempotency` provider — only the
in-memory provider has shipped, so replay depends on a second track that does
not exist yet. Second, replay needs its own durable event-log substrate design
— what stores the log, who owns retention, the window — a decision as heavy
and as unspecified as the fan-out substrate itself
(`events-fanout-substrate`, this same pending file). Third, the live core —
emit, subscribe, pattern-filter, envelope, additive-version — is a complete,
useful pub-sub system without replay; replay is additive on top of it, not a
precondition for the core's correctness.

**What this track still owes replay.** The envelope (a later slice) carries
`eventId` and `schemaVersion` from day one, and the fan-out substrate decision
above chooses a fanout DO partly because it is the natural future log-owner —
this track ships replay's seams, not replay itself.

**Consequences.** This track's "done when" criteria exclude replay/backfill
and actors' Q8 by name, not by silent omission; a future track, filed once the
durable `Idempotency` provider exists, owns them.
