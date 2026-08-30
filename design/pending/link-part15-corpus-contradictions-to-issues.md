---
level: patch
changelog: "Part 15.1's refusal register named three residuals that were flagged in prose but never given a durable tracking home: an unresolved effect-inference contradiction between design notes §15 and type-system spec §2.8.4, a diagnostic-error-enum trigger whose registry-drift evidence is unmeasured since a July finding, and a tuples contradiction spanning ADR 0120, the type-system spec and design notes §11. Now tracked as issues ([#1529](https://github.com/accuser/bynk/issues/1529), [#1531](https://github.com/accuser/bynk/issues/1531), [#1530](https://github.com/accuser/bynk/issues/1530)) and cross-linked from `design/bynk-greenfield-compiler.md`."
---

## ADR: link-part15-corpus-contradictions-to-issues
title: Link Part 15.1's remaining refusal-register residuals to tracking issues
summary: Three residuals in the refusal register (effect inference, the diagnostic error enum, tuples) get a durable, linked home

**Context.** The four forward references phase 8 named at its own retirement were filed as issues
(#1523–#1526) and cross-linked from `design/bynk-compiler-trajectory.md` and
`design/archive/retired-tracks.md`. Part 15.1's refusal register in `design/bynk-greenfield-compiler.md`
names three further residuals — an effect-inference contradiction, a diagnostic-error-enum trigger with
aging evidence, and a tuples contradiction — that were flagged in prose but never given the same
treatment.

**Decision.** File each as its own issue, marked deferred/not-ready-to-build per R15.1's own discipline
against recording an intention as a refusal, and link them from Part 15.1 itself.

**Consequences.** Docs-only change; no code touched. The two contradictions (effect inference, tuples)
are documentation-consistency debt, not build-gated work — resolving them needs a decision, not a
trigger. The diagnostic-error-enum issue is genuinely trigger-gated, same shape as #1523/#1524.
