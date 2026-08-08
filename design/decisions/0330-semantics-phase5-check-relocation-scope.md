# 0330 — All seven of `bynk-check/src/analysis.rs`'s named gap categories are phase 5's scope, sequenced by whether they close a live editor regression

- **Status:** Accepted (v0.247.26)

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
