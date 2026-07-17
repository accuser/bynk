# Increment allocation — the version and the ADR number are stamped at merge, not chosen at authoring

- **Status:** **Adopted — core complete. Slices 0–2 shipped (v0.186, ADR 0206); only the
  deferrable Slice 3 remains.** The contention this track exists to remove is gone: a feature
  PR lands a `design/pending/` file with no numbers, and the per-merge stamp workflow assigns
  the version + ADR number on `main`. The track is a candidate for retirement (Slice 3 —
  surface-shrink — can be a standalone follow-on rather than a blocker). The spine is
  [#685](https://github.com/accuser/bynk/issues/685)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)); direction was settled
  by the merge of the settling PR [#684](https://github.com/accuser/bynk/pull/684). Adoption
  is **not** build authorisation — a slice is approved to build only when its own proposal is
  `accepted`. **Slice 0** ([#688](https://github.com/accuser/bynk/pull/688), #687) shipped the
  `design/pending/` format and the `xtask` validator; **Slice 1** ([#690](https://github.com/accuser/bynk/pull/690), #689) shipped the
  `cargo xtask stamp` command (version + changelog + ADR materialisation + consume); **Slice 2**
  (#691, ADR 0206) shipped the per-merge workflow. The §5.1 question (per-merge stamp vs.
  batched release PR) is **settled**: per-merge. **The Slice 1/2 boundary was re-cut** (Slice 1
  #689 DECISION A): idempotency (delete-what-you-consume) entangles version and ADR assignment,
  so Slice 1 absorbed ADR materialisation, and the per-merge *workflow* became its own Slice 2.
  Its open question — bot identity / branch protection — **resolved**: `main` is unprotected,
  so the default `GITHUB_TOKEN` pushes the stamp directly (ADR 0206). Live slice state is on the spine.
- **Realises:** [`../README.md` §"Versioning & release"](../README.md) (the single-repo
  version, `scripts/bump-version.sh`, the tag→publish backbone) and
  [`../bynk-release-discipline.md`](../bynk-release-discipline.md) (daily increments each
  cut a version, batched into monthly milestones). It does not change *what* a version or
  an ADR is — it changes *when and by whom the number is assigned*, removing the one
  structural reason two parallel increments collide.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on two axes (the bar is two of three): **multi-increment** — the pending-change
  unit, the merge-time stamp, ADR-number automation, and the optional derived-copy cutover
  are each their own slice with its own fixtures — and **surface not yet settled** — there is
  no agreed format for a pending increment, and the choice between a per-merge stamp and a
  batched release PR is open. It is *adjacent* to a safety boundary (the tag→registry
  publish path is irreversible, ADR 0167 / `../README.md`), but touches the release
  workflow's `verify` gate only to keep it satisfied, never to relax it.
- **Front-loaded decisions (named, not numbered).** Each is created and numbered by the
  slice that lands it — this doc deliberately does not pre-allocate numbers, since
  concurrent tracks would collide ([the recurring lesson](../decisions/README.md)).
  - **The two serial counters are allocated by automation on `main`, never chosen in a
    feature PR** (§3 — *proposed, the doc's spine claim*).
  - **A feature PR declares a pending increment as one uniquely-named file; it writes no
    version and no ADR number into any shared file** (§4 — *proposed*).
  - **The stamp runs per-merge, materialising the pending file into the numbered ADR, the
    version bump, and the changelog row in a follow-up commit on `main`** (§5 — **settled**:
    per-merge over a batched release PR, §5.1).
  - **ADR prose is still reviewed in the feature PR — the stamp assigns the number and moves
    the prose, it does not author it** (§4.2 — *proposed*).
  - **Shrinking the surface (derived copies generated in CI, not committed) is a separable
    win, not a prerequisite** (§6 — *proposed, deferrable*).

## 1. Motivation

Two increments developed in parallel cannot both merge without a conflict, and the loser
does not merely conflict — it silently ships a **wrong** number. The cause is structural,
not procedural: two shared, strictly-increasing counters are read and written by every
increment, at authoring time, hours before the merge that actually orders them.

**The version.** [`scripts/bump-version.sh`](../../scripts/bump-version.sh) writes one
concrete `X.Y.Z` into roughly fifteen files: `Cargo.toml`'s `[workspace.package]` version
*and* every in-workspace dependency requirement, `Cargo.lock`, both
`vscode-bynk`/`tree-sitter-bynk` `package.json` + `package-lock.json` pairs, the
`bynkServerVersion` extension pin, seven Book/docs current-version banners (guarded by
`bynkc/tests/doc_version.rs`), every `bynk*/README.md` `[dependencies]` example, and the
regenerated `site/public/llms-full.txt`. Two branches that both pick "the next version" edit
the *identical lines* of all fifteen. A merge conflict is guaranteed; and per
[`../README.md`](../README.md), the release workflow's `verify` job refuses a tag whose
version does not match every one of those sites, so a stale or duplicated bump is a release
failure, not a cosmetic one.

**The ADR number.** [`../decisions/README.md`](../decisions/README.md) is a hand-curated
table, *complete by construction*: the `decisions_index` drift guard fails if an ADR file
has no row or a row links to no file. Two increments that each add "the next ADR" both take
`0206`, both add a `0206` file, and both add a `0206` row. The table conflicts; and even
when the text merges cleanly, one record now bears a number another record already used —
and an ADR is **immutable once accepted**, so the fix is a rename that ripples through every
cross-reference.

Neither counter is contended by the *work*. They are contended only because the number is
transcribed into shared files while the PR is authored, when the merge order that alone can
assign it correctly is not yet known. The daily-increment cadence
([`../bynk-release-discipline.md`](../bynk-release-discipline.md)) makes this the common
case, not the exception: several increments are routinely open at once.

The existing model already defers *one* release decision correctly — a tag is cut when a
version is to be shipped, not on every increment ([`../README.md`](../README.md) step 3).
This track extends that same instinct one step earlier: defer the **number**, not just the
tag.

## 2. Scope and non-goals

**In scope.**

- A **pending-increment unit**: a single, uniquely-named file a feature PR adds, declaring
  the bump level, the changelog blurb, and (when the increment records a decision) the ADR
  prose — but no number (§4).
- A **merge-time stamp**: automation on `main` that consumes pending units, assigns the next
  version and next ADR number *in merge order*, runs `bump-version.sh`, materialises the
  numbered ADR file and the `decisions/README.md` row, and writes the changelog row (§5).
- **ADR-number automation** specifically — the counter that has no "release" concept and so
  must be assigned at every merge that records a decision, independent of whatever the
  version model becomes (§5.2).
- **Compatibility with the `verify` gate** — the stamp leaves the tree in exactly the state
  `verify` demands, so the tag→publish backbone is unchanged (§5.3).

**Non-goals (and why).**

- **Changing what a version *means*.** The single-repo version, the MAJOR.MINOR-per-language
  /-patch-for-non-language rule, and the monthly-milestone batching all stand. Only the
  moment of assignment moves.
- **Multi-package independent versioning.** The repo carries one version while everything
  lives together ([`../README.md`](../README.md)); the packaging track
  (`design/tracks/packaging.md`, gated on ABI stability ≈1.0 — an untracked draft not yet on
  `main`, so named here rather than linked) owns any future split. This
  track must not pre-empt it — its mechanism has to keep working as one counter and, later,
  as several.
- **A new release cadence.** Whether the stamp fires per-merge or batches into a release PR
  is an *open question* (§5.1), not a mandate to change how often versions are cut.
- **Relaxing the `verify` gate or the `decisions_index` drift guard.** Both stay; the design
  is correct only if it keeps satisfying them.

## 3. The core claim — allocate on `main`, not in the PR

A strictly-increasing counter can only be assigned correctly by whatever serialises the
events that consume it. For both the version and the ADR number, that serialiser is `main`
itself — the merge is the ordering. Any scheme in which the author writes the number while
the PR is open is *racing every other open PR*, and the race is lost silently because
nothing at authoring time can observe the eventual merge order.

The remedy is to make the feature PR carry **intent** and the merge carry **assignment**:

- The feature PR says *"this is a MINOR that records a decision, here is the decision's
  prose and the changelog line"* — in a file no other PR touches.
- The merge (via automation on `main`) says *"you are `v0.187` and ADR `0207`"* — computed
  against the actual, now-known order.

Because the feature PR touches none of the fifteen version files and neither adds an ADR file
nor edits the index table, two parallel feature PRs have **no shared line to conflict on**.
The conflict class is removed, not mitigated.

## 4. The pending-increment unit

### 4.1 One file, uniquely named, touched by no other PR

A feature PR adds exactly one file under a dedicated directory (working name
`design/pending/`), named from the branch or PR so collisions are impossible
(`design/pending/<branch-slug>.md`). The changesets ecosystem is the direct prior art
(§8): a per-PR file whose *filename* carries the uniqueness the shared counter cannot.

The file declares:

- **Bump level** — `minor` (a language increment) or `patch` (non-language), matching the
  existing MAJOR.MINOR / patch rule so the stamp can compute the next version without a
  human choosing it.
- **Changelog blurb** — the row prose, sans version number; the stamp prepends the number.
- **ADR prose** — present when the increment records a decision (most do): the full
  `Status / Context / Decision / Consequences` body, sans number and sans filename.

An increment that records no decision simply omits the ADR prose; an increment that records
several carries several prose blocks (the stamp assigns consecutive numbers in a defined
order, §5.2).

### 4.2 The prose is still reviewed where it is written

The design review that makes an ADR trustworthy happens in the feature PR, on the prose, under
line-anchored review — exactly as today. The stamp is mechanical: it assigns the number,
writes the numbered file, and adds the index row. It never authors or edits prose. This
preserves the property the current flow gets right — that the record and the code that
realises it are reviewed together — while removing the number, the one part of the record
that *cannot* be correctly chosen before merge.

This also keeps the `decisions_index` drift guard honest at every step. A feature PR adds no
ADR file and no index row, so the guard has nothing to check and passes. The stamp adds the
file and the row *together* in one commit, so the guard passes there too. The guard is never
in a state where a file lacks a row.

## 5. The merge-time stamp

On merge to `main`, automation:

1. Reads every pending-increment file not yet consumed.
2. Computes the next version from the current `Cargo.toml` version and the declared bump
   levels, in merge order.
3. Runs `scripts/bump-version.sh X.Y.Z` (unchanged — it is already the one command that
   touches all fifteen sites).
4. For each ADR prose block, assigns the next free number, writes
   `decisions/NNNN-<slug>.md`, and adds its `decisions/README.md` row.
5. Writes the changelog row with the stamped version.
6. Deletes the consumed pending files.
7. Commits — as a direct commit on `main` or as a "Version Packages" PR (§5.1).

Because this runs *on `main`, serialised behind a concurrency group*, the counters are never
contended: the automation sees the real order and assigns against it.

### 5.1 Decision — per-merge stamp (settled)

The stamp runs **per-merge**: every merge to `main` triggers it, and it commits the bump +
materialised ADR straight to `main`. The alternative — a batched "Version Packages" PR
(release-please style) that accumulates pending files and bumps once — is **rejected**, for
two reasons, the first decisive:

- **The ADR number has no release concept.** An ADR is accepted when its PR merges, so its
  number must be assigned at *that* merge regardless of the version model. Per-merge
  automation is therefore needed *anyway* for ADRs. The batched model would need per-merge
  ADR numbering *and* a separate batched version PR — two mechanisms where the per-merge
  stamp is one; it does not remove work, it adds a second track.
- **Batching changes the cadence.** A "Version Packages" PR makes several increments share
  one version bump, contradicting *"daily increments each cut a version"*
  ([`../bynk-release-discipline.md`](../bynk-release-discipline.md)) unless that rule is
  revisited too — an invasive change this track has no reason to force.

Per-merge preserves the current *"each increment cuts a version"* cadence exactly: each
increment still gets its own distinct version and ADR, just assigned a beat later. The
accepted costs are (a) a second automated commit per merge — serialised behind a concurrency
group (§9), so no contention — and (b) the version and ADR number no longer appear in the
feature PR's own diff, landing instead in the stamp commit that references it.

### 5.2 Deterministic ADR numbering

When one stamp materialises several ADRs (one increment recording several decisions, or —
under batching — several increments at once), the numbers are assigned in a defined order:
by merge order across increments, then by declaration order within a file. The order is a
function of state already on `main`, so a stamp re-run is reproducible.

### 5.3 The `verify` gate is untouched

The stamp runs `bump-version.sh` and nothing else touches the fifteen sites, so after the
stamp the tree satisfies the `verify` job's all-sites-agree check by construction. A release
tag is still cut by hand when a version is to be shipped
([`../README.md`](../README.md) step 3); the publish backbone (OIDC trusted publishing,
re-run-safe) is entirely unchanged. This track sits *before* the tag, in how the number that
`verify` checks gets chosen — never in the gate itself.

## 6. Shrinking the surface (separable)

Independently of *when* the version is assigned, the number is duplicated across far more
files than it needs to be. `Cargo.toml` and the two `package.json`s are the true source; the
eight doc banners, the README `[dependencies]` examples, and `llms-full.txt` are all
*derivable* from it — today they are committed and drift-guarded rather than generated.

Generating the derived copies in CI (and drift-checking, not storing, them) would collapse
the conflict surface even in the current per-PR model, and shrinks the stamp's write set in
this one. It is a genuine win but **not a prerequisite** — the §3 deferral removes the
conflict class on its own. Sequenced last, and only if it earns its keep against the
`doc_version.rs` guard it would replace.

## 7. Slice decomposition (candidate)

Numbers are provisional; each slice is an ordinary increment proposal, `accepted` on its own
sub-issue before build.

- **Slice 0 — the pending-increment unit.** The file format and a validator (a malformed
  pending file fails CI in the feature PR). No stamp yet; the format lands and is exercised
  first. **Shipped — #688 (#687).**
- **Slice 1 — the `cargo xtask stamp` command.** Reads the pending files, assigns each its
  next version (in merge order) and the next ADR number(s), and — with `--apply` — runs
  `bump-version.sh`, prepends the changelog row(s), writes the ADR file(s) + index row(s), and
  deletes the consumed pending file(s); dry-run by default. **Absorbs ADR materialisation**
  (#689 DECISION A: idempotency entangles version and ADR assignment into one atomic consume).
  **Shipped — #689.**
- **Slice 2 — the per-merge workflow.** Runs the stamp automatically on merge to `main` and
  commits the result (`.github/workflows/stamp.yml`). Resolved the open question §9 (`main` is
  unprotected → the default `GITHUB_TOKEN` pushes directly; a token push triggers no CI, which
  is the loop prevention *and* why the workflow self-validates before pushing) and carries the
  load-bearing **allocation-on-`main`** ADR ([0206](../decisions/0206-allocation-on-main.md)),
  deferred here from Slice 1 so it describes the mechanism once *operative*. **Shipped — v0.186, #691.**
- **Slice 3 (deferrable) — shrink the surface.** Move derived copies to CI generation (§6).

## 8. Prior art

- **changesets** (npm) — the direct model for §4: a per-PR file whose unique filename sidesteps
  the shared-counter conflict, consumed by a release step that bumps and writes the changelog.
- **release-please** (Google) — the batched "Version Packages" PR of §5.1's second option.
- **semantic-release** — fully automated version derivation from commit metadata; rejected in
  spirit here because Bynk's bump level is an editorial call (language vs. non-language), not a
  mechanical read of commit prefixes.

The adaptation this track makes over all three: it stamps a **second serial counter, the ADR
number**, which the JS-ecosystem tools have no analogue for, and which is the reason the
per-merge stamp (not the batched PR) is the natural fit.

## 9. Open questions

**Settled.** *Per-merge stamp vs. batched release PR* (§5.1) — **per-merge**, because the ADR
number must be assigned at every merge regardless, so per-merge automation is needed anyway,
and because batching would change the daily-increment cadence.

Still open:

1. **Pending-file format** — Markdown with frontmatter (level + changelog) plus prose ADR
   blocks, or a stricter split? What does the validator enforce?
2. **Where the changeset directory lives** — `design/pending/`, or outside `design/` so it is
   never swept into an unrelated PR (cf. the standing "don't `git add -A` untracked track
   docs" hazard).
3. **The stamp's identity and trigger** — a `push`-to-`main` workflow with a concurrency group,
   committing as a bot; interaction with branch protection and required checks on the stamp
   commit. (The per-merge shape is settled, §5.1; what stays open is the workflow's identity
   and its composition with branch protection.)
4. **Whether §6 (surface shrink) is in this track or its own** — it stands alone and could be
   sequenced independently.
