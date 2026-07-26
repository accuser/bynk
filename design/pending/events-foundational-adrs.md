---
level: patch
changelog: The Events track's foundational ADRs — fan-out substrate, closed-protocol-set extension, pattern-dispatch semantics, and the replay split — land before slice 0
---

## ADR: events-fanout-substrate
title: The Events fan-out substrate is a per-publisher fanout Durable Object, not queue-per-topic
summary: Slice 0 lowers `given Events` emission onto a fanout DO so per-publisher ordering and subscriber failure isolation are achievable, not aspirational

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

## ADR: events-protocol-set-extension
title: Events joins the closed protocol set; its header is the first to take a type argument
summary: Extends ADR 0079 to a sixth service protocol; `from Events(E)` parameterises the protocol header by the subscribed event type

**Context.** The service protocol set is closed
([ADR 0079](../decisions/0079-protocols-closed-set.md)), so admitting
`from Events(...)` (the Events track, spine #936) is a deliberate extension of
that set, not a neutral addition. `from <protocol>` on the service header
already ships ([ADR 0077](../decisions/0077-service-protocol-on-header.md)),
and `from WebSocket` already proves a `from`-protocol carrying a bundle of
associated surface — but no shipped protocol today takes a type argument on
the header.

**Decision.** Events becomes the service protocol set's sixth member.
`from Events(E)` parameterises the protocol by the event type `E` the service
subscribes to — the event type is the topic, not a string name — reusing the
existing protocol-header grammar position rather than inventing a new one. The
`on event` handler shape joins the per-protocol handler-shape checking that
already rejects a mismatched handler for every other protocol (e.g. a queue
handler returning a `Response`).

**Consequences.** Membership in the closed set is a one-line extension; the
genuinely new slice-0 grammar/checker work is the type-argument
parameterisation itself, which no prior protocol needed. A later slice adds a
structural pattern alongside the type argument
(`events-pattern-dispatch-deliver-and-filter`, this same pending file); that
pattern is additional refinement on top of this header shape, not a second
extension of the closed set.

## ADR: events-pattern-dispatch-deliver-and-filter
title: Subscription pattern filtering reuses refined-pattern dispatch and commits to deliver-and-filter
summary: No bespoke matching engine is introduced for Events; the DO delivers every emission and the subscriber's generated guard filters, with server-side pre-filtering left a later optimisation

**Context.** `from Events(E { region: Domestic, .. })` (§7 lines 198–227 of
`design/bynk-design-notes.md`, the Events track, spine #936) filters
emissions by a structural pattern on the payload, type-checked against the
event shape and enforced before the handler runs. §7 explicitly frames this as
mirroring the auth pattern already on services, and multi-actor sum dispatch
([ADR 0090](../decisions/0090-multi-actor-sum-dispatch.md)) plus authorisation
invariants
([ADR 0091](../decisions/0091-authorisation-invariants-refinement-actors.md))
already establish structural, declarative, enforced-before-the-handler
dispatch; the payload pattern itself is the shipped refined-pattern and
nested-payload machinery
([ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md),
[ADR 0252](../decisions/0252-or-patterns.md),
[ADR 0253](../decisions/0253-refined-patterns.md)). The one thing precedent
does not settle: *where* the filter runs — §7 wants "server-side filtering
where the platform supports it, deliver-and-filter as a transparent fallback"
(line 227).

**Decision.** Slice 1 ships **deliver-and-filter only**: the fan-out substrate
(`events-fanout-substrate`, this same pending file) delivers every emission of
`E` to every subscriber of `E`, and the subscriber's generated guard — built
from the same type-checked pattern, reusing the shipped refined-pattern
machinery rather than a new matching engine — filters before the handler body
runs. Server-side pre-filtering at the substrate is left a later,
semantics-preserving optimisation: it may change what work an unmatched
emission costs, never what a subscriber observes.

**Consequences.** Slice 1 introduces no bespoke pattern-matching mechanism.
Its fixtures must include negative cases — an emission that must **not**
arrive at a subscriber whose pattern excludes it — since deliver-and-filter
makes over-delivery a live risk if the generated guard is wrong, not merely a
hypothetical one a smarter substrate would have avoided by construction.

## ADR: events-replay-out-of-scope
title: Event replay and backfill split to a separate, future, currently-unfiled track
summary: This track ships the live pub-sub core and replay's seams (eventId, schemaVersion, a fanout-DO log-owner) but not replay itself, which depends on the not-yet-shipped durable Idempotency provider

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
