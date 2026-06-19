# `bynk.toml` manifest

A `bynk.toml` at a project's root marks it as a project and configures its
layout. A multi-file project with a `src/` and `tests/` split uses one.

```toml
[project]
name = "my-project"
version = "0.1.0"

[paths]
src = "src"
tests = "tests"
out = "out"

[fmt]
indent = "tab"
max_line_width = 100

[lsp]
diagnostics_mode = "live"
```

## `[project]`

| Key | Purpose |
|---|---|
| `name` | the project's name. |
| `version` | the project's version. |

## `[paths]`

| Key | Purpose |
|---|---|
| `src` | directory holding source units. |
| `tests` | directory holding test units. |
| `out` | default output directory. |

In a project (split-paths) layout, source units live under `src/` and test units
under `tests/`, each at a path matching its qualified name — `context
commerce.orders` in `src/commerce/orders.bynk`, `test commerce.orders` in
`tests/commerce/orders.bynk`. Mismatches raise
`bynk.project.inconsistent_commons_name` or
`bynk.project.inconsistent_test_path`.

## `[fmt]`

Formatter settings (consumed by `bynkc fmt`):

| Key | Purpose |
|---|---|
| `indent` | indentation style (e.g. `"tab"`). |
| `max_line_width` | target maximum line width. |

## `[lsp]`

Language-server settings (consumed by `bynkc-lsp`):

| Key | Purpose |
|---|---|
| `diagnostics_mode` | when diagnostics are computed (e.g. `"live"`). |

## Legacy mode

Without a `bynk.toml`, a single `.bynk` file compiles as a standalone unit (the
[first-program](../tutorials/01-first-program.md) flow). Project features —
a `src`/`tests` split, `bynkc test` — expect the manifest-driven layout above.
