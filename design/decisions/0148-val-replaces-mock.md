# 0148 — value fabrication is `Val[T]`: the `Mock[T]` value fabricator is renamed `Val[T]`/`Val[T](pin)` and retired

- **Status:** Accepted (v0.114; 2026-07-02)
- **Provenance:** the v0.114 increment — the testing track's third slice and its first *generative* rung. This is the value-fabrication half of that slice; the generation half (what a fabricated subject *is*) is a sibling record. The name change lands with the slice that makes generation real, because the two concepts — a *supplied* value and a *generated* one — become one vocabulary the moment a `property` can generate the same inhabitants a `case` fabricates.
- **Realises:** value fabrication under the testing track's "subjects are supplied or generated" pillar. A `case` supplies a subject — a pinned `Val[T](v)` or an `expect` over known inputs; a `property` generates them. Both draw from the *same* inhabitant space, so both spell fabrication the same way: `Val`.
- **Relates:** the one predicate surface (ADR 0144 — a `Val` is a subject the predicate is checked over, not a second grammar); the `suite`/`case` vocabulary (ADR 0146, whose D3 deferred the `Mock[T]` rename to this slice); ADR 0001 (compile-time literal admission — the closed literal set a `Val[T](pin)` is checked against, unchanged).

## Context

Since v0.9.4, `Mock[T]` fabricated a valid test value of a refined/opaque/sum/
record type — a bare `Mock[T]` produced a boundary-inclusive default, `Mock[T](v)`
pinned a refinement-checked literal. It was an identifier-triggered expression
(the identifier `Mock` followed by `[`), test-body-only, with a `bynk.mock.*`
diagnostic family for the value fabricator.

The name `Mock` was borrowed from collaborator-stubbing frameworks, where a "mock"
is a *behavioural double* — a fake implementation that records calls. Bynk already
has that concept under a different word: the `mocks` block replaces a
*collaborator*. `Mock[T]` never stubbed behaviour; it fabricated a *value*. The
two meanings collided under one word.

The v0.114 slice makes generation real: a `property`'s `for all x: T` draws
inhabitants of `T` from the same refinement domain a bare `Mock[T]` drew its
default from. At that point "the value the runner generates" and "the value the
author fabricates" are the *same space*, and calling one `Mock` while the other is
just "a generated `T`" is incoherent.

## Decisions

**D1 — `Mock[T]` is renamed `Val[T]`, a straight rename with no semantic change.**
Bare `Val[T]` fabricates a valid inhabitant (the boundary-inclusive seed the
runner also draws from); `Val[T](v)` pins a specific one, refinement-checked
exactly as `Mock[T](v)` was. The trigger is unchanged in kind — an
identifier-triggered expression (`Val` followed by `[`), *not* a reserved keyword,
so `Val` stays a usable identifier everywhere else. Refined, opaque, sum, and
record types fabricate exactly as before; the pin rules are unchanged.

**D2 — the change is clean-slate: `Mock[T]` is removed, not deprecated.** Bynk's
pre-1.0 posture is churn-now-with-codemods. A codemod rewrites `Mock[` → `Val[`
(bare and pinned); there is no grace period and no alias. Keeping `Mock` as a
deprecated synonym would re-introduce the very name collision this record removes.

**D3 — the value-fabricator diagnostics move `bynk.mock.*` → `bynk.val.*`.** The
eight value-fabricator codes (`outside_test`, `unknown_type`, `needs_pin`,
`pin_not_literal`, `literal_violates`, `arity`, `pin_unsupported`,
`unsupported_kind`) are renamed under the `bynk.val.*` category ("Value
fabrication"). The `bynk.mock.*` category is retained **only** for the `mocks`
*collaborator* block (`unknown_target`, `duplicate_target`, `signature_mismatch`,
`in_commons_test`), which is untouched this slice.

**D4 — the `mocks` collaborator block keeps its name for now.** This slice retires
only the *value fabricator's* use of "mock". The `mocks` block — a behavioural
collaborator double — retires later, when `provides` absorbs it and the word
"mock" leaves the language entirely. Splitting the two lets the value rename ship
now without waiting on the larger collaborator rework.

## Consequences

- The word "mock" now means exactly one thing in the language surface — a
  collaborator double (`mocks`) — until that too retires. A *value* is fabricated
  with `Val`, matching how a `property` generates one.
- Existing `Mock[T]` code is a mechanical `Mock[`→`Val[` rewrite; negative
  fixtures re-point `bynk.mock.*` value codes to `bynk.val.*`.
- The identifier-triggered (non-keyword) form is preserved, so no existing
  identifier named `Val` breaks — it is only special before a `[` in expression
  position, exactly as `Mock` was.
- **Re-openable:** the `mocks` collaborator block's name and its `bynk.mock.*`
  codes — they retire with the collaborator rework, at which point "mock" is gone.
