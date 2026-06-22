# Tooling

Documentation for the tools around the Bynk compiler. If you just want editor
support, the how-to [Set up editor support](../guides/editor-and-tooling/editor-support.md)
is the quickest path; the pages here are the reference for each tool.

| Tool | Binary / package | Purpose |
|---|---|---|
| [`bynk-fmt`](bynk-fmt.md) | `bynkc fmt` | The canonical source formatter. |
| [`bynk-lsp`](bynk-lsp.md) | `bynkc-lsp` | The language server (diagnostics, hover, go-to-definition, formatting). |
| [`tree-sitter-bynk`](tree-sitter-bynk.md) | — | The grammar used for highlighting and structural tooling. |
| [`vscode-bynk`](vscode-bynk.md) | VS Code extension | Editor integration built on `bynkc-lsp`. |

All are built from the same repository as the compiler, so they track the
language version for version (currently v0.70).
