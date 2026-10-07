---
level: patch
changelog: "`fmt` treats line endings as no formatting difference: a CRLF copy of a canonical file (a Windows checkout with `core.autocrlf=true`) passes `fmt --check` and isn't rewritten, and a file that does need formatting is written with LF throughout, where a `---` doc block used to keep its CRs. Format-on-save in the editor follows the same rule. `bynk new` also writes a `.gitattributes` (`*.bynk text eol=lf`) (#1763)"
---
