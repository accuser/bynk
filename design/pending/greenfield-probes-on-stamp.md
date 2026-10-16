---
level: patch
changelog: An increment acknowledges a gated greenfield-status probe it moves in its pending file's `probes` key, and the merge-time stamp rewrites `design/greenfield-status.md`, so parallel PRs no longer conflict on the table
---

## ADR: greenfield-probes-acknowledged-at-stamp
title: Gated greenfield probes are acknowledged in the pending file and rewritten by the stamp
summary: A PR names the gated probes it moves (`probes:`); the stamp rewrites the table's gated rows on `main` in merge order

**Context.** `design/greenfield-status.md` is a committed, generated table whose
sixteen gated rows `greenfield_status_table_is_current` diffs against a fresh
probe run. Gating them was deliberate (ADRs 0389/0390): a gated probe must not
move without someone acknowledging it in review. But the acknowledgement was
made by editing the shared table, so every probe-moving PR edited the same
handful of adjacent rows. Once the stamp (ADR 0206) took versions and ADR
numbers out of feature PRs, this table became the most-edited file
outside the stamp's own outputs (66 of 190 non-stamp commits to `main` in the
thirty days to 16 October 2026). Each merge left every other open probe-moving
PR either conflicting or failing the gate after a rebase.

**Decision.** Move the acknowledgement into the increment's own, uniquely named
pending file, exactly as ADR 0206 moved version and ADR numbers. A pending file
may carry `probes: a, b`, naming gated probes (validated against
`GATED_PROBES`, which the gate test pins to `run_gated()`). The gate fails only
on disagreements no pending file in the tree acknowledges, and prints the
acknowledged moves so review still sees them. `cargo xtask stamp --apply`
rewrites the table's gated rows from a live run (trend rows untouched, no
clippy pass) before deleting the pending files. Because a pending file survives
until its stamp lands, a just-merged increment's acknowledgement still covers
its move on other PRs' merge refs during the stamp window. A PR with no
increment can still regenerate and commit the table as before.

**Consequences.** Probe-moving PRs no longer touch the shared table, so they
stop conflicting on it; the acknowledgement remains explicit and reviewed. An
acknowledgement is by name, not value: two concurrent increments that move the
same probe each name it, and the stamp writes the combined live reading. A
pending file that names a probe it doesn't actually move is harmless (the
stamp rewrites the row to its live value) but should be caught in review. The
table on `main` can lag a merge by one stamp run, as the changelog and version
already do.
