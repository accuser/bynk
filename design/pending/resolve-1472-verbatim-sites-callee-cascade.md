---
level: patch
changelog: Resolves #1472 (scope the `verbatim_sites` capstone arc) — enumerates every direct callee of `emit_project`/`emit_test_module`/`emit_integration_module` (15 + ~13 respectively), finds 12 of `emit_project`'s own 15 already cheap to convert (real-node internally, signature-only), and settles the nested source-map design question #1461 left open: the printer should report the print-time offset of an opaque body blob instead of `emit_class_method_and_merge_source_map` reverse-engineering it, reusing `SourceMapBuilder::merge` machinery that already exists. Finds `inject_runtime_imports` was never actually entangled with that gap — it has its own independent fix (build the runtime-import node after the body loop, once `ctx.runtime_use` is fully known, order it first). Names 9 new permanent `Verbatim` sites by callee (`emit_provider`/`emit_agent`'s class wrappers, `emit_agent`'s history-driver, `emit_stub_class`'s class wrapper, 5 `tests_emit.rs` `include_str!` blocks) and 2 genuinely new, previously-unargued conversion candidates (four-plus near-identical test-function wrappers; an HS256 JWT-signer block better suited to `include_str!` extraction than tree conversion). Proposes an 8-slice sequence, printer extension first. No code change.
---

## ADR: verbatim-sites-capstone-callee-cascade-and-nested-checkpoint
title: The verbatim_sites capstone's callee cascade, enumerated and classified; the nested source-map gap resolves by having the printer report opaque-blob offsets instead of bynk-emit reverse-engineering them
summary: 12 of emit_project's 15 direct callees are cheap signature conversions; the nested-checkpoint gap has concrete prior art already in bynk-emit (emit_class_method_and_merge_source_map's own offset-recovery hack) and resolves by extending the printer to report that offset directly; inject_runtime_imports turns out unblocked by a simple construction-order fix, not entangled with the source-map gap after all

**Context.** #1461 (landed) found the `verbatim_sites` capstone's two named prerequisites already
solved or partly solved, and one previously-uncounted prerequisite — a callee cascade — as the
real remaining blocker, deferring it as "not a single tractable slice... comparable in scope to
Arc C/D/E/F," without counting the cascade or resolving the nested source-map question it left
open. #1472 is that count and that resolution.

**Decision.**

**The nested source-map gap already has working prior art inside `bynk-emit` — it needs
generalising, not designing from scratch.** `emit_class_method_and_merge_source_map`
(`bynk-emit/src/emitter/emit.rs:2270-2296`, review of #1381) is the shared helper every real
per-item body splice already funnels through (`emit_free_fn`, `emit_provider`'s ops,
`emit_agent`'s handler methods). It merges a body-local `SourceMapBuilder`'s own checkpoints into
the module map at the right offset — exactly prerequisite 3's job — but has to recover that offset
by subtracting known lengths and string-matching the printed text's own tail, because
`print_class_method` never reports where it placed the opaque body blob it was handed. It
degrades to silently skipping the mapping if that recovery fails. The fix: extend
`bynk_ts::printer::print`'s own existing per-statement mechanism (`out.len()` immediately before
`render_stmt`, already used for top-level `TsStmt.span` checkpoints) one level further — a
`TsStmt`/`TsClassMethod`/`TsObjectEntry::Method` variant carrying an optional nested checkpoint
table lets the printer call `SourceMapBuilder::merge` itself, at the real print-time offset, with
no reverse-engineering and no silent-skip fallback needed. One new optional field, one new printer
branch — the same "extend narrowly" posture this track's every prior operator/variant addition
took, not a parallel checkpoint system. `bynk-ts`-only; no `bynk-emit` behaviour change on its own.
This is the one prerequisite slice that unblocks every body-bearing callee in the cascade
simultaneously (`emit_free_fn`, every service/agent handler body, every generated test function),
so it sequences first.

**`inject_runtime_imports` was never actually blocked by the source-map gap.** #1461/#1462 both
assumed it was entangled with prerequisite 3 without checking its own two call sites directly.
Direct read: the runtime import's "base" names come from a syntax-level AST scan done *before*
the per-item body loop; the `bytes()`/`icu()` extra names it currently splices in via post-print
text surgery come from `ctx.runtime_use`, populated *during* that same loop. The text surgery
exists only because the current `String`-accumulator commits the header's bytes before the facts
needed to finish it are known. Once `emit_project` holds `Vec<TsStmt>` — where construction order
and final order are independent — the fix is: build the runtime-import node *after* the per-item
loop (same information available then as today), place it first in the assembled list. No second
pass, no reconciliation step, no dependency on the printer extension above. Moves from "argued,
not yet attempted" to solved design.

**`emit_project`'s own 15 direct callees, read in full:** 12 cheap (`write_header`/
`emit_project_imports`/`emit_cross_context_namespace_imports`/`emit_context_rebrands`/
`write_commons_doc`/`emit_type`/`emit_free_fn`/`emit_messages_bundle`/`emit_capability`/
`emit_make_surface`/`emit_boundary_helpers`/`emit_json_codec_helpers` — already real-node
internally, confirmed by direct read rather than the `writeln!`/`format!`-vs-`TsStmt::` heuristic
alone, since a function delegating to an already-real private helper under-reports on the
heuristic); 2 (`emit_provider`, `emit_agent`) cheap for their own skeleton but each contributing a
permanent Decision-C class-wrapper `Verbatim` site (`emit_agent` a second, the history-driver,
#1386); 1 (`emit_service`) the clearest recurring instance of the nested-checkpoint gap, not a
one-off. `project/tests_emit.rs`'s own two orchestrators carry the identical shape (own local
`SourceMapBuilder` merges, `:1936`/`:537`) plus two genuinely new findings: `emit_stub_class`'s own
Decision-C class wrapper, and 5 `include_str!` runtime-helper blocks (line citations corrected:
`:2279`/`:2287`/`:3404`/`:3408`/`:3420`, drifted from the track doc's prior clustered
`:2278-2289`). **Two real, previously-unargued conversion candidates, found here for the first
time:** four-plus near-identical hand-templated test-function wrappers
(`emit_test_case_function`/`emit_test_property_function`/`emit_test_history_property_function`/
`emit_contract_attack_function`, plus `emit_integration_module`'s own inline case wrapper) with no
comment anywhere arguing why they stay raw text; and `emit_system_http_support`'s own HS256
JWT-signer block, fixed boilerplate independent of any Bynk declaration — an `include_str!`
extraction candidate, not a tree-conversion one.

**Argued floor, numbered:** 9 new permanent `Verbatim` sites named by callee (`emit_provider`'s
class wrapper, `emit_agent`'s class wrapper and history-driver, `emit_stub_class`'s class wrapper,
5 `tests_emit.rs` `include_str!` blocks), on top of the 2 already counted
(`project.rs:2480`/`:2509`) — **11 known**, plus an unknown-until-attempted count of per-splice-point
`lower.rs` wraps proportional to the number of distinct real call sites once every body-bearing
callee converts — the single remaining open component, the same honesty ADR 0399/#1461 already
modelled, not resolved here.

**Sequencing proposed:** (1) the printer's nested-checkpoint extension, first, unblocks everything
else; (2) `inject_runtime_imports`'s reordering fix, independent, any time; (3) the 12+ cheap
callees across both files, batchable by cluster, several landable before (1) since they carry no
per-item bodies; (4) `emit_service`/`emit_agent` signatures, blocked on (1); (5) `emit_provider`/
`emit_stub_class` signatures; (6) the test-function-wrapper conversion, one shared fix across five
sites; (7) the JWT-signer's `include_str!` extraction; (8) the three orchestrators' own top-level
signatures, once every direct callee has landed or been permanently argued. Roughly 8-10 real
slices — confirms, rather than revises, #1461's own "comparable in scope to Arc C/D/E/F" estimate.

**Consequences.** No code changes in this pass. The capstone arc is now sequenceable: slice (1)
(the printer extension) and slice (2) (`inject_runtime_imports`'s reorder) are each independently
proposable as their own issue today, with no further grounding needed. The remaining slices are
sized against a real callee count, not a qualitative "dozen-plus." `design/tracks/the-typescript-tree.md`'s
§6 Floor-correction (Part 1) and §5's `verbatim_sites` bullet both updated with this outcome.
