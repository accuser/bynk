# 0375 — Dead code deleted; `is_effectful_return` relocates to fix a backwards `Ast ⇄ Ir` dependency

- **Status:** Accepted (v0.249.27)

summary: Opens Phase H of the #1137 retirement plan — first of the `emitter.rs`/`emit.rs`/`lower.rs` conversions that close real defects without moving the probe

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) targets
`emitter.rs`/`emitter/emit.rs`/`emitter/lower.rs`'s genuinely convertible surface. Neither file can
leave `ast_importers`'s count regardless of how much converts (`emit.rs`/`emitter/lower.rs` both stay
counted through their own `use super::*;` while `emitter.rs` itself imports the AST) — so every Phase
H slice is justified on defect-closure grounds, not probe movement, starting with the two cheapest,
lowest-risk items this file's own remaining surface offered:

1. **`build_deps_object_ty`** (`emitter/emit.rs`, `#[allow(dead_code)]`) has zero callers anywhere in
   the workspace — confirmed by grep. Dead code, one `Ident` reference gone with it.
2. **`is_effectful_return`** (`&TypeRef → bool`, matching `TypeRef::Effect(_, _)`) lived in
   `emitter/emit.rs`, but `ir/lower.rs`'s own `lower_service_handler_signature_ir` was calling *up*
   into it (`crate::emitter::is_effectful_return`) — the `Ast → Ir` lowering pass reaching into the
   `emitter` module it should only ever be called *from*. `ir/lower.rs:605-612`'s own doc comment
   already treated the function as canonical for computing `effectful` from the return type's
   syntactic shape (not the resolved `Ty::Effect(_)` shape) — the dependency direction was backwards
   relative to which side already understood itself as the authority.

**Decision.** Deleted `build_deps_object_ty` outright. Moved `is_effectful_return` into `ir/lower.rs`
(an `AST_IMPORTER_EXCEPTIONS` file), immediately before `lower_service_handler_signature_ir`, its own
only in-crate caller understanding its exact semantics. `emitter/emit.rs` and `emitter/lower.rs`
(which had its own separate call site, reached via the `use super::*;`/`pub(crate) use emit::*;`
glob-export chain that broke once the function moved) both now import
`crate::ir::lower::is_effectful_return` directly rather than relying on that chain.

**Consequences.** `ast_importers`: **unaffected (5)** — neither file's own import list changes shape
(`emit.rs` still counted via `use super::*;`; `ir/lower.rs` was already excluded). Fixes a real
layering defect: the `Ast → Ir` boundary now runs one direction consistently, and the function's own
doc comment records why it belongs on the `Ir`-lowering side (matching P6.29/P6.30's own established
"decision reads the resolved/canonical side" posture, applied here to *where a shared helper lives*
rather than to a caller's own dispatch). Verified: zero-diff bless over the full e2e fixture corpus,
`cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
