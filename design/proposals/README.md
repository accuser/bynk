# Increment proposals

The working home for **active** increment proposals — and only active ones. A
proposal is a **transient input** to an increment, not a durable artefact: the
durable record of a landed increment is the code and its fixtures, the
[normative spec](../../docs/src/spec/index.md) updated in place, and the
[decision records](../decisions/README.md).

## Lifecycle

1. **Propose.** A draft lands here as `vX.Y-<slug>.md` via its own PR. It is
   the sign-off artefact: the design forks marked `[DECISION]` with
   recommendations, the risks, and a sketch of the spec delta. **Merging the
   proposal is the approval to build.**
2. **Implement.** The increment consumes the proposal: the grammar/compiler
   change with fixtures, the spec chapters updated in place, a decision record
   per language-defining call, the changelog/docs/tooling deltas.
3. **Delete.** The final implementation PR **removes the proposal file**. An
   empty directory means nothing is in flight. The proposal's history remains
   in version control (`git log -- design/proposals/`).

## Writing one

Write a proposal knowing it will be consumed, not maintained. State **deltas
and decisions** — what changes, the forks and their recommendations, what the
spec sections will say. Do **not** duplicate normative content (full grammar
productions, worked emission output): the normative prose is written once, in
the spec, during implementation. Duplicated content is how the retired
instalment documents drifted.
