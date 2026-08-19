---
level: patch
changelog: "P6.26: deleted the five `use bynk_syntax::ast::*;` glob imports in bynk-emit (emitter.rs, project.rs, emitter/workers.rs, emitter/workers_entry.rs, emitter/serialisation.rs), adding each file its own explicit, minimal import list -- a mechanical no-behaviour-change refactor. emitter/emit.rs and emitter/lower.rs each carried a module-level `use super::*;` inheriting emitter.rs's own glob (Rust's own privacy rule makes a parent module's private `use` visible to descendant modules), so both also gained their own direct explicit list -- but that `use super::*;` channel itself was narrowed, not closed: emit.rs (4,632 lines, 168 AST type references, previously zero literal occurrences of the counted string) still inherits whatever emitter.rs continues to expose. Review (#1259) found this left a durable false-zero hazard for a future slice that deletes a child's own list while the channel and the parent's own AST dependency both remain, so the ast_importers probe itself (xtask/src/greenfield_status.rs) now also counts any file with a module-level `use super::*;` whose parent module still imports the AST -- catching not just emit.rs/lower.rs but project/diagnostics.rs, a third, currently AST-free file exposed to the identical latent channel. ast_importers rises 7 -> 9 as a direct, deliberate consequence of both changes -- the probe becoming durably honest, not a regression."
---

## ADR: delete-ast-glob-imports

title: `bynk-emit`'s five `use bynk_syntax::ast::*;` globs become explicit per-file imports; the `ast_importers` probe learns to see through `use super::*;`

summary: Phase A of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.26) — makes the `ast_importers` probe trustworthy, and durably so, before any further conversion slice lands

**Context.** The gated `ast_importers` probe (`xtask/src/greenfield_status.rs`) counts files under
`bynk-emit/src` containing the literal string `bynk_syntax::ast`, minus a named exclusion list, and
must reach 0 for phase 6 to retire. Five files carried `use bynk_syntax::ast::*;`
(`emitter.rs`, `project.rs`, `emitter/workers.rs`, `emitter/workers_entry.rs`,
`emitter/serialisation.rs`), and `emitter/lower.rs`/`emitter/emit.rs` inherited the same names
transitively through a module-level `use super::*;` — Rust's own privacy rule makes a parent module's
private `use` visible to descendant modules. `emitter/emit.rs` in particular is 4,632 lines with 168
AST type references and zero literal occurrences of the counted string: entirely invisible to the
probe. Driving `ast_importers` to 0 without addressing this would certify a cutover `emit.rs` never
received.

**Decision, part 1 — the source change.** Delete all five globs; add each affected file its own
direct, explicit `use bynk_syntax::ast::{...};` import list, including `emitter/lower.rs`,
`emitter/emit.rs`, and `project/tests_emit.rs` (an excluded file broken the same way, via its own
`use super::*;` from `project.rs`). Each list was determined by removing the globs, rebuilding, and
collecting every "cannot find type/value" error's name per file (`cargo check --all-targets`, to also
catch `#[cfg(test)]`-only usages), rather than hand-enumerating from source reading. Every file's list
is minimal — the build is clean with zero unused-import warnings — and no AST/IR name collision was
found, so no aliasing was needed.

**What this decision does *not* do, corrected during review (#1259).** `emitter/lower.rs` and
`emitter/emit.rs`'s own `use super::*;` was **not** deleted — both files also pull non-AST names
(`ts_*` helpers, `LowerCtx`, etc.) from `emitter.rs` through it, so removing it is its own, separate
piece of work, left to a future slice. Each file is now *self-sufficient* for AST names (neither
depends on inheritance today), but the inheritance channel itself stays open. Review found this is
exactly the failure mode this slice exists to close, one level up: several names appear in both
`emitter.rs`'s list and `emit.rs`'s (`Expr`, `ExprKind`, `TypeRef`, `TypeDecl`, `Ident`, `BaseType`,
`Statement`, `Block`), so a future slice that converts `emit.rs`'s real usage but deletes its own
explicit list — believing the file done — would still compile via inheritance for as long as
`emitter.rs` keeps any overlapping name in its own list, and `ast_importers` would then report
`emit.rs` cleared while it still walks the AST through those residual sites.

**Decision, part 2 — the probe itself.** `ast_importer_files` now also counts a file with a
module-level (column-0, not nested in a test block) `use super::*;` whose sibling parent module file
still contains `bynk_syntax::ast` — regardless of whether the child's own text spells the string.
This makes the probe durably honest rather than honest-as-of-today: as long as `emitter.rs` keeps
importing the AST directly and `emit.rs`/`lower.rs` keep `use super::*;`, both stay counted no matter
what their own explicit lists do. The same rule also newly counts `project/diagnostics.rs`, which
carries an identical module-level `use super::*;` from AST-importing `project.rs` and is today
genuinely AST-free in its own body (verified: zero occurrences of any AST type name `project.rs`
exposes) — it is counted anyway, on the same conservative principle, since nothing prevents a future
edit from using an inherited name unqualified without ever being caught. Two new unit tests
(`module_level_super_glob_detection_ignores_nested_test_mod`,
`super_glob_children_of_an_ast_importing_parent_are_detected`) pin both the detection logic and this
exact regression scenario against the live tree.

**Consequences.** `ast_importers`: **7 → 9** — `emit.rs` counted for having 168 real AST references
newly visible to the probe (source change), plus `emit.rs`/`lower.rs`/`diagnostics.rs` counted for
carrying a live `use super::*;` from an AST-importing parent (probe change); `emit.rs`/`lower.rs`
would already have been caught by the source change alone, so the probe change's own net new count is
`diagnostics.rs`, +1. This is the probe becoming durably honest, not a regression — read
`design/greenfield-status.md`'s own diff for this slice accordingly. `wildcard_arms`/`keep_in_sync`
trend rows are unchanged from the prior commit's own run (accumulated drift from earlier, unrelated
merges, picked up by that fresh probe run — not caused by this diff); `xtask`'s own `test_density`
moved (34.9% → 35.3%) from this slice's own two new unit tests, expected. Verified by a full zero-diff
bless against the entire e2e fixture corpus (a `bynk-emit` imports-only change cannot alter emitted
output) and `cargo test --workspace`. This slice makes every remaining AST-walking site in
`bynk-emit` — including through inheritance — visible to the probe, which is the precondition every
conversion slice in the completion plan (P6.27 onward) depends on.
