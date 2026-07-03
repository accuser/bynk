# 0149 — a generated subject is a valid inhabitant of its type: the generator *is* the type's refinement domain, and agents are the exception

- **Status:** Accepted (v0.114; 2026-07-02)
- **Provenance:** the v0.114 increment — the generation half of the testing track's third slice, the load-bearing record that *defines generation* for the whole track. It sits beside the value-fabrication rename: that record says fabrication is spelled `Val`; this one says what a fabricated-or-generated subject *is*.
- **Realises:** the "subjects are supplied or generated" pillar at the value/domain level. A `property "…" { for all x: T [where …] { expect … } }` runs the invariant predicate over subjects the runner produces — the same one predicate surface, now over generated inputs.
- **Relates:** the one predicate surface (ADR 0144 — a property's body *is* the invariant predicate, verbatim, over generated inputs); the `Val`-fabrication record (its sibling — `Val[T]` fabricates one inhabitant of the same space the generator draws many from); the refined-boundary-ID / privileged-constructor line (ADR 0014 — a generated refined value is constructed through the branded `unsafe` path, valid by construction).

## Context

A generative test needs an answer to one question: *what does the runner
generate?* The classical property-testing answer is a per-type **generator**
(`Gen[T]`/`Arb[T]`) the author writes or composes — a distribution over `T`,
separate from `T`'s definition. That answer is powerful and also a second surface:
a generator can disagree with the type (draw values the type would reject, or miss
values the type admits), and the author must maintain it.

Bynk already carries a type's admissible-value space in the type itself: a refined
type's `where` predicates, a sum's variants, a record's fields, an opaque type's
base. `Val[T]` fabrication already draws a boundary inhabitant from exactly that
space. The open question was whether generation should introduce a *separate*
generator surface or reuse the type's own domain.

A second question: agents. An agent has a state space constrained by invariants,
so one could imagine generating an agent *state* that satisfies every invariant.
But a state that satisfies the invariants need not be **reachable** by any
sequence of handler calls — fabricating one tests a fiction.

## Decisions

**D1 — a type is its own inhabitant space; the generator draws only values that
satisfy `T`.** The generator for `T` produces boundary-inclusive inhabitants of
`T`'s refinement domain: `Int where Positive` → `1`, small positives, an upper
boundary; `Int where InRange(a,b)` → `a`, `b`, interior; `String where
MinLength(k)`/`Length(k)` → strings at and above the length bound; a sum → each
variant (payloads generated recursively); a record → every field; an opaque type
→ over its base's domain. A generated subject is therefore **valid by
construction** — there is no generator that can disagree with the type, and none
to maintain. A custom-distribution `Gen[T]`/`Arb[T]` is a reserved future,
unneeded while a type is its own inhabitant space.

**D2 — a non-generable type must be pinned, not silently skipped.** A `String
where Matches(re)` has no refinement-driven generator for an arbitrary regex; a
bare `for all`/`Val` of one is rejected (`bynk.val.needs_pin`) and the author pins
a witness instead. Refusing is honest; a best-effort regex generator would be a
second, weaker surface.

**D3 — agents are the exception: `for all`/`Val` over an agent is rejected.**
Fabricating an agent *state* that satisfies every invariant does not make it
reachable, so it is rejected up front (`bynk.val.agent_not_generable`) rather than
allowed to erode trust with valid-but-unreachable states. Behavioural generation
over an agent is handler-*sequence* generation — generate call histories and check
the invariants at the real commit boundary — which is the history rung, a later
slice. Snapshot/step invariants run at the real commit boundary, never over a
fabricated state.

**D4 — a property that merely restates a refinement is flagged, conservatively.** A
predicate that re-checks a guarantee the bound variable's type already gives
(`for all q: Quantity { expect q > 0 }` when `Quantity` is `Int where Positive`)
adds nothing — the generator already guarantees it. The runner flags it
(`bynk.property.restates_refinement`), but the check is **conservative**: it fires
only when the predicate is *syntactically* the refinement over the bound variable.
Under-flagging is acceptable; over-flagging (a false positive on a subtle
predicate) is not.

**D5 — determinism is the generator's, from one root seed.** Each run draws one
root seed (printed only on failure); every property derives its seed
deterministically from it, so `bynkc test --seed <hex>` reproduces a run
byte-for-byte. A property body stays pure (the one predicate surface), so a fixed
seed fully determines the case. On failure the runner **shrinks** the
counterexample toward each input's boundary (integers toward the refinement floor,
strings toward minimum length, sums toward the first variant) and reports the
case count, the root seed, and the shrunk tuple with a copy-paste reproduce line.

## Consequences

- Authors write *properties*, not *generators*: a `property` names types and a
  predicate; the inhabitant space comes from the types for free.
- Generation can never produce an invalid subject, so a property failure is always
  a real counterexample within the type's domain — never a generator bug.
- Agent behaviour is deliberately *not* generated in this slice; the honest path
  (handler-sequence histories) is named and deferred, not faked.
- Reproduction is exact and cheap: one hex seed on the failure line replays the
  whole run.
- **Re-openable:** custom generators (`Gen[T]`/`Arb[T]`) for distributions the
  refinement domain cannot express; regex-aware generation for `Matches`; and
  handler-sequence generation for agents — each a named future, none blocking v1.
