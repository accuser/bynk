# 0275 — The handler's `by` principal is an actor that originates the request and receives replies

- **Status:** Accepted (v0.229.1)

**Context.** A handler with a `by <Actor>` clause (v0.45) has a named principal
— the `Visitor` of the rate-limiter's `GET /check/:client`. The original model
had no notion of it: the entry lifeline stood alone with no visible caller, and
the return-gating outcomes (per the sibling decision) rendered as self-notes
over the entry rather than replies to anyone. That both under-describes the
protocol (who calls this handler?) and leaves 0260's "reply to its own caller is
a message" only half-realised.

**Decision.** The effective principal (`handler.by_clause`, else the
service-level `default_by`) becomes a leftmost **`Actor`** participant. It
originates the handler: a single inbound `Call` message carrying the request
descriptor (`GET /check/:client`), and — because the request now labels that
message — the entry box drops to the bare owner name (`api`). Every
return-position outcome is then a `Return` message **to the actor** rather than
a branch note: the top-level tail, and each branch tail of a return-gating
`if`/`match`, replies to the actor with its value (`Ok(view)`,
`TooManyRequests(...)`). Return position is tracked explicitly as the walk
descends (statements are never in it; a block's tail inherits its block's
position; a return-gating branch's tail is in it). Handlers with no principal
(agents — which have none — and services without `by`) are unchanged: no actor,
outcomes stay notes.

This is the literal form of 0260's "reply to its own caller is a message" — the
caller being the principal. Agents keep the combined `Owner.method` entry label
and no actor; only a service handler with a `by` splits into actor + bare entry.

**Consequences.** `ParticipantKind` gains `Actor` (Rust + wire string + TS
union). `sequence_model` gains a `default_by: Option<&ByClause>` parameter,
threaded from `ServiceDecl.default_by` exactly as `default_given` already is;
the renderer emits an `Actor` with Mermaid's `actor` keyword. Regression-covered
by the actor assertions in `rate_limiter_get_check_client_...` (branching
replies) and `service_level_..._inherited...` (a non-branching tail reply, plus
`by` inheritance) in `bynk-ide/src/sequence.rs`, and by
`renders an Actor participant ... and routes replies to it` in
`vscode-bynk/test/suite/mermaid-gen.unit.test.ts`.
