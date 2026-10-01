---
level: minor
changelog: "`String.replace` inserts its replacement **literally** (#1650). It lowered to `replaceAll(from, to)` with a string replacement, which JS `$`-expands, so `\"aaa\".replace(\"a\", \"$&b\")` returned `\"ababab\"` rather than `\"$&b$&b$&b\"`, and `$1`, `` $` ``, `$'` and `$$` were rewritten too. When the replacement came from request data, the output depended on `$` sequences its author never wrote. It now lowers to a function replacer, `replaceAll(from, () => to)`, whose return value JS never expands. Programs that relied on the expansion lose it: Bynk's documented surface has no regex replace, so no `$1` group reference ever had a group to refer to. Proved at runtime by the behavioural fixture `1650_string_replace_literal`."
---
