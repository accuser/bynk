# `vscode-karn`

The Visual Studio Code extension for Karn. It provides syntax highlighting plus
the full language-server experience by launching [`karnc-lsp`](karn-lsp.md). For
step-by-step setup, see the how-to
[Set up editor support](../guides/editor-and-tooling/editor-support.md); this page is the
reference.

## Features

- Syntax highlighting (a TextMate grammar, mirrored from
  [`tree-sitter-karn`](tree-sitter-karn.md)).
- Live diagnostics, hover with type signatures and doc blocks, and
  go-to-definition — all from the bundled `karnc-lsp`.
- Format-on-save via the shared formatter (honours `editor.formatOnSave`).
- Status-bar items showing the project name and compiler version.
- **Snippets** for every construct — type a prefix (`context`, `commons`,
  `type`, `enum`, `fn`, `capability`, `provides`, `service`, `on http`,
  `on cron`, `agent`) and press <kbd>Tab</kbd> to scaffold it, then tab through
  the placeholders.
- **Scaffolding commands** — **Karn: New Project** (scaffolds `karn.toml` +
  `src/<name>.karn`) and **Karn: New Context** (adds a `context` file). Both
  refuse to overwrite an existing file.
- A **Get Started with Karn** walkthrough (Welcome page → Help → walkthroughs)
  that sets up a project and a first context.

The extension activates on opening a `.karn` file or any workspace containing a
`karn.toml`.

## Build and install

From the `vscode-karn/` directory:

```sh
npm install
npm run build           # tsc -p .
npx vsce package        # produces a .vsix
code --install-extension karn-vscode-*.vsix
```

The extension needs `karnc-lsp` available — build it with
`cargo build --release -p karn-lsp` and put it on `PATH`, or set
`karn.executablePath`.

## Settings

| Setting | Default | Purpose |
|---|---|---|
| `karn.executablePath` | `karnc-lsp` | Path to the language-server binary. |
| `karn.trace.server` | `off` | Trace LSP traffic (`off` / `messages` / `verbose`) in the "Karn LSP" output channel. |

## Layout

| Path | What it is |
|---|---|
| `src/extension.ts` | Entry point: resolves and launches `karnc-lsp` over stdio. |
| `src/scaffold.ts` | The **New Project** / **New Context** command handlers. |
| `snippets/karn.json` | Construct scaffolds, wired via `contributes.snippets`. |
| `walkthroughs/*.md` | The getting-started walkthrough steps. |
| `syntaxes/karn.tmLanguage.json` | TextMate grammar (highlighting fallback). |
| `language-configuration.json` | Brackets, comments, and editor behaviour. |
| `package.json` | Manifest: activation events, settings, commands, build scripts. |
