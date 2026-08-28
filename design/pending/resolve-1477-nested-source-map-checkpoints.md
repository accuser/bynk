---
level: patch
changelog: Resolves #1477 — `bynk_ts::TsStmt` gains an optional `nested_map: Option<SourceMapBuilder>` field, and the printer merges it into the caller's own module map at the real print-time offset (`render_block_stmts`'s own new handling), reachable from three new sibling entry points (`print_stmt_and_merge`, `print_class_method_and_merge`, `print_object_entry_and_merge`) that append into the caller's own buffer rather than returning a fresh string, so offsets stay correct once spliced anywhere but the very start of it. Existing `print_stmt`/`print_class_method`/`print_object_entry`/`print` are unchanged in behaviour; `print`'s own top-level loop additionally threads its module map through automatically. No `bynk-emit` call site converted — `bynk-ts` only.
---
