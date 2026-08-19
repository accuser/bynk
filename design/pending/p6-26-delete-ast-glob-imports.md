---
level: patch
changelog: "P6.26: deleted the five `use bynk_syntax::ast::*;` glob imports in bynk-emit (emitter.rs, project.rs, emitter/workers.rs, emitter/workers_entry.rs, emitter/serialisation.rs) plus the two `use super::*;` glob-inheritors (emitter/lower.rs, emitter/emit.rs), replacing each with an explicit, minimal per-file import list -- a mechanical no-behaviour-change refactor. emitter/emit.rs (4,632 lines) had inherited the entire AST module through the emitter.rs glob and use super::*, so it held 168 AST type references with zero literal bynk_syntax::ast occurrences and was structurally invisible to the gated ast_importers probe; deleting the glob makes it visible. ast_importers rises 7 -> 8 as a direct, deliberate consequence -- the probe becoming honest about a file it was blind to, not a regression. Zero-diff bless confirmed (imports-only change)."
---

## ADR: delete-ast-glob-imports

title: `bynk-emit`'s five `use bynk_syntax::ast::*;` globs (and two `use super::*` glob-inheritors) become explicit per-file imports

summary: Phase A of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.26) — makes the `ast_importers` probe trustworthy before any further conversion slice lands

**Context.** The gated `ast_importers` probe (`xtask/src/greenfield_status.rs`) counts files under
`bynk-emit/src` containing the literal string `bynk_syntax::ast`, minus a named exclusion list, and
must reach 0 for phase 6 to retire. Five files carried `use bynk_syntax::ast::*;`
(`emitter.rs`, `project.rs`, `emitter/workers.rs`, `emitter/workers_entry.rs`,
`emitter/serialisation.rs`), and two more (`emitter/lower.rs`, `emitter/emit.rs`) inherited the same
names transitively through `use super::*;` — Rust's own privacy rule makes a parent module's private
`use` visible to its descendant modules. `emitter/emit.rs` in particular is 4,632 lines with 168 AST
type references and zero literal occurrences of the counted string: entirely invisible to the probe.
Driving `ast_importers` to 0 without addressing this would certify a cutover `emit.rs` never received.

**Decision.** Delete all five globs and add each affected file (including `emitter/lower.rs`,
`emitter/emit.rs`, and `project/tests_emit.rs`, an excluded file that also inherited via
`use super::*;` from `project.rs`) its own direct, explicit `use bynk_syntax::ast::{...};` import list
— determined by removing the globs, rebuilding, and collecting every "cannot find type/value" error's
name per file (`cargo check --all-targets`, to also catch `#[cfg(test)]`-only usages), rather than
hand-enumerating from source reading. Each file's list is minimal: the build is clean with zero
unused-import warnings after adding exactly these names. No AST/IR name collisions were found — the
build succeeded on the first pass with no aliasing needed.

**Consequences.** `ast_importers`: **7 → 8**, entirely attributable to `emitter/emit.rs` becoming
counted (confirmed: `grep -rl bynk_syntax::ast bynk-emit/src/` lists exactly the same 11 files as
before plus `emit.rs`, minus the unchanged 3-file exclusion list). This is the probe becoming honest,
not a regression — read `design/greenfield-status.md`'s own diff for this slice accordingly. No other
file gained AST visibility (`emitter/events_fanout.rs` has exactly one AST name, inside a doc comment;
`project/diagnostics.rs` has none). Verified by a full zero-diff bless against the entire e2e fixture
corpus (imports-only change, no emitted-output difference possible) and `cargo test --workspace`. This
slice makes every remaining AST-walking site in `bynk-emit` visible to the probe for the first time,
which is the precondition every conversion slice in the completion plan (P6.27 onward) depends on.
