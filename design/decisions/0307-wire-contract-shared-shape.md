# 0307 — One IR (`bynk_check::wire::WireModel`) backs both the emitter's codec generation and the peek; `contract.rs`'s canonical hash form stays a separate, deliberately un-unified derivation

- **Status:** Accepted (v0.246)

**Context.** Before this increment, a boundary type's wire shape existed only
as a control-flow path through `bynk-emit`'s `serialisation.rs` — `TypeRef`
in, `writeln!`-ed TypeScript out, no intermediate value. A peek written
against that would necessarily re-derive the shape by hand, and a re-
derivation drifts from the original the moment either one changes without the
other. `bynk-check` already has a second boundary-canonicalisation pass —
`contract.rs`'s `service_normal_form`, which computes a cross-context
contract's hash — and an early pass considered folding the codec's shape into
that same canonical form as a third consumer, so there would be exactly one
"the shape of a boundary type" function in the crate.

**Decision.** That folding was investigated and **rejected on evidence**: the
two canonicalisations disagree on every axis that matters, each correctly for
its own job. `contract.rs:248` documents predicate sorting as a
*precondition* for hash correctness — hashing predicates in declaration order
"would make two contexts that agree perfectly fail closed against each
other," since the hash's whole job is that semantically-equal contracts hash
equal. `wire.rs`'s codec-generation IR needs the opposite: `Inline`
revalidation emits one `if` per predicate in **declaration order**, because
that is the order a JS `if`-chain (or a `switch` on sum variants, or a
record's emitted JSON keys) is observably rendered in. The same reversal
holds for record fields (sorted-by-name vs. declaration order) and for an
opaque type's predicate (elided from the hash — unobservable to a consumer
that cannot see it — but present in the IR, since the *owner* still
re-validates it). Unifying the two forms would either break the hash's
false-positive guarantee or misrender the codec — there is no shared
"canonical" form that is correct for both jobs simultaneously.

`contract.rs` is therefore **left untouched** by this increment. What the two
derivations genuinely share — and what would silently break if a future
change made `wire.rs`'s boundary walk and `contract.rs`'s reach a different
*set* of named types for the same handler, even though they render/order
that set completely differently — is asserted directly:
`bynk-check/src/wire.rs`'s `boundary_reachability_agrees_with_contract_normal_form`
test walks both derivations from the same handler and asserts equal
*reachability* (which types each one visits), never equal string form or
order.

**Consequences.** `bynk_check::wire::WireModel` has exactly **two**
consumers: `bynk-emit`'s codec generation (a byte-identical rewrite over the
IR, proven by an unchanged `bynkc/tests/fixtures` corpus) and
`bynk-ide::wire_contract` (the peek). A third derivation
(`contract.rs`'s hash) continues to exist deliberately unfolded, pinned by
its own reachability cross-check rather than by shared code.
