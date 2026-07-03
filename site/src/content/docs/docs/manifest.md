---
title: "`bynk.toml` manifest"
---
A `bynk.toml` at a project's root marks a directory as a Bynk project and
configures its layout and tooling. A multi-file project uses one; a lone `.bynk`
file needs none (see [Legacy mode](#legacy-mode)).

Every section and every key is optional. An omitted section falls back to its
defaults, and a manifest that lists only `[project]` behaves identically to one
that omits it. The authoritative parser lives in the language server; the
compiler shares its path-resolution rules.

## Example

A fully-populated manifest, with every key set to its default:

```toml
[project]
name = "my-project"     # display only; no default
version = "0.1.0"       # display only; no default

[paths]
include = ["src", "tests"]  # trees to compile; default: the conventional
                            # roots that exist, else the project root
exclude = []                # subtrees to skip during discovery
out = "out"

[fmt]
indent = "tab"
indent_width = 2        # only consulted when indent = "spaces"
max_line_width = 100
trailing_comma = true

[lsp]
diagnostics_mode = "live"
diagnostics_debounce_ms = 300
```

Delete any line you are happy to leave at its default — the file above is
equivalent to an empty `bynk.toml`, which is equivalent to `[project]` alone.

## `[project]`

Project metadata. Neither key affects compilation; both are display only and
have no default (unset stays unset).

| Key | Type | Default | Notes |
|---|---|---|---|
| `name` | string | — | Display name for the project. |
| `version` | string | — | Version string for the project. |

## `[paths]`

The project's source tree. Test-ness is structural — a `suite` is a test
wherever it lives (since v0.113) — so the layout is a flat `include`/`exclude`,
not a source/test role split. Each path is resolved relative to the project root
(the directory containing `bynk.toml`).

| Key | Type | Default | Notes |
|---|---|---|---|
| `include` | array of strings | conventional roots | Trees to compile. Defaults to the conventional roots that exist (`src`, and `tests` when present), or the project root itself when neither does. |
| `exclude` | array of strings | `[]` | Subtrees to skip during discovery (monorepo, vendored, or generated `.bynk`). The tool's own `out`/`node_modules` caches and dot-directories are always skipped. |
| `out` | string | `"out"` | Default output directory. Consumed by the LSP; the compiler takes its output directory from the CLI, so this key does not override `bynkc`. |

A conventional `src/`(+`tests/`) project and a flat project (`.bynk` at the root)
both need no `[paths]` at all. The legacy `src`/`tests` keys are ignored if
present.

### Path consistency

Each **source** unit's file path must align with its qualified name. A
`context commerce.orders` must live at `commerce/orders.bynk` under an `include`
root (or be split across `commerce/orders/*.bynk`). A **`suite`** has no
path-identity requirement — it names its target and is legal in any file.

Misalignment of a source unit is rejected at load time:

- `bynk.project.inconsistent_commons_name` — a source unit's declared name does
  not match its location.

This code is described in the [diagnostic index](/book/reference/diagnostics/).

## `[fmt]`

Formatter settings, consumed by `bynkc fmt`. See the
[`bynk-fmt` reference](/docs/tooling/bynk-fmt/) and the
[Format your code](/docs/editor-and-tooling/format/) how-to.

| Key | Type | Default | Notes |
|---|---|---|---|
| `indent` | string | `"tab"` | Indentation style: `"tab"` (one tab per nesting level) or `"spaces"`. |
| `indent_width` | integer | — (falls back to `2`) | Spaces per nesting level. Only consulted when `indent = "spaces"`; defaults to `2` in that case. |
| `max_line_width` | integer | `100` | Soft guide for parameter wrapping. |
| `trailing_comma` | boolean | `true` | Emit trailing commas in multi-line lists. |

## `[lsp]`

Language-server settings, consumed by `bynkc-lsp`. See the
[`bynk-lsp` reference](/docs/tooling/bynk-lsp/).

| Key | Type | Default | Notes |
|---|---|---|---|
| `diagnostics_mode` | string | `"live"` | When diagnostics are computed: `"live"` or `"on_save"`. Any value other than `"on_save"` is treated as `"live"`. |
| `diagnostics_debounce_ms` | integer | `300` | Debounce interval, in milliseconds, for live diagnostics. |

## Legacy mode

Without a `bynk.toml`, a single `.bynk` file compiles as a standalone unit (the
[first-program](/book/tutorials/01-first-program/) flow). This is the simplest
way to start.

Project features expect the manifest-driven layout above. In particular
`bynkc test`, and the project-aware [`bynk` driver CLI](/docs/bynk-cli/)
commands — `bynk doctor`, `bynk new`, and `bynk dev` — require a project (a
`bynk.toml`, or a `src/` directory).
