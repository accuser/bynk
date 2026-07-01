# bynk-render

[![crates.io](https://img.shields.io/crates/v/bynk-render.svg)](https://crates.io/crates/bynk-render)
[![docs.rs](https://img.shields.io/docsrs/bynk-render)](https://docs.rs/bynk-render)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **shared diagnostic-rendering layer for the
[Bynk](https://github.com/accuser/bynk) compiler**.

Given a slice of `bynk_syntax::CompileError` plus the source and a filename, it
produces the human and machine forms of a Bynk diagnostic:

- **human** — rich, source-pointing [`ariadne`](https://crates.io/crates/ariadne)
  output (with a colourless variant for byte-stable transcripts),
- **`short`** — one terse `path:line:col: severity[category]: message` line per
  error, the format the editor problem-matcher parses, and
- **`json`** — the structured line form the same span/severity data feeds for
  machine consumers.

Every Bynk front-end renders through this one crate, so the CLI, the project
builder, and the editor all display the same error identically. The crate is a
pure presentation layer: it depends on
[`bynk-syntax`](https://crates.io/crates/bynk-syntax) **only** (plus `ariadne`)
and never sees the checker or emitter — structured diagnostics flow *down* into
it, never the other way.

## Where it sits

```text
bynk-syntax  ◀── bynk-render · bynk-fmt · bynk-check ◀── bynk-emit ◀── bynk-ide
```

The `bynkc`, `bynk`, and `bynk-lsp` binaries are front-ends over this set. Most
users see this crate's output through the
[`bynkc`](https://crates.io/crates/bynkc) / [`bynk`](https://crates.io/crates/bynk)
CLIs rather than depending on it directly.

## Use

```toml
[dependencies]
bynk-render = "0.113"
```

```rust
// `errors: &[bynk_syntax::CompileError]`, with the source and a label.
bynk_render::print_errors(errors, source, filename);          // ariadne, to stderr
let short = bynk_render::render_errors_short(errors, source, filename);
```

See the [API docs](https://docs.rs/bynk-render) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
