# 0274 — A return-gating branch renders its outcome, not an empty `alt`

- **Status:** Accepted (v0.229.1)

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
