# 0122 — `Queue` is not a storage kind: a queue is a *delivery* concern, already decomposed into the `from Queue` service protocol (inbound, shipped) and an outbound enqueue capability; the storage catalogue closes at five kinds

- **Status:** Accepted (storage track, slice 5 / Q5; 2026-06-26)
- **Provenance:** the storage track (the open question Q5 — "`Queue` placement: this track vs held-resources, and its delivery contract"). This is the track's last settling decision; it rules `Queue` *out* of the storage catalogue.
- **Realises / sharpens:** `design/bynk-design-notes.md` §10 (which lists `Queue[T]` — "durable async stream" — in the storage-kind catalogue) **against** §1's architecture (§158/§382 — the services-vs-agents line, and "queue subscriptions sit on the runtime side"). Where the catalogue and the architecture disagree, this ADR follows the architecture, which is exactly the "realise and sharpen the design notes" job of a track's settling phase.
- **Relates:** ADR 0078 (`QueueResult` — the `from Queue` service handler's ack/retry verdict, the *inbound* half); ADR 0002 (cron/queue slicing); §12 (at-least-once delivery + the `Idempotency` capability, the safe-use story a queue leans on); the **held-resources / delivery track** (not yet opened) that already owns `Ref[A]` / `Held[T]` / `Connection[F]` and now inherits `Queue`.

## Context

`Queue` is the sixth and last kind in design-notes §10's storage catalogue. The
storage track flagged it (Q5) as not a clean build: it **overlaps the shipped
platform Queue transport** (`service Jobs from Queue`), and the track doc said to
"confirm in the settling phase whether it belongs here or with held resources."

The tension is inside the design notes themselves. §10 lists `Queue[T]` as a
`store` kind alongside `Map`/`Set`/`Log`/`Cache`. But §1's architecture
decomposes what a "queue" *is* along the services-vs-agents line:

- §147 — **protocol vs transport.** `service X from Queue` is a *protocol*
  adapter (the language sees handler shapes); the *transport* (Cloudflare Queues,
  SQS, …) is platform-supplied. This is the **inbound** side and it is **shipped**
  (ADR 0078).
- §158 / §382 — **"long-lived runtime resources flow from services to agents at
  the moment of acceptance."** WebSocket connections, alarm schedules, and
  explicitly **queue subscriptions** "sit on the runtime side of the
  architectural line, but appear in the language as **typed messages** (things the
  runtime delivers to agents) or **capabilities** (things agents ask the runtime
  to do)."

So the architecture already accounts for both directions of "queue" — inbound
delivery as a service protocol, outbound enqueue as a capability — and places both
on the *runtime* side, not in agent-owned durable state. The question Q5 really
asks is whether a `store q: Queue[T]` kind has any residue left after that.

## Decisions

**D1 — A queue is a *delivery* concern, not agent-owned durable state.** A
`Queue`'s defining property is **asynchronous, at-least-once delivery** (§12) — a
runtime/transport responsibility. Storage kinds are the opposite: **agent-owned
state, mutated only by the owning agent's handlers, committed atomically at
handler end** (ADR 0109). Delivery is not storage; conflating them would put a
runtime-side concern behind the `store` surface.

**D2 — Its responsibilities are already decomposed, and neither half is a `store`
field.** The shipped architecture splits "queue" cleanly:

- **Inbound** (the runtime delivers messages to the agent) → the **`from Queue`
  service protocol** — `service Jobs from Queue { on message(m: T) -> Effect[QueueResult] }`,
  with the ack/retry verdict of ADR 0078. **Shipped.**
- **Outbound** (the agent asks the runtime to enqueue work) → a runtime
  **capability** (the platform `Queue` / `Events` capability, §357/§1314), invoked
  like any other `given` capability. To be **formalised by the held-resources /
  delivery track.**

A `store q: Queue[T]` kind would have to *be* one of these, and both are better
modelled as a protocol and a capability respectively — the forms §158 names.

**D3 — `Queue` is removed from the storage-kind catalogue.** The storage
catalogue is **five kinds — `Cell` / `Map` / `Set` / `Log` / `Cache`** (all
shipped, v0.82–v0.95). `Queue` joins `Ref[A]` / `Held[T]` / `Connection[F]` as
**out of this track** — it belongs to the held-resources / delivery track, where
it sits next to its natural neighbours (runtime-managed resources) and the
at-least-once + `Idempotency` (§12) machinery it depends on. The `Queue` kind name
keeps a **relocating diagnostic** at `store q: Queue[…]` — steering to
`service … from Queue` (inbound) or the enqueue capability (outbound) — rather
than the current "lands in a later slice" gate, which would now mislead. (The
diagnostic-message change is a small follow-up; this ADR settles direction.)

**D4 — The agent-local "durable work queue" need, if it arises, is `Log` + a
cursor.** The one reading of §10's terse "durable async stream" that *is* genuine
agent-owned storage — an agent enqueuing deferred work and draining it later (e.g.
on an alarm) — is already expressible: a `store work: Log[Task]` plus a
`store cursor: Cell[Int]` consumed-position. No new kind is needed for v1; a
dedicated convenience can be added later if pressure emerges (the §10 `Local[T]`
deferral sets the precedent).

**D5 — This closes the storage *catalogue*, not yet the whole track.** With
`Queue` relocated, the kind catalogue is complete. Two non-catalogue items remain
before the track retires: the deferred **parity slice** (`state{}` removal + the
`bynk-fmt` codemod, ADR 0108) and the **rehydration** questions Q6/Q7. Those are
the track's only open work; no further *kinds* are coming.

## Consequences

- **The storage-kind catalogue is closed at five.** Every `store` kind that ships
  is agent-owned, atomically-committed durable state — a clean, defensible line.
- **`Queue` becomes the seed of the held-resources / delivery track.** That track
  (to be opened) inherits `Queue` alongside `Ref[A]`/`Connection[F]`, and owns the
  outbound enqueue capability + the at-least-once/`Idempotency` delivery contract.
- **The `from Queue` transport is untouched** — it was never the storage kind;
  this ADR only confirms it is the inbound half and leaves it as-is (ADR 0078).
- **One small follow-up:** retarget the `store … : Queue[…]` diagnostic from
  "later slice" to the relocation steer (D3).
- **Rejected alternative — keep `Queue` as a sixth storage kind** (for the
  work-queue case). Rejected: §10's one-liner under-specifies enqueue/dequeue and
  delivery semantics; the genuine storage reading is covered by `Log` + a cursor
  (D4); and the streaming/delivery flavour is a runtime concern the architecture
  (§158) explicitly places off the storage line. Adding it would buy a name that
  collides with the shipped `from Queue` transport for a capability already
  expressible.
