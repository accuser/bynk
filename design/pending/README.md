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

**Status.** Accepted

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

Each `## ADR: <slug>` heading opens a block; its prose runs until the next
`## ADR:` heading or end of file. An increment that records no decision has no
blocks; one that records several has several. The stamp moves each block
verbatim into `design/decisions/NNNN-<slug>.md`, assigning `NNNN` at merge — so
the prose is reviewed **here**, in the feature PR, exactly as an ADR is today.

- `<slug>` is kebab-case and unique within the file; it becomes the ADR filename.
- A block's body must not be empty.

## Validation

`cargo xtask check-pending` validates every file in this directory (this
`README.md` is skipped). It runs in CI so a malformed file fails the PR that adds
it — via the main test job on a code-touching PR, and via the `drift` job on a
`design/pending/`-only PR. The validator lives in the unpublished `xtask` crate
(`xtask/src/lib.rs`).

## Lifecycle

Until the merge-time stamp lands (a later slice), a pending file is removed by
hand in the same PR that ships the increment. Once the stamp exists, it consumes
and deletes the file automatically.
