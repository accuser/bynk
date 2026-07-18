---
level: patch
changelog: "The LSP no longer panics when a multi-byte non-identifier char (`\"`, `€`, `—`, an emoji) precedes the receiver/callee it extracts from the line prefix — receiver and callee extraction now advance past the matched char by its UTF-8 length instead of assuming one byte, so completion, signature help, and hover survive a keystroke inside a string literal (#715)."
---
