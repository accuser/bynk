# 0389 — `ts_writes` and `ts_any` gate despite crossing #999 Decision D's own churn line, on purpose

- **Status:** Accepted (v0.249.38)

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
