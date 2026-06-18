# 0095 — The unit→source map: analysis exposes which files comprise each unit

- **Status:** Accepted (LSP tooling track, slice 6b)
- **Spec:** `design/karn-lsp-spec.md` §3.21
- **Realises:** the LSP tooling track (`design/tracks/lsp.md`), slice 6b; the unit→file map ADR 0068 flagged as deferred surface for context-source navigation.

## Context

`uses B` / `consumes B` name a *unit*, not a symbol. To make those names
navigable — clickable document links (slice 6b), and the consumed-context half of
go-to-type-definition that ADR 0068 deferred — the LSP must resolve a qualified
unit name (`billing.charge`, `karn.list`) to the file(s) that declare it. The
binding index can't help: it indexes **user symbols** (types, fns, capabilities,
…), and units deliberately aren't symbols. ADR 0068 named this exact gap — "context
units aren't index symbols, so context-source nav needs a unit→file map, a
separate data source" — and deferred the feature for it.

The data already exists at analysis time. `analyse_project` parses every project
file into a `ParsedFile { source_path, unit, synthetic, … }`, and the checker's
`groups: HashMap<String, Vec<usize>>` already maps each unit name to its file
indices. It is simply not exposed: `ProjectDiagnostics` carries `files`, `index`,
`expr_types`, `hints`, `locals` — but no unit→file mapping. This ADR adds it.

## Decision

**The project analysis exposes `unit_sources: HashMap<String, Vec<PathBuf>>` — a
qualified unit name to the project source file(s) that comprise it** (a unit may
span files; entries are in discovery order). It is a new public field on
`ProjectAnalysis` and `ProjectDiagnostics`, a sibling of `index`/`expr_types`,
threaded to the LSP `Analysis` and cached like the rest.

- **Project units only.** Built from the parsed files, **excluding `synthetic`
  ones** — the toolchain-injected `karn`/`karn.cloudflare` surface is embedded via
  `include_str!`, not an on-disk file the editor can open. So the map resolves
  only *openable* units; a `uses karn.list` resolves to nothing, by design.
- **Built on the structurally-analysed path.** The map is populated whenever the
  project reaches the checker (`RunChecks::Checked`) — which **includes
  type-error projects** (per-unit checks collect errors without bailing). It is
  empty only when discovery/parse itself fails (`RunChecks::Bailed`), where there
  is no parsed tree to map. Graceful degradation, consistent with the other
  analysis-derived features (locals, hints, expr_types).
- **A map, not an index kind.** Unlike `CallEdge`/`ImplEdge`, this is not a
  relationship graph over symbols — it is a flat name→path lookup, so it lives as
  a plain field, not on `ProjectIndex`. Consumers do their own span detection
  (parse the open document for `uses`/`consumes` ranges) and use the map only for
  target resolution.

## Consequences

The map is the **shared enabler** for two features: document links (slice 6b, its
first consumer — `uses`/`consumes` names underlined and clickable to the unit's
primary source file) and the consumed-context navigation half of slice 6a
(`uses B` → B's source via go-to-definition), which can now follow without
further surface. Both reduce to "detect the name span, look the unit up, point at
its first source file."

Costs are small and bounded: one `HashMap` entry per project unit, populated in a
single pass over the already-parsed files. A unit that spans several files keeps
all of them (document links target the first; future consumers may offer the
rest). The deliberate gap — first-party `uses` don't resolve — is acceptable:
their sources aren't files a developer can open, and their *symbols* already
surface through hover/completion (slice 9). A parse-broken project loses the map
until it parses again, the same ceiling the rest of the analysis carries.
