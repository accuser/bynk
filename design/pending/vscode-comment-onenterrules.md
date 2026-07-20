---
level: patch
changelog: "vscode-bynk: pressing Enter inside a `--` line comment now continues it, and inside/after a `---` doc-comment fence now keeps the same indentation instead of falling back to VS Code's generic behaviour; the two are disambiguated so a `---` fence is never treated as a `--` line comment (closes #306)."
---
