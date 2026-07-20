# 0239 — LSP file-rename awareness — reusing the compiler's own path↔name rule instead of `unit_sources`

- **Status:** Accepted (v0.213)

**Context.** `bynk-lsp` declared `workspace.file_operations: None`, so renaming or
moving a `.bynk` file in the editor silently broke every `uses`/`consumes`
reference pointing at the old unit name — the spec had already anticipated this
gap (`design/bynk-lsp-spec.md` §3.9, calling it "the A-3 file-operations
increment"). The hard part was already built: ADR 0095's `unit_sources`
(qualified unit name → its source files) and `symbols::unit_reference_spans`
(the `uses`/`consumes` reference spans in a buffer), both already backing
`documentLink` and go-to-definition.

**Decision.** Don't reverse-scan `unit_sources` (name → paths) to find "which
unit does this old path belong to." A unit's `unit_sources` entry conflates its
own file with any `suite` file that tests it — a suite's `SourceUnit::name()`
returns its *target*'s name, so `unit_sources` files the suite under the
target's key too. Using that list's length to guess single- vs multi-file
would misclassify any single-file unit that happens to have a suite.

Instead, parse the *old* file's own snapshot directly to get its own declared
name (`symbols::own_declaration_name`, new — `None` for a `SourceUnit::Suite`,
which has no name of its own, so a suite rename naturally produces no edits).
Then derive the *new* name the moved path implies from the compiler's own
single-file/multi-file arrangement rule (`check_path_name_alignment`), exposed
as `bynk_ide::renamed_unit_name` → `bynk_emit::project::renamed_unit_name` →
the promoted `bynk-emit/src/project/paths.rs` logic — one source of truth for
path↔name, instead of a second copy of `stem_parts`/`is_multi_file_parts` living
in the LSP crate.

`renamed_unit_name` matches the old name as a **suffix** of the old path's stem
(or parent-directory, for the multi-file arrangement), not the whole thing: the
LSP passes project-relative paths (ADR 0198), which — unlike the root-stripped
`source_path` `unit_path_matches` itself is checked against — still carry a
split project's `src`/`tests` root segment. Whatever prefix length the suffix
match implies for the old path is applied unchanged to the new path, correct as
long as the rename stays under the same `include` root.

Renaming one member file within a multi-file unit's directory resolves to the
same name — the qualified name is the directory, not the filename — so it is a
no-op by construction, without needing to special-case multi-file units at all.

When the name does change, the edit set is the moved file's own declaration
header (targeting its **old** URI, since the client applies the returned
`WorkspaceEdit` before performing the physical rename) plus every other
project file's matching reference spans. The handler is gated by
`analysis_covering_open_buffers` — the same whole-project freshness `rename`
uses — because it emits the same kind of multi-file versioned edit, and a
stale open buffer would otherwise carry a version the client rejects; this is
a different call than `documentLink`'s `committed_analysis`, whose read-only
decoration output tolerates a round lagging by one debounce cycle. The handler
never refuses: a filesystem rename isn't something this edit-only hook can
block, so anything it can't confidently resolve — an unparseable file, a
suite, a no-op rename, a cross-project move — is skipped rather than erroring
the whole batch.

Scope is deliberately single-file: the capability filter matches
`FileOperationPatternKind::File`, not folders, so a directory move is left for
a follow-up.

**Consequences.** Renaming or moving a `.bynk` file in an editor that supports
`willRenameFiles` (VS Code among them) keeps the project compiling instead of
leaving every importer pointing at a name that no longer exists. The
`renamed_unit_name` promotion also gives any future caller (not just the LSP) a
single, tested function for "what would this path's arrangement call itself if
moved here" — previously that logic existed only as the boolean predicate
`unit_path_matches`.
