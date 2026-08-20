---
level: patch
changelog: "P7.0: ts_writes and ts_any gated probes added to `cargo xtask greenfield-status` (#1296) -- ts_writes reads 1641, ts_any reads 55. Phase 7's own completion probe was never measured before this slice."
---

## ADR: p7-0-ts-writes-gating-despite-churn
title: `ts_writes` and `ts_any` gate despite crossing #999 Decision D's own churn line, on purpose
summary: Both probes move on any `bynk-emit` PR touching a single write!-family or `any`-typed line -- the same volatility Decision D cites for leaving `wildcard_arms` ungated -- but stay gated because Arc C's dozens of conversion slices need a CI-checkable ratchet, not a self-reported trend

**Context.** #999 Decision D drew a bright line between gated (zero/closure) probes and reported-only
trend probes on volatility: a count that "moves on nearly any ordinary Rust PR" (`wildcard_arms`,
`test_density`) is left ungated, because hard-gating it "would make the committed table churn, and
conflict, on routine work." Review of #1297 pointed out, correctly, that `ts_writes` (1641) fits
that description better than it fits the nine pre-existing gated probes: those are booleans, or
counts pinned at a small, argued floor (`ast_importers` = 5, `emit_abi_shapes` = 1); `ts_writes` is
a count over `bynk-emit/src`, the crate this repo changes most, that moves by one on any PR adding
or removing a single `write!`/`writeln!`/`format!` line anywhere in it -- more volatile than
`wildcard_arms` (311), not less. The module doc's own "eleven are zero/closure probes" line was
also, simply, inaccurate for these two.

**Decision.** Gate both anyway. The distinction Decision D actually drew is churn-avoidance for a
probe nobody is actively driving down -- `wildcard_arms` has no slice-by-slice owner, so gating it
would tax every unrelated PR for a number nobody is tracking. `ts_writes`/`ts_any` are the opposite:
they are `the-typescript-tree.md`'s own two completion ratchets, Arc C is dozens of slices each
claiming "I converted this file's emission to the tree" or "I removed this `Any`," and only a
diffed, committed number turns that claim into something CI checks rather than something a reviewer
has to take on trust. `ast_importers` already made and lived with the identical trade across phase
6's 59 slices -- a large count, converging slowly, gated throughout, needing a table update on
close to every slice's own PR. `ts_writes`/`ts_any` are that same shape, not `wildcard_arms`'s.

**Consequences.** Every Arc C slice (and any ordinary, phase-7-unrelated `bynk-emit` PR that happens
to touch a `write!`-family line or an `any` type) needs a `cargo xtask greenfield-status --apply` +
table commit -- a real, accepted cost, not an oversight. Two things this doesn't currently provide,
named so a later track slice doesn't rediscover them as surprises: the gate is equality, not a
ratchet, so it fails identically whether either count rises or falls -- it does not itself enforce
monotone decrease, only "matches what was last committed." And unlike `ast_importers`'s single
five-entry exclusion list, `ts_writes`/`ts_any` share a six-file list plus a path-construction-line
idiom; a genuinely new non-TS-producing file added to `bynk-emit` later would silently inflate the
count until someone notices and adds it to `TS_WRITES_EXCLUDED_FILES`, the same class of gap
`AST_IMPORTER_EXCEPTIONS` already lives with.

## ADR: p7-0-ts-writes-ts-any-corrections
title: Two corrections this slice's own grounding found, not carried forward from the accepted proposal
summary: `project/tests_emit.rs` is not excludable test-assertion noise, and R7.1's real remaining surface (55, not the settling review's ~24) is materially larger than earlier estimated -- neither changes what the track needs to do, only how big the job honestly is

**Context.** `design/tracks/the-typescript-tree.md` §5/§6 name `ts_writes` and `ts_any` as this
track's first two completion probes. #1296 (this slice's own increment proposal), following the
track doc's own baseline table, assumed `ts_writes` should exclude `project/tests_emit.rs` as
excludable "test-assertion strings." Grounding that claim against the tree found it false: all 128
of that file's `write!`-family sites sit outside its one `#[cfg(test)] mod tests { .. }` block
(starting line 3740) -- they are `process_tests`/`process_integration_tests`, real production
TypeScript-emission code, per `semantics-in-the-checker.md`'s own settling review finding for a
*different* probe (`emit_diagnostics`) on this exact file ("tests_emit.rs was mischaracterised as
fixture noise in the draft ... real production code and the largest remaining category"). Separately,
`the-typescript-tree.md` §3.3 (Q3) argued full `TsType::Any` elimination is achievable from a
site-by-site classification that found "~24 real emission sites." A full programmatic scan --
verified by hand, line by line, against the live tree before trusting it, and again after review of
#1297 widened the predicate to catch generic-position `any` -- reads **55**: more than double. The
gap is concentrated in `project/tests_emit.rs` (15 sites, missed by the earlier manual survey's
narrower focus on `as any` casts), several additional bare `: any` sites in
`emitter/workers.rs`/`emitter/serialisation.rs`, and three `Record<string, any[]>` sites in
`emitter/lower.rs` that contain neither `as any` nor `: any` at all.

**Decision.** Ship both probes counting what the tree actually contains, not what the earlier
survey assumed: `ts_writes` does not exclude `project/tests_emit.rs`; `ts_any` matches `as any`,
bare `: any`, and generic-position `any` (`<any`, `any>`, `any[]`).

**Consequences.** Neither correction changes Q3's own conclusion -- full elimination is still
achievable, no site needs an IR extension, since every additional site found is the same
already-argued shape (`unknown`, a structural type, or a generated index-signature type), not a new
residual class. But `the-typescript-tree.md`'s own §1/§3.3 baseline numbers ("~1,540" TypeScript-
producing sites, "~24" `Any` sites) are now stale against the live, programmatic probe (1641, 55)
and need a follow-up correction -- named here, not fixed in this dev-tooling-only slice, the same
"the evidence ages" discipline this trajectory's own §9 names for every prior track.
