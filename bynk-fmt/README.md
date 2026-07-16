# bynk-fmt

[![crates.io](https://img.shields.io/crates/v/bynk-fmt.svg)](https://crates.io/crates/bynk-fmt)
[![docs.rs](https://img.shields.io/docsrs/bynk-fmt)](https://docs.rs/bynk-fmt)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **formatter for the [Bynk](https://github.com/accuser/bynk) language**.
Given Bynk source, it produces the one canonical formatting — comments and
layout-significant trivia preserved.

Formatting is fundamentally an AST walk, so this crate depends on the
[`bynk-syntax`](https://crates.io/crates/bynk-syntax) leaf **only** — not the
type checker or emitter. That keeps it small: downstream consumers (the
[`bynkc-lsp`](https://crates.io/crates/bynk-lsp) language server and third-party
tools) get the formatter without pulling in the whole compiler. The
[`bynkc`](https://crates.io/crates/bynkc) compiler re-exports this crate as
`bynkc::fmt` for its `bynkc fmt` command.

Most users format Bynk through the CLI (`bynkc fmt`) or format-on-save in the
editor, rather than depending on this crate directly. See
[Format your code with `bynk-fmt`](https://bynk-lang.org/docs/editor-and-tooling/format/).

## Where it sits

```text
bynk-syntax  ◀── bynk-render · bynk-fmt · bynk-check ◀── bynk-emit ◀── bynk-ide
```

`bynk-fmt` sits directly on the `bynk-syntax` leaf, alongside the other
first-layer libraries. The `bynkc`, `bynk`, and `bynk-lsp` binaries are
front-ends over the compiler set.

## Use

```toml
[dependencies]
bynk-fmt = "0.180"
```

```rust
use bynk_fmt::{format_source, FormatOptions};

let pretty = format_source(source, &FormatOptions::default())?;
```

The public API is small:

- `format_source(source, options) -> Result<String, FormatError>`
- `FormatOptions` / `IndentStyle` — formatting configuration.
- `FormatError` — a parse error in the input (you cannot format what does not
  parse).

See the [API docs](https://docs.rs/bynk-fmt) for details.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
