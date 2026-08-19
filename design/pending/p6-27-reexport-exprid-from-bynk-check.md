---
level: patch
changelog: "P6.27: re-exported bynk_syntax::ast::ExprId as bynk_check::checker::ExprId and retargeted bynk-emit's two direct-ExprId sites (project.rs, emitter.rs's sum_owner_of_variant) to it -- enabling only, no behaviour change. The checker's own public API already traffics in ExprId (expr_types and Callee are both HashMap<ExprId, _>, Q2's own settled totality story), so this exposes an already-public dependency rather than adding one, and preserves the exact identity the checker keyed by (a bynk-emit-local id type would not). ast_importers unaffected (9) -- neither retargeted site was the reason either file was counted."
---

## ADR: reexport-exprid-from-bynk-check

title: `bynk_check::checker::ExprId` re-exports `bynk_syntax::ast::ExprId`; `bynk-emit`'s two direct-`ExprId` sites read it from there instead

summary: Phase B of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.27) — a cheap decoupling, enabling later slices rather than moving the probe itself

**Context.** `project.rs:46` and `emitter.rs:2161` (`sum_owner_of_variant`'s `id` parameter) each spelled
`bynk_syntax::ast::ExprId` directly — the one line keeping each file's own `bynk_syntax::ast`
dependency alive independent of every other AST name either file uses. The checker's own public API
already keys both `TypedCommons::expr_types` and its `Callee` map by `ExprId` (`HashMap<ExprId, _>`),
so `bynk-emit` was already coupled to this exact identity type through `bynk-check` in every other
call site — `project.rs`'s own `unit_callees: HashMap<ExprId, Callee>` fields, for one — just without
a name to reach it by except through the AST crate directly.

**Decision.** Add `pub use bynk_syntax::ast::ExprId;` to `bynk-check/src/checker.rs`, next to its
existing (private) `use bynk_syntax::ast::*;`. Retarget `project.rs`'s import
(`bynk_check::checker::{ExprId, TyId, Types}`, dropping the separate `use bynk_syntax::ast::ExprId;`
line) and `emitter.rs`'s (`bynk_check::checker::{CheckedProgram, ExprId, NamedKind, Ty, TyId,
TypedCommons, Types}`, dropping `ExprId` from its own P6.26 explicit AST import list and un-qualifying
`sum_owner_of_variant`'s parameter). §3.2 (Q2)'s own settled totality story is unaffected — this
re-exports an identity type, not a container shape; `expr_types` stays `HashMap<ExprId, TypedExpr>`,
`R4.9`'s `IndexVec` conversion remains filed as separate, non-blocking residue. `bynk-emit::ir`
(`ir.rs`/`ir/lower.rs`, excluded from `ast_importers`) keeps importing `ExprId` from `bynk_syntax::ast`
directly — the `Ast → Ir` lowering pass's own job, unaffected by this slice, not in scope.

**Consequences.** `ast_importers`: **9 → 9**, unaffected — neither `project.rs` nor `emitter.rs` was
counted *because* of `ExprId` (each has other, still-open AST names in its own list), so removing this
one name moves nothing. Purely enabling: any future slice touching `project.rs`'s or `emitter.rs`'s
own `ExprId`-keyed call sites now has a `bynk-check`-local name to reach it by, one fewer reason either
file needs to spell `bynk_syntax::ast` at all once its remaining real R6.13 surface (§6a Phase C/E)
converts. Verified by a full zero-diff bless against the entire e2e fixture corpus (a signature/import
retarget, same underlying type, cannot alter emitted output) and `cargo test --workspace`.
