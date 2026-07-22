# 0260 — An `if`/`match` renders even when every branch is call-free, when it gates the handler's own return

- **Status:** Accepted (v0.223)

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
