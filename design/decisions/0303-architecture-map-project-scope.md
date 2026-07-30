# 0303 — The architecture map covers the active file's project, resolved the same way as the extension's own project lookup

- **Status:** Accepted (v0.245)

**Context.** A workspace may hold several Bynk projects. Rendering every
project superimposed in one diagram produces an unreadable tangle with no
project boundary drawn between them.

**Decision.** `bynk/architectureModel`'s params are a bare `textDocument` —
used only to resolve *which* project's committed round to read (the same
`committed_analysis` gate every pull-based request already uses), never to
restrict the result to that one file. The VS Code command requires an active
`.bynk` editor for the same reason the sequence/documentation commands do:
resolving a project needs a file to walk up from.

**Consequences.** A workspace with several open Bynk projects shows one map
per invocation, scoped to whichever project the active editor belongs to. A
multi-project picker (distinct from this scoping question) is left as a
follow-up; the command's `when` clause stays `editorLangId == bynk`,
consistent with the sequence/documentation commands, rather than introducing
a new `bynk.hasProject` context key this increment does not otherwise need.
