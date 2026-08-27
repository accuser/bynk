---
level: patch
changelog: Resolves #1475 — reads `tests_emit.rs`'s 83 and `workers_entry.rs`'s 34 `ts_writes` sites end to end, closing Arc F's own long-standing sampling caveat. `workers_entry.rs` confirms Arc F's "all already-argued" claim exactly (no new gap; the unelaborated "small fold-in candidate" is most plausibly a repeated `format!("__r_{pname}")` re-derivation across 4-5 call sites, named but not scheduled). `tests_emit.rs`'s 83 breaks down as 48 cheap glue, 20 permanent (Decision A + Decision C, 10 each), 12 already inside #1472's own wrapper-function finding (confirmed, not just cross-referenced — the wrapper functions sit inside this 83, not on top of it), 2 further new small gaps in `destructure_vals` (folded into #1484), and 1 probe-only artifact (`:592`, already documented as an accepted gap by the probe's own code). Also lands two stale corrections: the track doc's own description of `pred_condition_and_message`/`inject_runtime_imports` (both since resolved, by #1471 and #1472 respectively) had drifted. No code change.
---

## ADR: ts-writes-sampled-files-end-to-end
title: tests_emit.rs's 83 and workers_entry.rs's 34 ts_writes sites read end to end, closing Arc F's sampling caveat
summary: workers_entry.rs confirms "all already-argued"; tests_emit.rs's 83 breaks into 48 cheap, 20 permanent, 12 already covered by #1472's wrapper-function finding, 2 new small gaps folded into that same slice, and 1 probe-only artifact — no large unconverted residue in either file

**Context.** Arc F's own opening pass (#1449) counted `ts_writes` file by file but only sampled
`project/tests_emit.rs` (83 sites) and `emitter/workers_entry.rs` (34 sites), never reading either
end to end the way #1462 later did for `emit_agent`'s 99-site cluster. #1472's own callee-cascade
read of `tests_emit.rs` (for `verbatim_sites`, a different probe) already found genuinely
unconverted content there — the test-function wrappers, the JWT-signer block — without settling
whether those sites fall inside the existing 83-count or sit outside it. #1475 is that end-to-end
read, replicating the probe's own exact predicate (`ts_writes_violations`: every non-comment,
non-`#[cfg(test)]`-range, non-path-construction-line line containing `write!`/`writeln!`/`format!`)
via a temporary scratch test, not approximated.

**Decision.**

**`emitter/workers_entry.rs`'s 34 sites confirm Arc F's own "all already-argued" claim exactly —
no new gap.** Every site is either cheap identifier/message-text glue feeding one of this file's
own real node-builder helpers (`ident`/`const_`/`str_lit`/`method_call`/…, all already real
`bynk_ts` construction), or a raw-text argument to the already-argued cross-file `deserialise_call`/
`brand_assertion` pair (the exact precedent `workers.rs` already established, #1321/#1323). The one
"small fold-in candidate" Arc F's own citation named but never elaborated could not be confirmed
against its original intent (never written down) — most plausibly the repeated
`format!("__r_{pname}")` re-derivation, independently rebuilt at 4-5 call sites across
`emit_http_route_dispatch`/`emit_path_param_construction` rather than bound once to a local
variable. A real, tiny `ts_writes`-reducing tidy (binding once genuinely removes `format!` call
sites, not just changes their spelling) — named, not scheduled; no issue filed, since it is purely
optional cleanup with nothing blocking on it.

**`project/tests_emit.rs`'s 83 sites are not uniformly "no target" the way Arc F's own citation
implied.** Read in full:

- **48 cheap** — the same identifier/message-text/import-specifier glue shape as
  `workers_entry.rs`'s 34, feeding real nodes throughout.
- **10 permanent, Decision A (#1407)** — `emit_system_http_support`'s own request-init
  options-object shape (`typed_options`/`raw_options`/`noauth_options`/`rawnoauth_options` and
  their own sub-fragments, `secret_read`/`auth_header`/the URL template), already argued in place
  as the same "odd, one-off shape stays text" call Decision B (#1327)/Decision C (#1359) already
  made.
- **10 permanent, Decision C** — `emit_stub_class`'s own class wrapper (`:2296-2493`, ten sites
  across the header/field/sequence-counter/property-value-capture construction), a second,
  independent instance of the same `emit_provider`/`emit_agent` pattern, already argued in place
  by its own comment (`:2372-2385`) — #1472's own finding, confirmed again here by the full-file
  read rather than assumed.
- **12 already inside #1472's own "test-function wrapper" finding** — confirmed, not just
  cross-referenced: `emit_integration_module`'s own inline case wrapper plus the four named wrapper
  functions' `async function { try { ... } catch (e) {...} }` headers/bodies are 12 of this file's
  83 `ts_writes` sites, not a separate residual sitting on top of the count. Directly answers
  #1475's own "reconcile explicitly" ask.
- **2 further new, small, previously-unnamed gaps** — `destructure_vals` (`:3706-3735`) returns raw
  text for both its branches despite already building real nodes for the value side internally.
  The single-name branch converts today with `TsStmt::const_stmt`; the multi-name branch needs a
  new `TsBindingName::ArrayPattern(Vec<String>)` (`bynk-ts` has no array-destructuring binding shape
  yet). Both call sites are already inside #1484's own scope (`emit_test_property_function`/
  `emit_contract_attack_function`) — folded into that issue rather than filed separately.
- **1 probe-only artifact, not a real gap** — `:592`'s `target_name: format!("integration ·
  {suite}")` is the probe's own already-documented accepted over-count (a human-readable
  struct-field label, not TypeScript text; `ts_writes_violations`'s own doc comment already names
  this exact site).

**#1472's own JWT-signer finding sits inside `emit_system_http_support`'s own already-counted
surface**, not separately flagged by this probe's line-level scan — its own `format!`/string-literal
construction reads no differently from the file's other already-argued Decision-A text to a
line-level scanner. Neither of #1472's two new candidates moves `ts_writes`'s live count; both were
already inside the existing 83, just unclassified until this pass.

**Two stale track-doc corrections landed alongside this reading**: the "Arc F closing" narrative
still described `pred_condition_and_message` as blocked on two missing `TsBinaryOp` variants and
`inject_runtime_imports` as entangled with the nested-source-map question — both since resolved
(#1471 landed the former for real; #1472 found the latter was never actually entangled, just
unimplemented). Corrected in place rather than left to drift further.

**Consequences.** `ts_writes`'s two long-sampled files carry no large unconverted residue — the
floor for `tests_emit.rs`/`workers_entry.rs` combined is now fully known: 20 permanent sites, the
rest either already scheduled (the wrapper functions and `destructure_vals`, #1484) or optional
cleanup (the `workers_entry.rs` fold-in tidy, unscheduled). No code change in this pass.
