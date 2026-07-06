---
name: Feature track
about: Open the tracking spine for a far-reaching, multi-increment feature (ADR 0076 / ADR 0167). Settling happens in a draft PR that adds the track doc; this issue stays open until the track retires.
title: "Track: <theme — no version prefix; slices are versioned at implementation, not here>"
labels: track
---

<!--
A feature track runs GitHub-native — see design/tracks/README.md for the full
lifecycle. The short form:

- THIS ISSUE is the track's spine. It stays OPEN for the track's whole life and
  is closed only by the retirement PR (`Closes #<this>`). The settling PR
  references it ("Part of #<this>") — it must NEVER close it.
- SETTLING happens in a DRAFT PR that adds `design/tracks/<slug>.md`. Draft
  status = still settling. Marking the PR ready for review = the open design
  questions below are closed and the front-loaded ADRs are identified. MERGE =
  direction settled. Merge is NOT build authorisation.
- SLICES are ordinary increment proposals (.github/ISSUE_TEMPLATE/
  increment-proposal.md), opened as SUB-ISSUES of this issue, each citing the
  track doc and the foundational ADRs. Accepting a slice proposal (label
  `accepted`) is the approval to build that slice — per
  design/proposals/README.md. Keep the "Slice status" checklist below current
  as slices are cut and land.
- RETIREMENT is a PR that removes the doc (its decisions live on in the ADRs
  and the spec-in-place), appends the closing summary to
  design/archive/retired-tracks.md, and closes this issue.

Every section below is required. Where a section genuinely does not apply, say
so explicitly with a reason ("none, because …"). Silence is treated as an
oversight, not a no-op — the same rule the increment-proposal template holds
itself to.
-->

## The theme

<one or two paragraphs: the feature in language a newcomer can follow, and the
end state when the track retires.>

- **Realises:** <the design-notes / roadmap sections this sharpens, e.g. `design/bynk-design-notes.md` §N.>
- **Track doc (added by the settling PR):** `design/tracks/<slug>.md`

## Why a track (the ADR 0076 trigger)

<!-- A feature track applies when TWO OR MORE hold. Tick and justify each; if
fewer than two hold, this is a single increment proposal, not a track. -->

- [ ] **Multi-increment** — <why a single delete-on-merge proposal cannot carry it.>
- [ ] **Surface not yet settled** — <the genuinely open questions.>
- [ ] **Security/safety boundary** — <what a wrong foundational shape would compromise.>

## Open design questions

<the questions the settling phase must close, each with the investigation or
prior art it needs. These are the review agenda for the settling draft PR;
marking that PR ready for review asserts they are closed.>

## Candidate slice decomposition

<the ordered slices as currently foreseen — provisional until the doc settles.
Each becomes an increment-proposal sub-issue of this spine when it is cut.>

## Slice status

<!-- Keep current as the track progresses; link each slice's sub-issue when it
is cut. GitHub's sub-issue progress bar tracks the authoritative state; this
checklist is the human-readable mirror. -->

- [ ] Slice 0 — <…>
- [ ] Slice 1 — <…>

## Front-loaded ADR candidates

<the load-bearing, hard-to-reverse calls that must land with — or just before —
the first slice, per ADR 0076. Do NOT pre-allocate ADR numbers; they are taken
at merge.>

## Threat model

<required when the security/safety trigger is ticked: the assets, the
adversary, and where verification happens. Otherwise "none, because …">
