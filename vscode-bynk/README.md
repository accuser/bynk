# Bynk for VS Code

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Language support for the **[Bynk](https://github.com/accuser/bynk) language**
(`.bynk` files) in Visual Studio Code: syntax highlighting plus full language
features backed by the bundled
[`bynkc-lsp`](https://github.com/accuser/bynk/tree/main/bynk-lsp) language
server.

The extension activates on any `.bynk` file, or on any workspace containing a
`bynk.toml`.

## Features

- **Syntax highlighting** — a TextMate grammar, mirrored from the
  [tree-sitter grammar](https://github.com/accuser/bynk/tree/main/tree-sitter-bynk)
  (the source of truth).
- **Live diagnostics** — errors and warnings as you type, exactly as
  `bynkc check` reports them, each tagged with its dotted category.
- **Hover** — type signatures and doc blocks.
- **Go-to-definition** and **find references** for types, functions,
  capabilities, services, and agents.
- **Rename** — workspace-wide and validated.
- **Completion**, **inlay hints** (inferred types), and **semantic tokens**
  (type-aware highlighting).
- **Code actions** — quick fixes for diagnostics that carry a suggestion.
- **Formatting** and **range formatting** via `bynk-fmt` (honours
  `editor.formatOnSave`).
- **Document & workspace symbols** and **document highlights**.
- **Test Explorer** — run a project's `test` and `integration` blocks from the
  Testing view (or **Bynk: Run Tests**), with click-through from a failing
  assertion to its `.bynk` line. Runs `bynkc test --format json`; compile
  failures land in the Problems panel. A **`▷ Run Test | Debug Test`** CodeLens
  (and native gutter run/debug glyphs) sit at each test in the editor — disable
  the lens with `bynk.testCodeLens`.
- **Debugging** — set a breakpoint in a `.bynk` file and press **Debug**: in the
  Test Explorer (Node, `bynkc test --inspect`) or via a `bynk` `launch.json`
  config for the dev-server worker (workerd, `bynk dev --inspect`). Delegates to
  VS Code's JavaScript debugger; breakpoints resolve to the exact `.bynk`
  statement through the emitted source maps.
- **`bynkc: check` build task** — type-check the whole project on demand, errors
  routed to the Problems panel.
- **Status bar** — the active project name (click to open `bynk.toml`) and the
  language-server state (click to show its output).

All language features come from `bynkc-lsp`; the extension is the client that
provisions and launches it.

## The language server

The extension needs the `bynkc-lsp` binary and resolves it in this order, most
explicit first:

1. the `bynk.executablePath` setting, when set;
2. `bynkc-lsp` on your `PATH` (a dev or global install);
3. a copy previously downloaded by the extension;
4. otherwise it **downloads** the release matching this build for your platform
   from GitHub, verifies it against the release `SHA256SUMS`, and caches it.

So in the common case the extension just works — no manual server install. The
version it downloads is pinned per extension build (`bynkServerVersion` in
`package.json`); if a `bynkc-lsp` already on your `PATH` reports a different
version, the extension warns but still uses it.

If no server can be provisioned — an `executablePath` that doesn't resolve, an
unsupported platform, or a failed download — the failure is loud and actionable
(an error notification, a status-bar indicator, and commands to retry) rather
than silently degrading to grammar-only highlighting.

On a platform with no prebuilt server, build it from the workspace root and
point the setting at it:

```sh
cargo build --release -p bynk-lsp   # → target/release/bynkc-lsp
```

## Commands

Available from the Command Palette under **Bynk**:

| Command | What it does |
| ------- | ------------ |
| **Bynk: Restart Language Server** | Re-provision and restart the server. |
| **Bynk: Download Language Server** | Force a fresh download of the pinned server. |
| **Bynk: Show Language Server Output** | Open the "Bynk LSP" output channel. |
| **Bynk: Open Project Config (bynk.toml)** | Open the workspace's `bynk.toml`. |
| **Bynk: Run Tests** | Run the project's tests via `bynkc test --format json` and report them in the Testing view. |

## Settings

| Setting | Default | Purpose |
| ------- | ------- | ------- |
| `bynk.executablePath` | `""` (auto-resolve) | Absolute path to a `bynkc-lsp` binary to use. When empty, the extension resolves the server automatically (see above). |
| `bynk.compilerPath` | `""` (`bynkc` on `PATH`) | Path to the `bynkc` compiler used by the `bynkc: check` task and the Test Explorer. When empty, `bynkc` is taken from `PATH`. |
| `bynk.trace.server` | `off` | Trace LSP protocol traffic (`off` / `messages` / `verbose`) in the "Bynk LSP" output channel. |
| `bynk.inlayHints.enable` | `true` | Show Bynk inferred-type inlay hints. A persistent, Bynk-only preference; takes effect on the next edit or scroll. |

Two built-in VS Code settings also apply:

- **`editor.inlayHints.enabled`** — the instant, editor-wide on/off for inlay hints (toggles immediately). Use `bynk.inlayHints.enable` when you want hints off for Bynk specifically and left alone elsewhere.
- **`editor.semanticHighlighting.enabled`** — turns semantic tokens (the type-aware highlighting) on or off. The extension ships theme fallbacks for Bynk's `capability` / `service` / `agent` / `provider` token types, so they colour out of the box.

## Build & install from source

From this directory:

```sh
npm install
npm run package                       # bundle, then package a .vsix
code --install-extension bynk-vscode-*.vsix
```

`npm run package` bundles the extension with [esbuild](https://esbuild.github.io/)
and packages it with [`@vscode/vsce`](https://github.com/microsoft/vscode-vsce)
(both pinned as dev dependencies). Use `npm run build` alone for a plain bundle,
or `npm run watch` while developing.

See also
[Set up editor support](https://github.com/accuser/bynk/blob/main/docs/src/guides/editor-and-tooling/editor-support.md)
for using `bynkc-lsp` with other editors.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
