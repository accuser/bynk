# 0284 — The Events fan-out substrate is a per-publisher fanout Durable Object, not queue-per-topic

- **Status:** Accepted (v0.237.1)

**Context.** `design/bynk-design-notes.md` §7's emit-lowering note (line 1321)
offers a fork without choosing it: emit "maps to Queues with topic-as-queue
routing, or a custom event-fanout DO for higher-fanout scenarios." This is the
Events track's (spine #936) load-bearing decision — three of §7's asserted
properties hang off it: per-publisher FIFO ordering, subscriber failure
isolation with independent transactions, and, for a future replay track, replay
from a durable log. It is also the most expensive of the track's decisions to
reverse, so it lands before any slice-0 code, not alongside it.

**Decision.** Slice 0 lowers emission onto a **fanout Durable Object**, one per
publishing context, that owns the subscriber registry and the fan-out —
**not** one Cloudflare Queue per event type. A queue-per-topic reuses the
shipped `on queue` consumer path and its `QueueResult` verdict, but a single
queue fanning out to *N* independent subscribers needs a per-subscriber
offset/ack a shared queue does not natively give: one subscriber's retry would
re-deliver to siblings that already acked, which is exactly the failure
isolation §7 asserts. A DO gives each subscriber its own delivery state, and
because a DO is single-threaded, it is a natural, honest home for
per-publisher ordering — the DO can literally sequence one publisher's
emissions, where a bare shared queue cannot. It is also the natural future
owner of a durable replay log, so it is not throwaway relative to that
follow-on track.

**Costs accepted, not hidden.** This puts new runtime machinery — a DO not
required by any shipped first-party capability today — on the emission hot
path, and a fanout DO is a scaling chokepoint at very high fan-out, where a
managed queue would shard more gracefully. Queue-per-topic remains a
documented future optimisation for the high-fan-out, ordering-relaxed case; it
is not slice 0's substrate.

**Consequences.** Slice 0's emit/subscribe loop is implemented against the
fanout-DO shape. This decision does not itself establish the per-publisher
FIFO guarantee as shipped — that is a separate, empirical decision (an
integration fixture against the chosen substrate) recorded once slice 0 has
something to measure, in the tradition of
[ADR 0193](../decisions/0193-multi-context-deploy-ordering.md). Subscriber
failure isolation and the future replay track's log-owner both follow
directly from this shape and need no further foundational decision.
