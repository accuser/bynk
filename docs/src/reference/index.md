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

## Generated reference

These pages are generated directly from the compiler (or the grammar) and
guarded by tests so they cannot drift:

- [Syntax & grammar](grammar.md) — from the `tree-sitter-karn` grammar.
- [Keywords](keywords.md) — from the lexer's keyword tokens.
- [CLI (`karnc`)](cli.md) — from the clap command tree.
- [Diagnostic index](diagnostics.md) — from the diagnostic registry.
