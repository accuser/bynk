# 0066 — CodeLens: reference counts from the index

- **Status:** Accepted (v0.33)
- **Spec:** `design/karn-lsp-spec.md` §3.17
- **Relates to:** ADR 0053 (the binding index this reads)

## Context
The binding index already records each top-level symbol's definition and every
reference (`SymbolEntry { def, refs }`). A reference-count CodeLens is that count
surfaced above the definition — no new analysis, the cheap half of the
tooling-queue's CodeLens item.

## Decision
`textDocument/codeLens` returns **one reference-count lens per top-level index
definition** in the file (types, free fns, capabilities, services, agents,
providers — the v0.25 index set; locals/methods/fields aren't indexed and get
none). The lens is served from the **cached analysis round** (like references and
tokens), positions converting against the analysed snapshot.

- **The count is `refs.len()`** from a pure `index_queries::code_lenses(index,
  path)` returning `(def site, reference sites)` per definition in `path`, sorted
  by definition position.
- **The action is the standard `editor.action.showReferences`** (args: the def
  URI, the def position, the reference `Location`s) — clicking peeks the
  references with no extension support required. Non-VS-Code clients still render
  the `"{n} reference(s)"` title.
- **Zero is shown** (`"0 references"`) rather than hidden — an unused top-level
  symbol is a dead-code signal, matching rust-analyzer.
- **Computed eagerly** — the count is `O(1)` off the index, so no
  `codeLens/resolve` lazy pass (`resolve_provider: false`).

## Consequences
A small, low-risk lens that falls straight out of the index, sharing the cached
round and the `site_to_location` conversion with references. The deferred half —
**test-run lenses** ("▶ Run") — needs test discovery + a run task/command and is
a separate, larger increment; lenses for non-index kinds (methods/fields/locals)
and implementation/override lenses are later still. Lens visibility is the
client's (`editor.codeLens`).
