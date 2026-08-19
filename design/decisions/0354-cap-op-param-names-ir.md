# 0354 — `cap_op_param_names` reads `ir::lower::capability_op_sig_from_commons` instead of walking `CommonsItem::Capability` by hand

- **Status:** Accepted (v0.249.6)

summary: Phase C of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.29) — a declaration-read conversion, evidence for the §6a.D re-settling rather than a file clear

**Context.** `emitter/lower.rs`'s `cap_op_param_names` looked up a capability operation's parameter
names by walking `cx.commons().commons.items` for a `bynk_syntax::ast::CommonsItem::Capability`
matching `cap`, then its `ops` for one matching `op` — the one remaining raw-AST-declaration read in
an otherwise already-`OpSig`-driven function (the parameter list itself already read
`lower_op_sig_ir_from_commons`'s resolved output, per #1187's own prior pass). `LowerCtx`/`ModuleCtx`
carry only a `&TypedCommons`, never a `&CheckedProgram`, so the existing `CheckedProgram`-driven
`lower_capability_item_ir` isn't directly callable from this call site — the same reason
`lower_op_sig_ir_from_commons` exists as `lower_op_sig_ir`'s own commons-only sibling.

**Decision.** Added `capability_op_sig_from_commons(commons: &TypedCommons, cap: &str, op: &str) ->
Option<OpSig>` to `ir/lower.rs` (excluded from `ast_importers`) — the `TypedCommons`-only counterpart
to `lower_capability_item_ir`, wrapping the existing `lower_op_sig_ir_from_commons` once a match is
found. The walk itself is unchanged, only relocated: "find the op named `op` on the capability named
`cap`" still has no IR-native replacement (nothing indexes capabilities by name once lowered), so this
still reads `TypedCommons::commons.items` directly — the same acknowledgment #1187's own scoping pass
already made for this exact spot. `cap_op_param_names` now calls it and maps `Option<OpSig>` to
`Vec<String>`/`Vec::new()`, preserving both of the original loop's behaviours precisely: first match in
item order (a capability name match with no matching op falls through to check later items, exactly
like the original `if let ... && ... && let Some(o) = ...` chain), and an empty result rather than a
panic on no match anywhere. A new unit test
(`capability_op_sig_from_commons_finds_the_named_op`) pins both the found case (against the same
fixture `lower_capability_item_ir_assembles_ops_in_declaration_order` already uses) and both `None`
cases (unknown capability, known capability's unknown op).

**Consequences.** `ast_importers`: **8 → 8**, unaffected — `emitter/lower.rs` was never counted
*because* of this one site; it retains other, still-open AST names (Q7-surviving body-rendering
params, per §6a's Phase E). Verified by a full zero-diff bless against the entire e2e fixture corpus
(a lookup relocation with identical fallthrough behaviour cannot alter emitted output) and a full
`cargo test --workspace`.
