# 0331 — `check_function_type_boundaries` moves into `bynk-check`, closing its optional-hook seam

- **Status:** Accepted (v0.247.26)

**Context.** `bynk-check::project_model::phase_group` accepts an optional boundary-check hook; the new
analysis entry point passes `None`, so `check_function_type_boundaries` (defined in
`bynk-emit/src/project/validate.rs`, `pub(crate)`) genuinely does not run on the editor's analysis path —
named directly in `bynk-check/src/analysis.rs`'s own doc as "a documented residual gap" and confirmed a
live regression by `analysis_residual_gap.rs`. The hook shape means `bynk-check` reaches forward into
`bynk-emit` for this one check specifically — the reverse of every other relocation this phase makes, and
arguably the same reverse-boundary shape R3.5's invariant ("no crate reaches back across a boundary to
drive the checker") exists to remove, just pointed the opposite direction.

**Decision.** `check_function_type_boundaries` relocates into `bynk-check`, called directly from
`phase_group` (or its successor) like every other check in the module. The optional hook is deleted.

**Consequences.** Closes the reverse-reach the hook created and the live gap in the same move — `bynk-check`
no longer depends on `bynk-emit` at all for this check, matching the direction of every other relocation in
this settling pass. `content-ownership.md`'s precedent for permanent, named exceptions (the three
`fs_below_driver` cases) does not apply here: those exceptions carry no user-facing regression, and this
one does.
