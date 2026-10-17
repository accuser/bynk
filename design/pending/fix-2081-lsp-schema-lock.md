---
level: patch
changelog: The editor now reports a non-additive event change against the committed `bynk.schema.lock` as `bynk.event.non_additive_schema_change`, as `bynk check` does: the language server reads the lock read-only, caches it by its modification stamp, re-reads it when it changes (a new `**/bynk.schema.lock` file watcher), and passes it to `bynk_ide::diagnose_project_with`, whose new `schema_lock` argument `bynk_check::analysis::analyse_project` now also takes (#2081).
---

## ADR: editor-reconciles-events-against-the-schema-lock
title: The language server reads `bynk.schema.lock` and hands it to the analysis, read-only
summary: The lock travels as a `SchemaLock` argument through `diagnose_project_with` and `analyse_project`; bynk-lsp owns the read and the cache

**Context.** After #2078, `bynkc check`, `bynk check` and `bynkc test` reconcile events against the committed `bynk.schema.lock`, read-only. The editor did not. It analyses through `bynk_ide::diagnose_project_with` and `bynk_check::analysis::analyse_project`, and that path always reconciled against an empty registry. An empty registry can report nothing, so a breaking event change showed no diagnostic until the user ran check or test (#2081). The review note on #1133 had also flagged that empty sweep as a cost paid on every analysis for a diagnostic that could never fire.

**Decision.** The lock travels as an explicit argument. It is not folded into the content overlay, which carries `.bynk` sources and `bynk.toml`.
- `analyse_project`, `analyse_project_with_policy` and `diagnose_project_with` take a `schema_lock: &SchemaLock`. This is ADR 0321's type, so the build, `check` and the editor share the `Off` / `On { existing }` vocabulary. `bynk-ide` re-exports it.
- Under `On`, step 6d mirrors `run_checks`. The lock is parsed (`bynk.project.schema_registry_corrupt` if it is corrupt) and every event is reconciled. The reconciled registry is then discarded. Under `Off`, the sweep is skipped entirely, which closes #1133's cost note.
- `diagnose_project`, the single-tree convenience, keeps its signature and passes `Off`. Every caller of it is a test or a test kit. The language server calls only `diagnose_project_with`.
- `bynk-lsp` owns the disk read, above the driver boundary like its other reads. Its new `schema_lock` module is a per-project `SchemaLockCache` that keeps the last content with the file's `(mtime, len)` stamp, so an unchanged lock costs one `stat` per round.
  - An absent lock is `Off`, not `On { existing: None }`. The two report the same thing, since a fresh project only baselines, but `Off` skips the sweep.
  - An unreadable lock is `Off` with a logged warning and is not cached. An editor round never fails over the lock.
- The server now registers a `**/bynk.schema.lock` watcher. A change clears that project's cache and schedules a round, so a `git checkout` or a `bynkc compile` shows up without an edit. A client without dynamic watchers still sees a changed lock on its next round, because the stamp moved.
- Only the diagnostics round passes the cached lock. The completion fast path's receiver-typing re-analysis passes `Off`, since it reads only expression types, which the lock cannot change. Rename validation passes `Off` too: lock diagnostics are project-level and neither validator compares project-level diagnostics, so a rename that changes an event non-additively is reported by the next round on `bynk.toml`, not refused.
- Nothing in the editor writes the lock.

**Consequences.** The editor shows `bynk.event.non_additive_schema_change`, `bynk.event.schema_version_mismatch` and `bynk.project.schema_registry_corrupt` when the build would. Like every project-level diagnostic with no single owning file, they anchor at the start of `bynk.toml`. Projects with no lock, and analyses whose caller passes `Off`, no longer pay for an event sweep. Every caller of `analyse_project` and `diagnose_project_with` passes a lock explicitly, and the tests pass `Off`. No gated probe moves: the disk read sits in `bynk-lsp`, outside `fs_below_driver`'s crates.
