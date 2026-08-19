---
level: patch
changelog: "P6.42: project.rs's in_memory_logical_path stops re-implementing SourceUnit::name(), and project/diagnostics.rs drops its use super::*; -- two false AST coupling channels, no behavioural change. ast_importers: 7 -> 6 (project/diagnostics.rs cleared; project.rs itself still counted, six names remain)."
---

## ADR: p6-42-source-unit-name-dedup

title: `project.rs`/`project/diagnostics.rs` drop two false AST coupling channels — opens the #1137 retirement plan's Phase G

summary: `in_memory_logical_path` was re-implementing `SourceUnit::name()` byte-for-byte; `project/diagnostics.rs`'s only import need was two names, not the whole parent module

**Context.** `design/tracks/the-ir.md` §6a's own closing assessment named `project.rs` as one of
three files that stay counted by `ast_importers` "for reasons this plan traced directly rather than
left as estimates" — but did not itself re-verify that every one of those reasons was still a real
structural read rather than an artefact of how the code happened to be written. A fresh research pass
scoping the #1137 retirement plan (§6b) checked this directly.

**Decision.** Two independent findings, landed together because both are pure dedup with no
behavioural surface:

1. `project.rs:645-650`'s `in_memory_logical_path` matched `SourceUnit::{Commons, Context, Adapter,
   Suite}` to extract each variant's own `name`/`target` field — the exact four arms
   `bynk_syntax::ast::SourceUnit::name()` (`bynk-syntax/src/ast.rs:213-221`) already implements. This
   was never a structural AST-declaration read this track exists to close; it was an inherent method
   call spelled out by hand. Replaced with `unit.name()`, which removes `SourceUnit` from
   `project.rs`'s own import list — nothing else in the 4,448-line file names it.

2. `project/diagnostics.rs`'s sole `use super::*;` existed to reach exactly two names from its parent,
   `PathBuf` and `CompileError` — not because the module needs anything else `project.rs` re-exports.
   Replaced with direct `use std::path::PathBuf;` / `use bynk_syntax::error::CompileError;`, which
   removes the module's only inheritance channel into `project.rs`'s still-AST-importing parent (the
   P6.26-review super-glob hardening rule, `xtask/src/greenfield_status.rs`'s
   `has_module_level_super_glob`/`super_glob_parent_imports_ast`).

**Consequences.** `ast_importers`: **7 → 6**. `project/diagnostics.rs` (51 lines, zero AST references
of its own — confirmed by direct grep) drops out entirely; `project.rs` itself is unaffected by this
slice and remains counted on its own six remaining names (`Block`, `CommonsItem`, `FnDecl`, `FnName`,
`HandlerKind`, `ServiceProtocol`, `TypeDecl`, `TypeRef`, `Visibility` — `SourceUnit` gone from that
list). This confirms the retirement plan's most consequential premise: `project.rs`'s claimed blocker
was not structural, and the file has a real path to clearing (§6b's Phase G, P6.43–P6.49).
`design/greenfield-status.md`'s `ast_importers` row updated in the same commit (`cargo xtask
greenfield-status --apply`). Verified: zero-diff bless over the full e2e fixture corpus, `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check` — all pass
unchanged, as expected for a pure dedup.
