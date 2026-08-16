---
level: patch
changelog: "AST_IMPORTER_EXCEPTIONS now also names project/tests_emit.rs, whose test/suite case bodies call straight into emitter.rs's Q7-settled (design/tracks/the-ir.md §3.7) body-rendering pass and read a handler's declared signature with no TyId available, the same shape #1176 already excluded for ir.rs/ir/lower.rs; ast_importers moves 8 to 7 — emitter.rs/emitter/lower.rs stay counted, since both still hold live, in-scope AST-declaration reads distinct from that body-rendering surface"
---
