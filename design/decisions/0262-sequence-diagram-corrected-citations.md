# 0262 — Two citation corrections carried from issue review into the implementation

- **Status:** Accepted (v0.223)

**Context.** Review of #846 (posted as GitHub comments before implementation)
found two inaccurate citations: "same-context agent addressing is recognised
at `checker.rs:1150`" actually names `predicate_cross_agent_ref`, an
unrelated invariant-well-formedness check; and "reuses the existing lens
plumbing (`bynk-check` `code_lenses`)" — `code_lenses` is defined in
`bynk-lsp/src/index_queries.rs`, not `bynk-check`, and in any case only
indexes agent handlers (`SymbolKind::Handler` excludes service handlers).

**Decision.** The implementation cites the correct locations: agent-dispatch
recognition mirrors `bynk-check/src/checker/calls.rs:387,2038`
(`ctx.input.agents.get(...)`), reimplemented in
`bynk-ide/src/sequence.rs::classify_target`. The "Show Sequence" CodeLens
does **not** reuse `index_queries::code_lenses` — it is sourced from a new,
separate AST walk (`bynk-lsp/src/sequence_request.rs::handler_lens_sites`)
over `Service.handlers`/`AgentDecl.handlers` directly, so it covers service
handlers (which `SymbolKind::Handler` indexing excludes) as well as agent
handlers.

**Consequences.** None beyond the correction itself — flagged here so the
durable ADR record does not repeat the issue's original mis-citations.
