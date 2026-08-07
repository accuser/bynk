# 0328 — A new `bynk-check` analysis entry point closes R10.2 without moving `run_checks` or checking itself

- **Status:** Accepted (v0.247.23)

**Context.** `bynk-ide/Cargo.toml`'s own comment names the reason it depends on `bynk-emit`:
`analyse_project`, "the non-bailing project analysis," lives there. Tracing `analyse_project_with`
(`bynk-emit/src/project.rs:970`) found it calls `run_checks` (`:3644`, private to `bynk-emit::project`) —
the same function `compile_project` (`:573`) calls for the CLI/emission path (`:584`). `run_checks`
(`:3644-4206`, ~560 lines) performs discovery, parsing, resolution *and* checking as one sequence; there is
no existing seam inside it separating "project model" from "checking" at the granularity phase 4 needs —
a structural fact independent of the function's size.
`bynk-ide`'s real dependency on `bynk-emit` is therefore not a dependency on a relocatable discovery
function — it is a dependency on a function that also checks, which the new `bynk-project` crate (sitting
below `bynk-check`) cannot absorb without breaking the layering phase 4 exists to establish, and which
moving to `bynk-check` in full is phase 5's job (R3.5), not phase 4's, and larger than phase 4's review
budget.

**Decision.** Phase 4 does not move `run_checks`. It adds one narrow entry point in `bynk-check` —
`bynk-check`'s natural long-term home under R3.5 regardless — performing the same
discovery(`bynk-project`)→parse→resolve→check(`bynk-check`, already local) sequence `run_checks`'s
`Mode::Analyse` arm performs today, returning what `bynk-ide` needs in place of today's `ProjectAnalysis`.
`bynk-ide` calls this instead of `bynk-emit::analyse_project`. `run_checks` stays in `bynk-emit`,
unchanged, serving `compile_project`/emission alone. `ProjectAnalysis` itself is a composite of discovery
outputs (`snapshots`, `unit_sources`, `doc_scope`) and checker outputs (`index`, `hints`, `expr_types`,
`ty_intern`, `locals`, `requirements`, `sequence_info`, `boundary_info`) — see the companion
`project-model-symbols-boundary` ADR — so this entry point's return type has to surface both kinds, not
just checker output; "the analogue of `ProjectAnalysis`" means composing `bynk-project`-sourced data with
`bynk-check`-sourced data, not producing a purely `bynk-check`-shaped value.

**Consequences.** This is a deliberate, temporary duplication: the new `bynk-check` entry point and
`run_checks`'s `Mode::Analyse` arm do overlapping work until phase 5 centralises checking in `bynk-check`,
at which point `bynk-emit`'s CLI path calls the same entry point and `run_checks`'s checking half is
deleted rather than ported. The alternative — doing phase 5's centralisation now to avoid the duplication —
is explicitly out of phase 4's scope and would decide `validate.rs`'s new home under a much smaller
review budget than that decision deserves. The duplication is named here specifically so phase 5 inherits
it as known, bounded debt rather than rediscovering it as a surprise. This is the most load-bearing and
hardest-to-reverse of this settling pass's three decisions: it fixes the shape `bynk-ide`'s live analysis
path takes for the phase-4-to-phase-5 window. The direct edit is one call site
(`bynk-ide/src/lib.rs:320`, behind the stable `diagnose_project`/`diagnose_project_with` wrapper); 85
`diagnose_project(` call sites across the tree (87 raw matches include the function's own two definitions)
exercise this path without naming it, which is a coverage argument for the relocation, not a statement
that 85+ sites need editing.
