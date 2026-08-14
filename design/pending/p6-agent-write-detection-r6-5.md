---
level: patch
changelog: "An agent handler no longer gets an unnecessary implicit-commit wrapper when a locally-shadowed name (e.g. a handler parameter reusing a store Map/Set/Cache/Log field's name) merely looks like a store write by name — write detection now reads the checker's own resolved dispatch instead of matching bare identifiers (R6.5). Cell fields were never affected by this shadowing gap"
---
