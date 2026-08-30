---
level: patch
changelog: On-keystroke diagnostics no longer re-lex/re-parse every first-party file on every request — `bynk-project` gains a durable `FileId` interning table and one shared, content-keyed parse cache (P8.4), replacing the IDE-local `PROJECT_UNIT_CACHE` completion alone used to benefit from.
---

## ADR: p8-4-durable-parse-cache-expr-id-and-strict-vs-recovery
title: P8.4's shared parse cache — durable `ExprId` allocation, and why it caches the strict parse only
summary: Two forks ADR 0413 didn't examine, found while implementing the cache it specified

**Context.** ADR 0413 settled that phase 8's file level needs one real, `bynk-project`-owned parse
cache — not a `bynk-ide`-local patch — so `bynk_check::analysis::analyse_project`'s own diagnostics
path (which cannot depend on `bynk-ide`) can share it with completion. The issue proposing this
slice (#1515) named three implementation decisions within that settled design (cache scope
`Ast(FileId)` alone; a `path ↔ FileId` interning table with content-keyed invalidation; migrate both
call sites and delete `PROJECT_UNIT_CACHE` in one slice). Implementing it surfaced two further,
genuine forks neither the issue nor ADR 0413 examined — both are about what "durable" and "shared"
actually require once the cache spans separate calls and two different parsers, not just cache
mechanics.

**Decision.**

1. **`ExprId` allocation must also become durable (never reset), not just `FileId`.**
   `parser::parse_units_with_warnings_from`'s own doc comment already names the hazard one level
   up: a multi-file commons merges sibling files' methods into one `check_record` call
   (`collect_unit_methods`), and two independently zero-based files would collide on the same
   `ExprId` in the same `expr_types` map — avoided today by threading one counter across every file
   **within a single `phase_parse` call**. Caching a file's parsed `SourceUnit` **across calls**
   reopens the identical hazard one level up: if a later call serves file A from cache (keeping its
   `ExprId`s from whenever it was last actually parsed) while freshly parsing changed file B from a
   counter that started over at 0, A's and B's `ExprId`s collide in that call's own `expr_types`
   map. Fixed the same way `FileId` itself is fixed: `next_expr_id` lives in the cache's own durable
   state, advanced only on an actual parse (a cache hit consumes no new ids), never reset. Global
   uniqueness across the whole process trivially implies uniqueness within any one call, so this
   strictly strengthens the existing T3.4/R2.4 guarantee rather than replacing it.

   Shares its `u32` space with `bynk_check::project_model`'s own first-party `ExprId` reservation
   (`FIRSTPARTY_ID_BASE = 1_000_000_000`, spaced `FIRSTPARTY_ID_BLOCK = 1_000_000` apart), which was
   sized for a counter that reset every call. A durable counter growing forever could in principle
   reach it — in practice this needs on the order of a billion `ExprId`s consumed over one process's
   lifetime, many orders of magnitude past any real editing session. Noted, not defended against —
   the same "generous headroom, revisit if it ever measurably matters" posture this codebase already
   applies to `PROJECT_UNIT_CACHE_CAP` and to `FIRSTPARTY_ID_BLOCK`'s own spacing.

2. **The shared cache stores the strict parse only** (`parser::parse_units_with_warnings_from`,
   `recover_mode: false`), not completion's recovery-tolerant parse
   (`parser::parse_unit_with_recovery`, `recover_mode: true`). These are genuinely different parser
   configurations, not two entry points over one result: a build must never silently succeed on
   broken syntax by reading a best-effort recovered AST, so the diagnostics/build path needs the
   strict result's real errors. For syntactically clean source — the overwhelming common case — the
   two parsers necessarily produce the identical AST (there is nothing to recover from), so
   completion reads the shared cache directly and gets `PROJECT_UNIT_CACHE`'s old behaviour
   unchanged for every file that parses cleanly. Only when the cached/fresh strict result actually
   carries errors does completion fall back to its own local, uncached recovery-parse for that one
   file — a real, deliberate trade: a project file that currently has a syntax error, while
   completion enumerates it from a *different* buffer under edit, is re-parsed on every request
   instead of cached, in exchange for never caching two different parser configurations' output
   under one key. The buffer actually under the cursor was never served by either cache, before or
   after this slice — it is always parsed fresh, unconditionally, per keystroke, matching #733's own
   original fix.

**Consequences.** `bynk-project::parse_cache` is the one new module (interning table + parse cache);
`bynk-project::discovery::parse_sources` no longer takes `next_expr_id`/`next_file_id` — it resolves
both through the cache internally, which also simplified `bynk_check::project_model::phase_parse`
(no more locally-declared, per-call counters). `bynk-ide::completion`'s `PROJECT_UNIT_CACHE`,
`CachedUnit`, `PROJECT_UNIT_CACHE_CAP` are deleted outright; `cached_project_unit` keeps its name and
call shape but now delegates to the shared cache. A staleness fixture
(`bynk-project/src/parse_cache.rs`'s own test module) proves a rename, a cap-driven eviction, and
concurrent same-path edits from multiple threads all leave the cache in a correct, uncorrupted
state — not just a byte-golden "same input twice" pass. `cargo xtask greenfield-status` now reads
`shared_cache migrated`.
