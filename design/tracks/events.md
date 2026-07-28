# The Events protocol — in-system pub-sub for cross-context decoupling

- **Status:** Settling (draft). This is a settling-draft doc per
  [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md): the
  **spine issue** is
  [#936](https://github.com/accuser/bynk/issues/936); this doc lands via a
  draft PR referencing it (*"Part of #936"*, never `Closes`). §3.1, §3.2, §3.3,
  and §3.6 are now genuinely settled, with their foundational ADRs landed as
  [ADR 0284](../decisions/0284-events-fanout-substrate.md),
  [ADR 0285](../decisions/0285-events-protocol-set-extension.md),
  [ADR 0286](../decisions/0286-events-pattern-dispatch-deliver-and-filter.md),
  and [ADR 0287](../decisions/0287-events-replay-out-of-scope.md); §3.5 is a
  named slice-3 concern. **Slice 0 has shipped** (#939): `event` declarations,
  `given Events` emission with owner-only enforcement, and unpatterned `from
  Events(E)` subscription, across contexts and across all three platforms —
  its own implementation decisions (the concrete fan-out mechanism per
  platform, and the owner-only checker pass) landed as
  [ADR 0288](../decisions/0288-events-slice0-fanout-implementation.md) and
  [ADR 0289](../decisions/0289-events-owner-only-emission-check.md). **§3.4 is
  now also settled, empirically:** per-publisher FIFO holds within one
  emission batch and across successive non-overlapping calls to one agent,
  but not across concurrent invocations of the same agent — narrower than §7
  originally asserted, measured on real `workerd` and recorded in
  `design/pending/events-per-publisher-fifo-scope.md` (pre-stamp). Structural
  pattern refinement on the subscription header (§3.3's deliver-and-filter),
  the envelope, and additive versioning remain later slices.
- **The one scope decision made up front.** This track delivers the **live
  pub-sub core** — declaration, emission, subscription with pattern refinement,
  the envelope, and additive versioning. It **splits event replay / backfill
  out** to a separate, future, currently-unfiled track (§3.6), which also
  absorbs the actors track's deferred **Q8 (replay/ordering)**
  ([#260](https://github.com/accuser/bynk/issues/260)). This mirrors
  [`idempotency-capability.md`](idempotency-capability.md) §3.2, which shipped
  the in-memory provider and moved its durable half to a future track so its own
  scope "stays honest about what it actually delivers."
- **Realises:** `design/bynk-design-notes.md` §7 ("Events in depth", lines
  162–288) and the emit-lowering note (line 1321); the §12 event-subscriber
  idempotency idiom (lines 641, 656–666), whose substrate — the `Idempotency`
  capability keyed on `env.eventId` — **already shipped** ahead of this track
  ([#554](https://github.com/accuser/bynk/issues/554), the
  [`idempotency-capability.md`](idempotency-capability.md) track).
- **Posture:** Feature track per
  [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on all three
  axes (§2), including the **correctness/safety-boundary** one: cross-context
  fan-out where a wrong pattern-match delivers a fact to a subscriber that should
  not see it, and a wrong ordering or duplication story silently corrupts
  downstream state.
- **The unusual starting position — triage, not invention.**
  [`idempotency-capability.md`](idempotency-capability.md) opened with "*What's
  already true: nothing*" — every question novel, no sibling to mirror. Events is
  the reverse: §7 is a large, opinionated, internally-consistent spec. The
  settling work here is therefore **sifting** the settled majority (§3.0) from a
  small set of genuinely-open corners (§3.1–§3.5), not designing the surface from
  scratch. The risk this doc must not fall into is relitigating settled §7 prose;
  the risk it must not hide is treating the two hard, substrate-level unknowns
  (the fan-out substrate, per-publisher ordering on Cloudflare) as settled merely
  because §7 asserts an outcome for them — the fan-out substrate (§3.1) is now
  settled by argument, and per-publisher ordering (§3.4) deliberately stays
  open until it can be settled by evidence instead.

## 1. The theme

A context declares a typed fact with the `event` keyword; a handler in that
context emits it with `given Events`; zero or more **subscriber services** in
other contexts, declared `from Events(SomeEvent)`, receive each emission as an
ordinary typed handler invocation. The event *type* is the topic — no string
topic names — and a subscriber may refine which emissions it receives with a
structural pattern on the payload, mirroring the auth pattern already on
services. Emission is fire-and-forget and releases at handler commit; an aborted
handler emits nothing. Delivery is at-least-once; effectful subscribers dedup
via the already-shipped `Idempotency` capability keyed on the runtime
envelope's `eventId` — though only to that capability's current limit, since
its **in-memory** provider does not survive a crash, so a duplicate delivered
across an isolate restart is not deduped until the durable provider (a future,
unfiled track, §3.6) exists (see the threat model, §6).

The end state when this track retires: `event` declarations, `given Events`
emission, and `from Events(...)` subscription (pattern-refined, with envelope
metadata and additive versioning) are real, checked surface with their one
first-party provider, and §7's `PaymentConfirmed` worked example (lines 166–241)
compiles and runs end to end — **minus** replay/backfill, which §3.6 moves to a
follow-on track.

## 2. Why a track (the ADR 0076 trigger)

- [x] **Multi-increment.** The core is naturally four-to-five separable slices
  (§4): the emit/subscribe loop, pattern refinement, the envelope + idempotency
  idiom, and versioning. Each lands as its own increment proposal.
- [x] **Surface not yet fully settled.** Despite §7's richness, the *substrate*
  it lowers onto is an explicit open fork ("Queues with topic-as-queue routing,
  **or** a custom event-fanout DO", line 1321), and the per-publisher FIFO
  guarantee (§7, line 231) is asserted against a platform whose queues do not
  provide it for free — both need settling, one empirically.
- [x] **Correctness/safety boundary.** Events cross context boundaries carrying
  domain payloads (§7 permits opaque fields, line 180) to an open set of
  subscribers. A pattern-filter bug over-delivers; an owner-emission-enforcement
  bug lets a context emit another's facts; a broken ordering or dedup story
  double-applies effects. See the threat model, §6.

## 3. Open design questions

### 3.0 — What §7 already settles (recorded so the settling pass does not reopen it)

Treated as **committed by the design notes**, not open questions for this track:

- **Type-as-topic routing** (§7, line 229) — every emission of `E` is offered to
  every subscriber of `E`; no explicit topic names, no namespace/typo risk.
- **Owner-only emission** (§7, line 180) — the compiler enforces that only the
  declaring context emits a given event type; `Events` is parameterised by the
  publishing context.
- **Release-at-commit** (§7, line 196) — emission releases at handler commit, an
  aborted handler emits nothing; identical semantics to outbound agent calls
  ([ADR 0106](../decisions/0106-async-message-send.md)).
- **Subscribers are services, not agents** (§7, line 280) — broadcast has no
  agent address; a subscriber service routes to a specific agent from the payload
  (`Orders(e.orderId).…`).
- **Subscriber failure isolation** (§7, line 282) — each subscription is an
  independent handler invocation with its own atomic transaction; one fault does
  not propagate to sibling subscribers.
- **Additive versioning by field defaults + breaking-by-convention** (§7, lines
  245–256, 278) — old wire events fill missing fields from pure default
  expressions; renames/narrowings introduce a new versioned event type, not a
  language feature.
- **The at-least-once safety floor** — subscribers taking effects on receipt
  dedup on `env.eventId` via the shipped `Idempotency` capability (§12, line 241;
  [`idempotency-capability.md`](idempotency-capability.md)). This track *reuses*
  it; it does not rebuild it.

### 3.1 — The fan-out substrate on Cloudflare — SETTLED (the load-bearing fork)

**The question.** §7's lowering note offers a fork without choosing it: emit
"maps to Queues with topic-as-topic routing, **or** a custom event-fanout DO for
higher-fanout scenarios" (line 1321). This is the hard, expensive-to-reverse
decision of the whole track, because **three** of §7's asserted properties all
hang off it: per-publisher FIFO ordering (§3.4), subscriber failure isolation
with independent transactions, and (for the follow-on track) replay from a
durable log.

**The two candidates and their tension.**

- **Queue-per-topic (topic-as-queue).** One Cloudflare Queue per event type;
  each subscriber a consumer. Cheap, reuses the shipped `on queue` consumer path
  and `QueueResult` verdict ([ADR 0078](../decisions/0078-queueresult-typed-verdict.md)).
  But a single queue fanning out to *N* independent subscribers, each with its
  own at-least-once retry and its own dead-letter policy, is not what a plain
  consumer queue is — it needs a per-subscriber offset/ack, which a shared queue
  does not natively give. Failure isolation (§7 line 282) is the specific casualty:
  one subscriber's retry must not re-deliver to siblings that already acked.
- **Fanout DO.** A Durable Object owns the subscriber registry and the fan-out,
  giving per-subscriber delivery state and a natural home for per-publisher
  ordering (a DO is single-threaded) and, later, the replay log. But it is new
  runtime machinery, a scaling chokepoint at high fan-out, and it puts a DO on the
  emission hot path.

**Settled.** Slice 0 lowers emission onto the **fanout-DO** shape, because it is
the only one of the two that can *honestly* deliver §7's per-publisher ordering
and failure-isolation guarantees rather than approximate them — and the
follow-on replay track (§3.6) needs a durable log-owner anyway, so the DO is not
throwaway. The queue path stays a documented future optimisation for the
high-fanout, ordering-relaxed case, not slice 0's substrate. Recorded as
[ADR 0284 — events-fanout-substrate](../decisions/0284-events-fanout-substrate.md)
— the **foundational ADR landed before slice 0** (§5); everything else
composes above it. This settles the substrate choice itself; per-publisher
FIFO as a *verified* guarantee (§3.4) was a separate empirical decision, now
also settled — scoped narrower than this section's rationale implies (see
§3.4).

### 3.2 — Admitting Events to the closed protocol set — SETTLED, precedented

**The question.** Events is a sixth service protocol. The protocol set is
**closed** ([ADR 0079](../decisions/0079-protocols-closed-set.md)), so admitting
`from Events(...)` is a deliberate extension of that closed set, not a neutral
addition. The `on event` handler shape must join the per-protocol handler-shape
checking that already rejects, e.g., a queue handler returning a `Response`
(§7 line 160).

**Why this is mostly mechanical.** `from <protocol>` on the service header ships
([ADR 0077](../decisions/0077-service-protocol-on-header.md)), and `from
WebSocket` already proves a `from`-protocol carrying a bundle of associated
surface (the WebSocket track). Events differs in one real way: `from Events(E)`
**parameterises the protocol by an event type**, and (§3.3) by a pattern — no
shipped protocol takes a type argument on the header. That parameterisation is
the genuinely new grammar/checker slice-0 work; the closed-set membership itself
is a one-line extension. Recorded as
[ADR 0285 — events-protocol-set-extension](../decisions/0285-events-protocol-set-extension.md).

### 3.3 — Subscription pattern refinement — SETTLED, leaned hard on precedent

**The question.** `from Events(E { region: Domestic, .. })` filters emissions by a
structural pattern (§7 lines 198–227). The pattern is type-checked against the
event shape at compile time, enforced by the runtime before the handler runs, and
its statically-known fields are available in the body (`e.region` is statically
`Domestic`).

**Why it leans on precedent.** §7 explicitly frames this as "mirroring the auth
pattern on services" — and multi-actor sum dispatch
([ADR 0090](../decisions/0090-multi-actor-sum-dispatch.md)) plus authorisation
invariants ([ADR 0091](../decisions/0091-authorisation-invariants-refinement-actors.md))
already establish structural, declarative, enforced-before-the-handler dispatch.
The payload pattern itself reuses the shipped refined-pattern and nested-payload
machinery (ADRs [0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md),
[0252](../decisions/0252-or-patterns.md),
[0253](../decisions/0253-refined-patterns.md)). **The one thing settled here:**
where the filter runs — §7 wants "server-side filtering where the platform
supports it, deliver-and-filter as a transparent fallback" (line 227). Slice 1
ships the **deliver-and-filter fallback** only (the DO delivers, the
subscriber's generated guard filters), with server-side pre-filtering a later
optimisation that cannot change observable semantics. Recorded as
[ADR 0286 — events-pattern-dispatch-deliver-and-filter](../decisions/0286-events-pattern-dispatch-deliver-and-filter.md).

### 3.4 — Per-publisher FIFO: the contract vs. what Cloudflare delivers — SETTLED empirically (scoped narrower than asserted)

**The question.** §7 asserts a specific ordering *contract* (line 231): events
from the same publishing agent are delivered to each subscriber in emission
order; across publishers, no ordering. The publisher is the *agent*, not the
context. This composes with the atomic-handler invariant (events within a handler
release at commit in emission order; events from successive handlers of one agent
release in handler order).

**Settled, by measurement, and narrower than asserted.** The deploy track's
discipline applies directly here — it settled "Cloudflare resolves bindings
at upload — a hard barrier, not a soft nicety" **empirically**, not by
assertion ([ADR 0193](../decisions/0193-multi-context-deploy-ordering.md)),
and its headline finding was "the assumption was wrong." This guarantee got
the same treatment: `bynkc/tests/events_ordering_workerd.rs` runs a
two-context project on real `workerd` under two `wrangler dev` processes and
measures three cases. **Holds:** emissions within one handler body (one
batch, one sequential delivery loop); successive, sequentially-awaited
invocations of one agent (each flush completes before the next begins).
**Does not hold:** two *overlapping* invocations of the same agent —
interleaving was observed in every trial run. The mechanism: the fan-out DO
(§3.1) is a **stateless router** with no storage operation and no
`blockConcurrencyWhile`, so it yields at every delivery instead of gating on
one; the publisher's own flush happens *after* `commitState` reopens that
agent's storage gate, so two concurrent invocations can have both flushes in
flight at once with nothing to serialise them. Single-threaded DO execution
prevents parallelism, not interleaving across `await`-separated batches — the
§3.1 rationale ("a DO can sequence a publisher's emissions") holds for a
*single* batch, not across concurrent ones. Recorded as the
`events-per-publisher-fifo-scope` ADR,
`design/pending/events-per-publisher-fifo-scope.md` (pre-stamp), including
the decision **not** to change the emitter to close this gap (a substrate
redesign, not a documentation increment — see §6 for the consequence a
subscriber needs to know).

### 3.5 — The cross-build schema registry — OPEN

**The question.** Additive versioning (§3.0) needs `env.schemaVersion`, and §7
specifies "the compiler maintains a schema registry across builds, computing the
version from the type's structural shape" (line 258), emitting a
schema-evolution report when a version changes, with optional `@schema(N)` pins.
This is **new persistent build-time state** — the family of the
increment-allocation stamp ([ADR 0206](../decisions/0206-allocation-on-main.md))
and the cross-context contract hash
([ADR 0200](../decisions/0200-cross-context-contract-hash.md)), which already
computes a canonical structural normal form this could reuse.

**Why it is open.** *Where* the registry lives (a committed file stamped on
merge, like allocation? a derived build artifact?), *how* a version is computed
(reuse the 0200 normal form, or a distinct event-schema hash?), and *what* counts
as a version-bumping change all need settling. This is a slice-3 concern, not a
slice-0 blocker; naming it now keeps versioning from being retrofitted onto an
envelope that did not plan for it.

### 3.6 — Replay / backfill — SPLIT OUT to a future track (the precedent move; recorded)

**Decision.** Event replay — a new subscriber "backfilling from log history",
the runtime upgrading old wire events to the current schema on read (§7 lines
243, 276) — is **not delivered by this track.** It moves, together with the
inherited actors **Q8 (replay/ordering)**
([#260](https://github.com/accuser/bynk/issues/260)), to a separate, future,
currently-unfiled track.

**Why split, not carry.** Three reasons, exactly paralleling
[`idempotency-capability.md`](idempotency-capability.md) §3.2's durable-provider
split:

1. **It drags in an unbuilt dependency this track cannot satisfy.** Replay-safe
   subscribers dedup on `env.eventId` across a crash and a re-delivery window
   measured in the *log's* retention, not a request retry — which wants the
   **durable** `Idempotency` provider. Only the **in-memory** provider shipped;
   the durable one was itself deferred to a future unfiled track
   (`platform_capability_overrides.md`; `idempotency-capability.md` §3.2/§3.3).
   Events' replay story therefore depends on a provider that does not exist yet.
   Carrying replay here would make this track's completion hostage to a second
   unbuilt track.
2. **It needs a durable event log — its own substrate design.** §7 asserts
   backfill works but never says what stores the log, who owns retention, or the
   window. That is a substrate decision as heavy as §3.1 and genuinely unspecified
   — the definition of "not settled enough to carry."
3. **The live core is coherent and shippable without it.** Emit → subscribe →
   pattern-filter → envelope → additive-version is a complete, useful pub-sub
   system on its own. Replay is an additive capability on top, not a precondition
   for the core to be correct.

**What this track still owes replay (so the split is honest, not a dodge).** The
envelope (§4 slice 2) must carry `eventId` and `schemaVersion` from day one, and
the fan-out substrate (§3.1) must not foreclose a durable log — the §3.1 fanout
DO is chosen partly because it is the natural future log-owner. This track ships
the *seams* replay will need; it does not ship replay. Recorded as
[ADR 0287 — events-replay-out-of-scope](../decisions/0287-events-replay-out-of-scope.md)
— durable and citable rather than implicit in a diff.

## 4. Candidate slice decomposition

MVP-first. Each slice is an ordinary increment proposal, a sub-issue of the spine,
`accepted` before build.

- **Slice 0 — the emit/subscribe loop.** `event` declarations (owner-only, §3.0),
  the `Events` capability with `emit` (release-at-commit), and pattern-less
  `from Events(E)` subscriber services delivering via the §3.1 substrate,
  deliver-and-filter. Opens the closed protocol set to Events (§3.2) and adds the
  event-type-parameterised `from Events(E)` header grammar. The smallest thing
  that proves one publisher → one subscriber across contexts. Proven against a
  real `tsc --strict` project fixture (a two-context emit/receive), not
  golden-diffed alone.
- **Slice 1 — subscription pattern refinement.** `from Events(E { field: X, .. })`
  (§3.3), reusing the refined-pattern machinery; statically-known fields available
  in the body; deliver-and-filter enforcement. Multi-dimensional refinement
  composes; a pattern-less subscriber still matches all.
- **Slice 2 — the envelope + the idempotency idiom.** `EventEnvelope`
  (`eventId`, `publisherId`, `emittedAt`, and `schemaVersion` reserved for slice
  3) passed alongside the payload; the documented `env.eventId` →
  `Idempotency.dedup`/`remember` pattern for effectful subscribers (§12). This is
  where the idempotency track's parked "event-subscriber sugar" (its own §7
  possible slice 1) actually lands — settle here whether it is bespoke syntax or
  documented convention.
- **Slice 3 — additive versioning.** Default expressions on event-type fields
  (pure, compiler-verified), `env.schemaVersion`, and the cross-build schema
  registry (§3.5) with its evolution report and optional `@schema(N)` pin.
- **Slice 4 — `via` version-aware dispatch.** The `via schema(...)` envelope
  pattern clause (§7 lines 260–274): literal versions, ranges, `_`; a
  no-`via` subscriber receives any version. Generalisable `via <field>(pattern)`
  grammar, but only `via schema(...)` committed here.

**Not slices of this track** (moved to the future replay track, §3.6): replay /
backfill-from-log, the durable event log substrate, and the inherited actors Q8
([#260](https://github.com/accuser/bynk/issues/260)). Recorded there, not here,
so this track's scope stays honest about what it delivers — the same discipline
`idempotency-capability.md` applied to its durable provider.

**Ordering is not a slice.** Per-publisher FIFO (§3.4) is a *property* every slice
must preserve; it is now established and verified empirically against the
chosen substrate, scoped to what the substrate actually delivers (§3.4).

## 5. Front-loaded ADR candidates

The load-bearing, hard-to-reverse decisions to land up front (ADR 0076/0167's
"foundational ADRs"). Four of the five landed as ADRs 0284–0287 ahead of any
slice-0 code; the fifth — deliberately deferred until there was something to
measure — has now landed too, recorded in
`design/pending/events-per-publisher-fifo-scope.md` (pre-stamp; its ADR
number is assigned at merge).

- **[Written] The Events fan-out substrate** (§3.1, `events-fanout-substrate`) —
  records the fanout-DO-vs-queue choice and *why*, because per-publisher
  ordering, subscriber failure isolation, and the future replay log all depend
  on it and it is the most expensive decision to reverse. **This is the one
  that had to land before slice 0 — it now has.**
- **[Written] Events joins the closed protocol set; the header takes a type
  argument** (§3.2, `events-protocol-set-extension`) — records extending
  [ADR 0079](../decisions/0079-protocols-closed-set.md) to a sixth protocol and
  the new event-type-parameterised `from Events(E)` header shape (no prior
  protocol parameterises on a type).
- **[Written] Per-publisher FIFO is a verified guarantee, scoped to what was
  measured** (§3.4, `events-per-publisher-fifo-scope`) — records the
  empirical fixture that backs the ordering claim on the chosen substrate, in
  the deploy track's evidence-not-assertion tradition
  ([ADR 0193](../decisions/0193-multi-context-deploy-ordering.md)), and the
  finding that the guarantee holds only within a batch and across
  non-overlapping calls — not across concurrent invocations of one agent.
- **[Written] Subscription pattern dispatch reuses auth/refined-pattern
  machinery** (§3.3, `events-pattern-dispatch-deliver-and-filter`) — records
  that no bespoke matching engine is introduced for Events; deliver-and-filter
  is the committed semantics, server-side pre-filter a later
  semantics-preserving optimisation.
- **[Deferred to slice 3] Event schema versioning + the cross-build registry**
  (§3.5) — slice-3 ADR; records where the registry lives and whether it reuses
  the ADR 0200 normal form. Not a slice-0 blocker, so not written now.
- **[Written] Replay is out of scope** (§3.6, `events-replay-out-of-scope`) —
  records the split decision itself, so it is durable and citable rather than
  implicit in a diff, and names the future track and its dependency on the
  durable `Idempotency` provider.

## 6. Threat model

**Asset.** The event payload in transit from a publishing context to an open set
of subscribers — potentially carrying domain data (§7's `PaymentConfirmed`
amount/customer/region), including opaque fields a foreign subscriber may hold but
not introspect (§7 line 180) — and the two correctness guarantees the mechanism
makes: that *only* the owning context emits a given fact, and that a subscriber
receives *exactly* the emissions its pattern admits.

**Adversary / failure modes.**

- **Over-delivery via a pattern-filter bug.** A subscriber whose generated guard
  admits emissions its declared pattern should exclude sees data outside its
  intended slice — a cross-context data exposure if the excluded emissions carry
  data that subscriber should not react to. *Mitigation:* the filter is generated
  from the type-checked pattern and enforced before the body runs (§3.3), and
  deliver-and-filter keeps the guard in one generated place; slice 1's fixtures
  must include negative cases (an emission that must **not** arrive).
- **Forged/cross-context emission.** A context emitting another context's event
  type would let it fabricate facts attributed to a peer. *Mitigation:* owner-only
  emission is statically enforced (§3.0, §7 line 180) — this is a compile error,
  not a runtime check, and is the primary boundary guarantee.
- **Duplicate delivery double-applying effects.** At-least-once means a subscriber
  can receive the same event twice. *Mitigation:* the shipped `Idempotency`
  capability keyed on `env.eventId` (slice 2, §12) — but note the **in-memory**
  provider does not survive a crash, so a duplicate across an isolate restart is
  *not* deduped until the durable provider (the future track, §3.6) exists. This
  is the same accepted-gap posture idempotency itself shipped with, named here not
  hidden.
- **Mis-ordered delivery corrupting state.** A subscriber that assumes emission
  order (e.g. a `Created` before an `Updated`) breaks if the substrate reorders.
  *Mitigation, and its real scope (§3.4, empirically measured):* order is
  preserved within one emission batch and across successive,
  non-overlapping calls to one agent — **not** across *concurrent*
  invocations of the same agent, which the fan-out DO's stateless,
  yield-at-every-`await` delivery loop can interleave. Cross-publisher
  ordering is explicitly **not** guaranteed either. A subscriber that must
  not observe either of these reorderings needs its own sequence number in
  the payload; this is a design constraint the docs must call out, not
  something the runtime absorbs on the subscriber's behalf.
- **Payload validation at the boundary.** An event is a value crossing a trust
  boundary; refined fields in the payload are validated on receipt, malformed
  events routed to the platform dead-letter policy (§7 line 286, the shipped
  boundary-validation model). Subscribers never defensively re-check a refinement
  the type already established.

## 7. Slice status

- [x] Slice 0 — emit/subscribe loop + closed-protocol-set extension (#939)
- [ ] Slice 1 — subscription pattern refinement (deliver-and-filter)
- [ ] Slice 2 — `EventEnvelope` + the `env.eventId` idempotency idiom
- [ ] Slice 3 — additive versioning + the cross-build schema registry
- [ ] Slice 4 — `via schema(...)` version-aware dispatch
- [ ] (Not a slice of this track) Replay / backfill + actors Q8 — future track (§3.6)

## 8. Done when

- [ ] `event` declarations, `given Events` emission, and pattern-refined
  `from Events(...)` subscription are checked surface with their one first-party
  provider; §7's `PaymentConfirmed` example compiles and runs end to end (a real
  `tsc --strict` project fixture), **minus** replay. **Slice 0 done** (#939):
  `event`/`given Events`/unpatterned `from Events(E)` ship, verified by a real
  `tsc --strict` two-context project fixture on both Cloudflare and Bundle
  targets. Still open: the pattern refinement on the subscription header
  itself (§3.3/slice 1) — this bullet stays unchecked until that lands too.
- [x] Events is a member of the closed protocol set
  ([ADR 0079](../decisions/0079-protocols-closed-set.md)) with the
  event-type-parameterised header; the per-protocol handler-shape check rejects a
  malformed `on event` handler. Shipped in slice 0 (#939).
- [x] Per-publisher FIFO (§3.4) is backed by an **empirical** integration fixture
  on the chosen substrate, not asserted (`bynkc/tests/events_ordering_workerd.rs`).
  The guarantee entering the spec is **narrower** than §7 originally asserted:
  ordered within a batch and across non-overlapping calls to one agent, not
  across concurrent invocations of one agent.
- [ ] The envelope carries `eventId`, `publisherId`, `emittedAt`, `schemaVersion`;
  effectful subscribers dedup on `env.eventId` via the shipped `Idempotency`
  capability (no new dedup mechanism built here).
- [ ] Additive versioning (defaults + `env.schemaVersion` + the registry) and
  `via schema(...)` dispatch ship; the schema-evolution report is emitted on a
  version change.
- [ ] The doc is explicit that **replay/backfill and the actors Q8**
  ([#260](https://github.com/accuser/bynk/issues/260)) are **not** delivered by
  this track — named as a future track with its durable-`Idempotency` dependency,
  not silently dropped.
- [x] Foundational ADRs (§5) written that do not depend on slice-0 code —
  the substrate ADR (§3.1, ADR 0284), the closed-protocol-set extension
  (§3.2, ADR 0285), the deliver-and-filter dispatch commitment (§3.3, ADR
  0286), and the replay split (§3.6, ADR 0287) — landed **before** slice 0.
- [x] The ordering-evidence ADR (§3.4, `events-per-publisher-fifo-scope`)
  landed, backed by the empirical fixture, before per-publisher FIFO is
  documented as shipped — in its measured, scoped form.
  Spec-in-place updates for the Events protocol are a per-slice follow-on.
  **On retire:** remove this doc; append its closing summary to
  [`../archive/retired-tracks.md`](../archive/retired-tracks.md); close the spine
  (`Closes #<n>`).
