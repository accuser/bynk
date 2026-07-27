# 0289 — Owner-only event emission is enforced by a new checker pass, not a reuse

- **Status:** Accepted (v0.238)

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
