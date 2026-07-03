# 0151 — the invariant subject widens to the step: `transition` is a commit-boundary invariant over `old`/`new`, not a runner attack, and not an emission clause

- **Status:** Accepted (v0.116; 2026-07-02)
- **Provenance:** the v0.116 increment — the testing track's fifth slice, the rung that widens the invariant subject from a *snapshot* to a *step*. It is the load-bearing record for step invariants; it also settles the track's open keyword-surface question (DECISION I) and records the enum-`Ord` prerequisite disposition (DECISION O).
- **Realises:** the testing track's subject ladder — `value → domain → call → snapshot → step → history`. A snapshot `invariant` constrains a committed state; a `transition` constrains the move between two committed states (`old → new`). Both are declared on the agent and carried by the code, not written in a test.
- **Relates:** the one predicate surface (ADR 0144 — a transition clause *is* the invariant predicate, now over a state pair); the snapshot-invariant machinery (v0.80 — transitions ride the same `commitState` gate, before `storage.put`); the contract record (ADR 0150 — the *call* rung, with `result` as its contextual binding; `transition` is the *step* rung, with `old`/`new` as its contextual bindings, parsed the same contextual way); the generation record (ADR 0149 — a fabricated agent state is valid but *not reachable*, which is why a transition gets **no** runner attack here; behavioural attack is handler-sequence generation, a later slice).

## Context

Bynk already carries a *snapshot* invariant on an agent — a named, pure `Bool`
predicate over state that must hold of every committed state, runtime-checked at the
commit boundary. Some durable behavioural facts are not about a single state, but
about a *move* between states: a paid order never becomes unpaid, a counter never
decreases, a closed account never reopens. These are claims about the pair
`(old, new)`.

Three questions had to settle. First, the keyword surface (DECISION I): a dedicated
`transition` over `old`/`new`, or a folded `invariant … step old -> new { … }`?
Second, enforcement: a snapshot invariant runs at the commit boundary; should a
transition also be *attacked* by the runner, the way a contract's `ensures` is
(ADR 0150)? Third, emission: the track flagged whether the invariant subject should
widen to *emission* too — a per-handler "emits" clause.

The reachability problem decides the second question. The generator (ADR 0149)
draws *valid* inhabitants of a type. A valid agent state — one satisfying every
snapshot invariant — is **not** necessarily a *reachable* one: no handler sequence
need ever produce it. Feeding a fabricated `(old, new)` pair to a generative
transition check yields counterexamples production can never hit — false positives
that erode trust in the runner. So a transition cannot be attacked by fabricating
states; the sound route is to drive the real handlers and observe the pairs they
reach, which is the history rung.

## Decisions

**D1 — a transition is the invariant predicate widened to the step.** An agent
carries any number of named `transition <name>: <pred>` declarations, beside its
snapshot `invariant`s, between the store fields and the handlers. The predicate is
the invariant predicate verbatim — pure `Bool`, `implies`, `is`, operators, pure
methods — over two contextual bindings, `old` and `new`, each the agent's state
record (so `old.status` / `new.status` read like any record field). No new grammar
beyond the declaration head. Impurity is `bynk.transition.impure_predicate`; a
non-`Bool` clause is `bynk.transition.not_bool`; two transitions sharing a name is
`bynk.transition.duplicate_name`; a cross-agent reference is
`bynk.transition.cross_agent_reference`.

**D2 — a dedicated `transition` keyword, with `old`/`new` as contextual bindings
(settles DECISION I).** The step rung is spelled `transition`, not folded into
`invariant`, so a reader sees at the declaration head whether a claim is about one
state or a move. `old` and `new` are special *only* inside a `transition` predicate
(as `result` is only inside an `ensures`); everywhere else they are ordinary
identifiers, so existing code naming a value `old`/`new` still parses. They are not
reserved words.

**D3 — a transition is checked at one point: the commit boundary, from the second
commit onward.** The generated `commitState` — which already evaluates snapshot
invariants against the proposed state before `storage.put` — also evaluates each
transition. The old state is the last committed state, still in storage at that
point (the gate performs the write), so it is read there directly; `undefined` is
the **genesis commit**, which has no prior state to transition from and is skipped
(snapshot invariants still apply to it). A violation throws the same
`InvariantViolation`-family fault (agent type + transition name, never the key —
ADR 0107) before the write, so the offending commit never persists. Because the
check lives at the commit boundary, a transition fires at *every* tier for free.

**D4 — a transition gets no runner attack (validity ≠ reachability).** Unlike a
contract's `ensures` (ADR 0150), a transition is not generatively attacked. A
fabricated agent state is valid but not reachable (ADR 0149), so a generated
`(old, new)` pair would produce false counterexamples. The sound way to exercise a
transition adversarially is handler-sequence generation — drive the real handlers
from the initial state and observe the moves they reach — which is the *history*
rung, a later slice. Until then, a transition is checked against the real old→new
of every actual commit, at every tier.

**D5 — placement is structural; a step claim must mention the step.** A
`transition` is an agent-body-only declaration, so there is no "transition on a
non-agent" diagnostic to raise — the grammar prevents it. A `transition` whose
predicate references neither `old` nor `new` is not a step claim but a snapshot
invariant misfiled; it is flagged (`bynk.transition.no_step_reference`) and the
author is pointed at `invariant`.

**D6 — the invariant subject does *not* widen to emission; ordered transitions wait
on enum `Ord` (records DECISION O).** Emission is not an invariant rung: what a
handler emits is derivable from its body, and Bynk already rejects a source-level
`@requires` on that ground (ADR 0127). A scenario-specific emission is a test
observation (a later slice); a universal emission guarantee is a cross-cutting
policy / use-case minimal guarantee (deferred). Separately, an *ordered* transition
(`new.status >= old.status`) needs enums to be orderable, which they are not today;
this slice ships transitions with `is`/`implies`/`==`/`!=` (and the ordering already
available on numeric and temporal fields), and a declaration-positional enum `Ord`
is a **separate prerequisite** decided before ordered-status transitions are
authored (DECISION O).

## Consequences

- A step claim is written once, on the agent, and is checked at every real commit —
  at every tier — with no test code; promoting a `case` can surface a transition a
  stub was hiding.
- The genesis commit is deliberately exempt from transition checks (it has no prior
  state); snapshot invariants still constrain it.
- A transition reuses the snapshot-invariant fault path (`InvariantViolation`), so a
  transition-only agent still imports the fault helper and reverts atomically.
- `old`/`new` are lowered to `__old`/`__new` in the emitted gate (`new` is a JS
  reserved word); the `is` form (`old.status is Paid`) lowers to a structural tag
  comparison, robust across serialisation — the form the track recommends.
- **Re-openable:** handler-sequence (behavioural) generation that *does* attack
  transitions soundly (the history rung); enum positional `Ord` for ordered-status
  transitions; and where a universal emission guarantee lives — each a named future,
  none blocking v1.
