# Appendix A — Planned features

> [!WARNING]
> This appendix is **non-normative**. The features named here are design
> directions, **not part of the normative Bynk language**. They are not
> implemented in the version this specification defines, are subject to
> change, and impose no requirement on a conforming implementation. They are
> recorded only to mark intended direction.

Two directions are designed but not yet shipped:

- **Events** — first-class domain events a context can publish and others react
  to, beyond the present synchronous `consumes` call.
- **Sagas** — long-running, multi-step workflows with compensation, coordinating
  effects across contexts.

Two named follow-ons extend [agent invariants](static-semantics.md#541-invariants-v080)
(shipped runtime-checked in v0.80):

- **Static provable-violation analysis** — a compile-time error for a handler all
  of whose paths provably commit a state that violates an invariant; a layer on
  top of runtime checking (static *satisfaction* proving remains further
  deferred).
- **A general typed-agent-fault channel** — making an `InvariantViolation` (and
  every other uncaught agent fault, such as a non-exhaustive match) a
  caller-distinguishable fault envelope, rather than the present bare 500. The
  surface stays a *fault*, never a `Result` variant.

These are sketches of intent, not specifications: this appendix deliberately
states no syntax or behaviour for them. For the design rationale and the current
thinking on what is deferred, see
[Versioning & roadmap](../about/versioning-and-roadmap.md#what-is-deferred-to-v1).
