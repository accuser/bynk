---
level: patch
changelog: Arc F design pass — decomposes `ts_writes`'s remaining non-`serialisation.rs` surface into three concretely-scoped conversion slices, one open investigation, and a named already-argued residual
---

## ADR: arc-f-ts-writes-remaining-surface-decomposition

title: `ts_writes`'s remaining non-`serialisation.rs` surface splits into three concretely-scoped conversion slices (Arc E's own deferred steps 6/7, `project.rs`'s duplicate provider-instantiation function, `emit.rs`'s already-flagged `emit_context_deps_interface`), one open investigation (`emit.rs`'s deps-object-type builders), and a large already-argued residual

summary: With Arc E's own 7 slices landed, a file-by-file re-count of `ts_writes`'s 901 sites finds `lower.rs` (371, ADR 0391) and `serialisation.rs` (71, Arc E's own closed residual) still excluded, and the remaining 459 sites in `emit.rs`/`emitter.rs`/`project.rs`/`tests_emit.rs`/`workers_entry.rs`/`workers.rs`/`events_fanout.rs` are overwhelmingly already-argued residual — three real, bounded targets remain

**Context.** Arc E (#1422, ADR 0398) converted `serialisation.rs`'s own four clusters across 7
slices (#1435–#1447, all merged); its own step 6/7 — the caller-side `emitter.rs` wrapper trio and
the legacy `emitter::emit`/`Compiled.ts` boundary — was named but never filed as its own issue. This
pass re-counts `ts_writes` (901, `design/greenfield-status.md` on current `main`) directly against
the tree, file by file: `emitter/lower.rs` (371, ADR 0391's permanent exclusion, not this pass's
concern), `emitter/emit.rs` (235), `project/tests_emit.rs` (83, sampled at every large cluster —
`emit_system_http_support`/`emit_test_module`/`binding_gen`/`emit_integration_module`/
`emit_stub_class` — and found fully tree-native internally or small identifier/text construction; no
target), `emitter/serialisation.rs` (71, Arc E's own closed residual — `RecordInst`/`SumInst`'s
boundary-print blank lines and function-name `format!`s, not re-opened here), `emitter.rs` (64),
`emitter/workers_entry.rs` (34, sampled in full — all already-argued, one small fold-in candidate),
`project.rs` (29), `emitter/workers.rs` (12, sampled in full — all already-argued, Arc C slices
#1317/#1321), `emitter/events_fanout.rs` (2, both already-argued — an in-place doc comment and a
non-identifier event-type object key). The in-scope total (everything but `lower.rs` and
`serialisation.rs`) is 459.

**Three sites, independently re-verified against current `main`, ground the concrete targets:**

- `emitter.rs`'s `emit_json_codec_helpers`/`emit_boundary_helpers`/`emit_consumed_context_helpers`
  (lines 1075/1114/1376) are still `out: &mut String` — Arc E's own step 6/7, never filed.
- `project.rs`'s `plan_agent_given_deps` (2637–2749, 5 sites) and `instantiate_provider_expr`
  (2787–2891, 6 sites — this pass's own line-range attribution corrects the issue's initial swap of
  which range belongs to which function) are a duplicate pair: `instantiate_provider_expr` already
  has a tree-native twin, `instantiate_provider_ts_expr` (2920, `-> bynk_ts::TsExpr`, built by
  #1321/#1327 for `emit_worker_compose`/`emit_composition_root`). `native_platforms_of_context`
  (2572–2622) calls the `String`-returning original twice purely for its referenced-unit side effect
  (`let _ = instantiate_provider_expr(...)`, result discarded) — a separate oddity, not necessarily
  fixed in the same slice.
- `emit.rs`'s `emit_context_deps_interface` (3287–3342, 9 sites, re-confirmed) is already flagged
  in-place as unconverted by its own caller's doc comment (`emit_make_surface`, 3376–3379: "a
  separate, unconverted, confirmed-unaffected `String`-returning sibling this function calls once,
  not touched by this slice").

**A real gap, named rather than guessed at:** `emit.rs`'s `build_deps_object_ty_with_surface`/
`cap_ref_ty`/`surface_ty`/`workers_env_ty` (2969/3119/3175/3228, 12 sites) build the `deps: {...}`
parameter type as semicolon-joined text for three real callers (`emit_service`/`emit_agent`/
`emit_ws_do_method`). No in-place comment argues this stays opaque — but `emit_ws_do_method`'s own
string-surgery on the returned type text (6161–6171, re-confirmed: raw `identity: {field}` splicing
into the already-built type string via `trim_end_matches('}')`) is a real complication not traced to
a conversion plan here.

**Decision.** Schedule three concretely-scoped slices, in this order:

1. **Arc E's own step 6/7** — `emit_boundary_helpers`/`emit_json_codec_helpers`/
   `emit_consumed_context_helpers` thread real `Vec<TsDecl>`/`Vec<TsStmt>` into `emit_project`'s
   tree instead of an `&mut String`, closing `emit_one`'s 4-site print-then-append loop and
   `emit_generic_helpers_qualified`'s 6-site equivalent (both already print real nodes at their own
   boundary today) plus the wrapper trio's own 5 sites (1227/1228/1256/1261/1513) — 15 sites total.
   **Load-bearing, not optional**: skipping this leaves Arc E's real `TsDecl` nodes printed into a
   `String` that never itself becomes a tree node, reading as `ts_writes` progress while
   `verbatim_sites` stays exactly where it is. The legacy `emitter::emit`/`Compiled.ts` boundary
   (`emitter.rs:188`, `lib.rs:81`) needs its `-> String` signature confirmed compatible with
   `bynk-driver`/`bynk check`'s own real callers before shipping — unresolved here, checked at
   implementation time.
2. **`project.rs`'s duplicate provider-instantiation function** — repoint `plan_agent_given_deps`
   (the DO-side agent-deps-reconstruction path, #527) at the existing tree-native twin,
   `instantiate_provider_ts_expr`, printing-and-splicing at its own still-textual caller
   (`emit_agent`, `emit.rs:4755`) — the same "print a real fragment, splice into a still-textual
   caller" pattern #1395 already used. No new `bynk_ts` algebra needed.
3. **`emit.rs`'s `emit_context_deps_interface`** — its one real caller hand-`writeln!`s a plain
   `interface {name}Deps { readonly f: T; ... }` shape `TsDecl::Interface`/
   `TsTypeMember::readonly_prop` already renders elsewhere in this file (`emit_agent`'s own state
   interface, 4437). Small enough to possibly bundle with a slice for the item 4 investigation below,
   if that investigation confirms it's equally tractable.

Slices 1–3 do not interact with each other (different files, no shared dependency edge) — safe to
schedule and implement in any order.

**A fourth item stays an open investigation, not a slice:** `build_deps_object_ty_with_surface`'s
cluster (12 sites) needs `emit_ws_do_method`'s string-surgery (6161–6171) traced in full before
committing to a slice count or conversion shape — the same "don't guess the shape, read it"
discipline #1319 named for `serialisation.rs` before Arc E could decompose it.

**Confirmed not targets, named so a future pass doesn't rediscover them as gaps:** `brand_assertion`
(`workers_entry.rs:2482–2491`, 6 sites, already named in-place by `workers.rs:2053–2057`'s own
`claim_predicate_to_js` precedent — foldable into slice 1 or 2's own PR, not its own slice);
`pred_condition_and_message` (`emitter.rs:5109–5154`, 16 sites, already decided opaque by Arc E
slice 4's own review, #1441/#1442); `inject_runtime_imports`/`missing_bindings` (`emitter.rs`, ADR
0142's post-print text-surgery mechanism, architecturally entangled with the same source-map-rebuild
complication ADR 0399's own floor-correction pass already left open); `emitter/workers.rs`'s 12
sites and `emitter/events_fanout.rs`'s 2 (Arc C slices #1317/#1321, fully converted); `tests_emit.rs`'s
83 sites (Arc C slices #1395–#1409, fully tree-native internally).

**A probe-accuracy finding, independent of any conversion slice:** `project.rs:2098`'s
`sibling_path` builds a filesystem path via `output_path.with_file_name(format!("{name}.{suffix}"))`
— semantically the same `PathBuf`-construction idiom `xtask`'s `is_path_construction_line`
(`xtask/src/greenfield_status.rs:1629`) already excludes (the `PathBuf::from(format!` and
`.join(format!` forms), but spelled with `.with_file_name(`, which that substring match does not
catch — confirmed by direct read. A real, small false positive in the probe's own counting rule,
worth a one-line `xtask` fix independent of any of slices 1–3 (drops the in-scope total by 1, to
458).

**Consequences.** Slices 1–3 close 15 + 11 + 9 = 35 of the 459 in-scope sites plus whatever the
item-4 investigation resolves to; a large, real, already-argued residual remains after all of them
land — the DO-class Decision-C hand-written wrapper text, `pred_condition_and_message`,
`claim_predicate_to_js`, `inject_runtime_imports`, the `__eventsDispatch` carve-out, and
`emit_agent`'s own 101-site cluster (this pass's own sampling found it consistently
tree-native-with-print-and-splice at roughly a dozen sampled points, not read end to end — supports
confidence but is not a guarantee of no hidden unconverted chunk). This does **not** retire
`ts_writes` to 0 — the honest next step after slices 1–3 (and 4, if it proves tractable) land is a
separate argued-floor writeup for `ts_writes`'s own final residual, the same shape ADR 0399 already
produced for `ts_any`/`verbatim_sites`, not a claim that a fourth or fifth slice reaches a genuine 0.
