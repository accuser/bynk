# 0286 — Subscription pattern filtering reuses refined-pattern dispatch and commits to deliver-and-filter

- **Status:** Accepted (v0.237.1)

**Context.** `from Events(E { region: Domestic, .. })` (§7 lines 198–227 of
`design/bynk-design-notes.md`, the Events track, spine #936) filters
emissions by a structural pattern on the payload, type-checked against the
event shape and enforced before the handler runs. §7 explicitly frames this as
mirroring the auth pattern already on services, and multi-actor sum dispatch
([ADR 0090](../decisions/0090-multi-actor-sum-dispatch.md)) plus authorisation
invariants
([ADR 0091](../decisions/0091-authorisation-invariants-refinement-actors.md))
already establish structural, declarative, enforced-before-the-handler
dispatch; the payload pattern itself is the shipped refined-pattern and
nested-payload machinery
([ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md),
[ADR 0252](../decisions/0252-or-patterns.md),
[ADR 0253](../decisions/0253-refined-patterns.md)). The one thing precedent
does not settle: *where* the filter runs — §7 wants "server-side filtering
where the platform supports it, deliver-and-filter as a transparent fallback"
(line 227).

**Decision.** Slice 1 ships **deliver-and-filter only**: the fan-out substrate
(`events-fanout-substrate`, this same pending file) delivers every emission of
`E` to every subscriber of `E`, and the subscriber's generated guard — built
from the same type-checked pattern, reusing the shipped refined-pattern
machinery rather than a new matching engine — filters before the handler body
runs. Server-side pre-filtering at the substrate is left a later,
semantics-preserving optimisation: it may change what work an unmatched
emission costs, never what a subscriber observes.

**Consequences.** Slice 1 introduces no bespoke pattern-matching mechanism.
Its fixtures must include negative cases — an emission that must **not**
arrive at a subscriber whose pattern excludes it — since deliver-and-filter
makes over-delivery a live risk if the generated guard is wrong, not merely a
hypothetical one a smarter substrate would have avoided by construction.
