# 0167 — Feature tracks run GitHub-native: a `track` spine issue, a settling draft PR, sub-issue slices, a retirement PR

- **Status:** Accepted (posture; 2026-07-06). Amends [[0076]] (the feature-track
  posture): the artefact set is unchanged — the lifecycle *mechanics* move onto
  GitHub. Like 0059 and 0076, a process posture, not a language-defining call.
- **Relates:** [[0076]] (the feature track), 0059 (the refactor track), the
  proposals-to-issues move
  ([#491](https://github.com/accuser/bynk/pull/491) — increment proposals live
  as issues; `accepted` is the approval to build; the implementing PR closes
  them).

## Context

ADR 0076 introduced the feature track — a persistent design doc, a settling
phase, front-loaded ADRs, per-slice proposals — but left the lifecycle
mechanics implicit. In practice a track doc was hand-written, committed by an
ordinary PR, and its state (settling / active / slice N done / retired) was
curated as prose in `design/tracks/README.md`, which grew a
paragraph-per-track status section duplicating what the tracker already knew
and rotting between updates (the retired-track entries alone reached several
screens). Meanwhile #491 moved increment proposals to GitHub issues precisely
because an issue number is **durable plain-text provenance** — it resolves
forever — and because the tracker carries discussion-shaped state better than
committed-then-deleted files.

A track is the opposite artefact to a proposal — persistent, not transient —
so the same move lands differently. The doc must remain a committed file: ADRs
and slice proposals cite it, agents and tools read it from the repo, and it is
updated as slices land. What *can* move to GitHub is the state and the
ceremony. And a track doc is exactly the document where **line-anchored
review** earns its keep — a paragraph-level disagreement in a threat model or
a slice decomposition matters — which an issue body cannot give but a PR can.

## Decision

Track **state** lives on GitHub; track **design** lives in the committed doc.

1. **A `track` issue is the spine.** A track opens as an issue from
   `.github/ISSUE_TEMPLATE/feature-track.md` (label `track`): the theme, the
   0076 trigger check, the open design questions, the candidate slice
   decomposition. The spine stays **open for the track's whole life** and is
   closed only by the retirement PR.
2. **Settling is a draft PR.** The doc lands via a draft PR adding
   `design/tracks/<slug>.md`, referencing the spine ("Part of #n" — never
   `Closes`). Draft status *is* 0076's settling phase, made structural:
   marking the PR ready for review asserts the open questions are closed and
   the front-loaded ADRs are identified; **merge settles direction**. Merge
   remains *not* a build authorisation — that stays with each slice's
   `accepted` proposal, per 0076 and #491.
3. **Slices are sub-issues of the spine.** Each slice is an ordinary
   increment-proposal issue opened as a **sub-issue** of the track issue,
   citing the doc and the foundational ADRs. GitHub's sub-issue progress bar
   is the live slice status; marking a slice done in the doc rides the
   implementing PR, needing no ceremony of its own.
4. **Substantive direction changes re-settle by PR.** A surface decision
   reversed, a slice re-scoped away, a new phase — a small reviewed PR against
   the doc (a mini settling pass), never a ride-along on an implementing PR.
5. **Retirement is a PR.** It removes the doc, appends the closing summary to
   `design/archive/retired-tracks.md` (the durable record 0076's "removed (or
   archived)" left unspecified), and closes the spine (`Closes #n`).
6. **The tracks README is a map, not a status board.** `design/tracks/README.md`
   lists doc ↔ spine pairs for active tracks and one-liners for retired ones;
   per-track status prose moves to the spine issue, where it cannot silently
   drift from the tracker.

## Consequences

Line-anchored review lands where it matters (the doc), provenance is durable
(spine and slice issue numbers resolve forever, so ADRs can cite them in plain
text), slice progress is a checklist the tracker maintains for free, and the
settling state 0076 could only describe in prose is carried structurally by
the PR's draft flag. The cost is a long-lived open issue per track to keep
honest, and one more place a reader may look — mitigated by the README pairing
each doc with its spine. Existing tracks are grandfathered: their docs were
committed by ordinary PRs; retro-fitted spine issues
([#557](https://github.com/accuser/bynk/issues/557),
[#558](https://github.com/accuser/bynk/issues/558)) carry their state forward,
and `deploy.md` — committed as a draft mid-settling — continues settling via
reviewed PRs against the doc.
