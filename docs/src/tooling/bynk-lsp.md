# `bynk-lsp`

The Bynk language server. The `bynk-lsp` crate builds the `bynkc-lsp` binary, a
[tower-lsp](https://github.com/ebkalderon/tower-lsp) server that communicates
over **stdio**. Editors talk to it; most users reach it through the
[VS Code extension](vscode-bynk.md) rather than directly.

## Features

| Capability | Notes |
|---|---|
| Diagnostics | Live, from `bynkc::diagnose` (recovering compilation); published on change. |
| Hover | Type signatures and doc blocks; binding-correct via the project index (v0.25), with a name-match fallback for not-yet-indexed kinds. |
| Go-to-definition | Types, functions, capabilities, services, agents; cross-file and binding-correct via the project index (v0.25). |
| References | Project-wide, from the binding index (v0.25) — including clause lists and test units. |
| Rename | Project-wide with `prepareRename`; validated by re-analysis + index equality, versioned edits (v0.25). |
| Formatting | Whole-document and range formatting, via the shared [`bynk-fmt`](bynk-fmt.md). |
| Document symbols | An outline of the file for the editor's symbol view. |
| File watching | Re-checks diagnostics when `.bynk` files change on disk. |

Text is synced in full (`TextDocumentSyncKind::FULL`). When a project root with a
`bynk.toml` is found, the server enables cross-file lookups; otherwise it works in
single-file mode.

## Build

From the workspace root:

```sh
cargo build --release -p bynk-lsp
```

The binary is `target/release/bynkc-lsp`. Put it on `PATH`, or point your editor
at it explicitly (in VS Code, the `bynk.executablePath` setting).

## Internals

The crate is split into focused modules:

| Module | Role |
|---|---|
| `main.rs` | Server entry point, `Backend` state, request dispatch. |
| `position.rs` | Byte-offset ↔ LSP position conversion. |
| `symbols.rs` | Symbol lookups for hover and go-to-definition. |
| `index_queries.rs` | Pure queries over the project binding index: references, rename planning and validation (v0.25). |
| `document_symbols.rs` | The document-symbol outline. |
| `project.rs` | `bynk.toml` project configuration. |

`Backend` holds the project root, parsed config, and open documents behind a
`tokio::sync::RwLock`.

## Logging

The server logs to `~/.bynk-lsp.log`; the verbosity is tunable via the
`BYNK_LSP_LOG` environment variable. `bynkc-lsp --version` prints the version
without entering the protocol loop.
