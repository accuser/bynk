# §8 Compilation model

This chapter defines how Karn sources are organised into a project, how a unit's
name relates to its file, and the pipeline that turns sources into type-correct
TypeScript. Emission itself — what each construct compiles to — is
[§7](emission.md).

## §8.1 The manifest

A `karn.toml` file at a directory's root marks it as a **project** and configures
its layout. Its keys:

| Table | Key | Controls |
|---|---|---|
| `[project]` | `name`, `version` | the project's name and version |
| `[paths]` | `src` | the directory holding source units |
| | `tests` | the directory holding test units |
| | `out` | the default output directory |
| `[fmt]` | `indent`, `max_line_width` | formatter settings (consumed by `karnc fmt`) |
| `[lsp]` | `diagnostics_mode` | language-server settings |

## §8.2 Project and legacy modes

Karn compiles in one of two modes.

**Project mode** — a `karn.toml` is present. Source units live under `[paths].src`
and test units under `[paths].tests`. This is the mode that supports a
`src`/`tests` split and `karnc test`.

**Legacy mode** — no manifest. A single `.karn` file compiles as one standalone
unit. The project-only features — the `src`/`tests` split and `karnc test` —
require project mode.

## §8.3 Source layout

In project mode a unit's file path MUST mirror its qualified name. A
`context commerce.orders` MUST live at `src/commerce/orders.karn`, and a
`test commerce.orders` at `tests/commerce/orders.karn`. A path that does not match
the declared name is rejected (`karn.project.inconsistent_commons_name`,
`karn.project.inconsistent_test_path`), as is a name declared as both a `commons`
and a `context` (`karn.project.kind_conflict`). The source tree therefore mirrors
the program's architecture.

## §8.4 Build pipeline & conformance to TypeScript

A build runs the pipeline **lex → parse → resolve → check → emit**. The first four
stages establish well-formedness ([§3](lexical-grammar.md)–[§5](static-semantics.md));
only a well-formed program reaches emission ([§7](emission.md)).

Emission writes, into the output directory, the per-context and per-test modules,
the composition root, the runtime library ([§7.4](runtime-library.md)), and a
generated `tsconfig.json`. Every emitted module imports the runtime as
`./runtime.js` (or `../runtime.js` by directory depth). The `tsconfig.json`
enables `strict` and targets `ES2022` with `NodeNext` module resolution.

A successful Karn build emits TypeScript that is **type-correct end to end**: it
compiles under `tsc --strict` with no errors. This is the final gate of the
compilation model — a Karn program's well-formedness is realised, not merely
asserted, in a type-checked TypeScript program. `karnc test` continues past this
gate, running the compiled, aggregated test runner on Node.

> [!NOTE]
> The detailed validation requirements a conforming build and test run MUST meet
> — that the emitted runtime and modules compile under `tsc --strict`, that
> refinement validation and agent-state lifecycle behave as specified, and that
> deliberate failures are reported — are part of conformance, specified in §10.
> This note is informative.
