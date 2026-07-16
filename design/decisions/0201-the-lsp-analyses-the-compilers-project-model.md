# 0201 — The LSP analyses the compiler's project model

- **Status:** Accepted (v0.178)
- **Provenance:** proposed in #647, a slice of the LSP foundations work (spine
  #640). Depends on [[0198]], which gave a multi-root project's files an
  unambiguous identity — without it this change would have collided two files
  onto one key. Supersedes that proposal's `[DECISION B]`, which [[0198]]
  settled instead.
- **Relates:** [[0147]] (structural test-ness and the flat `include`/`exclude`
  layout — the manifest this honours), [[0198]] (file identity — the
  prerequisite), [[0052]] (project diagnostics — the round this re-roots),
  [[0156]] (the editor surface tracks the language).

## Context

`bynkc` and the language server read the same `bynk.toml` and disagreed about
what it meant.

The compiler resolves `CompileOptions::split(input, read_project_paths(input))`
(`bynk-driver`), honouring every `[paths] include` tree, `exclude`, and its own
`out`/`node_modules` caches. The server did this instead
(`bynk-lsp/src/project.rs`, before this record):

```rust
// The primary `include` tree is the source root used for cross-file
// lookups (defaults to `src`).
src_dir: raw.paths.include.first().cloned().unwrap_or_else(|| "src".into()),
```

One string. First entry only. `exclude` was parsed and thrown away — the field's
own comment said so: *"the LSP's analyse walk does not yet prune by `exclude`
(the compiler's discovery does)"*. The server then passed `<root>/src` to the
analyser **as if it were a project root**, so `analyse_project` looked for
`<root>/src/bynk.toml`, found none, and defaulted again.

Four consequences, each confirmed:

| Consequence | Cause |
|---|---|
| A secondary `include` tree is invisible | `include[1]` — precisely `Roots::resolve`'s secondary — dropped |
| `exclude`d and generated sources analysed | no excludes passed; `out`/`node_modules` not skipped |
| A manifest-backed **flat** project fails | server defaults to the string `"src"`; the compiler defaults to `conventional()` |
| Diagnostics, references, rename, symbols and completion disagree with `bynkc` | different file sets, by construction |

`examples/todo` is the proof and it is in this repository: its `bynk.toml`
declares no `[paths]`, so `conventional()` yields `include = ["src", "tests"]` —
and `examples/todo/tests/todos.bynk`, a real `suite` the compiler compiles, was
invisible to the server.

Completion had the same defect one layer down: `for_each_unit` walked a single
directory with a hand-rolled `walk_bynk_files` that knew nothing of `exclude`.
Two re-derivations of one manifest, both lesser than the original.

## Decision

**(A) `bynk-ide` owns an `AnalysisRoots` type; `diagnose_project` stays the
single-tree convenience.** The analysis API grows
`diagnose_project_with(&AnalysisRoots, overlay)`; `AnalysisRoots::Project(root)`
reads the manifest exactly as `bynk-driver` does, `AnalysisRoots::SingleTree(root)`
walks one tree and consults nothing.

`bynk-ide` owns the type rather than re-exporting `bynk_emit::project::Roots`:
this crate is the IDE-facing published surface, and `Roots` carries
`tests_prefix` semantics an IDE caller has no business knowing. The lowering is
five lines, and it is the seam where the server's needs and the compiler's can
diverge later without a break.

**The convenience is not politeness — it is why this is additive.**
`diagnose_project` has ~50 call sites across three crates and is public API of
**both** `bynk-ide` and `bynkc`. Almost all of them hand in a fixture root and
mean exactly one tree. Changing its signature would have churned them all and
broken `bynkc`'s API for no gain. Instead only the server's three round sites
move, and **nothing breaks**.

*Consequence:* two entry points where one might look sufficient. The proposal's
own count said "three callers" and was wrong by an order of magnitude; measuring
it is what produced this shape.

**(B) The server stops reading `[paths]` at all.** `ProjectConfig` no longer
carries a source root — it carries formatting and the diagnostics mode, which
are what the *server* owns. The trees are the *compiler's* to resolve.

This is the actual repair. A "fix" that taught `bynk-lsp/src/project.rs` to read
both `include` entries would have left two implementations of one manifest,
agreeing until the next `[paths]` feature. Deleting the reduction is what makes
the two tools structurally incapable of disagreeing.

*Consequence:* `project_name`, `project_version` and `out_dir` went too — all
three had zero readers, kept alive by `#[allow(dead_code)]`.

**(C) The server mirrors the driver's rooting, and gets it for free.**
`bynk-driver` takes the split path when a `bynk.toml` exists **or** `src/` is a
directory. `Backend::resolve_root` enters project mode on exactly those two
conditions (a manifest, else the nearest ancestor `src/`), so
`AnalysisRoots::Project` agrees with `project_options` by construction rather
than by a second copy of the predicate.

**(D) Completion consumes the compiler's discovery too.** `bynk_ide::discover_files`
exposes the same walk `compile_project` performs, and the threaded
`src_root: Option<&Path>` becomes `files: Option<&[PathBuf]>` — the discovered
set, not a directory to re-walk. `walk_bynk_files` survives only for tests that
enumerate a fixture tree directly.

Leaving completion on its own walk would have kept the headline defect fixed and
the same bug alive in the surface a user touches most. It would also have become
*worse*: pointed at the true project root instead of `<root>/src`, an
exclude-blind walk would sweep `out/` and `node_modules/` straight into the
candidate list.

## Consequences

- The server analyses **exactly** the files `bynkc` compiles. `examples/todo`'s
  `tests/` tree resolves; a flat project works; `exclude` excludes.
- **Every capability widens in coverage without changing in behaviour** — hover,
  completion, semantic tokens and signature help ([[0156]]) all now see files in
  secondary `include` roots that were previously not in the round at all. None
  of the four changes what it *does*.
- Identity is project-relative ([[0198]]), so `Analysis.src_root` becomes
  `project_root` and `uri_to_rel` is one `strip_prefix` that is **total** across
  `include` trees, where the old `src` base could only ever name files in one.
- `bynk-ide` and `bynkc` gain API (`AnalysisRoots`, `diagnose_project_with`,
  `discover_files`); no existing signature changes.
- This crate's **first behaviour-over-time tests**: three in-crate cases drive a
  real `Backend` through `LspService::new(Backend::new)` and assert on a round's
  contents. The track doc's §4.1 notes this was always possible and never done;
  the harness needed no `[lib]` target and no refactor. The sibling-reading
  fixtures live in `tests/project_model.rs`, on `Cargo.toml`'s `exclude` list —
  because `bynk-lsp` is published and an in-crate test cannot be excluded from
  its own package.
- Every fixture is mutation-checked. Reverting the lowering to
  `Roots::Single(root.join("src"))` reproduces the original defect exactly —
  `["thing.bynk"]` where both trees were expected, `[]` for a flat project. On
  this track's own evidence, a green suite proves nothing about a file set
  unless something asserts the set.
