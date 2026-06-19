# `bynk-fmt`

Bynk's formatter. There is one implementation, in `bynkc::fmt`; the `bynk-fmt`
crate is a thin re-export of it so other tools (the CLI, the LSP) can share it.
You invoke it as `bynkc fmt` — see the how-to
[Format your code with `bynk-fmt`](../guides/editor-and-tooling/format.md) for usage.

## What it does

`format_source(source, &FormatOptions)` tokenises and parses the source, then
re-prints the AST in canonical form. It is **idempotent** — formatting formatted
code is a no-op — and it returns a `FormatError` (carrying the parse diagnostics)
if the source does not parse.

## Options

`FormatOptions` controls the output:

| Field | Type | Default |
|---|---|---|
| `indent` | `IndentStyle` (`Tab` or `Spaces(n)`) | `Tab` |
| `max_line_width` | `u32` | `100` |
| `trailing_comma` | `bool` | `true` |

The CLI uses the defaults; a project can set `[fmt]` keys in
[`bynk.toml`](../reference/manifest.md) (`indent`, `max_line_width`).

## Canonical style

- Tab indentation, one tab per nesting level.
- K&R braces — the opening brace stays on the construct's line.
- Trailing commas in multi-line lists (records, sums, parameters).
- One blank line between top-level declarations; none inside record/sum/parameter
  lists or between match arms.
- A doc block sits directly above its declaration, with no blank line between.
- One space around binary operators and after commas; no padding inside
  parentheses.
- A soft 100-column width guides parameter wrapping.

## Programmatic use

```rust
use bynk_fmt::{format_source, FormatOptions};

let formatted = format_source(source, &FormatOptions::default())?;
```

This is exactly what `bynkc fmt` and the language server's formatting requests
call, so editor format-on-save and CLI formatting always agree.
