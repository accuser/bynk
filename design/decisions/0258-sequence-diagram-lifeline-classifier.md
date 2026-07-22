# 0258 — Sequence-diagram lifelines — consumed capabilities, consumed contexts, and agents, including same-context agents

- **Status:** Accepted (v0.223)

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
