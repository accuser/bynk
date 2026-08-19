# 0357 — `file_mentions_json_error`/`_http_result`/`_connection` share one marker-parameterised `TypeRef` walk

- **Status:** Accepted (v0.249.9)

summary: Phase C of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.32) — the highest semantic-risk slice in this phase (drives conditional runtime imports), landed as a scoped deduplication rather than the plan's own more ambitious "`TyId` walk" framing

**Context.** `emitter.rs` carried three separately hand-written ~50-line functions
(`file_mentions_json_error`, `file_mentions_http_result`, `file_mentions_connection`), each pairing an
identical-shaped recursive `TypeRef` walk with an outer `CommonsItem` enumeration, differing from each
other in exactly one line of the inner walk (which built-in wrapper variant — `JsonError`/
`HttpResult`/`Connection` — stops the recursion and reports `true`). These three booleans drive
conditional runtime imports in the emitted header; a false negative is a `tsc --strict` failure, a
false positive an unused import — the reason this row was flagged the highest-risk item in this
phase's Phase C.

**Scoping correction: not a `TyId` walk.** The completion plan's own P6.32 row described this as
converting to "one shared `TyId` walk". That is not what these functions operate over: they walk
*declared* signature and type positions (`FnDecl::return_type`, a record field's `type_ref`, …), which
carry a raw `TypeRef`, not an already-resolved `TyId` — unlike an expression position, nothing in
`TypedCommons` pre-resolves a declaration's own type annotations into the `TyId` space. Doing so here
would mean invoking the checker's own type-reference resolution (a `LowerIrCtx`-shaped operation) at
every declaration site three predicates iterate over, purely to re-derive an identity these functions
never needed in the first place — real scope creep for a slice whose actual defect is duplication, not
representation. Landed as a `TypeRef`-based deduplication instead.

**Decision.** Added `TypeRefMarker` (`JsonError`/`HttpResult`/`Connection`) and one shared
`type_ref_mentions(t: &TypeRef, marker: TypeRefMarker) -> bool`, replacing all three inner walks.
Behavioural equivalence is exact, not approximate: `marker == <variant>'s own marker || type_ref_mentions(inner,
marker)` short-circuits to `true` without evaluating the recursive call when `t` matches the marker's
own wrapper — precisely reproducing each original function's own unconditional `=> true` arm (which
never recursed into that variant's own payload either) — and evaluates the recursive call, unchanged,
for every other wrapper variant. `file_mentions_json_error`'s and `file_mentions_http_result`'s outer
`CommonsItem` enumerations were byte-identical (word for word) and now share one
`commons_mentions_type` helper; `file_mentions_connection`'s outer walk stays separate (it also checks
agent `store_fields`, since a `Connection` can additionally live in a held `store` field, and treats
`Type`/`Event` as `false` rather than recursing into their fields) but calls the same shared inner
walk. Four new unit tests (`type_ref_mentions_tests`) pin the truth table directly — a marker's own
wrapper matching regardless of its inner type, a non-matching wrapper still recursing, `JsonError`
having no inner type to recurse into, and a plain base type matching nothing.

**Consequences.** `ast_importers`: **8 → 8**, unaffected — these three functions were never counted
*because of themselves*; `emitter.rs` retains its own much larger, still-open AST surface. Verified by
a full zero-diff bless against the entire e2e fixture corpus — the primary gate for this slice's own
named risk, since a wrong equivalence would show up as a spurious or missing runtime import — plus the
four new unit tests proving the truth table by direct construction, and a full `cargo test
--workspace`.
