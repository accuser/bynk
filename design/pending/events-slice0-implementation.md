---
level: minor
changelog: The `Events` capability, slice 0 — `event` declarations, `given Events` emission with owner-only enforcement, and `from Events(E)` subscription, across contexts and across all three platforms
---

## ADR: events-slice0-fanout-implementation
title: Events slice 0's fan-out mechanism, concretely — a per-context DO on Cloudflare, an in-process closure everywhere else
summary: Resolves issue #939's DECISION D — grounds the fanout-substrate ADR's per-publisher Durable Object in real codegen, and confirms the non-Cloudflare path this issue left open

**Context.** `events-fanout-substrate` ([ADR 0284](../decisions/0284-events-fanout-substrate.md))
committed slice 0 to one fanout Durable Object per publishing context, but not
its concrete shape; issue #939 (slice 0's own implementing proposal) explicitly
left DECISION D's non-Cloudflare mechanism and the DO's wire contract as
implementation-PR decisions, "grounded against the real Bundle-target agent
lowering before committing to a shape" rather than assumed. This ADR is that
grounding.

**Decision.**

**A. The fanout DO is hosted in the publishing context's own Worker script,
not a dedicated Worker.** A compiler-synthesised `__EventsFanout` class
(`bynk-emit/src/emitter/events_fanout.rs`) is emitted as a new
`events_fanout.ts` file per publishing context and re-exported from that
context's `index.ts` — Cloudflare resolves a Durable Object binding's
`class_name` against the exports of the Worker's `main`, the identical
requirement an ordinary agent's DO class already satisfies via `handlers.ts`
(`emit_agent`, `emit.rs:2867`, referenced by issue #939 as the proven codegen
path to reuse). The fanout DO folds into the *same* `[[durable_objects.
bindings]]`/`[[migrations]]` blocks a context's real agents already get
(`wrangler.rs`), rather than a parallel mechanism — Cloudflare does not care
which generated file a class comes from. Double-underscore-prefixed (not
`EventsFanout`) because a Bynk `agent` name can never start with `_` — an
agent coincidentally named the same as the synthetic class is therefore
structurally impossible, not merely unlikely, closing a real collision review
found (the first pass used the un-prefixed name).

**B. The subscriber registry is a compile-time literal, not carried on the
wire.** `discover_event_subscribers` (`project.rs`) already resolves, at
compile time, every `from Events(E)` service to `E`'s declaring context —
project-wide information no publisher's own request could supply at runtime
without either a registration protocol or trusting the caller. The fanout DO's
routing table (event type name → subscriber Service Binding + service name) is
therefore baked into `events_fanout.ts` as a literal object, and the DO's
`fetch` only ever receives the event batch itself.

**C. A publisher gains a reverse-direction Service Binding to each
subscriber it does not itself `consumes`.** An ordinary `consumes` edge wires
a Service Binding in the *subscriber's* direction only (it needs the
publisher's types); nothing upstream gives the publisher a binding back to a
subscriber it doesn't otherwise depend on. `wrangler.toml`'s `[[services]]`
list is extended with exactly these targets, deduplicated against any
`consumes` binding that happens to already cover the same context.

**D. Non-Cloudflare (`BuildTarget::Bundle`, `Platform::Node`/`Browser`)
dispatches in-process — no DO, no wire.** `composeApp`'s per-context
`__eventsDispatch` closure switches on the emitted event's type and calls
directly into each subscriber's `.event()` method with that subscriber's own
already-built `deps` object (`project.rs`'s `emit_composition_root`),
confirming DECISION D's premise: the DO's ordering/isolation properties exist
specifically to substitute for the multi-isolate concurrency Cloudflare
introduces, which does not exist under `Bundle`, so nothing is lost by
skipping it there. `Events` is now the first first-party capability shipped
identically across `Platform::Cloudflare`/`Node`/`Browser` in the sense that
matters for this feature — no binding difference, only a target difference
already governed by the existing `BuildTarget` axis.

**E. `deps.__eventsDispatch` does not survive the Durable Object's JSON wire
any better than a capability provider does — the existing #527 fix is
extended, not duplicated.** An agent method whose own body emits directly is
reached through the same `/_bynk/agent/<method>` `fetch` dispatch a `given`
capability provider already crosses (`emit_agent`); like a provider, a
function cannot serialise, so `deps.__eventsDispatch` silently becomes
`undefined` on the far side of `JSON.stringify` if trusted as-arrived. The fix
mirrors #527 exactly: an agent whose handler emits gets the same env-carrying
DO constructor a `given`-provider agent already gets, and its `fetch`
dispatcher rebuilds `__eventsDispatch` from `env.EVENTS_FANOUT` before
invoking the method, alongside (not instead of) any real provider rebuild
already happening there.

**F. The `/_bynk/event/<service>` route has no boundary decoding, no
contract-skew guard, and no auth — an accepted, named gap, not an oversight.**
Compare it to the `/_bynk/call/<service>` route in the same generated
`index.ts`: `on call` decodes each argument field-by-field through the
boundary codec and refuses a deploy-skewed caller with a `409`
(`X-Bynk-Contract`, v0.177/#643); `on event` parses the body as opaque JSON
and passes it straight through as `any`. Three consequences, each accepted for
slice 0 rather than silently shipped: **(1)** a rich field type (`Duration`,
`Uuid`, `Bytes`, a refined type) the codec machinery would normally
reconstruct arrives as raw JSON on Workers and a real object on Bundle — a
target divergence a subscriber's handler body cannot see from its own source.
**(2)** no `X-Bynk-Contract`-equivalent skew detection — a subscriber
deployed against an older event shape accepts a mismatched payload silently,
where the call path fails closed; the primitive already exists, it is simply
not reached from this route yet. **(3)** the route sits before any auth in
the `fetch` chain, and Service Bindings share the Worker's public `fetch`
surface — anything that can reach a subscriber Worker's URL can `POST
/_bynk/event/<service>` with arbitrary JSON, bypassing `bynk.event.
emit_outside_owner`'s compile-time-only guarantee entirely at the wire. This
mirrors `/_bynk/call/`'s own unauthenticated posture (an existing,
project-wide stance this slice does not change), but `Events` is the one
capability whose whole selling point is an ownership guarantee `calls` don't
claim — so unlike `/_bynk/call/`, this is named here rather than left
implicit. Closing it (per-field boundary decoding, a contract-hash guard, an
internal-only auth seam) is future work, not assumed done.

**Consequences.** Verified: a hand-compiled two-context Workers-target
project (`bynkc/tests/events_workers_wiring.rs`) asserts the `wrangler.toml`/
`events_fanout.ts` content directly — nothing else in the suite reads either
file — and passes real `tsc --strict` over the whole emitted tree, alongside
the pre-existing Bundle-target behavioural proof
(`bynkc/tests/events_behaviour.rs`) that release-at-commit delivery and
abort-suppression actually run under `node`, extended with a sibling
subscriber whose handler always throws to prove failure isolation is real on
both targets (a throwing subscriber no longer aborts its siblings nor
propagates into the already-committed publisher — found missing on the
Bundle target specifically by review, since the two targets had silently
diverged on this exact guarantee). Not verified, and not claimed: real
delivery through an actual Cloudflare Durable Object — this suite has no
`workerd`/`wrangler dev` to run one, so the DO's `fetch` wire contract is
proven by `tsc --strict` and direct inspection of the generated TOML/TS, not
by an executed round trip. Per-publisher FIFO ordering under real concurrent
load ([ADR 0284](../decisions/0284-events-fanout-substrate.md)'s own
"separate, empirical decision") remains open for the same reason, and is now
joined by two further named-not-hidden gaps review surfaced: **(1)**
`wrangler.toml`'s DO migration always writes `tag = "v1"` — a context that
already has a deployed agent and adopts `Events` regenerates the same tag
with an added class, which Cloudflare treats as already-applied and skips, so
`__EventsFanout` is never actually created on a redeploy (a pre-existing
limitation for adding any new agent to an already-deployed context, made
newly reachable by this feature). **(2)** the fan-out DO's dispatch is fully
synchronous from the publisher's perspective — one emission's tail latency is
the sum of every subscriber's own processing time, serialised through the one
DO instance `idFromName("singleton")` gives each publishing context; a
`waitUntil`-based fire-and-forget (precedent: fixture `221_async_send_
waituntil`) is a future optimisation, not assumed already fast. Two review
findings that *were* fixed rather than merely named: `Events.emit` inside a
parenthesised (or otherwise nested) expression previously compiled to
invalid TypeScript with no bynk diagnostic (`block_uses_emit` now drives off
the exhaustive expression-walking visitor instead of a hand-rolled match that
had drifted from the lowering it gates); and `Events.emit[E]`/`from
Events(E)` naming a type that resolves but isn't actually a declared `event`
silently failed open in both directions (a dropped emission, a permanently
dead subscriber) — both now diagnosed (`bynk.event.emit_not_an_event`,
`bynk.event.unknown_subscription`).

## ADR: events-owner-only-emission-check
title: Owner-only event emission is enforced by a new checker pass, not a reuse
summary: Resolves issue #939's DECISION E — the shipped mechanism, keyed on `is_local_type` at the `Events.emit[E]` call site

**Context.** Issue #939 (slice 0's implementing proposal) named owner-only
emission as the track's one genuinely new checker mechanism: every other
cross-context boundary in the language governs what a context may *name*
(`uses`/`consumes`); this is the first that instead restricts what a context
may *do* with something already visible to it (an event type is transparently
visible cross-context by design, `events.md` §3.0). DECISION E called for this
to be built and named as new, not discovered mid-implementation the way ADR
0282's Decision A was.

**Decision.** `Events.emit[E](...)`'s type argument is checked, at the
capability-operation call site (`bynk-check/src/checker/calls.rs`, the generic
capability-op type-argument resolution loop ADR 0281 already built), against
`ctx.input.is_local_type(ename)` — the same "declared here vs. merely visible
via `uses`/`consumes`" table ADR 0256's locale-types-split gap analysis used,
so this reads existing provenance data rather than plumbing new. Naming a
foreign event type fails closed with `bynk.event.emit_outside_owner`, noting
that the type remains legitimately subscribable via `from Events(E)` — only
emission is restricted, not visibility.

**A same-named user capability must not be mistaken for the first-party
`Events`.** Mirroring the precedent #934 already established for
`Idempotency`, every site that treats `Events` specially — the owner-only
check above, the call-site lowering that buffers an emission
(`bynk-emit/src/emitter/lower.rs`), the deps-shape exclusions that keep
`Events` out of every constructed deps object (no `EventsProvider` exists to
construct), and the fan-out DO wiring itself — checks genuine first-party-ness
(declared in-unit or flattened from `bynk`) via a single shared
`is_first_party_events` predicate, not a bare string match on the name
`"Events"`. An earlier pass of this same implementation had these sites
checking the string only in some places and the genuine predicate in others —
found by review, not by a failing test, since no fixture in this slice
declares a same-named user capability; closed by routing every site through
the one shared check.

**Consequences.** Verified by a negative fixture
(`bynkc/tests/fixtures/negative/503_events_emit_outside_owner`): a context
attempting to emit a peer's event type fails to compile with
`bynk.event.emit_outside_owner`, not merely a positive end-to-end case. The
risk this decision exists to close — a forged/cross-context emission,
`events.md` §6's primary threat-model guarantee — rests entirely on this one
pass; it has no fallback mechanism behind it.

**Two changes ride along that are visible beyond `Events`, both corrected
during implementation and now covered by their own fixtures.** First,
`ResolvedCommons::local_type_names` (`bynk-check/src/resolver.rs`) was
narrowed from "local, `uses`, and `consumes` merged" (`typed.types`) to
"declared directly in this unit" (`table.types`) — the provenance
owner-only emission actually needs, since a `uses`-rebranded or
`consumes`-surfaced type must not read as emittable just because it's
visible. This is the same field `bynk.types.opaque_raw_outside`/
`opaque_unsafe_outside` already gate on, so the narrowing also newly rejects
`.raw`/`.unsafe()` on a `uses`-imported commons opaque type used inside a
context — plausibly the correct reading of ".raw is only available within
its defining commons" (it wasn't, before), but previously untested in either
direction; the actor-identity-sealing check (`bynk-emit/src/project/
validate.rs`) depended on the old, broader meaning and was compensated by
reading `uses_commons_type_names` (the exact set `emit_context_rebrands`
already rebrands) alongside the narrowed field, rather than by re-widening
it and re-breaking owner-only emission. Fixture 506 locks in the new,
narrower behaviour. Second, `bynk-syntax/src/parser/expressions.rs`'s
primary-expression parser now admits `case`/`event`/`messages`/`on`/`suite`
(`keywords::RESERVED_CONTEXTUAL`) when *reading back* a binding declared
with one of these names, not only at the declaration site — a gap latent
since `messages`/`on`/`case`/`suite` shipped (no existing fixture happened to
name a binding after one and read it back), surfaced concretely by `event`
joining the tier and colliding with `examples/event-log`'s pre-existing
`add(event: Event)` handler. Covered by a new parser unit test naming a
parameter after each of the five and reading it back in the body.
