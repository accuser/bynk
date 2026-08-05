---
level: patch
changelog: Slice 0+1 of the content-ownership track (#1086) ships — `bynk-lsp`'s completion, signature help, and hover no longer read project files from disk themselves; ADR 0322's `ProjectDirs`/`resolve_dirs` design is superseded, unneeded once implementation found `bynk_ide::discover_files` already closes the gap
---

## ADR: content-ownership-seam-simplification
title: ADR 0322 is superseded — no `ProjectDirs`/`resolve_dirs`; `bynk_ide::discover_files` already closes the seam
summary: Implementation found the seam ADR 0322 designed already exists as a public, already-used function; slice 0 merges with slice 1 since a scaffolding-only slice has no legitimate caller here

**Context.** [ADR 0322](0322-content-ownership-seam-type.md) designed a new
`bynk-ide` type, `ProjectDirs`, and a new method,
`AnalysisRoots::resolve_dirs`, so `bynk-lsp` could resolve a project's
include/exclude directories for its own disk sweep without depending on
`bynk-emit`. Implementing content-ownership.md's slice 0 (sub-issue #1089)
found this unnecessary: `bynk-ide` already exposes `pub fn
discover_files(roots: &AnalysisRoots) -> Vec<PathBuf>`
(`bynk-ide/src/lib.rs:234`), which resolves `include`/`exclude` and
enumerates a project's `.bynk` files with no `bynk-emit` type crossing into
`bynk-lsp` — and is already in production use, by `Backend::project_files`
(`bynk-lsp/src/lib.rs`), which today hands the resulting bare paths to
`bynk-ide`'s completion/signature-help/hover code for THEM to read from disk.
`bynk-lsp` never needed *directories* — it needed *files*, and the function
that already gives it that list was sitting unused for this purpose.

Slice 0 as ADR 0322 scoped it — land the seam, exercised only by its own
direct test, "no caller changes yet" — turned out not to be independently
shippable in this repository: `cargo clippy --workspace --all-targets -- -D
warnings` (the CI-required lint gate) fails `dead_code` on a real,
`pub(crate)` function with no call site, module-privacy and `pub`-ness
notwithstanding. The one place a bare `bynk_ide::discover_files` result
already flows into is `Backend::project_files`'s own consumers — completion
(`bynk-ide/src/completion.rs`'s `for_each_unit`/`cached_project_unit`) and
signature help (`resolve_label`) — precisely slice 1's scope, not a new,
independently-useful caller for slice 0 alone. Wiring the sweep into
`Backend::run_project_diagnostics` instead (a plausible-looking alternative
caller) was considered and rejected: that path's `diagnose_project_with`
already receives the open-buffer overlay and is already correctly
overlay-aware via `bynk-emit`'s own `read_source` fallback — pre-sweeping its
content would only add a second, redundant directory walk (`discover_files`
inside the sweep, then `bynk-emit`'s own `discover_project_files` again
inside `analyse_project_with`) for no freshness gain, since the staleness bug
this track exists to fix (`cached_project_unit`'s own untracked disk read)
never lived on that path at all.

**Decision.** No `ProjectDirs`/`resolve_dirs` are built. `bynk-ide` gains no
new public type for this. Slice 0 and slice 1 ship as one combined slice:
`bynk-lsp` gains `content.rs`'s `sweep_project_content` (built directly on
the existing `bynk_ide::discover_files`) and `Backend::project_content`
(overlay-then-sweep, mirroring `project_files`'s existing shape and its
finding-#62 current-file exclusion); `bynk-ide/src/completion.rs`'s
`for_each_unit` and `cached_project_unit` become content-supplying (the
per-file parse cache is now keyed by content equality, not disk mtime/len —
correctness-positive, since supplied content, e.g. an open buffer's unsaved
edit, has no meaningful mtime at all) and every `files: Option<&[PathBuf]>`
signature in `completion.rs`/`signature_help.rs` (thread-through parameters
only, no intermediate function inspected the value) becomes `Option<&HashMap<PathBuf,
String>>`. `Backend::project_files` (bare paths) is untouched and still
backs the still-path-based cross-file symbol lookups
(`find_declaration_cross_file`/`describe_symbol_cross_file`,
`bynk-ide/src/symbols.rs`) — that migration stays its own later slice
(renumbered slice 2 in `design/tracks/content-ownership.md` §4, unchanged
in substance).

**Consequences.** `design/tracks/content-ownership.md` §4's slice list
merges its old slices 0 and 1 into one ("slice 0" from here); slices 2–6
keep their content unchanged, renumbering down by one. §3.1's `ProjectDirs`
decision is corrected in place (the doc records the correction rather than
silently editing away what was actually decided, the same way this ADR does
not edit ADR 0322). An unsaved edit to file A is now visible from file B's
completion, signature help, and hover (rung 8) — the first concrete instance
of `design/tracks/content-ownership.md` §8's "done when" behaviour, ahead of
`fs_below_driver` reaching 0 (still slices 2 and 6's job). `ADR 0322` is not
edited (decisions are immutable once accepted, `design/decisions/README.md`)
but is superseded: its file and `design/decisions/README.md`'s index row get
a pointer to this ADR's assigned number in a small follow-up edit once the
stamp assigns it, mirroring how ADR 0315 handled superseding ADR 0313.
