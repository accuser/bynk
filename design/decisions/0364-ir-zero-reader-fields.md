# 0364 — Four zero-production-reader AST-typed `ir.rs` fields deleted; two others investigated and found to be load-bearing, not redundant

- **Status:** Accepted (v0.249.16)

summary: Phase F of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.39) — R6.13's own field-level gap, invisible to `ast_importers` by construction

**Context.** `ir.rs` still held six AST-typed fields inside otherwise-IR-native structures, each named
by an earlier grounding pass as having zero production readers: `GlobalRef::sum: Arc<TypeDecl>`,
`IrExprKind::Record::def: Arc<TypeDecl>`, `IrItem::Type::def: Arc<TypeDecl>`, `IrItem::Fn::def:
Arc<FnDecl>`, `IrExprKind::RefinedCheck::{base: BaseType, refinement: Option<Refinement>}`, and
`IrPat::Refined::refinement: Refinement`. Per P6.24a's own precedent, `ast_importers` cannot see this
surface at all — `ir.rs`/`ir/lower.rs` are named exclusions (the `Ast → Ir` lowering pass's own job) —
so this is real R6.13 progress with zero probe movement, not a substitute for it.

**Four deleted, re-verified directly against the tree before removal.** `GlobalRef::sum` — read only
by a test assertion (`ir/lower.rs`); its own construction site already computes `nullary_variant_owner`
purely for its `.is_some()` disambiguation, so dropping the field is a one-line change at the call
site. `IrExprKind::Record::def` — read only by test assertions across two construction sites (one a
bare `named_decl(ty, cx)` call whose only purpose was populating this field). `IrItem::Type::def` —
its one production consumer (`emitter.rs`'s `type_shape_for` call site) already destructured `{ shape,
.. }`, ignoring it. `IrItem::Fn::def` — `lower_fn_item_ir`, the one constructor, has no production call
site anywhere in the tree today; only test assertions read the field, including one that specifically
asserted `Arc::ptr_eq` identity-sharing, now moot. All four are redundant identity metadata: nothing
that could not already be recovered from another field, the caller's own context, or the checker's
`TypedCommons` directly.

**Two investigated and declined — a real correction, not an oversight.** `RefinedCheck`'s own doc
comment states its purpose plainly: "a refined-type/inline-predicate boolean check." Its three fields
are `value: Box<IrExpr>` (what to check) and `base`/`refinement` (what to check it *against*) — removing
the latter two would leave a node that can express "check something" with no way to say what the check
*is*. `IrPat::Refined`'s own doc comment: `refinement`'s payload is R5.4's own refinement test itself,
not a cross-reference to it. Both fields differ categorically from the four deleted above: those were
redundant caches of an identity already available elsewhere; these are the *only* place the dormant
node's own semantic content lives. Both nodes are unreached by any shipped emitter path today (P6.2's
own emitter-side cutover hasn't landed) — the same "zero production readers because nothing has been
wired to this yet" shape `Question`/`Is` were in before P6.15/P6.16, not "zero production readers
because the field is redundant." Left as-is; a future slice that wires either node into a real call
site is what will exercise them, not this one.

**Consequences.** `ast_importers` unaffected — invisible to the probe by construction (excluded
files). Verified by the full `ir::lower` unit suite (134/134) and a full zero-diff bless against the
entire e2e fixture corpus (structural field removal in dormant/redundant-metadata positions cannot
alter emitted output) plus a full `cargo test --workspace`.
