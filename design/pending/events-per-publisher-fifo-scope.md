---
level: patch
changelog: Per-publisher event ordering is scoped to non-concurrent emission from one agent — verified empirically on real workerd, and narrowed from the broader claim design/tracks/events.md §7 originally asserted
---

## ADR: events-per-publisher-fifo-scope
title: Per-publisher event FIFO holds within a batch and across non-overlapping calls, not across concurrent invocations of one agent
summary: The §3.4 empirical check (spine #936), run against a real Cloudflare Durable Object under workerd — the guarantee ships narrower than design/bynk-design-notes.md §7 asserted

**Context.** Events slice 0 (#939, PR #951, ADR 0284/0288) shipped the
fan-out Durable Object but deliberately left `design/tracks/events.md` §3.4
open: the design notes assert that events from the same publishing *agent*
are delivered to each subscriber in emission order, but this is a claim about
runtime behaviour that had never been measured against the real substrate —
exactly the posture [ADR 0193](../decisions/0193-multi-context-deploy-ordering.md)
established for the deploy track's own binding-resolution-order claim
("the assumption was wrong" was that ADR's own headline finding).

**The mechanism, traced from the generated code.** The fan-out DO
(`bynk-emit/src/emitter/events_fanout.rs`) is a **stateless router**: its
constructor discards `DurableObjectState` entirely, it performs no storage
operation, and there is no `blockConcurrencyWhile` anywhere in it or in the
runtime. Cloudflare's Durable Object input gate closes only around storage
operations — an outbound `await deliverEvent(...)` is not one — so the DO's
delivery loop (`for (const ev of events) { for (const sub of subs) { await
deliverEvent(...) } }`) *yields* at every delivery. Within one batch this is
harmless: one handler's flush is one array, one `fetch`, one sequential loop,
and yielding mid-loop cannot reorder that loop's own resumptions. Across
batches it is not: the publisher flushes *after* `commitState`
(`bynk-emit/src/emitter/emit.rs`), so the agent's own storage gate has
already reopened before the fan-out fetch is issued, and two overlapping
invocations of the same agent key can have both flushes in flight
simultaneously with nothing to serialise them.

**The evidence.** `bynkc/tests/events_ordering_workerd.rs` — a two-context
project (`fifo.publisher` / `fifo.subscriber`) compiled for the Workers
target and run under two real `wrangler dev` (workerd) processes, wired by
wrangler's own dev registry, exactly the topology `bynk dev` already uses for
a multi-context project. Three cases:

- **(a) eight emissions from one handler body** — delivered in order every
  run. Holds by construction (one batch, one sequential delivery loop).
- **(b) eight successive, sequentially-awaited invocations of one agent** —
  observed to hold in every trial; the mechanism traced above explains why
  (each call's flush completes before the next begins, so there is no
  concurrency to interleave).
- **(c) two overlapping invocations of one agent** (`burst("p")` and
  `burst("q")`, issued from separate threads so the requests genuinely
  overlap) — **interleaving was observed in all 9 genuine trials run** (a
  mostly clean alternating `p, q, p, q, …` delivery order, with the
  occasional `q, p` or `p, p, q, q` swap at the start). Each burst's own
  relative order survived every time (`p0..p7` and `q0..q7` each arrived in
  order) — only the *relative* interleaving of the two bursts was
  unconstrained, exactly as the mechanism above predicts.

A harness bug needed fixing first, recorded here so it isn't mistaken for a
substrate finding: `wrangler dev` binds a default V8 inspector port (9229)
whether or not `--inspector-port` is passed, so the test's two spawned
instances collided with each other and, in three early trials, stalled past
the 180s boot deadline — which the test's local skip path (silent without
`BYNK_REQUIRE_WORKERD=1`) let through as a false pass rather than a failure.
Fixed by pinning distinct `--inspector-port` values per instance
(`bynk/src/dev.rs`'s multi-context `serve_args` does the same, for the same
reason). The 9-trial count above is only the genuine runs: 3 before the fix
(which happened not to hit the collision) plus 6 run afterward with
`BYNK_REQUIRE_WORKERD=1` forcing a hard failure instead of a skip; the 3
false-pass runs are excluded, not folded in.

Because whether an interleave manifests is a scheduling race, the committed
test does not assert on it in either direction — asserting "must interleave"
or "must stay in two contiguous blocks" would both be flake generators. What
CI asserts for (c) instead: completeness (every event delivered exactly
once) and each burst's own internal order — both deterministic. The
interleaving observation itself is this ADR's evidence, not a CI gate.

**Attribution limit.** An interleave observed under (c) falsifies the
end-to-end ordering claim, but cannot be attributed to the fan-out DO
specifically in isolation from the subscriber: two `on event` invocations
also run concurrently at the subscriber's `Trace` agent, which is a single
Durable Object and therefore itself serialises the writes it receives (no
lost update was observed in any trial) — but the *order* two concurrent
deliveries arrive in is exactly the thing under test, so a subscriber-side
race and a fan-out-side race would look identical from this fixture alone.
Cases (a) and (b) have no concurrency anywhere in the chain and are
attribution-clean; case (c)'s finding is stated as "order is not preserved
end-to-end under concurrent invocation of one agent," not "the fan-out DO
reorders."

**Decision.** The guarantee ships **narrower** than `design/bynk-design-notes.md`
§7 originally asserted. Per-publisher FIFO holds for:

- the relative order of any single emission batch (everything emitted
  within one handler invocation), and
- successive, non-overlapping invocations of one agent (each call's flush
  completes before the next begins).

It does **not** hold across batches produced by *concurrent* invocations of
the same publishing agent — nothing in the current substrate serialises two
overlapping flushes, and this decision does not change that. A subscriber
that needs a total order across concurrent publishes must carry its own
sequence number in the event payload; this is not a language-level
guarantee.

**The emitter is deliberately not changed by this increment.** A fix (e.g.
`blockConcurrencyWhile` around the fan-out DO's dispatch loop) would
serialise all fan-out for a publishing context, worsening the tail-latency
gap ADR 0288 already named, and would still not address the publisher-side
initiation-order gap (the flush happens after the storage gate reopens).
Closing this is a substrate redesign in ADR 0284/0288's own "separate,
empirical" family, not a documentation increment.

**Consequences.** `design/tracks/events.md` §3.4, §5, §6 (threat model), and
§8 are updated to state the scoped guarantee rather than the open question;
the user-facing "Understand events" guide's per-platform table is corrected
— it previously overclaimed "single-threaded — emissions … are sequenced,"
which conflates single-threaded execution with serialisation across
`await`-yielding concurrent requests. Spine issue #936's slice-status
checklist, already stale (slice 0 unchecked despite shipping in #939), is
brought current alongside this change.
