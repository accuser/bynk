# 0053 — The LSP binding index: reference-table sink, references & rename

- **Status:** Accepted (v0.25)
- **Spec:** `design/bynk-lsp-spec.md` §3.8, §3.9

## Context
The LSP's `definition` and `hover` were name-string matching — walk the
items, return the first declaration with a matching name, cross-file by
parsing every project file — so both mis-navigated under duplicate
names. `references` and `rename` cannot be built that way at all: a
missed or mis-bound reference makes rename silently corrupt code.

Resolution in `bynkc` is **distributed across three components**, which
shaped the design: the resolver covers type refs, free-fn calls,
expression identifiers, and static/constructor receivers; the **checker**
covers capability op-calls (`Cap.op`, `B.Cap.op`), cross-context service
calls, instance-method dispatch, and annotation resolution (handler and
test bodies never pass through the resolver's reference walk); the
**project driver** covers `given` prefixes, flattened caps, provider
links, and the `exports` / `consumes`-selection clause lists. A
"resolver-only" reference table would silently miss checker- and
driver-resolved kinds.

## Decision
A **reference-table sink** (`bynkc::index::RefSink`, the `ErrorSink`
analogue) is threaded through **all three resolution sites**, recording
`(name-segment span, kind, name, unit?)` edges only where resolution
succeeded — never an AST/text re-derivation. The v0.24 analyse pass
assembles the edges into a **`ProjectIndex`**: definition site + all
reference sites per symbol, exposed on `ProjectDiagnostics`.

- **Symbol identity is structural** — `(unit, kind, name)`; no `DefId`
  plumbing, no `Span` change. The resolver works on bare-name merged
  tables, so **unit qualification happens at assembly** (local decls →
  `uses` imports → consumed units' exports, mirroring the merge
  priority); cross-context and flattened edges record pre-qualified.
- **Attribution is collection-point with an owner correction**: sibling-
  file methods and unit-level handler tables are processed under a file
  other than the one their spans index into, so edges carry the enclosing
  top-level declaration and assembly re-attributes to its declaring file.
  Assembly dedupes — multi-pass resolution (resolver + checker over the
  same fn bodies; per-file re-checks of unit-level tables) is harmless.
- **Coverage:** types (annotation, static-receiver, qualified-variant,
  constructor, pattern-qualifier, `Mock[T]`), free fns (calls and
  first-class values), capabilities (bare/dotted/flattened `given`,
  op-call capability segments, `provides`, `exports capability`,
  `consumes U { Cap }` selections), services (cross-context calls and
  test-body `svc.call(…)` — a new recording point in the checker,
  mirroring the emitter's `test_services` rule), agents (construction),
  providers (declaration), plus **test/integration units** (the
  integration harness declares its synthetic namespace's resolution
  order). Synthetic first-party units are excluded — not user-editable.
- **Deferred kinds** — instance methods, record fields, capability op
  names, local bindings — record no edges; `prepareRename` refuses them
  (null), as it does unit/context names (renaming one is a file move —
  the A-3 increment).
- **Rename is validated twice, both correct-by-construction:**
  1. **Collisions:** apply the candidate edits to an in-memory overlay,
     re-run `diagnose_project`, refuse on any new per-(file, category)
     diagnostic — every collision class (same-unit clash, `uses`
     name-conflict, flattened-cap clash, …) without hand-enumeration.
  2. **Capture/escape:** re-analysis alone misses silent re-binding
     (declared fns shadow fn-typed locals in call position), so the
     re-built index must equal the pre-index **modulo the rename** —
     whole-index, remapped through the edit deltas; strictly stronger
     than a per-symbol check (it also catches a third symbol's
     resolution flipping).
  Edits are **versioned** (`TextDocumentEdit` against the analysed
  snapshot's captured document versions), so a drifted buffer rejects
  the rename instead of mis-applying it. Rename plans against a fresh
  analysis; failures surface as LSP request errors, not `bynk.*`
  diagnostics.
- **Rider:** `definition` and `hover` re-point at the index (binding-
  correct), keeping the legacy name-matching path only as fallback for
  the deferred kinds.

Test strategy: the `bynkc` fixture matrix proves every captured kind
maps to its one definition with name-segment spans and no same-name
conflation; the LSP's query/rename core is pure functions
(`index_queries`), unit-tested end-to-end (plan → apply → re-analyse →
validate) including a real capture refusal. The JSON-RPC harness remains
deferred: the handlers are thin position/packaging shims over the pure
core, so nothing here needs round-trip testing yet.

## Consequences
References and rename are project-wide and binding-correct, including
from test units; a rename that would corrupt code refuses with a reason.
The index retains per-file resolution on the analyse pass (acceptable at
current scale, debounced; incremental later) and is the base for
workspace-symbols and document-highlights (A-2), the same-file follow-up
(methods/fields/ops/locals — the checker sink and owner-qualified keys
are already in place), and the code-action catalogue's prescriptive
`given` edits.
