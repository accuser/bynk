# 0198 — File identity is not the unit-validation path

- **Status:** Accepted (v0.175)
- **Provenance:** proposed in #650, the first slice of the LSP foundations work
  (spine #640). Found while implementing #647, which it unblocks. The defect it
  fixes predates both and lives in the compiler, not the language server.
- **Relates:** [[0147]] (structural test-ness and the flat `include`/`exclude`
  layout — the ADR that made two roots possible, and whose structural test-ness
  rules out the cheap repair), [[0052]] (project diagnostics — the consumer that
  maps by the ambiguous key), [[0023]] (each increment stays single-purpose).

## Context

[[0147]] replaced the role-named `src`/`tests` manifest keys with a flat
`include` list. The moment `include` could hold two entries, a project's files
stopped having unique names — and nothing noticed for sixty increments.

`parse_tree` (`bynk-emit/src/project.rs`) computes each file's `source_path` by
stripping **its own tree's root**, and it is called once per `include` tree:

```rust
let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
snapshots.push((rel.clone(), source.clone()));
```

So `src/todos.bynk` and `tests/todos.bynk` both become `todos.bynk`. Measured on
the repo's own `examples/todo` before this record:

```
include = ["src", "tests"]  exclude = []
SNAPSHOT KEYS = ["todos.bynk", "todos.bynk"]
count=2 unique=1
```

`Roots::tests_prefix` does **not** address this. It is applied downstream, in
`tests_emit.rs`, to build an emitted test's reported `path:line:col` — it never
reaches `snapshots` or the diagnostic attribution.

**Why nothing caught it.** Two reasons, both worth recording because they are the
actual mechanism:

1. **`ProjectAnalysis.snapshots`' doc comment claimed `(project-relative source
   path, analysed text)`.** It was not project-relative. The comment described
   the intent; a reader checking the invariant read the comment and moved on.
2. **No fixture asserts an attributed path.** `expected_error.txt` lists
   *category strings only*. The e2e suite has 331 negative fixtures and not one
   of them can observe which file a diagnostic was blamed on — so the identity
   could be wrong for every split project and every test would still pass. It
   did, and they did.

The consumer that would have noticed is the one that *maps* by the key rather
than iterating: `bynk-ide::diagnose_project` does `by_file.remove(&source_path)`
per snapshot, so with a duplicate key the first file drains **both** files'
diagnostics and the second silently gets none. That consumer was never handed a
split project — the language server reduced the manifest to a single directory
([[0052]]'s analysis entry point takes one root) — so the defect stayed latent
exactly as long as the LSP stayed wrong in a compensating way.

## Decision

**(A) `source_path` and identity are two fields, not one.** `ParsedFile` gains
`identity_path` — the **project-relative** path — beside `source_path`, which
stays **relative to its `include` root**. Everything that *keys* a file (the
analysed snapshots, the diagnostic attribution) reads `identity_path`; the thing
that *validates a unit's name* keeps reading `source_path`.

The alternatives were considered and both fail against a legal manifest:

- **Make it project-relative everywhere.** `check_path_name_alignment`
  (`bynk-emit/src/project/consistency.rs`) requires the tree-relative form:
  `src/todos.bynk` may declare `context todos` precisely because its
  `source_path` is `todos.bynk`. Project-relative gives `["src","todos"]` against
  name parts `["todos"]`, and **every unit in every project** fails alignment.
- **Prefix only the secondary tree.** `check_path_name_alignment` exempts
  `UnitKind::Test | Integration`, so this works for a `tests/` tree of suites —
  and breaks the moment `include[1]` holds an ordinary unit. [[0147]] made
  test-ness **structural, not directory-based**; a `context` in the second root
  is a legal program, and this repair would reject it. Correct for the example
  in front of us, wrong for the layout the ADR deliberately permits.

The two roles were never the same thing. They coincided while there was one
root, which is why one field sufficed and why the conflation was invisible.

*Consequence:* a third path form joins `abs_path` (the source-map entry). Three
is the honest count — a file has an absolute location, a name within its tree,
and a name within its project — and collapsing any two of them is what produced
this defect.

**(B) The prefix is empty for a single root, so single-root behaviour is
unchanged by construction.** `Roots::src_prefix()` joins the primary tree's
`include` entry onto its files' identity, mirroring the existing
`tests_prefix()`. `Roots::Single` resolves to `(root, root)` with **empty**
prefixes, and an empty prefix is a join identity — `"".join(rel) == rel`.

This is not a courtesy to existing callers; it is why the change is small enough
to be one slice. Every one of the ~50 `diagnose_project`/`analyse_project` call
sites is single-tree, so none of them moves, and the entire fixture suite is
untouched — verified, not assumed: the full workspace gate passes 1073 tests with
**zero** golden or `expected_error.txt` churn.

*Consequence:* the churn the proposal budgeted for did not materialise, and that
is a finding rather than a relief — it means no existing fixture exercised a
split project with a diagnostic. The coverage hole and the defect have the same
shape.

**(C) Unit naming is not reconsidered.** It is tempting to ask whether
`check_path_name_alignment` should read the project-relative form too — whether
`context todos` in `tests/` should even be legal. That is a language-surface
question, it would break every project, and identity does not need it. Declined
explicitly so the next reader knows it was weighed, not missed.

## Consequences

- A diagnostic in a secondary-root file is attributed to that file. Before this,
  a consumer keying by the attributed path folded it into the primary root's
  namesake and one file's diagnostics vanished.
- **Split projects' reported paths move**: `todos.bynk` becomes `src/todos.bynk`.
  No fixture asserted these, so nothing in the tree changed — but a user with two
  `include` roots will see it, which is why it is in the changelog. It is a fix:
  the path now resolves from the project root, agreeing with what `tests_emit`
  already produces for emitted tests.
- Single-root projects — every fixture, and every project with one `include`
  entry — are byte-identical.
- `bynk-ide` and `bynk-lsp` are untouched. The LSP still analyses one directory;
  this record only makes it *possible* for it to stop, which is #647's work.
- The tests for this live in-crate (`bynk-emit`'s `#[cfg(test)] mod tests`),
  because the e2e fixture suite structurally cannot express them: it asserts
  categories, never paths. A fixture format that cannot observe attribution is
  worth revisiting on its own; not here.
