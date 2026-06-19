# 0055 — `workspace/symbol` and `documentHighlight` are `ProjectIndex` queries

- **Status:** Accepted (v0.26)
- **Spec:** `design/bynk-lsp-spec.md` §3.11, §3.12

## Context
v0.25's binding index (ADR 0053) holds every in-scope symbol's
definition and reference sites, binding-correct. Two small LSP surfaces
fall nearly free out of it: project-wide symbol search enumerates the
definitions; in-file occurrence highlighting is the references query,
file-scoped. Neither touches `bynkc`.

## Decision
Both ship as **pure `index_queries` reads** alongside references/rename:

- **`workspace/symbol`** enumerates index definitions filtered by a
  case-insensitive substring match on the name (empty query lists all),
  ordered by (name, unit), the owning unit as the container name.
- **`textDocument/documentHighlight`** returns the symbol-at-cursor's
  sites within the active file, definition included. The index does not
  distinguish read from write references, so highlight **`kind` is
  omitted** rather than guessed.

Coverage and limits are exactly the index's (ADR 0053): top-level
types, fns, capabilities, services, agents, providers; the deferred
kinds (locals, methods, fields, op names) return nothing until the
index grows them. Positions convert against the analysed snapshots.

## Consequences
Two editor surfaces (Cmd-T symbol search, occurrence highlighting) for
two small query functions and two capability advertisements — and both
inherit every future index improvement (the deferred kinds, methods,
fields) with no further LSP work.
