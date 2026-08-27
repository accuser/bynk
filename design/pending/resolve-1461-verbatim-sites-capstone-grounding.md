---
level: patch
changelog: Resolves #1461 (ground the verbatim_sites capstone) — the lower.rs splice-boundary prerequisite the track doc named as unresolved is already solved by ADR 0391; the source-map prerequisite is only partly solved (bynk_ts::printer::print already checkpoints top-level TsStmt spans, but has no equivalent yet for the nested, per-statement checkpoints emit_project's own body-splicing callees merge in today, so it stays a real, narrower open question, not fully closed). The real size driver, uncounted by either prior pass, is that every direct callee of emit_project/emit_test_module/emit_integration_module must also convert (or be individually, permanently argued as opaque) before the top-level signature conversion has any real verbatim_sites benefit — several of those callees (emit_provider's own class wrapper, foreign runtime-helper text) are already permanently decided to stay opaque. The capstone is deferred as a single slice and named as its own future multi-slice arc, not attempted here. No code change.
---

## ADR: verbatim-sites-capstone-callee-cascade
title: The verbatim_sites capstone's lower.rs prerequisite is already solved, its source-map prerequisite only partly so; the real blocker is a callee cascade neither prior pass counted
summary: ADR 0391 answers Part 1's lower.rs question in full; bynk_ts::printer::print answers the source-map question only for top-level checkpoints, not the nested ones emit_project's callees rely on today — the real remaining size driver is converting (or permanently arguing) every direct callee of the three orchestrator functions, not just their own signatures

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

**Prerequisite 3 is only partly answered — narrower than first claimed, review of #1467 caught
this before merge.** `bynk_ts::printer::print` (`bynk-ts/src/printer.rs:229`) does build a real
source map from a `TsProgram`'s own **top-level** `TsStmt.span: Option<Span>` fields (`TsStmt`'s
own doc, `program.rs:47-56`, confirms the scope explicitly: "only a *top-level* statement's own
span is currently recorded"). That covers items that convert to genuinely flat, top-level real
nodes (`emit_type`/`emit_free_fn`'s straightforward cases). It does **not** cover what
`emit_project` relies on today for the common case: a **nested**, per-statement source map
merged in from a body-local `SourceMapBuilder` at a computed byte offset
(`emitter/emit.rs:975`'s `module.borrow_mut().merge(&body_smb.borrow(), &body_text, out, base,
0)`, the same pattern at `:2301`/`:2744-2770` for other spliced bodies) — and that finished,
merged map is what actually ships today (`project.rs:1273`'s own `emit_project` call, passed
straight through to the `Verbatim`-wrapped `StagedFile` at `project.rs:1285`, whose own node
carries `span: None` — `printer::print`'s mechanism isn't even in the current production path).
Once a handler/method body stays an opaque printed blob (ADR 0391's own splice-boundary
decision, and — per the callee-cascade finding below — the common case for
`emit_service`/`emit_agent`/`emit_provider`), that blob's own *interior* checkpoints have no
`TsStmt`-side home: `printer::print` only ever records one checkpoint at the blob's own start,
losing the fine-grained per-statement mapping inside it that real debugging depends on. This
residue is close to the chicken-and-egg Part 1 originally named — a real, still-open design
question (extending the printer to accept a pre-built nested map alongside a `Raw`/`Verbatim`
node, or a post-print reconciliation step in `bynk-emit`), not one this pass resolves.

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
`adapter_bindings`/`runtime.ts`/foreign-runtime-helper sites, plus the nested-source-map gap
above wherever a converted item's own body must stay an opaque blob. This capstone is deferred
as a single slice; scheduling it needs its own multi-slice arc (comparable in scope to Arc
C/D/E/F themselves — a real decomposition pass, not attempted here), not a follow-up issue sized
against the original "convert three signatures" framing. `design/tracks/the-typescript-tree.md`'s
Floor-correction §6 Part 1 is corrected: the `lower.rs`-opacity prerequisite is struck as
already-solved, the source-map prerequisite is narrowed to its real remaining scope (nested
checkpoints inside an opaque blob, not top-level items), and the real remaining-size driver (the
callee cascade, with `emit_provider`'s own permanent wrapper named as the first concrete
non-zero-floor contributor) is recorded so a future pass doesn't re-derive any of the three as a
surprise.
