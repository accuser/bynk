---
level: patch
changelog: Settle phase 5 of the compiler trajectory (`design/tracks/semantics-in-the-checker.md`) — the remaining `bynk-emit` diagnostic sites relocate to `bynk-check` by priority (five close a named, fixture-pinned editor regression; two close for R3.5 compliance alone), `check_function_type_boundaries`'s reach-back hook closes, and R10.1 closes with a crate-doc correction rather than a `bynk-driver` split
---

## ADR: semantics-phase5-rule-scope
title: R4.6, R4.11 and R10.4 stay in phase 5's scope as verify-only, not reopened decisions
summary: All three already read landed in the reference's Appendix D; phase 5's business with them is confirming the relocations don't regress them, not building anything new

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

## ADR: semantics-phase5-check-relocation-scope
title: All seven of `bynk-check/src/analysis.rs`'s named gap categories are phase 5's scope, sequenced by whether they close a live editor regression
summary: Five categories close a named, fixture-pinned, CHANGELOG-documented editor regression; two close for architectural compliance alone with no observable change; one flagged site is confirmed not a diagnostic at all

**Context.** `bynk-check/src/analysis.rs`'s own module doc — written during phase 4's P4.1 slice (#1115),
not under a phase-5 settling review — already enumerates exactly seven categories of whole-project
checking that `bynk-emit::run_checks`'s `Mode::Analyse` arm performs and the new
`bynk-check::analyse_project` entry point does not port: schema-registry reconciliation, `messages`
bundle validation, locale bundle ambiguity, event-subscription validation, platform-lock enforcement,
function-type-boundary checks, and test/integration-suite processing. Of these, `CHANGELOG.md` names five
as a live regression in the editor's project analysis (`bynk-ide`'s repoint off `bynk-emit`, P4.2, #1122)
and states explicitly that they are "accepted, tracked debt … closed when phase 5 of that track ports
these checks into `bynk-check`'s analysis entry point." `bynk-lsp/tests/analysis_residual_gap.rs` pins
each live-gap category as a direct assertion, sourced from real negative-fixture cases, that today's
editor output lacks it. Two categories (schema-registry reconciliation, platform-lock) are confirmed
unreachable on the analyse path regardless of where the checking code lives, because
`analyse_project_with` hardcodes `SchemaLock::Off` and `Platform::default()`/`BuildTarget::Bundle` — the
regression-fixture file's own header records a correction made while grounding it: platform-lock was
initially miscounted as a sixth live regression before this was found.

Separately, a naive grep for `bynk.*`-prefixed strings in `bynk-emit` turned up three sites outside this
seven-category accounting. `bynk-emit/src/emitter/emit.rs`'s `bynk.emit.unresolved_cross_context_signature`
is not a registered diagnostic — both occurrences are inside a `panic!`/`assert_eq!` message string, never
a `CompileError::new(...)` construction, and its own comment frames it as the emitter disagreeing with a
call the checker already resolved — a compiler-internal-consistency assertion, not a diagnosable program
error. `bynk-emit/src/emitter/secrets.rs`'s `bynk.secrets.computed_name` is a real, registered diagnostic,
reachable from `bynk check`/the LSP path per its own surrounding comment, but not named among the seven
categories — its exact relationship to the new entry point wasn't traced far enough during this settling
pass to classify with confidence. `bynk-emit/src/project.rs`'s own `bynk.project.schema_registry_corrupt`
is a real, registered diagnostic outside all seven categories too, but unambiguously in scope: it's the
site that best illustrates why "all seven categories relocate" is not the same claim as "R3.5 closes" —
a genuine eighth site the seven-category accounting doesn't cover, needing its own relocation regardless.

**Decision.** All seven named categories, plus `project.rs`'s `bynk.project.schema_registry_corrupt`, are
phase 5's scope. They ship in priority order: the five live-gap categories first (`messages` bundle
validation, locale bundle ambiguity, event-subscription validation, function-type-boundary checks,
test/integration-suite processing — the last of these also carries a second consequence, a
`RefSink`/go-to-definition regression, and is sequenced last for being the most emission-coupled), then
the two gap-in-name-only categories (schema-registry reconciliation, platform-lock) for R3.5 compliance
alone, then `project.rs`'s own site alongside the crate-doc correction (R10.1) once the rest has landed.
`emit.rs`'s flagged site is out of scope entirely, needing no relocation. `secrets.rs`'s site is carried
into the verify-only slice (P5.5) as an open item — either already covered by an existing path or a ninth
relocation — rather than assumed either way.

**Consequences.** This is the most load-bearing decision of this settling pass: it fixes the whole slice
list for phase 5 (`design/tracks/semantics-in-the-checker.md` §6). It is also the one this settling pass
found the most direct, contemporaneous evidence for — the module doc, the regression-fixture file and the
CHANGELOG entry all name this exact phase, by name, as already committed to doing this work; this ADR
formalises a decision the codebase had effectively already made. Each relocation deletes or flips its
corresponding pinned assertion in `analysis_residual_gap.rs`. If `secrets.rs`'s open item resolves to a
genuine eighth category during P5.5, that is new scope discovered late, not a reversal of this ADR.

## ADR: semantics-phase5-function-boundary-hook
title: `check_function_type_boundaries` moves into `bynk-check`, closing its optional-hook seam
summary: `bynk-check` currently reaches back into `bynk-emit` for this one check through a hook passed as `None`; the function relocates so every check has the same home

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
