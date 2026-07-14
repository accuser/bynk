# 0076 — Far-reaching language features run as a feature track: a persistent design doc, a design-settling phase, front-loaded ADRs, then per-slice proposals

- **Status:** Accepted (posture; 2026-06-16). The actors feature track is the inaugural instance, opened as a follow-up. **Amended by [[0167]]** (the lifecycle runs GitHub-native: a `track` spine issue, a settling draft PR, sub-issue slices, a retirement PR).
- **Living doc:** `design/tracks/<feature>.md`

## Context

The standard increment mechanism (`design/proposals/README.md`) assumes a
**mostly-settled design**: a `vX.Y-<slug>.md` proposal *finalises and
authorises* a known shape, then the implementing PR **deletes** it. Every
increment to date fits this — Float, the JSON codec, collections, string
interpolation, the LSP feature line, the v0.44 protocol restructure. The
durable record is the code, the spec-in-place, and the ADRs; the proposal is a
transient sign-off.

ADR 0059 established a *track* for the one prior multi-increment effort — the
refactor track — but as the **inverse** of what a language feature needs: it
works *because* refactors are behaviour-preserving and **not** language-defining
(single anchoring ADR, no per-increment ADRs, golden-identical as the gate).

Some language features break the single-proposal assumption on every axis at
once. **Actors** (design notes §6 *Actor Declarations as Contracts* / §7
*Services and Protocol Composition*) is the first:

- **Multi-increment.** Declaration + `by`-clause + identity; each auth scheme;
  multi-actor dispatch; authorisation invariants; replay/ordering; cross-context
  and platform actors — ~5–7 increments. A delete-on-merge proposal leaves
  nothing to carry the connective design across them.
- **Surface not yet settled.** The design notes commit the *concept* but flag
  the surface as provisional. Genuinely open questions (how authorisation
  invariants desugar; multi-actor dispatch; the verification-codegen seam;
  closed-vs-open scheme set) need *working out* before a buildable increment
  exists. The proposal's small `[DECISION]` block is sized for a handful of
  local calls, not a feature's architecture.
- **A security boundary.** Auth verification is the one place where a wrong
  foundational shape or a verification bug is a *vulnerability*, not a defect.
- **Path-dependent.** The scheme representation, the verification seam, and the
  identity model constrain every later slice; they want durable decisions made
  *up front*, not re-derived per slice.

## Decision

A feature with **two or more** of {multi-increment, surface-not-yet-settled,
security/safety boundary} runs as a **feature track**, not a single proposal.
The track is the lightest generalisation of the existing machinery — it adds
**one artefact** and **one phase**, and reuses proposals, ADRs, and
spec-in-place otherwise:

1. **A persistent design doc — `design/tracks/<feature>.md`.** The north star
   *for this feature*: the sharpened conceptual model (harvested from the design
   notes), the concrete surface, a **security/threat model** where the feature
   is a boundary, the internal architecture (the openable-later seams), and the
   **ordered slice decomposition**. Unlike a proposal it is **not deleted on
   merge** — it is the living map, updated as slices land (each slice marked
   done), and retired only when the theme completes. It *realises and sharpens*
   the design notes; it does not replace them.
2. **A design-settling phase precedes any build proposal.** The doc's first job
   is to *close* the open design questions (by investigation and prior-art where
   needed). The **load-bearing, hard-to-reverse ADRs land up front** — with or
   just before the first slice — so every later slice inherits them. This is the
   step the standard mechanism has nowhere to put.
3. **Slices stay ordinary increment proposals.** Each is a normal `vX.Y-*.md`
   that **cites the track doc and the foundational ADRs**, and builds / lands /
   deletes per `proposals/README.md`. The track doc tracks slice status the way
   the proposal queues track backlog. Contrast 0059: a feature track's slices
   are language-defining, so each **may earn its own ADR** — the opposite of the
   refactor track's single-anchor rule.
4. **Security-bearing slices carry a threat model and a security-review gate.**
   Any slice that emits verification/authentication logic states its security
   properties in the track doc and runs `/security-review` (and `/code-review`)
   before landing.

## Consequences

The connective design survives across slices, the irreversible calls are made
deliberately and once, and the security surface is reviewed as a first-class
concern — none of which a delete-on-merge proposal can hold. The cost is one
more durable artefact to keep current and a heavier up-front phase before the
first line of feature code; both are proportionate to features that are, by the
trigger, far-reaching and hard to reverse. The pattern is reusable: Events,
WebSocket, and Alarm (all design-notes §7 protocols sharing the actor surface)
are the obvious future tracks. This ADR is a **process posture**, not a
language-defining call, and like 0059 anchors a way of working rather than a
surface; within-track design calls earn their own ADRs as the slices land.
