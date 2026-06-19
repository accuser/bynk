# bynk-lsp

[![crates.io](https://img.shields.io/crates/v/bynk-lsp.svg)](https://crates.io/crates/bynk-lsp)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **Language Server for the [Bynk](https://github.com/accuser/bynk)
language**. The crate builds the `bynkc-lsp` binary, which any LSP-capable
editor can speak to for live diagnostics, navigation, and refactoring of `.bynk`
projects.

Built on [`tower-lsp`](https://crates.io/crates/tower-lsp), it shares the
compiler ([`bynkc`](https://crates.io/crates/bynkc)) and formatter
([`bynk-fmt`](https://crates.io/crates/bynk-fmt)) with the CLI, so the editor
sees exactly the diagnostics `bynkc check` would report.

## Capabilities

- **Diagnostics** — re-runs the compiler on change and publishes errors and
  warnings with their dotted categories.
- **Hover** — type signatures and doc blocks.
- **Go-to-definition** and **find references** for types, functions,
  capabilities, services, and agents.
- **Rename** (workspace-wide, validated).
- **Formatting** and **range formatting** (via `bynk-fmt`).
- **Document & workspace symbols**, **document highlights**.
- **Completion**, **inlay hints** (inferred types), **semantic tokens**
  (type-aware highlighting), and **code actions** (quick fixes for suggested
  diagnostics).
- **File watching** across the project.

The full capability list is specified in
[`design/bynk-lsp-spec.md`](https://github.com/accuser/bynk/blob/main/design/bynk-lsp-spec.md).

## Install

```sh
cargo install bynk-lsp
```

Or build from the workspace:

```sh
cargo build --release -p bynk-lsp   # → target/release/bynkc-lsp
```

This produces the **`bynkc-lsp`** binary. Requires a stable Rust toolchain,
2024 edition (MSRV 1.85).

## Use

`bynkc-lsp` speaks LSP over stdio. Most users consume it through the
[VS Code extension](https://github.com/accuser/bynk/tree/main/vscode-bynk),
which bundles and launches it automatically. For other editors, point your LSP
client at the `bynkc-lsp` binary and associate it with the `bynk` language /
`.bynk` files. See
[Set up editor support](https://github.com/accuser/bynk/blob/main/docs/src/guides/editor-and-tooling/editor-support.md).

The server discovers a project by walking up to the nearest `bynk.toml` (falling
back to single-file mode if there is none). It logs to `~/.bynk-lsp.log` at
`warn` by default; set `BYNK_LSP_LOG` (e.g. `BYNK_LSP_LOG=debug`) to raise the
level. `bynkc-lsp --version` prints the version.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
