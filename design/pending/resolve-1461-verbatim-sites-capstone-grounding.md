---
level: patch
changelog: Resolves #1461 (ground the verbatim_sites capstone) — both prerequisites the track doc named as unresolved ("chicken-and-egg") turn out to already be solved by existing infrastructure (ADR 0391's lower.rs splice-boundary decision; bynk_ts::printer::print's own span-checkpoint source-map mechanism). A third, previously-uncounted prerequisite is the real blocker: every direct callee of emit_project/emit_test_module/emit_integration_module must also convert (or be individually, permanently argued as opaque) before the top-level signature conversion has any real verbatim_sites benefit — several of those callees (emit_provider's own class wrapper, foreign runtime-helper text) are already permanently decided to stay opaque. The capstone is deferred as a single slice and named as its own future multi-slice arc, not attempted here. No code change.
---

## ADR: verbatim-sites-capstone-callee-cascade
title: The verbatim_sites capstone's two named prerequisites are already solved; the real blocker is a callee cascade neither prior pass counted
summary: ADR 0391 and bynk_ts::printer::print already answer Part 1's "chicken-and-egg" — the real remaining size driver is converting (or permanently arguing) every direct callee of the three orchestrator functions, not just their own signatures

**Context.** The track doc's Floor-correction §6 Part 1 (#1423/ADR 0399) named `emit_project`/
`emit_test_module`/`emit_integration_module`'s own `String`-returning signatures as the real
remaining `verbatim_sites` work, scheduled "after Arc E's steps 1–6 land" — which they have
(#1435–#1447, #1451–#1453). It named two open prerequisites: (2) a per-splice-point
representation for `emitter/lower.rs`'s permanently-opaque output (ADR 0391), and (3) a
source-map rebuild at the printing boundary, calling the pair "a genuine chicken-and-egg the
retirement review needs to accept rather than resolve." #1461 re-grounds both directly against
current `main` rather than carrying that framing forward unchecked.

**Decision.**

**Prerequisite 2 is already answered, not open.** ADR 0391 (accepted before #1423 was even
written) already decided `lower.rs`'s output stays opaque text "carried as opaque pre-rendered
text at its one well-defined splice boundary (`emit_block_as_function_body_with_return`'s
return value)... each wrapper function converts its own signature/declaration shape to real tree
nodes while carrying its own spliced body as one opaque blob." This is exactly the
per-splice-point representation Part 1 asked for — already in active use by every Arc C/D/E/F
slice that touches a `lower.rs`-calling function. Nothing new to design here.

**Prerequisite 3 is already answered, not open.** `bynk_ts::printer::print` (`bynk-ts/src/
printer.rs:229`) already builds a real source map from a `TsProgram`'s own top-level
`TsStmt.span: Option<Span>` fields (`TsStmt`'s own doc, `program.rs:47-56`, confirms this — "a
top-level statement's own span is currently recorded as a source-map checkpoint"). This is a
complete, already-shipped answer to "how does the map get rebuilt once printing happens once at
the boundary instead of at every `out.len()` checkpoint" — no `bynk-ts` change needed, no
choosing between "printer carries span attribution" and "rebuild from node spans" as Part 1
framed it: the latter already exists and already works. Converting these three functions would
need every top-level item they build to carry its own real span through to a `TsStmt`/`TsDecl`
construction site (mirroring the `smb.borrow_mut().record(out.len(), span)` calls already made
at the same points today) — real, mechanical work, but not a research question.

**The real blocker, uncounted by either prior pass: every direct callee of these three
functions must also convert (or be individually, permanently argued as opaque) before the
top-level conversion has any real effect.** `verbatim_sites` counts *source construction call
sites*, not runtime node counts — so wrapping `emit_project`'s own per-item loop body in one
`TsStmt::verbatim(...)` call, executed once per commons item at runtime, is still exactly one
call site in the source, identical to today's single whole-file wrap (`project.rs:1285`). The
metric only moves if the callees themselves stop needing that wrap. Direct read of
`emit_project`'s own per-item dispatch (`emitter.rs:420-473`) finds it calls `emit_capability`/
`emit_provider`/`emit_service`/`emit_agent`, all four still `(out: &mut String, ...)` — not by
oversight, but because `emit_provider`'s own class wrapper is *already, permanently* decided to
stay hand-written text (Decision C, found and confirmed during #1457's own grounding read of a
different function: "building the WHOLE class as one real `TsDecl::Class` tree would need every
method's own body captured into a local buffer for `Raw`-embedding," a scope decision, not a gap).
`emit_test_module`/`emit_integration_module` have the same shape one level down: both call
`include_str!`-based runtime-helper text (`stub_runtime_helpers`/`expectation_runtime_helpers`/
`property_runtime_helpers`/`history_runtime_helpers`/`observation_runtime_helpers`, `project/
tests_emit.rs:2278-2289` and siblings) — hand-authored, foreign-shaped TypeScript files bundled
at compile time, the same permanent footing `verbatim_sites`' own `adapter_bindings`/`runtime.ts`
exclusions already have — plus `emit_stub_class`/case-body builders that themselves still return
`String`, not a real node.

**Consequences.** The capstone is **not a single tractable slice** — it cascades into converting
(or individually, permanently arguing opaque) a dozen-plus functions across `emitter.rs`/
`emitter/emit.rs`/`project/tests_emit.rs`, several of which (`emit_provider`'s class wrapper, the
`include_str!` runtime-helper text) already have accepted, permanent reasons to stay opaque —
meaning the true floor for `verbatim_sites` via this path is **not 0 even after full conversion**,
at minimum one wrap per commons that declares a provider, plus the already-permanent
`adapter_bindings`/`runtime.ts`/foreign-runtime-helper sites. This capstone is deferred as a
single slice; scheduling it needs its own multi-slice arc (comparable in scope to Arc C/D/E/F
themselves — a real decomposition pass, not attempted here), not a follow-up issue sized against
the original "convert three signatures" framing. `design/tracks/the-typescript-tree.md`'s
Floor-correction §6 Part 1 is corrected: the source-map/lower.rs-opacity prerequisites are struck
as already-solved, and the real remaining-size driver (the callee cascade, with `emit_provider`'s
own permanent wrapper named as the first concrete non-zero-floor contributor) is recorded so a
future pass doesn't re-derive either as a surprise.
