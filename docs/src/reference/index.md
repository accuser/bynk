# Reference

Consultable, complete, and dry. These pages describe exact behaviour; for
learning, start with the [tutorials](../tutorials/01-first-program.md), and for
tasks see the [how-to guides](../how-to/index.md).

## Language

- [Type system](types.md) — opaque, sum, record, and refined types.
- [Refined-type API](refined-types.md) — `.of`, `.unsafe`, predicates, admission.
- [Operators & built-ins](operators.md) — operators, precedence, built-in types.
- [Agents](agents.md) — declaration, state, zeroability, lifecycle.
- [HTTP](http.md) — `on http` handlers and `HttpResult`.
- [Testing](testing.md) — `test`, `assert`, `mocks`, `Mock[T]`.

## Project & output

- [`karn.toml` manifest](manifest.md) — every manifest key.
- [Emission](emission.md) — the TypeScript each construct emits.
- [Diagnostic index](diagnostics.md) — every `karn.*` code (generated).
- [Version compatibility & changelog](changelog.md).

## Generated reference (pending)

Some reference is intended to be generated directly from the compiler. The
[diagnostic index](diagnostics.md) already is. The **grammar**, **keyword list**,
and **CLI** pages are stubs awaiting their generators; until then, the
[operators](operators.md) and how-to pages cover the same ground for users.
