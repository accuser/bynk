# 0261 — A nested block records its parent's branch index, not just its parent block id

- **Status:** Accepted (v0.223)

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
