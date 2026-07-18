---
level: patch
changelog: "A lex error inside a string-interpolation hole (`\"…\\(…)…\"`) is now reported at the offending bytes within the hole instead of at the file's opening bytes — the hole is re-lexed on its own and the error's spans were never rebased on the failure path, so an unexpected character, an integer overflow, or any lex error pointed at the wrong location and could split a multi-byte codepoint, tripping the parser's char-boundary invariant and panicking a source-slicing consumer (#716)."
---
