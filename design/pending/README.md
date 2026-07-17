# Pending increments

Part of the **increment-allocation** track
([`../tracks/increment-allocation.md`](../tracks/increment-allocation.md), spine
[#685](https://github.com/accuser/bynk/issues/685)).

A feature PR does **not** choose its version or its ADR number. Both are
strictly-increasing counters, and a value picked while a PR is open races every
other open PR — the merge order that alone can assign them correctly is not yet
known, so two parallel increments conflict and the loser silently ships a
number another increment already took.

Instead, a feature PR declares its **intent** in one file here, and a merge-time
stamp (a later slice) assigns the numbers in merge order. Because each pending
file is uniquely named per branch and touches none of the shared version/ADR
files, two parallel PRs have no shared line to conflict on.

## The file

Add one `design/pending/<slug>.md`, where `<slug>` is a kebab-case name unique to
your branch (e.g. the branch name):

```markdown
---
level: minor
changelog: A test case drives an http handler at the unit tier by address
---

## ADR: unit-tier-service-address
title: A test case drives an http handler at the unit tier by address
summary: How a case addresses a handler and names its principal

**Context.** …

**Decision.** …

**Consequences.** …
```

### Frontmatter (required)

- **`level`** — `minor` (a language increment) or `patch` (non-language). The
  stamp turns this into the next `X.Y.Z`.
- **`changelog`** — the changelog row, on one line, **without** a version number
  (the stamp prepends it).

No other keys are permitted — a typo like `levl:` is rejected rather than
silently ignored.

### ADR blocks (zero or more)

Each `## ADR: <slug>` heading opens a block, followed by a short header of key
lines and then the ADR body, which runs until the next `## ADR:` heading or end
of file. An increment that records no decision has no blocks; one that records
several has several. The stamp assigns the number `NNNN` at merge and writes
`design/decisions/NNNN-<slug>.md` — a `# NNNN — <title>` heading and a status
line, then the body verbatim — plus the `decisions/README.md` index row. So the
prose is reviewed **here**, in the feature PR, exactly as an ADR is today.

- `<slug>` is kebab-case and unique within the file; it becomes the ADR filename.
- **`title`** (required) — the one-line title for the ADR heading and the index row.
- **`summary`** (optional) — the one-line distillation for the index row;
  defaults to the title.
- **`status`** (optional) — defaults to `Accepted`.
- A block's body must not be empty.

## Validation

`cargo xtask check-pending` validates every file in this directory (this
`README.md` is skipped). It runs in CI so a malformed file fails the PR that adds
it — via the main test job on a code-touching PR, and via the `drift` job on a
`design/pending/`-only PR. The validator lives in the unpublished `xtask` crate
(`xtask/src/lib.rs`).

## The stamp

`cargo xtask stamp` reads the pending files, assigns each its next version (in
filename order) and the next ADR number(s), and — with `--apply` — runs
`scripts/bump-version.sh`, prepends the changelog row(s), writes the ADR file(s)
and index row(s), and **deletes** the consumed pending file(s). Without
`--apply` it prints the plan and changes nothing. Deleting what it consumes is
what makes a re-run a no-op.

## Lifecycle

The stamp command exists, but the **per-merge workflow** that runs it
automatically is a follow-on slice. Until it lands, a maintainer runs
`cargo xtask stamp --apply` when the increment merges (or removes the pending
file by hand). Once the workflow lands, this is automatic on merge to `main`.
