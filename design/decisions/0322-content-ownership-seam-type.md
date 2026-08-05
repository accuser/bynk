# 0322 — `bynk-ide` exposes `ProjectDirs`, a narrow resolved-directories type, instead of `Roots`

- **Status:** Accepted (v0.247.13). **Superseded by
  [ADR 0325](0325-content-ownership-seam-simplification.md)** (v0.247.14):
  implementing content-ownership.md's slice 0 found this type unnecessary —
  `bynk-ide` already exposed `pub fn discover_files(roots: &AnalysisRoots) ->
  Vec<PathBuf>`, already in production use by `Backend::project_files`, which
  closes the gap this decision built a new type for. No `ProjectDirs`/
  `resolve_dirs` were built.

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
