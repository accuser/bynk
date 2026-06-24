# 0107 — Agent invariants: a closed, agent-local predicate surface, and a dedicated commit-time fault

- **Status:** Accepted (v0.80; 2026-06-23)
- **Spec:** `docs/src/spec/syntactic-grammar.md` §4.5.4 (`invariant_decl`) and §4.6.6 (`implies`); `docs/src/spec/static-semantics.md` (invariant well-formedness); `docs/src/spec/emission.md` (the `commitState` gate); `docs/src/spec/diagnostics.md` (the four `bynk.invariant.*` codes); proposal `design/proposals/v0.80-agent-invariants.md` (deleted on merge).
- **Realises:** `design/bynk-design-notes.md` §14 ("Invariants on agents") — the second validation commitment, alongside test contexts. Agents may declare universally-quantified predicates that must hold of every committed state; a commit that would violate one **faults and reverts** (in the precise sense of D6 below).
- **Relates:** corrects `design/bynk-design-notes.md` §14 wording on "revert" and the implicit-`Cell`-deref predicate model (both written against the pre-divergence `store`/`Cell` design; restated for the current `state { }` + explicit-`commit` implementation — see D5/D6).

## Context

§14 pinned the *semantics* of agent invariants — commit-boundary checking,
faults-not-outcomes, per-agent scope — but against a `Cell`/`store` state model
with an *implicit* commit at handler end. The compiler has since diverged: an
agent is `key id: T` then a `state { …fields… }` record then handlers, and
`commit expr` is an **explicit statement** lowering to `await this.commitState(expr)`
— one generated chokepoint. So "the proposed committed state" is already a
first-class value (the argument to `commit`), and the runtime check is a gate at
the top of `commitState`. Two questions §14 and the current code do not settle
between them are load-bearing enough to ratchet here.

## Decisions

**D1 — The predicate surface is closed and agent-local (DECISION A).** An
invariant predicate may reference the agent's own **state fields** (by bare
name), the base operators, `implies`, `is`, and the closed set of **pure value
methods** the §14 examples need (`Option.isSome`/`isNone`, sum `is`-checks). It
may **not** reference capabilities, perform effects (`<-`/`Effect`), mutate
storage, or name **another agent**. Rationale: this keeps predicates pure and
agent-local and the checker bounded (no call-graph purity analysis), and it fixes
exactly what an invariant *is*. The wider "arbitrary pure-helper calls" surface
is rejected for v0.80 — it pulls purity analysis into invariant checking and
widens what a reader must trust; it can be admitted later by amending this ADR.
Cross-agent predicates are rejected outright (§14 closes that door); the right
tools are sagas/scenarios.

**D2 — State fields are referenced by bare name.** Mirroring every §14 worked
example (`status == Paid implies paymentRef.isSome()`), a predicate reads state
fields unqualified — *not* `self.state.status` (which would read the *persisted*
pre-commit state, the wrong value). The checker resolves bare names against the
state record; the emitter lowers them to fields of the proposed-state value `s`.

**D3 — `implies` is a reserved keyword operator (DECISION C).** `P implies Q`
desugars to `!P || Q`; it is the lowest-precedence binary operator (below `||`),
right-associative, and reads directionally (P → Q). The cost is one reserved
word; the readability is the point. The desugaring was already pinned by the
design notes' IR-lowering step, so only the spelling was open.

**D4 — Invariants form a phase between `state { }` and the handlers (DECISION D).**
Pinned order keeps the parse a straight-line three-phase walk (identity → state →
contracts → behaviour) and keeps the agent readable top-to-bottom. An `invariant`
after a handler is a parse error (`bynk.parse.invariant_after_handler`).

**D5 — A dedicated `InvariantViolation` runtime fault, not a `BoundaryError`
(DECISION E).** `BoundaryError` is the cross-Worker call/refinement layer
(maps to 4xx). A commit-time invariant breach is an *internal agent fault* — a
server-side refusal to enter an inconsistent state. Conflating them would
mis-signal a 4xx. `invariantViolation(agent, invariant)` is thrown inside the
generated `commitState` **before** `storage.put`, rides the existing
uncaught-fault channel, and surfaces to the caller as a **fault, not an
outcome** — consistent with Bynk's failure model (only outcomes are typed parts
of contracts). The honest caller-visible surface is a 500-class response.

**D6 — "Revert" means non-persistence of the offending commit, not whole-handler
rollback.** Throwing before the `put` guarantees only that the faulting committed
value is *never written*. It does **not** undo effects the handler already
performed (a `~>`/`<-` send, a prior `commit`); the handler is not transactional,
and `commit` is an eager per-statement chokepoint, so `commit good; …; commit bad`
persists `good` then faults at `bad`. This restates §14's "the agent reverts to
its pre-handler state" (written for the implicit single-commit model), which is
inaccurate under the eager-commit implementation. Restoring true handler
atomicity is a larger semantic change, out of scope here.

## Consequences

- **Diagnostics.** Four new well-formedness codes —
  `bynk.invariant.not_bool`, `bynk.invariant.duplicate_name`,
  `bynk.invariant.cross_agent_reference`, `bynk.invariant.impure_predicate` —
  plus the parse code `bynk.parse.invariant_after_handler`.
- **Emission.** Each invariant lowers to a pure TS predicate over the
  proposed-state record; `commitState(s)` gates on all of them before the `put`,
  `console.error`-logging the agent **type** and invariant **name** (never the
  key value — it is domain-chosen and frequently PII; deferred to a general
  logging/redaction convention) so a refusal is distinguishable from a crash *in
  the logs*.
- **MVP observability limitation (stated, not glossed).** The thrown
  `InvariantViolation` is structured, but the *response* is not: the DO→service
  hops erase the type, so the caller observes a bare 500 — observationally
  identical to a panic. Making the refusal caller-distinguishable is a **general
  typed-agent-fault channel** (serving non-exhaustive match and every other agent
  fault uniformly), carried as a named follow-on — never a `Result` variant.
- **Named follow-ons (each its own later proposal):** (i) the static
  provable-violation pass (DECISION B — flag a handler all of whose paths
  provably commit a violating state; static *satisfaction* proving stays deferred
  per §14); (ii) the general typed-agent-fault channel above.

## Alternatives considered

- **Arbitrary pure-helper calls in predicates** — more expressive, but widens the
  trusted surface and pulls call-graph purity analysis into the checker.
  Rejected for v0.80 (D1); re-openable by amendment.
- **`InvariantViolation` as a `BoundaryError` kind** — rejected (D5): wrong
  layer, wrong status class.
- **A bespoke per-fault wire path for `InvariantViolation`** — rejected as a
  second fault path; the right home is the general typed-fault channel follow-on.
- **`implies` as a non-keyword form** — rejected (D3): the directional reading is
  the whole point and the keyword cost is trivial.
