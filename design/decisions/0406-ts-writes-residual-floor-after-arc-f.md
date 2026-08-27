# 0406 — ts_writes's residual after Arc F is mostly already-argued print-and-splice, with one real, previously-unverified gap in emit_agent's own history-driver

- **Status:** Accepted (v0.289.28)

**Context.** Arc F (#1449, landed as #1451–#1453) named a large residual after its own three
slices — the DO-class Decision-C wrapper text, `pred_condition_and_message`,
`claim_predicate_to_js`, `inject_runtime_imports`, the `__eventsDispatch` carve-out, and
`emit_agent`'s own ~101-site cluster — and explicitly flagged the last as unverified: "this
pass's own sampling found it consistently tree-native-with-print-and-splice at roughly a dozen
sampled points, not read end to end — supports confidence but is not a guarantee against a
hidden unconverted chunk." #1462 is that end-to-end read, plus formal collection of the other
named clusters. `ts_writes` reads **875** today (re-verified directly against the gated probe's
own predicate, file by file: `lower.rs` 371, `emit.rs` 226, `tests_emit.rs` 83,
`serialisation.rs` 63, `emitter.rs` 64, `workers_entry.rs` 34, `project.rs` 20, `workers.rs` 12,
`events_fanout.rs` 2).

**Decision.**

**`emit_agent`'s own 99 sites (current line count, drifted slightly from Arc F's own "~101"
citation, reconciled) read as two genuinely different shapes, not one.** Read in full,
not sampled:

- **66 sites are the already-argued print-and-splice idiom, confirmed correct on this read, not
  assumed:** the state interface (`TsDecl::Interface`, fully real), the zero-value factory
  (`TsDecl::Function`, fully real except per-field `Cell` initialisers, which lower through
  `lower.rs`'s own permanently-excluded general expression lowering, ADR 0391), the rehydration
  checks (each splices a real `bynk_ts::print_expr(&serialisation::deserialise_expr(...))` result
  into a small hand-templated check line — the same "opaque consumer needs its own print"
  treatment the in-place comment at `emit.rs:4681-4685` already names), `loadState`/`commitState`
  (both fully real `TsClassMethod` fragments; only each invariant/transition's own predicate stays
  opaque, per ADR `arc-c-lower-rs-permanent-exclusion`'s own general-expression-lowering scope —
  confirmed by direct read, not the stale comment's own claim alone), the per-handler method
  prologue/epilogue wrapper (params/return-type real via `TsClassMethod`, the load/commit/
  events-flush wiring one opaque `TsStmt::Raw`, the exact "two-level offset merge" shape
  `emit_service` (#1361) already established — named in place at `emit.rs:5354-5361`), the
  Workers `fetch` dispatch method and the agent-construction factory (both fully real, no
  opacity at all).
- **The DO class's own wrapper (header/fields/constructor, `emit.rs:4808-4844`) is `emit_agent`'s
  own instance of the same Decision-C precedent `emit_provider`'s class wrapper already
  established** (`emit.rs:4845-4868`'s own comment confirms this explicitly — "the same boundary
  #1359's own `emit_provider` already established") — not a separate, unnamed gap; the permanent-
  wrapper reasoning #1461 named for `emit_provider` covers this site too.
- **A real, previously-unverified gap: the history-driver (`__bynkDriveHistory_*`,
  `emit.rs:5893-6023`, gated on `ctx.history_target_agents.contains(&a.name.name)`) is genuinely
  raw hand-templated text throughout — exactly 33 sites, not print-and-splice.** Every line in this
  function is a `writeln!`/`format!` building TypeScript text directly, with no `bynk_ts` node
  construction anywhere in it (confirmed by reading the whole function, not sampling it — the
  same discipline that caught the residue). Several individual `any`-typed values inside it
  already carry their own P7.2-deferred reasoning (found during #1459's own pass over the same
  function for a different probe) — but no comment anywhere argues why the *surrounding
  structure* stays raw text rather than converting the same way `loadState`/`commitState`/the
  handler wrapper above it in the same function already did. This is a real, tractable,
  single-function conversion candidate: bounded, already has a working print-and-splice
  precedent immediately above it in the same function to follow, and does not depend on any
  currently-unresolved prerequisite. Named here for its own future slice, not attempted in this
  grounding pass.

**The other named clusters, formally collected — and, on closer evidence, not permanent either:**

- **`pred_condition_and_message`** (`emitter.rs:5148-5195`, matches Arc F's own 16-site citation)
  — a single canonical `PredKind`→condition/message mapping shared by both the owner-side and
  boundary-side checks (confirmed in place, `emitter.rs:5128-5136`); both real callers already
  wrap the returned condition text in a genuine `TsExpr::Unary`/`Paren` (`emit_pred_check`,
  `emit.rs:379-389`) — only the condition/message *text itself* stays opaque. Its real blocker,
  confirmed by direct read of `bynk_ts::TsBinaryOp` (`bynk-ts/src/program.rs:1107`), is two
  missing variants (`>=`/`<=`) — this enum's own doc comments record `LessThan`/`InstanceOf`/`In`
  each landing the same way, "this slice's own real gap," an ordinary algebra addition, not a
  boundary this track has ever treated as permanent elsewhere. **Not permanent — a real, small,
  unattempted conversion candidate**, the same shape as the history-driver below.
- **`inject_runtime_imports`/`missing_bindings`** (`emitter.rs:606-683`) — confirmed by direct
  read to be a **post-print text-surgery pass**: it re-scans the *already-printed* module text
  line by line to splice extra bindings into one specific `import { ... } from "<runtime>"` line.
  This cannot become node-based without first making imports themselves mutable real nodes
  collected *before* the final print — the same tension #1461 just named for the nested
  source-map problem (a "print once at the boundary" architecture colliding with a mechanism that
  needs to inspect/mutate output after some parts already exist). **Genuinely entangled with a
  real, still-open design question — not proven impossible, so not claimed permanent either.**
- **The `__eventsDispatch` carve-out** (`emit.rs:2857`, `emit_service`'s own deps-widening) is the
  same `TsType::named(deps_ty)`-wrapped opaque type #1463 ("Arc F, slice 4") already scopes to
  convert — not a separate residual to argue here, tracked there.

**Consequences.** The argued floor for `ts_writes` is **not 0** and **not fully known until the
history-driver, `pred_condition_and_message`, and `inject_runtime_imports` conversions are each
attempted** — the same honest "genuinely unknown until the slice lands" posture `ast_importers`
(floor 5) and `verbatim_sites` (floor 2+) already modelled, not a claim this pass invents new
here. What *is* now argued genuinely permanent: only `lower.rs`'s 371 (ADR 0391) and the DO-class
wrapper text at both `emit_provider` and `emit_agent` (Decision C). The history-driver's exactly
33 sites, `pred_condition_and_message`'s 16, and `inject_runtime_imports`/`missing_bindings`'s
sites are each a real, scoped, tractable future slice — named and bounded here rather than left
as an unverified assumption the way Arc F's own "not read end to end" left the first of them, or
a premature "permanent" the way an earlier draft of this same pass mislabelled the other two
(review of #1468 caught it before merge). `tests_emit.rs`'s 83 sites and `workers_entry.rs`'s 34
carry forward Arc F's own sampling (not re-verified end to end in this pass, since #1462's own
scope was `emit_agent`'s cluster specifically) with the same caveat Arc F itself already recorded.
