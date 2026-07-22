---
level: patch
changelog: Sequence diagrams show the handler's `by` principal as an actor that originates the request and receives the replies; return-gating branches no longer collapse to an empty `alt`
---

## ADR: sequence-diagram-branch-outcomes
title: A return-gating branch renders its outcome, not an empty `alt`
summary: The reply ADR 0260 named is emitted per branch — as a note, or (with a principal) a return to the actor

**Context.** ADR 0260 ruled that an `if`/`match` renders as a block even when
every branch is call-free, on the stated grounds that "a handler's reply to
its own caller is itself always a message in the model (never folded away)."
That reply was never actually emitted: a return-gating branch's tail (`Ok(view)`,
`TooManyRequests(...)`) folds away as local computation, so the rate-limiter's
`GET /check/:client` produced an `AltBlock` with **two message-free branches**.
`mermaid-gen.ts` rendered that as `alt then / else else / end` — an empty `alt`,
which Mermaid 11 draws as a mangled zero-width box with its `[then]`/`[else]`
labels wrapping one character per line. The block rendered, but illegibly; the
model's own justification for rendering it (a reply the reader could see) was
absent.

**Decision.** The branch outcome is now first-class. `Branch` gains a `reply:
Option<String>` — the rendered tail value the handler yields on that path — and
the renderer draws it as a `note over` the entry lifeline, giving the block real
content. `reply` is `None` when the tail carries no distinguishable signal: a
unit `()` tail, or control flow already rendered as its own nested structure.
Two adjacent legibility fixes ride along, since they share the "don't render an
empty branch" goal: an `if` branch is labelled by its **condition**
(`alt view.allowed`) rather than a bare `then`, and an **else-less `if`** renders
as a single-branch `opt` instead of an `alt` with an empty second branch. As a
defensive floor, a branch that still emits nothing renderable (an explicit
`{ () }`) is anchored with a placeholder note so no `alt`/`opt` can ever collapse
again.

This supersedes 0260's "reply is a message" framing for the **no-principal**
case: the reply is a per-branch **note**, not a lifeline message. When a
principal *is* present, the sibling decision below makes the reply a real
message after all — to the actor. 0260's operative ruling — that a return-gating
block renders unconditionally — stands throughout.

**Consequences.** The wire shape gains `WireBranch.reply` (Rust `bynk-lsp`)
mirrored by `Branch.reply` (TS `vscode-bynk`). Click-to-code generalises:
`toMermaid` now returns `noteOrder` (every emitted note — collapsed marker,
branch reply, or placeholder — in emission order) in place of `collapsedOrder`,
each note linking to its owning block's span. Regression-covered by
`rate_limiter_get_check_client_...` and
`else_less_if_is_a_single_branch_opt_and_match_arms_capture_replies` in
`bynk-ide/src/sequence.rs`, and by the `toMermaid` note assertions in
`vscode-bynk/test/suite/mermaid-gen.unit.test.ts`.

## ADR: sequence-diagram-principal-actor
title: The handler's `by` principal is an actor that originates the request and receives replies
summary: A principal renders as a leftmost actor; the request is an inbound message and outcomes return to it

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
