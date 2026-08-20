# 0390 — Two corrections this slice's own grounding found, not carried forward from the accepted proposal

- **Status:** Accepted (v0.249.38)

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
