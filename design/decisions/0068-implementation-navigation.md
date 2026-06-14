# 0068 — Implementation navigation from a provides-clause impl graph

- **Status:** Accepted (v0.35)
- **Spec:** `design/karn-lsp-spec.md` §3.19
- **Relates to:** ADR 0053 (the binding index), ADR 0067 (the owner resolution this reuses)

## Context
`given Cap` / `provides Cap = Provider` is Karn's interface/impl seam. The
natural navigation is `textDocument/implementation` on a capability → the
provider(s) implementing it. The capability→provider mapping lives in
`UnitTable.providers`, which the project pass computes and then discards — but
exporting it into the cached analysis is **unnecessary**: every `provides Cap`
clause already records a `Capability` reference whose enclosing `owner` is the
provider, and ADR 0067 added the `owner_keys` map that resolves an owner to its
`SymbolKey`.

## Decision
Surface implementation nav as an **`ImplEdge { capability, provider, site }`
side table** on `ProjectIndex`, built in `IndexBuilder::build` alongside the
v0.34 `CallEdge` push — the same owner resolution, the symmetric structure.

- **A `provides`-flagged capability ref is the edge.** A provider may also
  declare `given Cap2` (its own deps), which is *also* a `Capability` reference
  owned by the same provider — so the owner alone can't tell "implements" from
  "depends on". The `provides Cap` clause therefore records its capability ref
  with a **`provides: bool` flag** on the edge (`record_provides`); only flagged
  edges whose owner resolves to a `Provider` become `ImplEdge`s. The ref is
  still a normal capability reference (it stays in the capability's `refs`).
- **`implementation` resolves a capability symbol → its providers' defs.** The
  symbol under the cursor (declaration, `given Cap` use, or `provides Cap` use)
  must be a `Capability`; the query returns every implementing provider's
  definition site (the provider is an index symbol — the edge only names it),
  sorted by position. Covers "given Cap → provider" and "capability → providers"
  uniformly. A non-capability symbol returns `None`.
- **The reverse is goto-definition.** Provider → its capability is already
  go-to-def on the `provides Cap` name (an existing index ref → the capability
  def); not re-plumbed under `implementation`.
- **External providers are included.** An `external` provider still has a Karn
  `provides Cap = Name { external }` declaration that is an index symbol;
  navigation lands on that declaration, never the `.binding.ts` (off-tree).
- **Cross-context works by construction.** A `provides` for a consumed
  capability records `record_provides` into the defining unit, so the edge links
  the capability's key (defining unit) to the provider's key (providing unit).
- **Pure query, thin transport.** `index_queries::implementations(index, key)`
  over `ProjectIndex.impls`, served from the cached round; `goto_implementation`
  only converts positions and gates on `SymbolKind::Capability`.

## Consequences
Implementation nav falls out of the v0.34 owner machinery — a flag on the
provides-clause ref, an `ImplEdge` vector, one accessor, one query, one handler;
no analysis export. The graph reflects the last clean round (§3.2). The other
half of A-3 — `textDocument/typeDefinition` (value→type, and consumed-context →
source) — is deferred: context units aren't index symbols, so context-source nav
needs a unit→file map, a separate data source.
