# 0206 — The version and the ADR number are allocated by a per-merge stamp on `main`, not chosen in the feature PR

- **Status:** Accepted (v0.186)
- **Provenance:** The increment-allocation track (spine [#685](https://github.com/accuser/bynk/issues/685)), built across three slices — the pending-increment format + validator [#688](https://github.com/accuser/bynk/pull/688), the `cargo xtask stamp` command [#690](https://github.com/accuser/bynk/pull/690), and this per-merge workflow, proposed and `accepted` as [#691](https://github.com/accuser/bynk/issues/691). This record is the one the first two slices deferred to here, so it describes the mechanism once it is operative rather than a command yet to be wired.
- **Relates:** [[0167]] (feature tracks run GitHub-native — the process this track followed), and the release backbone in `design/README.md` §"Versioning & release" (the single repo version, `scripts/bump-version.sh`, the tag→publish flow the stamp feeds).

## Context

The repo carries a single version, and every increment records at most a few ADRs, numbered sequentially in a hand-curated index (`design/decisions/README.md`, held complete by the `decisions_index` drift guard). Both the **version** and the **ADR number** are strictly-increasing counters, and until now both were transcribed into shared files *while a PR was authored*: `scripts/bump-version.sh` wrote a concrete `X.Y.Z` into ~15 files, and the author added the next `NNNN` ADR file plus its index row.

A value chosen while a PR is open races every other open PR. The merge order that alone can assign these counters correctly is not known at authoring time, so two increments developed in parallel conflicted on every version-bearing file and on the index table — and the loser did not merely conflict, it silently carried a number another increment had already taken (an ADR is immutable once accepted, so the fix was a rename rippling through cross-references). The daily-increment cadence made this the common case, not the exception.

The existing model already deferred one release decision correctly — a release *tag* is cut when a version is shipped, not on every increment. The gap was one step earlier: the *number* was still chosen by the author.

## Decision

**The version and the ADR number are allocated by an automation that runs on `main`, in merge order — never chosen in the feature PR.** A feature PR declares only *intent*; a per-merge stamp assigns the numbers.

1. **Intent, not numbers, in the PR.** A feature PR adds one `design/pending/<slug>.md` — a bump level (`minor`/`patch`), a one-line changelog blurb without a version, and zero or more `## ADR: <slug>` blocks carrying the ADR title and prose. It touches none of the version-bearing files, no ADR file, and no index row, so two parallel PRs share no line to conflict on. A CI validator (`cargo xtask check-pending`) rejects a malformed pending file in the PR that adds it.

2. **The stamp assigns, on `main`.** `cargo xtask stamp` reads the pending files, assigns each its next version (in filename order — a deterministic proxy for merge order) and the next ADR number(s), and `--apply` runs `bump-version.sh`, prepends the changelog row(s), writes each `NNNN-<slug>.md` and its index row, and **deletes** the consumed pending file(s). Deleting what it consumes is what makes a re-run a no-op — which is why version assignment and ADR materialisation are one atomic pass, not separable steps (a pending file holds both, so it can be deleted only once both are done).

3. **Per-merge, not batched.** The stamp runs on every merge to `main`, so each increment keeps its own version and ADR, assigned a beat later. A batched "version-packages" PR was rejected: the ADR number has no release concept and must be assigned at *every* merge that records a decision, so per-merge automation is needed regardless; batching would also break the daily-increment cadence.

4. **`GITHUB_TOKEN`, directly to `main`, self-validated.** `main` is unprotected, so the default `GITHUB_TOKEN` pushes the stamp commit directly. A `GITHUB_TOKEN` push triggers no further workflow run: this is the loop prevention (the stamp commit touches `design/pending/**` but does not re-fire the workflow), and it also means the stamp commit runs no CI — so the workflow validates the stamped tree itself (`doc_version`, `decisions_index`, a `--locked` build, the xtask suite, the llms-full `--check`) and pushes only if green. A failure pushes nothing and leaves `main` untouched; the pending files remain, so the run is retryable.

## Consequences

- **Parallel increments stop colliding on the two counters.** The conflict class is removed, not mitigated — parallel feature PRs no longer edit any shared version or index line.
- **The version and ADR number leave the feature PR's diff.** They land in the follow-up stamp commit on `main`, referencing the merge. Review of the ADR *prose* still happens in the feature PR, exactly as before; only the number moves.
- **A dependency on `main` being unprotected is now load-bearing, and named.** Direct `GITHUB_TOKEN` push, and its no-re-trigger loop prevention, both rest on it. If `main` is ever branch-protected requiring PRs or reviews, the stamp must move to a GitHub App with a push-bypass allowance, or to opening a stamp PR a human merges — a change to this decision, recorded when made rather than discovered.
- **The tag→publish flow is unchanged.** The stamp leaves the tree in the state the release `verify` job demands (all version sites agree); a release tag is still cut by hand to ship.
- **This increment is the last one stamped by hand.** Its own version and ADR number were taken the existing way, because the workflow it introduces did not yet exist at author time; from the next increment, the stamp does it.
