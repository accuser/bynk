---
title: "`vscode-bynk`"
---
The Visual Studio Code extension for Bynk. It provides syntax highlighting plus
the full language-server experience by launching [`bynkc-lsp`](/docs/tooling/bynk-lsp/). For
step-by-step setup, see the how-to
[Set up editor support](/docs/editor-and-tooling/editor-support/); this page is the
reference.

## Features

- Syntax highlighting (a TextMate grammar, mirrored from
  [`tree-sitter-bynk`](/docs/tooling/tree-sitter-bynk/)).
- Live diagnostics, hover with type signatures and doc blocks, and
  go-to-definition — all from the bundled `bynkc-lsp`.
- Format-on-save via the shared formatter (honours `editor.formatOnSave`).
- Status-bar items showing the project name and compiler version.
- **Snippets** for every construct — type a prefix (`context`, `commons`,
  `type`, `enum`, `fn`, `capability`, `provides`, `service`, `from http`,
  `from cron`, `agent`) and press <kbd>Tab</kbd> to scaffold it, then tab through
  the placeholders.
- **Scaffolding commands** — **Bynk: New Project** (scaffolds `bynk.toml` +
  `src/<name>.bynk`) and **Bynk: New Context** (adds a `context` file). Both
  refuse to overwrite an existing file.
- A **Get Started with Bynk** walkthrough (Welcome page → Help → walkthroughs)
  that sets up a project and a first context.
- A **`bynkc: check` build task** (Terminal → Run Task) that type-checks the
  whole project and routes errors — including in files you haven't opened — into
  the Problems panel via the `$bynkc` problem-matcher.

The extension activates on opening a `.bynk` file or any workspace containing a
`bynk.toml`.

## Build and install

From the `vscode-bynk/` directory:

```sh
npm install
npm run build           # tsc -p .
npx vsce package        # produces a .vsix
code --install-extension bynk-vscode-*.vsix
```

The extension needs `bynkc-lsp` available — build it with
`cargo build --release -p bynk-lsp` and put it on `PATH`, or set
`bynk.executablePath`.

## Settings

| Setting | Default | Purpose |
|---|---|---|
| `bynk.executablePath` | `bynkc-lsp` | Path to the language-server binary. |
| `bynk.compilerPath` | `bynkc` | Path to the `bynkc` compiler used by the `bynkc: check` build task. |
| `bynk.trace.server` | `off` | Trace LSP traffic (`off` / `messages` / `verbose`) in the "Bynk LSP" output channel. |

## Layout

| Path | What it is |
|---|---|
| `src/extension.ts` | Entry point: resolves and launches `bynkc-lsp` over stdio. |
| `src/scaffold.ts` | The **New Project** / **New Context** command handlers. |
| `src/tasks.ts` | The `bynkc: check` build-task provider. |
| `snippets/bynk.json` | Construct scaffolds, wired via `contributes.snippets`. |
| `walkthroughs/*.md` | The getting-started walkthrough steps. |
| `syntaxes/bynk.tmLanguage.json` | TextMate grammar (highlighting fallback). |
| `language-configuration.json` | Brackets, comments, and editor behaviour. |
| `package.json` | Manifest: activation events, settings, commands, build scripts. |
