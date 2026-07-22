# bynk-ide

[![crates.io](https://img.shields.io/crates/v/bynk-ide.svg)](https://crates.io/crates/bynk-ide)
[![docs.rs](https://img.shields.io/docsrs/bynk-ide)](https://docs.rs/bynk-ide)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **IDE/LSP analysis surface for the [Bynk](https://github.com/accuser/bynk)
compiler** — the non-bailing diagnostics a language server consumes.

Where the CLI compile path *bails* on the first failure and emits, this layer
analyses a whole project (or a single file) **without bailing**, returning every
diagnostic plus the captured analysis tables for the editor to query:

- `diagnose` — best-effort single-file diagnostics (lex → parse-with-recovery →
  resolve → check), always returning a diagnostic list.
- `diagnose_project` — overlay-aware, file-attributed whole-project analysis: the
  per-file diagnostics, the binding index, inlay hints, expression types, scoped
  locals, and the unit→source map.

## Where it sits

```text
bynk-syntax  ◀── bynk-render · bynk-fmt · bynk-check ◀── bynk-emit ◀── bynk-ide
```

`bynk-ide` is the top of the library set, over
[`bynk-syntax`](https://crates.io/crates/bynk-syntax) +
[`bynk-check`](https://crates.io/crates/bynk-check) +
[`bynk-emit`](https://crates.io/crates/bynk-emit). The
[`bynk-lsp`](https://crates.io/crates/bynk-lsp) language server is built on it, so
it links the analysis libraries — not the whole compiler binary. The `bynkc`,
`bynk`, and `bynk-lsp` binaries are front-ends over the compiler set.

## Use

```toml
[dependencies]
bynk-ide = "0.224"
```

```rust
use std::collections::HashMap;

let single = bynk_ide::diagnose(source);                 // Vec<Diagnostic>
let project = bynk_ide::diagnose_project(root, &HashMap::new());
for file in &project.files {
    // file.source_path, file.diagnostics, …
}
```

Most users get these diagnostics through an editor (via
[`bynk-lsp`](https://crates.io/crates/bynk-lsp)) rather than depending on this
crate directly. See the [API docs](https://docs.rs/bynk-ide).

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
