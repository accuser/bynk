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
not a dedicated Worker.** A compiler-synthesised `EventsFanout` class
(`bynk-emit/src/emitter/events_fanout.rs`) is emitted as a new
`events_fanout.ts` file per publishing context and re-exported from that
context's `index.ts` — Cloudflare resolves a Durable Object binding's
`class_name` against the exports of the Worker's `main`, the identical
requirement an ordinary agent's DO class already satisfies via `handlers.ts`
(`emit_agent`, `emit.rs:2867`, referenced by issue #939 as the proven codegen
path to reuse). The fanout DO folds into the *same* `[[durable_objects.
bindings]]`/`[[migrations]]` blocks a context's real agents already get
(`wrangler.rs`), rather than a parallel mechanism — Cloudflare does not care
which generated file a class comes from.

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

**Consequences.** Verified: a hand-compiled two-context Workers-target
project (`bynkc/tests/events_workers_wiring.rs`) asserts the `wrangler.toml`/
`events_fanout.ts` content directly — nothing else in the suite reads either
file — and passes real `tsc --strict` over the whole emitted tree, alongside
the pre-existing Bundle-target behavioural proof
(`bynkc/tests/events_behaviour.rs`) that release-at-commit delivery and
abort-suppression actually run under `node`. Not verified, and not claimed:
real delivery through an actual Cloudflare Durable Object — this suite has no
`workerd`/`wrangler dev` to run one, so the DO's `fetch` wire contract is
proven by `tsc --strict` and direct inspection of the generated TOML/TS, not
by an executed round trip. Per-publisher FIFO ordering under real concurrent
load ([ADR 0284](../decisions/0284-events-fanout-substrate.md)'s own
"separate, empirical decision") remains open for the same reason.

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
