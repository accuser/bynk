# 0374 — `project.rs` clears `ast_importers` entirely — closes Phase G of the #1137 retirement plan

- **Status:** Accepted (v0.249.26)

summary: The last pre-check `TypeRef` walk relocates as an owner-side accessor; four re-exports carry the rest, each argued against an already-parameterised `bynk-check` API, not added for the probe's sake

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b) targeted `project.rs`'s
remaining `bynk_syntax::ast` import as Phase G's final slice. Eight prior slices (P6.42–P6.48)
relocated every declaration-read site that had one; this slice closes the two that were left:

1. **`collect_history_target_agents`** — a pre-check walk over every test suite's `for all run:
   History[Agent]` properties, matching `TypeRef::History(inner)`/`TypeRef::Named` to find the agent
   names those properties drive. This walks `ParsedFile::test()`'s own `SuiteDecl`, pre-check-stage
   data `bynk-project` already owns — the same shape as P6.46's `declares_messages()`/
   `cron_and_queue_triggers()` relocations, just not caught until this slice's own final review of
   `project.rs`'s import list.
2. **`TypeDecl`/`FnDecl`/`Visibility`/`ActorDecl`** — four types `project.rs` only ever received as
   already-built `bynk-check` tables (`compose_unit_symbols`'s `combined_types`/`combined_fns`,
   `collect_unit_methods`'s return, `UnitInfo::exports`, and `bynk_check::actors`'s five seam-resolution
   functions' shared `&HashMap<String, ActorDecl>` parameter) and forwarded — never read a field, never
   matched a variant.

**Decision.**

`ParsedFile::history_target_agent_names(&self) -> impl Iterator<Item = &str>` added to
`bynk-project/src/discovery.rs`, beside `test()`/`declares_messages()`. `collect_history_target_agents`
becomes `parsed.iter().flat_map(|pf| pf.history_target_agent_names()).map(String::from).collect()`.

Four re-exports, each argued against an already-parameterised public API — the P6.27 `ExprId`
precedent (`bynk_check::checker::ExprId`), applied case-by-case per this track's own decision that
cosmetic aliasing of a type `bynk-emit` genuinely *walks* is forbidden, but exposing an existing
dependency its public API already carries is not:

- `pub use bynk_syntax::ast::{FnDecl, TypeDecl, Visibility};` in `bynk_check::project_model` —
  `compose_unit_symbols`'s `combined_types: HashMap<String, Arc<TypeDecl>>`/`combined_fns:
  HashMap<String, Arc<FnDecl>>`, `collect_unit_methods`'s `HashMap<String, Vec<FnDecl>>`, and
  `UnitInfo::exports: HashMap<String, Visibility>` are all already `project_model`'s own return/field
  types.
- `pub use bynk_syntax::ast::ActorDecl;` in `bynk_check::actors` — `bearer_seam_for`/`oidc_seam_for`/
  `signature_seam_for`/`sum_members_for`/`caller_binder_for` all already take `&HashMap<String,
  ActorDecl>` by signature. `project.rs`'s `EmitProjectCtx::actors` field clones and forwards this
  table (P6.34 investigated and declined resolving it earlier — `lower_actor_seam_ir` needs the raw
  declarations to do its own resolution on a security-sensitive path); this re-export changes only the
  import path, not that data flow, and P6.34's decline stands untouched.

Three comments in `project.rs` spelled the literal string `bynk_syntax::ast` (the probe matches
comments too, per its own doc); reworded without changing their meaning
(`` `bynk_syntax::ast` match `` → `raw-AST match`, `` bynk_syntax::ast::CapRef `` → `the raw AST
CapRef`, ×2).

**Consequences.** `ast_importers`: **6 → 5**. `project.rs`'s own `use bynk_syntax::ast::{...}` import
(down to `TypeRef` alone after P6.42–P6.48) is deleted entirely — confirmed live: `grep -rl
bynk_syntax::ast bynk-emit/src` no longer lists `project.rs`. `project/diagnostics.rs` stays cleared
(it rode on `project.rs` back in P6.42, via the super-glob rule, and has nothing of its own to lose).
**No new `AST_IMPORTER_EXCEPTIONS` entry** — `project.rs` cleared on its own evidence, not a probe
exemption; the exclusion list's doc block records this explicitly as proof its four existing entries
are real, earned exclusions rather than a standing habit.

`xtask/src/greenfield_status.rs`'s `ast_importers_excludes_the_named_pairs_but_counts_project_rs` test
renames to `ast_importers_excludes_the_named_pairs_and_project_rs` and flips its own two `project.rs`/
`project/diagnostics.rs` assertions from `contains` to `!contains` — the opposite of what it checked
before, since the fact it pinned is now the opposite fact. The other three probe tests
(`ast_importer_exclusion_is_named_not_prefixed`, `ast_importer_exceptions_still_exist_and_still_import_the_ast`,
`super_glob_children_of_an_ast_importing_parent_are_detected`) are unaffected —
`AST_IMPORTER_EXCEPTIONS` itself did not change. `design/greenfield-status.md`'s `ast_importers` row
updated in the same commit.

**This closes Phase G of the #1137 retirement plan** (`design/tracks/the-ir.md` §6b). Phase H
(emitter conversions, no further probe movement expected) and Phase I (re-settling + retirement)
remain.

Verified: zero-diff bless over the full e2e fixture corpus, with the actor/adapter-adjacent
`185_adapter_given_workers` fixture inspected individually given this slice's own `bynk-check::actors`
touch, `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
