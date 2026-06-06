# `karn-lsp`

The Karn language server. The `karn-lsp` crate builds the `karnc-lsp` binary, a
[tower-lsp](https://github.com/ebkalderon/tower-lsp) server that communicates
over **stdio**. Editors talk to it; most users reach it through the
[VS Code extension](vscode-karn.md) rather than directly.

## Features

| Capability | Notes |
|---|---|
| Diagnostics | Live, from `karnc::diagnose` (recovering compilation); published on change. |
| Hover | Type signatures and doc blocks, with a cross-file fallback in a project. |
| Go-to-definition | Types, functions, capabilities, services, agents; cross-file in a project. |
| Formatting | Whole-document and range formatting, via the shared [`karn-fmt`](karn-fmt.md). |
| Document symbols | An outline of the file for the editor's symbol view. |
| File watching | Re-checks diagnostics when `.karn` files change on disk. |

Text is synced in full (`TextDocumentSyncKind::FULL`). When a project root with a
`karn.toml` is found, the server enables cross-file lookups; otherwise it works in
single-file mode.

## Build

From the workspace root:

```sh
cargo build --release -p karn-lsp
```

The binary is `target/release/karnc-lsp`. Put it on `PATH`, or point your editor
at it explicitly (in VS Code, the `karn.executablePath` setting).

## Internals

The crate is split into focused modules:

| Module | Role |
|---|---|
| `main.rs` | Server entry point, `Backend` state, request dispatch. |
| `position.rs` | Byte-offset ↔ LSP position conversion. |
| `symbols.rs` | Symbol lookups for hover and go-to-definition. |
| `document_symbols.rs` | The document-symbol outline. |
| `project.rs` | `karn.toml` project configuration. |

`Backend` holds the project root, parsed config, and open documents behind a
`tokio::sync::RwLock`.

## Logging

The server logs to `~/.karn-lsp.log`; the verbosity is tunable via the
`KARN_LSP_LOG` environment variable. `karnc-lsp --version` prints the version
without entering the protocol loop.
