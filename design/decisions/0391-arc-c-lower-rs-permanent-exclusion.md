# 0391 — `emitter/lower.rs` is a deliberate, permanent exclusion from Arc C's own scope, not a future conversion target

- **Status:** Accepted (v0.259.1)

summary: `lower.rs` is the compiler's own second code-generation pass, not a bounded, file-specific conversion target — converting it would re-architect the compiler's whole lowering strategy, comprehensive language-surface work Arc C was never scoped to cover

**Context.** Arc C's own remaining surface, once slice 7 (#1329) landed, reduced to `emit_project`'s
call graph — an orchestrator spanning `emitter.rs`, `emitter/emit.rs` (4,776 lines, 72 top-level
functions), and, pulled in transitively through seven of `emit.rs`'s own top-level functions via
`emit_block_as_function_body_with_return`, `emitter/lower.rs` (6,210 lines, 372 write-macro calls).
Every prior Arc C slice's own scope has been "wrap this function's existing string-building calls in
real `bynk-ts` nodes" — a bounded, groundable, file-specific unit of work. `lower.rs` does not fit
that shape: it is a 90-function statement/expression lowerer covering the entire Bynk expression and
statement grammar at once (general expression lowering — `lower_method_call` alone is 1,044 lines —
match-to-IIFE compilation, ten-plus per-builtin-type "kernels", if/binary-op/field-access/lambda/
record-spread lowering, indexed-collection index-maintenance codegen), not a bounded set of real
constructs the way even the largest prior conversions (`workers_entry.rs`, 1,660 lines) turned out to
be once actually read.

**Decision.** `lower.rs`'s real output stays a `String` permanently, carried as opaque pre-rendered
text at its one well-defined splice boundary (`emit_block_as_function_body_with_return`'s return
value) wherever `emit.rs`'s own future slices need it — the same "opaque carrier" pattern this track
has used repeatedly at smaller scale (`deserialise_call`/`brand_assertion`/`claim_predicate_to_js`,
#1327's own `__eventsDispatch` closure body), applied once at this one splice boundary instead of many
small ad-hoc ones. This does not block any future `emit.rs` slice — each wrapper function converts its
own signature/declaration shape to real tree nodes while carrying its own spliced body as one opaque
blob. Converting `lower.rs` for real remains a legitimate possible future track if ever justified,
scoped and argued on its own terms — a fundamentally larger, likely multi-month undertaking, not a
handful of Arc C slices.

`emitter/lower.rs` is deliberately **not** added to `xtask/src/greenfield_status.rs`'s own
`TS_WRITES_EXCLUDED_FILES` — that list's own doc comment already distinguishes `ir/lower.rs`
(excluded: builds Rust-internal strings during checker→IR lowering, never emitted syntax) from
`emitter/lower.rs` itself ("which is" real emission code, in the same comment's own words) — silently
excluding it now would reverse that documented distinction rather than answer it. `ts_writes` keeps
counting `lower.rs`'s own real write-macro calls; Arc C's own eventual retirement review names an
argued, non-zero floor for `ts_writes` instead — the same honest-correction shape `verbatim_sites`'s
own floor (at least 2) and `ast_importers`'s own floor (5, phase 6's own retirement) already took,
rather than chasing an unreachable 0.

**Consequences.** A real, permanent, deliberate narrowing of R7.1's original "the tree omits nothing
real" framing — recorded here, in `design/tracks/the-typescript-tree.md`'s §6, and in
`design/bynk-compiler-trajectory.md`'s own R7.1 text, not glossed over. `emit_integration_module`/
`emit_test_module` (`project/tests_emit.rs`, deferred by slice 7 specifically because of this
then-unresolved question) are unblocked. Arc C's own real remaining-slice estimate is revised upward
by this decision's own arithmetic, not downward — the decomposition it enables is real work still to
schedule, not work removed from the phase.
