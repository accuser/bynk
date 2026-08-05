---
level: patch
changelog: Settle the content-ownership track's remaining design questions (#1086) and front-load three ADRs — bynk-ide's `ProjectDirs` seam type, R2.3's content-only enumeration scope, and the `bynk-testkit` crate convention
---

## ADR: content-ownership-seam-type
title: `bynk-ide` exposes `ProjectDirs`, a narrow resolved-directories type, instead of `Roots`
summary: The seam `bynk-lsp`'s disk sweep and `AnalysisRoots::lower()` both build from, without re-exporting `bynk-emit`'s `Roots`

**Context.** `bynk-lsp` needs resolved include/exclude directories to do its
own disk sweep for the content-ownership track
(`design/tracks/content-ownership.md`, spine #1086), but deliberately does
not depend on `bynk-emit` (`bynk-lsp/Cargo.toml` depends on `bynk-ide`,
`bynk-check`, `bynk-syntax`, `bynk-fmt` only). `bynk-ide` does depend on
`bynk-emit` and already re-shapes one `bynk-emit` type this way:
`AnalysisRoots` (`bynk-ide/src/lib.rs:192`), whose private `lower()` method
turns it into `bynk_emit::project::Roots`. Re-exporting `Roots` itself
through `bynk-ide` was rejected: `bynk-ide` is itself one of the three crates
`design/greenfield-status.md`'s `fs_below_driver` probe checks, so
re-exporting would relocate the ambient-filesystem violation R2.3 bans, not
close it.

**Decision.** `bynk-ide` gains a new public struct,
`ProjectDirs { project_root: PathBuf, include: Vec<PathBuf>, exclude:
Vec<PathBuf> }`, and a new public method `AnalysisRoots::resolve_dirs(&self,
overlay: &HashMap<PathBuf, String>) -> ProjectDirs`. It mirrors `lower()`'s
match arms but threads `overlay` into
`bynk_emit::project::try_read_project_paths_with(root, overlay)` (already
overlay-aware, already `pub`) instead of the disk-only
`read_project_paths(root)` `lower()` calls today. `bynk-lsp`'s new sweep
calls `AnalysisRoots::Project(root).resolve_dirs(&overlay)` to get resolved
directories, then walks them for `.bynk` files not already covered by the
overlay.

**Consequences.** `bynk-lsp` gains a dependency-light way to build a complete
`(path, content)` map without depending on `bynk-emit`. `AnalysisRoots::lower()`
stays as today's disk-only path for existing callers until a later slice
(content-ownership.md §3.5) migrates it to also take an overlay.
`bynk-lsp/Cargo.toml` gains the dependency-exclusion comment
`bynk-ide/src/lib.rs:31` already (incorrectly) claims exists. `ProjectDirs`
is structurally close to `bynk_emit::project::ProjectPaths` but is a
distinct type, so a future divergence between what `bynk-lsp`'s sweep needs
and what `bynk-emit`'s own project model carries can happen without a
breaking change to either.

## ADR: content-ownership-enumeration-scope
title: R2.3's ambient-filesystem ban is scoped to file contents, not directory enumeration
summary: `discover_bynk_files`'s `fs::read_dir` walk stays below the driver; only `read_source`'s content fallback is in the content-ownership track's scope

**Context.** R2.3 (`design/bynk-greenfield-compiler.md`) says "`Sources` is
constructed once, at the process edge, and is the compiler's only view of
file *contents*." Two functions in `bynk-emit/src/project/discovery.rs` read
the filesystem directly: `read_source` (content, overlay-first with a disk
fallback) and `discover_bynk_files` (a bare `fs::read_dir` walk with no
overlay parameter, finding which `.bynk` files exist). The content-ownership
track (#1086) needed to decide whether R2.3 covers both or content only,
since it determines whether `bynk-emit`'s `fs_below_driver` count can reach 0
or has a permanent, named floor.

**Decision.** Narrow. R2.3's own wording is content-scoped; `discover_bynk_files`'s
enumeration walk is not in this track's scope and stays below the driver.
The alternative — treating `bynk-lsp`'s already-shipped
`didChangeWatchedFiles` watcher (`bynk-lsp/src/lib.rs:1260`,
`lsp-foundations.md` slice E) as a maintained enumeration index instead of an
invalidation signal — was considered and rejected for this track: checked
against its actual implementation (`did_change_watched_files`,
`bynk-lsp/src/lib.rs:3378`), the watcher reacts to file events by
re-scheduling analysis rounds; it does not itself hold an enumerated file
list. Building that index (its own initial-sweep-vs-first-event race, and a
fallback story for a client that doesn't support dynamic watcher
registration) is a real, independently-scoped subsystem, not a side effect
of this track's slice 6.

**Consequences.** `design/greenfield-status-rules.md`'s `fs_below_driver`
probe keeps flagging `bynk-emit=2` (`discovery.rs`'s walk, and `project.rs`'s
`use std::fs;` serving it) after this track's last slice ships, until a
follow-up amends the probe to stop counting pure-enumeration functions —
named here as a real follow-on, not silently absorbed into a false "0". A
future track wanting enumeration-side freshness (an unsaved rename/create
visible without a re-scan) can pick this back up with its own design pass;
this decision does not foreclose it, it just declines to bundle it into
#1086.

## ADR: content-ownership-testkit-crate
title: Cross-crate test fixtures get a new `bynk-testkit` crate, built on production discovery
summary: A dev-only crate over `bynk_ide::discover_files` replaces the ~142-site `diagnose_project(&root, &HashMap::new())`/`CompileOptions::single`/`::split` convention, instead of extending `bynk-emit`'s existing crate-private testkit

**Context.** `bynk-emit/src/lib.rs` already gates a `#[cfg(test)]
pub(crate) mod testkit` (`bynk-emit/src/testkit.rs`) with two helpers used
only by `bynk-emit`'s own tests. The content-ownership track (#1086) needs an
equivalent for ~142 call sites across `bynk-ide`'s inline tests,
`bynk-lsp/tests`, and `bynkc/tests`, that today rely on
`bynk-emit/src/project/discovery.rs`'s disk fallback via
`diagnose_project(&root, &HashMap::new())` or `CompileOptions::single`/`::split`
with no `.sources(...)` chained. Extending `bynk-emit`'s testkit was
considered and rejected: it's `pub(crate)` by design, and `bynk-lsp/tests`
could not reach it without `bynk-lsp` taking a production-excluded
dependency on `bynk-emit` just for tests.

**Decision.** A new dev-only workspace crate, `bynk-testkit`, depending on
`bynk-ide` (and, where needed, `bynk-emit` — unproblematic for a dev-only
crate). Its core helper is built directly on `bynk-ide`'s existing `pub fn
discover_files(roots: &AnalysisRoots) -> Vec<PathBuf>`
(`bynk-ide/src/lib.rs:234`), the same resolution production analysis already
uses — not a second, independent directory walk. `bynk-testkit` becomes a
`[dev-dependencies]` entry of `bynk-ide`, `bynk-lsp`, and `bynkc` (which
already dev-depends on `bynk-ide` for its own integration tests,
`bynkc/Cargo.toml`). Exact helper signatures for the two call-site shapes (a
`HashMap`-returning helper for `diagnose_project*`, a `CompileOptions`-returning
helper for `::single`/`::split`) are proven on a representative handful of
call sites (`design/tracks/content-ownership.md` §4 slice 4) before the full
migration (slice 5), not fixed by this ADR.

**Consequences.** The drift risk `content-ownership.md` names — a test
helper resolving include/exclude differently from `discover_bynk_files` and
silently missing files — is closed structurally: there is only one walk
implementation (`bynk_ide::discover_files`) for both production and test
code to call. `bynk-testkit` ships no production code and is invisible to
`fs_below_driver`'s probe. Once slice 5 completes, `discovery.rs`'s content
fallback (slice 6) has no caller left needing it.
