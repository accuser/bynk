# 0329 — R4.6, R4.11 and R10.4 stay in phase 5's scope as verify-only, not reopened decisions

- **Status:** Accepted (v0.247.26)

**Context.** The compiler trajectory's phase-5 section (`design/bynk-compiler-trajectory.md` §3) lists
five reference rules: R3.5, R4.6, R4.11, R10.1, R10.4. `design/bynk-greenfield-compiler.md`'s Appendix D,
regenerated since that section was written, already marks R4.6 (`ResolvedCommons` constructor, the three
checker gates back on), R4.11 (the phase-boundary-value constructor) and R10.4 (facade deletion) ✅
landed — closed by phase 1's paydown and a separate facade-deletion pass (#1048), not by phase 5's own
work. Only R3.5 and R10.1 are open in the sense the trajectory's phase-5 section originally meant.

**Decision.** Phase 5 keeps R4.6, R4.11 and R10.4 in its `Relates:` scope, narrowed to a single
verify-only slice (P5.5): confirming that relocating the seven diagnostic categories named in
`semantics-phase5-check-relocation-scope` (below) does not reintroduce a hand-rolled
`ResolvedCommons`-shaped construction at a new `bynk-check` call site, and that R10.4's facade discipline
holds at each new call site. No new construction against these three rules is in scope.

**Consequences.** A relocation that quietly hand-rolls a resolved-type view instead of reusing
`ResolvedCommons`'s real constructor would reopen phase 1's closed defect in a location Appendix D's next
sweep wouldn't catch until after the fact; naming this as an explicit slice (P5.5) rather than an
assumption is what catches it before merge instead of after. If a relocation is found to need something
`ResolvedCommons` doesn't provide, that is grounds to revisit this ADR's scope under its own review, not
to hand-roll a workaround silently.
