---
level: minor
changelog: "Bynk: Show Sequence Diagram" (VS Code command + per-handler CodeLens) renders a Mermaid sequence diagram for the handler under the cursor, via a new `bynk/sequenceModel` LSP query
---

## ADR: sequence-diagram-lifeline-classifier
title: Sequence-diagram lifelines — consumed capabilities, consumed contexts, and agents, including same-context agents
summary: The three-way predicate that decides which of a handler's calls become a diagram lifeline vs. folding into the entry activation

**Context.** A Bynk handler body is, structurally, a sequence of messages
across runtime boundaries (`let x <- Cap.op(…)`, `Agent(key).call(…)`), but
nothing in the tooling surfaces that shape. #846 adds a "Show Sequence
Diagram" view built on a new `bynk-ide::sequence::sequence_model` query,
served through a new `bynk/sequenceModel` LSP request, rendered client-side
by the VS Code extension as a Mermaid `sequenceDiagram`.

**Decision.** A call is a lifeline iff:

```
is_lifeline(target) =
      target is a consumed Capability            // in the handler's effective `given`
   || target.owning_context != current_context   // a consumed-context call
   || target is an Agent                          // same-context included
```

Everything else — commons fns (mixed in via `uses`), context-local `fn`s,
plain instance methods, record/`Result` construction — folds into the entry
lifeline's activation: no message is emitted for the call itself. A
same-context agent stays a lifeline even though it is lexically local,
because it is a distinct, separately-addressed, stateful runtime participant
(a Durable Object on `workers`) — folding it in would hide the single most
interesting message in most handlers.

Cross-context/agent calls are **boundary-stop** (ADR
`sequence-diagram-boundary-stop`): a single Call+Return pair, never inlining
the callee's own body — even where the callee's `Handler` AST is technically
reachable (via the consumed unit's cross-context table, or the local agent
table). Implementing transitive inlining would require resolving `fns`/
`methods` bodies too, a materially bigger extractor deliberately out of Tier
1 scope (see the "Tier-1 limitation" ADR below).

**Consequences.** A lifeline call written inside a commons `fn`'s own body
— three calls deep from the handler — is invisible to the diagram; the
extractor only sees calls written directly in the handler body (with
`if`/`match`/`block` nesting). This is a stated Tier-1 limitation, not a
bug: the issue's own framing ("commons fns… fold into the entry lifeline's
activation") describes folding, not transitive inlining, and this repo's
resolver does not inline call bodies either.

## ADR: sequence-diagram-tier-1-scope
title: Sequence diagram Tier 1 — on-demand/static, not live-synced
summary: This increment ships a static, command/CodeLens-triggered diagram from the last committed analysis round; cursor-following live sync is an explicit, deferred follow-up

**Context.** Two increment shapes were considered: a command/CodeLens that
builds a static diagram from the committed round (Tier 1), and a panel that
follows the active handler with cursor↔message highlighting (Tier 2).

**Decision.** Ship Tier 1 only. `bynk/sequenceModel` is served from the
`committed_analysis` gate (the #733 stale-while-revalidate mechanism the
same 5 pull-based decoration handlers already use) and is re-issued fresh
by the client every time the command/lens fires — there is **no** refresh-
push mechanism for it. This corrects the issue's own phrasing ("re-pull via
`workspace/*/refresh` nudge"): no generic "refresh a custom method" exists
in the LSP spec or in `tower_lsp::Client` (the #733 nudge only wraps three
typed built-in methods — `semanticTokensRefresh`/`inlayHintRefresh`/
`codeLensRefresh`), and Tier 1's on-demand model does not need one.

**Consequences.** The diagram does not update while the user keeps editing
with the panel open; re-invoking the command/lens gets a fresh render.
Tier 2 (live sync + cursor↔message highlighting) is deferred, tracked as a
follow-up, not scoped here.

## ADR: sequence-diagram-return-gating-blocks
title: An `if`/`match` renders even when every branch is call-free, when it gates the handler's own return
summary: Corrects a narrower reading of the issue text that would have dropped the rate-limiter's own worked example

**Context.** A naive reading of "fold local computation into the entry
activation" suggests an `if`/`match` should only render as an `alt`/`opt`
block when at least one branch contains a lifeline call. The issue's own
worked example contradicts this: `if view.allowed { Ok(view) } else {
TooManyRequests(...) }` renders as an `alt` block even though neither
branch calls a capability, agent, or consumed context.

**Decision.** A handler's reply to its own caller is itself always a
message in the model (never folded away), so an `if`/`match` renders
whenever at least one branch has a lifeline call **or** the block gates a
distinguishable final return — in practice, every `if`/`match` up to the
depth budget renders, since branches whose tail is a plain constructor
still count as "gating the return." `AltBlock`s are pushed unconditionally
by the extractor; there is no message-count gate on rendering.

**Consequences.** The rate-limiter's `GET /check/:client` diagram matches
the issue's own worked example exactly (regression-covered by
`rate_limiter_get_check_client_classifies_capability_and_agent_and_gates_the_return`
in `bynk-ide/src/sequence.rs`).

## ADR: sequence-diagram-nested-block-branch-tracking
title: A nested block records its parent's branch index, not just its parent block id
summary: Needed to place a nested `if`/`match` correctly when its parent branch is itself message-free

**Context.** The Mermaid renderer needs to interleave a block's direct
messages with any nested child blocks in source order. A child block
nested only in one branch of a two-branch parent cannot be placed correctly
from `parent: Option<u32>` alone when that branch has zero messages of its
own (the parent branch has "nothing to key off of" positionally) — this is
exactly the shape of the rate-limiter's `if`/`else`, and of any handler
whose branching gates further branching before any lifeline call occurs.

**Decision.** `AltBlock` carries `parent_branch: Option<u32>` alongside
`parent: Option<u32>`, populated at construction time in the Rust walker
(which already knows exactly which branch iteration it is in when it
recurses into a nested `if`/`match`). The VS Code webview's Mermaid
generator (`mermaid-gen.ts`) renders recursively from this parent/branch
tree — merging each branch's own messages with its direct child blocks,
ordered by source span — rather than from a single flat pass over
`messages` alone.

**Consequences.** Verified against Mermaid's own parser (`mermaid.parse`)
for nested `alt`-in-`alt`, single-branch `opt`, multi-arm `alt`/`else`, a
fire-and-forget `-)` send, and a `Collapsed`-depth `note` — all produce
syntactically valid `sequenceDiagram` text. Regression-covered in Rust
(`nested_if_collapses_past_the_depth_budget`'s `parent`/`parent_branch`
assertions) and in TypeScript (`mermaid-gen.unit.test.ts`'s nesting case).

## ADR: sequence-diagram-corrected-citations
title: Two citation corrections carried from issue review into the implementation
summary: The issue's grounding citations for agent-dispatch recognition and CodeLens plumbing pointed at the wrong code; the implementation uses the corrected locations

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
