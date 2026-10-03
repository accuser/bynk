# 0429 — A match arm that an earlier arm covers is an error

- **Status:** Accepted (v0.299)

**Context.** static-semantics says match arms MUST NOT be unreachable, but
`bynk.types.unreachable_arm` fired only after a leading wildcard. `Some(_)`
then `Some(Red)` was accepted, and its emitted narrowing failed `tsc`. A
defensive trailing `_` after every variant compiled silently.

**Decision.**
- An arm is unreachable, and an error, when every value its pattern matches is
  already matched by an earlier **unguarded** arm.
- The check recurses through nested payloads, or-patterns and literals, and is
  sound but bounded like exhaustiveness. A multi-field payload is covered only
  when one earlier arm covers it field by field. An arm is reported only when
  it is certainly unreachable.
- A guarded arm and a refined pattern (`n where P`) never cover a later arm,
  since the guard or predicate may fail.
- Duplicate arms keep their own codes (`duplicate_variant_arm`,
  `duplicate_literal_arm`).
- Severity is an error, matching the spec's MUST (issue Decision A).

**Consequences.** Some programs are newly rejected, including a defensive
trailing `_`. The diagnostic says why, and suggests removing the arm or moving
it above the arm that covers it.

Proved by negatives covering each shape in the issue, and the behavioural
fixture `1656_reachable_arms`, which shows that guarded duplicates, a refined
arm followed by the same refined arm or a wildcard, and narrow-before-wide arms
all stay legal.
