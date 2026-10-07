---
level: minor
changelog: "`bynk.toml` refuses what it doesn't define: an unknown table, a key outside any table, or an unknown key in `[project]` or `[lsp]` is now an error from `bynkc` and `bynk`, with the nearest name suggested (`[pahts]` → did you mean `[paths]`?). A table for a planned feature (`[dependencies]`, `[workspace]`, `[deploy]`) names the issue tracking it rather than building with none of its behaviour, so a manifest that built before can now fail. The language server still reads only the tables it needs (#1665)"
---
