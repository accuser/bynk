# 0301 — The architecture map reads CrossContextInfo and a re-parse, not the binding index's call graph

- **Status:** Accepted (v0.245)

**Context.** The increment proposal (#851) described `architecture_model` as
"a pure, read-only query over the binding index and call graph"
(`bynk-check/src/index.rs`, ADR 0053/0067) — the same table `call_hierarchy`
already reads. Implementation found this citation does not hold:
`index::SymbolKind` has no `Context`/`Adapter` variant (its own doc lists
`Type, Fn, Capability, Service, Agent, Provider, Method, Field, CapabilityOp,
Actor, Handler, Messages`), so a context is not an index symbol at all, and
`ProjectIndex.calls` — the call graph — does not carry `consumes` edges either;
the field's own comment already flags this: "service cross-context dispatch
remains the one uncovered relation (no per-handler index symbol to be its
callee)".

**Decision.** Source nodes and edges from `ContextSequenceInfo::cross_context`
(`resolver::CrossContextInfo`) instead — the exact table `bynk/sequenceModel`'s
own classifier (`sequence::Builder::classify_cross_context`) already resolves
consumed-context names and aliases against — plus a re-parse of each unit's own
committed snapshot text (`bynk_syntax::parser::parse_unit_with_recovery`) for
its local declarations (capabilities, providers, services, agents) and its raw
`consumes` clauses. There is no retained AST after a round to read instead —
the same constraint `sequence_request`/`documentation_request` re-parse
against for their own file-scoped queries.

**Consequences.** `bynk_ide::architecture::architecture_model` takes
`unit_sources`, `snapshots`, and `sequence_info` — the same three tables
already threaded `ProjectAnalysis` → `ProjectDiagnostics` → the LSP
`Analysis` for #846/#847/#848 — rather than `ProjectIndex`. No index change
was needed for this increment; a future increment that wants a `Context`
index symbol (e.g. to back go-to-definition on a bare context name outside a
`uses`/`consumes` clause) is unrelated follow-up, not a prerequisite this one
discovered.
