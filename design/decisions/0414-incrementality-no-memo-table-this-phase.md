# 0414 — This track builds no memo table of any kind — the granularity and the firewall proof are the whole deliverable, R3.15's scheduler decision deferred whole

- **Status:** Accepted (v0.289.49)

**Context.** R3.15 names three options — salsa, a hand-rolled memo table, or nothing — and defers
the choice to a later, separate trigger ("a hand-rolled memo table... measurably the bottleneck").
The draft's own worry was that stopping at the query decomposition, with no cache behind it,
would leave nothing for the phase's own probe to measure — implicitly pressuring this track toward
building at least a minimal hand-rolled table just to have a number to report.

**Decision.** No memo table ships in this phase, hand-rolled or otherwise. The gated probe
(`incremental_query_types`, settled in the track doc's own §3.5/§5 as an ordinary decision, not a
front-loaded ADR — a probe definition is cheap to revise later, unlike the three decisions that do
get an ADR here) is a one-time existence check — do the query types exist, is the shared file-level
cache wired into both call sites, does the body-edit stability test (P8.2) exist — not a latency
number and not a check that the test *passes* (a gated probe is a static read run from inside a
`#[test]` itself; shelling out to `cargo test` to check an outcome would be the same
nested-invocation cost `wildcard_arms` avoids by staying trend-only). So the draft's own pressure
toward "build something just to measure it" is resolved by changing what's measured, not by
building a scheduler. R3.15's own trigger for either salsa or a hand-rolled table
is "measurably the bottleneck," which cannot fire before real query types exist to be a bottleneck
in — building one now would be committing to solve a problem (cache-invalidation correctness
under concurrent IDE writes) this phase has no evidence yet even exists at a scale worth solving.

**Consequences.** This phase's own risk stays bounded to R3.13 (granularity) and R3.14 (the
firewall) — real, load-bearing architectural commitments — without also taking on cache-
correctness risk this phase's own evidence doesn't yet justify. `keystroke_latency` (the trend-only
probe) stays "not measured" through this phase's retirement; a future, separate track (or a later
slice of this one, if reopened) owns the scheduler decision once real evidence of a bottleneck
exists to trigger it. The settled slice count (§6 of the track doc) is 6, not the draft's
provisional 6–9 — dropping the conditional "P8.6, memo table" slice entirely rather than leaving
it open.
