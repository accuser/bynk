# 0304 — The architecture map shows capability binding, not residency

- **Status:** Accepted (v0.245)

**Context.** A `consumes U { Cap, … }` selection flattens `Cap` into the
consumer's own namespace (§3.3); a whole-unit `consumes U` (no braces) grants
qualified access to everything `U` exports without flattening anything. The
built-in `consumes bynk { Clock }` form (every project that uses a toolchain
capability) resolves to the synthetic `bynk` unit, which has no project file
and so is never itself rendered as a node — an edge to it would dangle.

**Decision.** A braced selection's capability labels are recorded twice: once
as the `consumes` edge's own label (only when the target resolves to a real
node), and once as a bound-capability entry directly on the *consuming*
node's `capabilities` list (origin `Consumed { from }`), regardless of
whether the target became a node. A whole-unit consumes draws an edge only
(unlabelled) and binds nothing onto the node — nothing is flattened to
enumerate. Residency/boundary-tier annotation is out of scope for this
increment.

**Consequences.** The rate-limiter fixture's `consumes bynk { Clock }`
renders as a single node with a bound `Clock` capability and *no* edge (the
synthetic `bynk` unit contributes no node) — the concrete case the "Done
when" fixture combination in #851 calls out by name. A future increment that
adds residency/boundary-tier data is additive, not a rework of this shape.
