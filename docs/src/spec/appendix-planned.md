# Appendix A — Planned features

> [!WARNING]
> This appendix is **non-normative**. The features named here are design
> directions, **not part of the normative Bynk language**. They are not
> implemented in the version this specification defines, are subject to
> change, and impose no requirement on a conforming implementation. They are
> recorded only to mark intended direction.

Three directions are designed but not yet shipped:

- **Events** — first-class domain events a context can publish and others react
  to, beyond the present synchronous `consumes` call.
- **Sagas** — long-running, multi-step workflows with compensation, coordinating
  effects across contexts.
- **Storage kinds** — declarative persistence beyond agent state, letting a
  context describe how its data is stored.

These are sketches of intent, not specifications: this appendix deliberately
states no syntax or behaviour for them. For the design rationale and the current
thinking on what is deferred, see
[Versioning & roadmap](../about/versioning-and-roadmap.md#what-is-deferred-to-v1).
