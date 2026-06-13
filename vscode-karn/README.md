# Karn for VS Code

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Language support for the **[Karn](https://github.com/accuser/karn) language**
(`.karn` files) in Visual Studio Code: syntax highlighting plus full language
features backed by the bundled
[`karnc-lsp`](https://github.com/accuser/karn/tree/main/karn-lsp) language
server.

The extension activates on any `.karn` file, or on any workspace containing a
`karn.toml`.

## Features

- **Syntax highlighting** — a TextMate grammar, mirrored from the
  [tree-sitter grammar](https://github.com/accuser/karn/tree/main/tree-sitter-karn)
  (the source of truth).
- **Live diagnostics** — errors and warnings as you type, exactly as
  `karnc check` reports them, each tagged with its dotted category.
- **Hover** — type signatures and doc blocks.
- **Go-to-definition** and **find references** for types, functions,
  capabilities, services, and agents.
- **Rename** — workspace-wide and validated.
- **Completion**, **inlay hints** (inferred types), and **semantic tokens**
  (type-aware highlighting).
- **Code actions** — quick fixes for diagnostics that carry a suggestion.
- **Formatting** and **range formatting** via `karn-fmt` (honours
  `editor.formatOnSave`).
- **Document & workspace symbols** and **document highlights**.
- **Status bar** — the active project name (click to open `karn.toml`) and the
  language-server state (click to show its output).

All language features come from `karnc-lsp`; the extension is the client that
provisions and launches it.

## The language server

The extension needs the `karnc-lsp` binary and resolves it in this order, most
explicit first:

1. the `karn.executablePath` setting, when set;
2. `karnc-lsp` on your `PATH` (a dev or global install);
3. a copy previously downloaded by the extension;
4. otherwise it **downloads** the release matching this build for your platform
   from GitHub, verifies it against the release `SHA256SUMS`, and caches it.

So in the common case the extension just works — no manual server install. The
version it downloads is pinned per extension build (`karnServerVersion` in
`package.json`); if a `karnc-lsp` already on your `PATH` reports a different
version, the extension warns but still uses it.

If no server can be provisioned — an `executablePath` that doesn't resolve, an
unsupported platform, or a failed download — the failure is loud and actionable
(an error notification, a status-bar indicator, and commands to retry) rather
than silently degrading to grammar-only highlighting.

On a platform with no prebuilt server, build it from the workspace root and
point the setting at it:

```sh
cargo build --release -p karn-lsp   # → target/release/karnc-lsp
```

## Commands

Available from the Command Palette under **Karn**:

| Command | What it does |
| ------- | ------------ |
| **Karn: Restart Language Server** | Re-provision and restart the server. |
| **Karn: Download Language Server** | Force a fresh download of the pinned server. |
| **Karn: Show Language Server Output** | Open the "Karn LSP" output channel. |
| **Karn: Open Project Config (karn.toml)** | Open the workspace's `karn.toml`. |

## Settings

| Setting | Default | Purpose |
| ------- | ------- | ------- |
| `karn.executablePath` | `""` (auto-resolve) | Absolute path to a `karnc-lsp` binary to use. When empty, the extension resolves the server automatically (see above). |
| `karn.trace.server` | `off` | Trace LSP protocol traffic (`off` / `messages` / `verbose`) in the "Karn LSP" output channel. |
| `karn.inlayHints.enable` | `true` | Show Karn inferred-type inlay hints. A persistent, Karn-only preference; takes effect on the next edit or scroll. |

Two built-in VS Code settings also apply:

- **`editor.inlayHints.enabled`** — the instant, editor-wide on/off for inlay hints (toggles immediately). Use `karn.inlayHints.enable` when you want hints off for Karn specifically and left alone elsewhere.
- **`editor.semanticHighlighting.enabled`** — turns semantic tokens (the type-aware highlighting) on or off. The extension ships theme fallbacks for Karn's `capability` / `service` / `agent` / `provider` token types, so they colour out of the box.

## Build & install from source

From this directory:

```sh
npm install
npm run package                       # bundle, then package a .vsix
code --install-extension karn-vscode-*.vsix
```

`npm run package` bundles the extension with [esbuild](https://esbuild.github.io/)
and packages it with [`@vscode/vsce`](https://github.com/microsoft/vscode-vsce)
(both pinned as dev dependencies). Use `npm run build` alone for a plain bundle,
or `npm run watch` while developing.

See also
[Set up editor support](https://github.com/accuser/karn/blob/main/docs/src/how-to/tooling/editor-support.md)
for using `karnc-lsp` with other editors.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
