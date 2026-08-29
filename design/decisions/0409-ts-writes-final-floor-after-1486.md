# 0409 — ts_writes's argued floor is 809, matching the live probe exactly, after all twelve #1462/Arc-F-named implementation issues landed

- **Status:** Accepted (v0.289.46)

**Context.** #1462 (landed as #1468/#1470) argued `ts_writes`'s residual after Arc F's own three
slices, explicitly naming the honest next step as a separate argued-floor writeup once the
remaining named clusters land — not a claim any further slice reaches a genuine 0. Twelve
implementation issues followed (#1475–#1486, all "Part of #1293"), the last of which (#1486, the
`verbatim_sites` capstone) landed 2026-08-29. §6's own slice table, though, had no entries past
#1480 (`ts_writes` 829 → 827) — #1481–#1486 all landed with no corresponding narrative row, and
the live probe (809) no longer matched the doc's last recorded figure. §5's `ts_writes` bullet
still read as an open-ended narrative of buckets, never converted into one settled number the way
`ts_any`'s (#1459/#1460, floor 26) and `verbatim_sites`'s (ADR 0399/#1486, floor 2) already were.

**Decision.** Reproduced `ts_writes_violations`'s own predicate directly (file exclusions,
`test_mod_ranges`, `is_path_construction_line`) rather than trusting a rough `grep`, confirming
809 live sites exactly matching the committed `design/greenfield-status.md`. Attributed every site
to its enclosing top-level function via real brace-depth tracking (a nested helper fn, e.g.
`project.rs`'s local `fn visit`, folds into its enclosing top-level function's own bucket, not a
separate one) and classified each function-bucket by direct reading of the current source, not by
trusting the track doc's own possibly-stale narrative:

- **Bucket A — permanent, individually argued (614 sites).** `emitter/lower.rs`, every function
  (371, ADR 0391's per-splice-point opaque representation). The four Decision-C hand-written
  class-wrapper functions in `emitter/emit.rs`/`project/tests_emit.rs` — `emit_provider` (11,
  #1480), `emit_service` (15, #1481), `emit_agent`'s DO-class wrapper plus its own
  `__bynkDriveHistory_*` history-driver (93 total; the history-driver itself, ~32 sites, is
  #1386's own standing test-support-only exclusion, reconfirmed by #1462/#1495), `emit_stub_class`
  (7, #1483) — 126 together. `emit_contract_guarded_body` (`emitter/emit.rs:1046`, 8 sites) — a
  fresh argument, not previously pulled into this track's own tally: two sites build predicate-
  failure message text, the same permanent shape #1471 already argued for
  `pred_condition_and_message`'s own `msg` half; the rest build into `out: &mut String` because
  `emit_block_as_function_body_with_return` calls `cx.record_span(out.len(), ...)` against it, so
  wrapping this in a local buffer would reproduce the exact stale-offset bug #1352 already fixed
  one level up in `emit_free_fn` — the same `lower.rs`-splice source-map entanglement ADR 0391
  already argues, extended here to a direct caller. `workers_entry.rs` (34) and `tests_emit.rs`'s
  non-`emit_stub_class` remainder (72) — both already read end to end by #1475/PR #1487, not
  re-derived here. `emit_composition_root`'s `__eventsDispatch` closure body (3, #1463's own
  finding that `TsType::Fn`'s anonymous params can't name its `events` argument).
- **Bucket B — a newly-named permanent structural category (190 sites).** Identifier/type-name/
  message-text `String` construction feeding an already-real `bynk_ts` node's leaf field —
  confirmed by direct sampling across every remaining file (`serialisation.rs`'s codec helpers,
  `emitter.rs`'s naming helpers, `emit.rs`'s `ws_*_do_method_name`-class helpers and
  `emit_message_entry_renderer`, `project.rs`'s remaining `emit_composition_root` sites,
  `workers.rs`, `events_fanout.rs`): a `format!` building an identifier (`format!("__b_{name}")` →
  `ident(...)`), a type name (`TsType::named(format!(...))`), an import path/alias, or message
  text. `bynk-ts`'s own algebra already represents these leaf fields as bare `String`, not further
  AST-decomposed — the same representational choice P7.9 made explicit for `TsType::Named`'s
  pre-rendered text. Genuinely structural, not residue.
- **Bucket C — real, small, tractable, named but not scheduled (5 sites).** A bare
  `writeln!(out).unwrap()` blank-line push in functions not yet promoted from `out: &mut String`
  to `Vec<TsStmt>` at their own top-level signature: `write_header_single` (`emitter.rs:2957` and
  `:3019`, 2 sites), `emit_ws_dispatch_handlers` (`emitter/emit.rs:6949` and `:7005`, 2 sites —
  found by review of #1504, corrected from an initial undercount of 1: this function's own two
  branches, `host.message`/`host.close`, each end with the identical blank-line push), and
  `emit_ws_do_method` (`emitter/emit.rs:6518`, 1 site). Mechanically trivial (swap for a
  `TsStmt::blank(None)` push once each function returns a `Vec`), the same "named, not scheduled"
  treatment #1487 already gave a near-identical minor fold-in candidate — not blocking retirement.

614 (A) + 190 (B) + 5 (C) accounts for 809 exactly. No unclassified site remains.

Also added six §6 landing entries (#1481 → #1486, PRs #1494–#1500), reconciling the doc's own
slice table with work that had already merged: `ts_writes` moved 827 → 826 (#1481) → 822 (#1482)
→ 822 unchanged (#1483) → 817 (#1484) → 817 unchanged (#1485) → 808 (#1486's initial landing) →
809 (a post-merge review fix on #1486 adding one `import_stmt` construction site) — each
transition verified directly against the corresponding PR's own body and `git show` of its merge
commit, not re-derived from memory.

**Consequences.** `ts_writes`'s own retirement bar (§12) is met: the argued floor (809) matches
the live probe exactly, the same shape `ts_any` (26) and `verbatim_sites` (2) already closed at.
`verbatim_origins` is the one probe left unargued at retirement's own three-probe bar — tracked
separately as #1502, not folded in here since it's a materially different, much smaller
investigation (one surviving `VerbatimOrigin` variant, already traced to the same two permanent
`verbatim_sites` sites). The retirement PR itself (removing this track doc, appending its closing
summary to `../archive/retired-tracks.md`, closing #1293) is tracked as #1503, blocked on both
#1501 (this) and #1502.
