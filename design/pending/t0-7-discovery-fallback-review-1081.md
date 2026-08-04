---
level: patch
changelog: "`bynk-driver`'s CLI-path project discovery (#1081) no longer silently skips the `no_sources`/`file_and_directory` project checks, panics on a missing `include` root, or produces nondeterministic build/diagnostic order"
---

Addresses the review on #1081 (#1077/T0.7 residue: `project_options`/
`try_project_options` populating `CompileOptions.sources` from a driver-side
walk instead of relying on `bynk-emit`'s own fs:: fallback).

The review found the change was not behaviour-neutral the way the PR
described:

1. **Two project-level checks stopped running.** `run_checks` skips
   `phase_discovery` whenever `discovered` is `Some` — which a
   `sources`-driven compile always is — and `bynk.project.no_sources` and
   `check_file_directory_conflicts` (`bynk.project.file_and_directory`) lived
   only inside that phase. `bynk-emit/src/project.rs`'s `phase_discovery` is
   now the on-disk walk alone; both checks moved into a new
   `check_discovered_files`, which `run_checks` calls unconditionally on
   whichever `(src_files, tests_files)` it ends up with — a real walk or a
   caller-supplied `discovered` list. Covered by two new
   `bynk-driver/tests/project_diagnostics.rs` cases that drive the real CLI
   path (`project_options` → `check_project`) against an empty project and a
   file/directory ambiguity.
2. **A missing `include` root panicked instead of erroring.**
   `bynk-driver/src/discovery.rs`'s `read_bynk_tree` (and
   `read_bynk_tree_single`/`read_bynk_tree_split`) now return
   `Result<_, DiscoveryError>` instead of `panic!`ing on any I/O failure;
   `project_options`/`try_project_options` thread it through (a new
   `ProjectOptionsError` wraps it alongside `ProjectPathsError` for the
   `try_` variant). A missing project tree is a normal, recoverable CLI input
   again, not a crash.
3. **Build order was nondeterministic.** `sources`'s own key order is a
   `HashMap`'s — unspecified, randomised per process — but `phase_parse`
   assigns sequential `FileId`s/`ExprId`s (embedded in emitted spans and
   source maps) by walking `src_files`/`tests_files` in order, and
   diagnostics print in push order. `bynk-emit`'s `sources_to_discovered`
   (shared by `compile_project`/`check_project`) now sorts both partitions,
   restoring the ordering guarantee a real disk walk always had via
   `discover_bynk_files`'s own `out.sort()`.

Also: `Roots::resolve`/`Roots::excludes` are now `pub`, and
`bynk-driver::discovery::sources_for_roots` calls them directly instead of
re-implementing the same root/exclude math (hardcoded cache list included) a
second time — one definition of "which files are in this project," so the
CLI and the LSP can no longer silently diverge on it. `discovery.rs` gained
unit test coverage over the behaviours that differ from the code it
replaced: excludes, hidden-directory skipping, a missing root, an empty
tree, and the `include`-length 0/1/2 branches `Roots::Split::resolve` takes.

`fs_below_driver` is unaffected either way — still 5, as #1081 itself
already established (that count doesn't move until #1079 lands).
