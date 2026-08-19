# 0358 — The `TypeRef`-driven JSON/wire codec layer is ruled phase-7 printer work; `emitter/serialisation.rs` joins `AST_IMPORTER_EXCEPTIONS`

- **Status:** Accepted (v0.249.10)

summary: Phase D of the #1137 completion plan (`design/tracks/the-ir.md` §6a) — the re-settling this track's own lifecycle step 4 calls for, closing the plan's own highest-value open question

**Context.** `design/tracks/the-ir.md`'s own completion plan (§6a) opened four questions under Phase D,
deliberately deferred rather than silently assumed: whether the `TypeRef`-driven JSON/wire codec
renderer belongs to phase 6 or phase 7; whether a body-rendering function's own AST-typed parameter
counts as an AST-walking decision under §5's prose criterion; how to treat `project.rs`'s
cross-crate-blocked `own_contract_hashes`; and how to treat `Callee`'s own AST-carrying variants,
invisible to the probe. This PR argues and rules on all four, per `design/tracks/README.md`'s lifecycle
step 4 (a re-settling gets its own small reviewed PR, not folded into a slice).

**Decision 1 — the codec renderers: phase 7.** `emitter/serialisation.rs` was inspected directly rather
than estimated: it holds no `CommonsItem`-declaration-read surface at all (confirmed:
`grep -c bynk_syntax::ast` finds only its own `use` line and its `#[cfg(test)]` module) and no
`use crate::ir` either — nothing in this file has been resisting an available IR-native alternative,
because none exists. Its entire AST surface *is* the codec renderer (`emit_record_codec`/
`emit_sum_codec`/`serialise_expr`/`deserialise_expr`/`ts_inner_type` and siblings) — the same "how do I
render this checker type as TS source text" question §3.7 (Q7) already settled belongs to the eventual
printer (phase 7, `bynk-ts`), just applied to types instead of expressions. `emitter/serialisation.rs`
joins `AST_IMPORTER_EXCEPTIONS`, on grounds distinct from every prior entry — not Q7 body-rendering
residue (`ir.rs`/`ir/lower.rs`'s own reason), not test-only reach (`project/tests_emit.rs`'s own
reason), but a phase boundary. `emitter/emit.rs` was considered for the same treatment and **rejected**:
unlike `serialisation.rs`, it mixes real, still-open declaration-read surface (this plan's own Phase
E/F rows) in among its own codec-adjacent `TypeRef` sites, so excluding it now would hide real,
fixable work the same way a path-prefix rule would — the exact harm the "named not prefixed"
discipline (§5) exists to prevent.

**Decision 2 — Q7 rendering-signature params: not a new question.** §3.7 already settled this when it
opened this track's own cutover: the string-writing functions' own `Lowered`-returning shape survives
the cutover unchanged; only the *decisions* they re-derive move to IR reads. This session's own P6.30
and P6.31 slices are a live demonstration — the *dispatch* now reads `IrHandlerKind`, the wrapper's own
`h: &Handler` rendering parameter stayed exactly as Q7 said it should. No probe redefinition needed.

**Decision 3 — `own_contract_hashes`: a named residual floor, not excluded now.** Cross-crate-blocked
by `bynk_check::resolver::CrossContextService`/`contract::service_contract_hash`, which take
`TypeRef`/`Arc<TypeDecl>` by definition, with a real caller/callee hash-symmetry correctness
requirement against `symbols.rs::build_cross_context_info`. Structurally the same shape as the codec
renderers, but **not** added to the exclusion list now — `project.rs` still carries other, genuinely
convertible declaration-read surface (this plan's own P6.34/P6.35, not yet landed); excluding the whole
file today would hide that real work. Named as `project.rs`'s own likely eventual floor, a decision for
whichever future slice actually isolates `own_contract_hashes` as the file's *only* remaining reason.

**Decision 4 — `IrExprKind::Call { callee: Callee }`: out of R6.13's own frame.** `Callee` carries
`Arc<FnDecl>`/`Arc<TypeDecl>` across six variants, embedded throughout `bynk-check`'s own dispatch and
shadowing logic, with many non-emit readers. R6.13 and this track's own boundary (§1: "`bynk-emit`
names no AST type") target `bynk-emit`; `Callee` is a `bynk-check`-internal representation choice,
invisible to `ast_importers` by construction and unreachable by any phase-7 printer either. Ruled out
of scope entirely — not deferred, named explicitly so it is not silently lost.

**Consequences.** `ast_importers`: **8 → 7**, `emitter/serialisation.rs` excluded — a real, direct
probe movement from a doc-only ruling, not a code change. `AST_IMPORTER_EXCEPTIONS` grew from three
entries to four; `ast_importer_exclusion_is_named_not_prefixed`, `ast_importer_exceptions_still_exist_
and_still_import_the_ast`, and `ast_importers_excludes_the_named_pairs_but_counts_project_rs` all
updated in this commit, per this probe's own established discipline for any exclusion-list change.
`design/greenfield-status.md` updated in the same commit. `serialisation.rs`'s own `#[cfg(test)]`
residue (named in §5's "known gap" paragraph) is moot now that the whole file is excluded.
`emitter/lower.rs:5963`'s own unrelated residue is unaffected, still P6.38's to close. Verified by a
full zero-diff bless against the entire e2e fixture corpus (this is a pure exclusion-list and doc
change — `serialisation.rs` itself is untouched) and a full `cargo test --workspace`.
