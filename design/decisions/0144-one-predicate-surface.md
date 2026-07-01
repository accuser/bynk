# 0144 — one predicate surface: every checked claim in Bynk is the invariant predicate, not a second assertion grammar

- **Status:** Accepted (testing track, settle; 2026-07-01) — the organising
  commitment, settled ahead of the first slice; the per-position spec and surface land
  with the slices that build them.
- **Provenance:** the testing feature track's settle step (DECISION A). The track
  reframes testing as facets of a construct Bynk already has, rather than a framework
  bolted onto the language; this ADR fixes the one hard-to-reverse commitment every
  later slice depends on, so it is landed up front (per the feature-track posture,
  ADR 0076).
- **Realises:** the thesis that an **invariant** — a pure `Bool` predicate over
  `is` / `implies` / the operators / pure value methods, already enforced at the
  agent commit boundary — is the single assertion language for the whole surface.
  Tests stop being a separate sublanguage.
- **Relates:** ADR 0076 (feature-track posture — front-loaded, hard-to-reverse ADRs
  land at settle); ADR 0127 (capability-requirement provenance — a source-level
  requirement that merely restates a derivable internal is rejected; the same
  "don't restate what the code already guarantees" reasoning recurs here as the
  no-second-grammar rule). The invariant predicate itself is the one enforced today
  by `bynk.invariant.not_bool` / `bynk.invariant.impure_predicate`.

## Context

Bynk's testing surface today is a separate sublanguage: `assert <bool>` reduces a
claim to one bit and reports only a location; `Mock[T]` fabricates values; a `mocks`
block re-implements collaborators. Meanwhile the language already has a richer way to
state a checked claim — an **invariant**: a pure `Bool` predicate, universally
quantified over an agent's reachable states, enforced at the commit boundary. It is
checked (`bynk.invariant.not_bool`) and constrained to purity
(`bynk.invariant.impure_predicate`).

The testing track's central bet is that this predicate is the thing to build on:
examples, properties, contracts, invariants, and interaction checks are not five
features but one predicate aimed at different **subjects** and evaluated at different
**checkpoints**. That bet is only coherent if there is genuinely *one* assertion
language across production code and tests — a decision that, once slices start
shipping surface against it, is extremely expensive to reverse. It therefore has to
be fixed before the first slice, not discovered mid-track.

## Decision

**D1 — There is exactly one assertion language: the invariant predicate.** Every
checked claim in Bynk — a test `expect`, a function `requires` / `ensures`, an agent
`invariant` / `transition`, an interaction observation predicate — is built from the
same grammar: `is`, `implies`, the operators, and pure value methods, typing to
`Bool`. Production code and tests share it verbatim; moving from writing code to
verifying it introduces no new vocabulary.

**D2 — No second assertion grammar and no matcher library, ever.** The surface grows
by aiming the one predicate at more *subjects* and evaluating it at more
*checkpoints*, never by adding a parallel matcher/DSL. There is nothing to *match* —
only a subject to *name*. This is the load-bearing constraint the rule exists to
enforce: a proposal that would introduce a distinct assertion syntax (a matcher zoo,
a bespoke comparison DSL) is out of bounds by this ADR, not by taste.

**D3 — Inside every predicate position: no effects, no capabilities, no test-only
constructs.** The purity discipline that already governs `invariant` governs every
position uniformly. A predicate cannot perform I/O, read a capability, or reach a
construct that exists only in tests. This is what lets the *same* predicate be
promoted between checkpoints (commit boundary ↔ dev call site ↔ test runner) without
being rewritten, and it is why the impurity diagnostic must bite in every position,
not only on agents.

**D4 — The diagnostic family is unified on `bynk.<position>.not_bool` /
`bynk.<position>.impure_predicate`.** The predicate contract is one contract, so its
diagnostics read as one family across positions. Today's inconsistent
`bynk.assert.non_bool` spelling is normalised to `bynk.expect.not_bool` when the
first slice replaces `assert` with `expect`; each later position (`ensures`,
`transition`, observation) reuses the same `not_bool` / `impure_predicate` pair
under its own position segment. The *codes* are the cross-slice contract; their
per-position wording is written with each slice.

**D5 — This ADR fixes the commitment, not the surface.** No grammar, checker, or
spec change lands with this record. Each concrete construct (`expect`, `requires` /
`ensures`, `transition`, the observation predicate) is authored, checked, spec'd, and
documented by its own slice, each citing this ADR. Reversing D1/D2 after a slice has
shipped surface against them is the expensive move this up-front settle exists to
prevent.

## Consequences

- Every subsequent testing-track slice cites this ADR and adds a predicate
  *position*, never a new assertion language. Reviews can reject a matcher-style
  addition on the strength of D2 alone.
- The purity/`Bool` checking logic is shared, not reimplemented per position — the
  existing invariant-predicate check (`bynk-check`) is the single implementation the
  new positions route through, so a fix to the predicate contract fixes it
  everywhere.
- The diagnostic registry gains a coherent `bynk.<position>.not_bool` /
  `.impure_predicate` family as positions land; the legacy `bynk.assert.non_bool`
  code is retired by the first slice, not carried forward.
- Failures can report against the *structure* of the predicate (expected-vs-actual,
  the operator and operands) uniformly, because there is one predicate shape to
  render — the affordance the old one-bit `assert` structurally could not offer.
