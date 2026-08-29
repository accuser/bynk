# 0413 — The file level gets one shared `Tokens(FileId)`/`Ast(FileId)` cache in `bynk-project`, migrating both completion and diagnostics onto it — not a `bynk-ide`-local patch, which the crate graph rules out entirely

- **Status:** Accepted (v0.289.49)

**Context.** `bynk-ide/src/completion.rs`'s `PROJECT_UNIT_CACHE` already closes issue #733 for
completion requests — a real, working, content-keyed parse cache. The draft's own framing treated
"share it with the diagnostics path" as the cheap option among three. Settling checked the actual
crate dependency graph rather than assume the option was available: `bynk-ide`'s own `Cargo.toml`
depends on `bynk-check` (`bynk-check.workspace = true` — "`analyse_project`... lives here too"),
and `bynk-project ← bynk-check ← bynk-ide` is the confirmed direction throughout. `analyse_project`
and its `phase_parse` step (the diagnostics path R3.13's probe is named after) live in
`bynk-check`, which cannot reach a cache owned by the crate that depends on it. This is a
structural fact, not a design preference — "patch `bynk-ide`'s own cache to also serve
diagnostics" cannot work regardless of how it's written.

Separately, settling checked whether `FileId` (`bynk-syntax/src/span.rs:16`, built at phase 3 for
R2.2/R2.4) is already the durable key `Tokens(FileId)` would need. It exists, and is a real,
per-file identifier — but `bynk_check::project_model::phase_parse` allocates it from a local `let
mut next_file_id: u32 = 0`, reset to zero on every call. It is stable *within* one project
analysis (its stated purpose: attributing a diagnostic to the right file inside one build), not
*interned* durably *across* calls the way a cache key spanning many keystrokes needs to be.

**Decision.** Build one real cache — a `Tokens(FileId)`/`Ast(FileId)` query type — in
`bynk-project`, the one crate both `bynk-check` and `bynk-ide` can reach (and where
`ProjectGraph`, this phase's other file-adjacent type, already lives per phase 4's own placement).
This requires, as part of the same slice, a durable path↔`FileId` interning table (also in
`bynk-project`, alongside discovery) so a given file keeps the same `FileId` across separate
calls for the life of an analysis session, not only within one `phase_parse` invocation as today.
Both `bynk-ide::completion`'s `cached_project_unit` and `bynk_check::analyse_project`'s
`phase_parse` migrate to read through this one cache; `PROJECT_UNIT_CACHE` retires as a
`bynk-ide`-local duplicate once the migration lands.

**Consequences.** This closes R3.13's own probe-namesake bug (the diagnostics path's uncached
`analyse_project` call) for real, not by proxy — the actual call site that re-parses on every
keystroke gets a cache it can reach, rather than one it structurally can't. It is a larger slice
than "share an existing cache" would have been, since the interning table is new work, not
reused — named explicitly so it isn't discovered as a surprise mid-slice. It is also the one
slice in this phase with real behavioural stakes: a wrong invalidation rule here produces stale
diagnostics silently, the same failure class #733 fixed for completion, reopened at a different
layer if the new cache's own content-equality check has a gap — worth a dedicated staleness
fixture, not just a byte-golden pass.
