# Tooling

Documentation for the tools around the Karn compiler. If you just want editor
support, the how-to [Set up editor support](../guides/editor-and-tooling/editor-support.md)
is the quickest path; the pages here are the reference for each tool.

| Tool | Binary / package | Purpose |
|---|---|---|
| [`karn-fmt`](karn-fmt.md) | `karnc fmt` | The canonical source formatter. |
| [`karn-lsp`](karn-lsp.md) | `karnc-lsp` | The language server (diagnostics, hover, go-to-definition, formatting). |
| [`tree-sitter-karn`](tree-sitter-karn.md) | — | The grammar used for highlighting and structural tooling. |
| [`vscode-karn`](vscode-karn.md) | VS Code extension | Editor integration built on `karnc-lsp`. |

All are built from the same repository as the compiler, so they track the
language version for version (currently v0.44).
