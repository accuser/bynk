# 0370 — `combined_types_for_unit_info` relocates to `bynk-check::symbols`; the `FnName::Method` filter folds into `lower_attached_fn_sig_ir_from_types`

- **Status:** Accepted (v0.249.22)

summary: Continues Phase G of the #1137 retirement plan — one owner-side relocation, one filter folded behind an existing IR-lowering boundary

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase G) targets
`project.rs`'s remaining `bynk_syntax::ast` imports. Two of its sites shared one prologue
(`build_emit_unit_ctx`'s attached-method gathering for `uses`-imported types):

1. `combined_types_for_unit_info` was `combined_types_for`'s (`bynk-check::symbols`) own
   `UnitInfo`-shaped sibling, entirely expressible in `bynk-check` terms — `UnitInfo` itself is
   already `bynk-check`-owned (`bynk-check/src/project_model.rs`), so this was never a structural
   `bynk-emit` read, just a function living one crate away from the type it walks and the sibling
   it mirrors.
2. `build_emit_unit_ctx`'s own method-gathering loop filtered `MethodTable::{instance,statics}` to
   `FnName::Method` entries before lowering each to a signature via
   `ir::lower::lower_fn_sig_ir_from_types` — a raw `FnName` read sitting one step in front of an
   already-IR-lowering call, in a file the AST-cutover track exists to clear.

**Decision.** `combined_types_for_unit_info` moves verbatim into `bynk_check::symbols`, immediately
after `combined_types_for` (its own doc comment already named the shape it mirrors). The `FnName`
filter folds into a new sibling of `lower_fn_sig_ir_from_types` in the *excluded*
`bynk-emit/src/ir/lower.rs`: `lower_attached_fn_sig_ir_from_types(mt: &MethodTable, types, tys) ->
Vec<FnSig>`, which takes a whole `MethodTable` and returns every attached method's signature,
already filtered and lowered. `project.rs`'s `build_emit_unit_ctx` calls both relocated functions;
its own `imported_methods` loop body shrinks from an inline filter+map chain to one call.

**Consequences.** `ast_importers`: **unaffected (6)** — `project.rs` still counted on its remaining
names (`Block`, `CommonsItem`, `FnDecl`, `HandlerKind`, `ServiceProtocol`, `TypeDecl`, `TypeRef`,
`Visibility` — `FnName` is now gone from the import list entirely); Phase G continues in
P6.46–P6.49. No behavioural change: both moves are mechanical, and the filter's own semantics
(`FnName::Method` only, `FnName::Free` skipped as "never present in practice" per
`bynk-check/src/resolver.rs`'s own `MethodTable` doc) are unchanged, just relocated one call
earlier. Verified: zero-diff bless over the full e2e fixture corpus, `cargo test --workspace`,
`cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
