# 0052 — LSP project-wide diagnostics: non-bailing, overlay-aware, file-attributed

- **Status:** Accepted (v0.24)
- **Spec:** `design/bynk-lsp-spec.md` §3.2

## Context
The LSP ran single-file `diagnose`, which resolves/checks `Commons` units
only — context files (most real Bynk code) got lex/parse diagnostics and
nothing else. `compile_project` was unusable for live diagnostics: it
reads disk, bails at the first failing phase, and its errors carry no
file identity (`CompileError` has no file; `Span` is a bare per-file byte
pair with no source id — spans from different files collide numerically,
so a span→file map would be unsound).

## Decision
`bynkc::diagnose_project(root, overlay) -> ProjectDiagnostics`:

- **Non-bailing**: `compile_project` was split into a shared pipeline
  with `Mode::Build` (the exact pre-v0.24 CLI contract — bail at the
  structural and pre-emit gates, flat error list) and `Mode::Analyse`
  (never bails after discovery, skips all emission). The per-group skip
  became an error-count **delta**, so one broken unit no longer
  suppresses other units' semantic diagnostics.
- **Collection-point file attribution**: each per-file phase tags errors
  with the file it is processing as they are collected (`ErrorSink`;
  helpers keep plain `&mut Vec<CompileError>` signatures, call sites
  attribute via temp-vecs). Syntax and semantic (resolve/check/context/
  v0.5) errors are precisely attributed; project-level validations
  (group/cycle/directory consistency, tests, platform lock) go to an
  **unattributed bucket** the LSP surfaces on `bynk.toml` — finer
  attribution can follow incrementally. No `Span` change.
- **Overlay**: open buffers (canonicalised absolute path → text) layer
  over disk reads, so unsaved edits are diagnosed.
- **Snapshots**: the result carries each file's **analysed text**;
  positions convert against it, never a newer buffer.
- **Publish-all-and-clear**: the LSP re-publishes every dirty file and
  sends an empty publish for newly-clean (or vanished) files; the diff is
  a pure, unit-tested function. Debounce is generation-counter based
  (200ms). Single-file mode keeps the old path.
- **Rider**: the same attribution gave the CLI proper `ariadne`
  source-context rendering for project errors (`print_project_failure` /
  `ProjectFailure`), fixing the standing bare-lines gap; the plain
  `compile_project*` wrappers still flatten to the pre-v0.24 list.

Test strategy (recorded, not assumed): `bynk-lsp` has no JSON-RPC
harness; this increment unit-tests the publish diff and proves
attribution/overlay/non-bailing at the `bynkc` level (a context-file
handler diagnostic the old path could never produce). The harness is
deferred to the first interactive feature needing round-trip testing.

## Consequences
Context files get real diagnostics; the code-action catalogue and the
slice-2 reference index build on this analysis. Build mode now reports
independent groups' errors past another group's failure (strictly more
information; negative fixtures match by substring and are unaffected).
The deferred span-source-id idea (the provisional 0053) proved unneeded.
