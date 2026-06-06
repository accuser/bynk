# Explanation

These pages are about *understanding* — the reasoning, trade-offs, and mental
models behind Karn. They are discursive and occasionally opinionated. For exact
behaviour, see the [reference](../reference/index.md); for step-by-step tasks,
see the [how-to guides](../how-to/index.md).

- [Why Karn exists](why-karn-exists.md) — the problems it sets out to solve.
- [Why compile to TypeScript](why-compile-to-typescript.md) — the runtime bet.
- [The type-system philosophy](type-system-philosophy.md) — refinement, opacity,
  errors-as-values.
- [The refined-literal admission model](refined-literal-admission.md) — why
  literals are admitted the way they are.
- [The agent model](the-agent-model.md) — what an agent is and why state must be
  zeroable.
- [The testing philosophy](testing-philosophy.md) — why `Mock[T]` and test
  isolation exist.
- [How a Karn program is shaped](how-a-karn-program-is-shaped.md) — the
  architecture-first model, end to end.
- [Versioning & roadmap](versioning-and-roadmap.md) — the spec-first method and
  what is deferred to v1.
- [Karn compared to TypeScript](karn-compared-to-typescript.md) — positioning and
  when to reach for it.
