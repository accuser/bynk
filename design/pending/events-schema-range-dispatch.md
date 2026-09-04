---
level: minor
changelog: "Events track: `via schema(...)` dispatch clauses gain range patterns (`v..`, `..v`, `v1..v2`, inclusive both ends) and an explicit `_` wildcard, extending slice 4's literal-only `via schema(N)`. Also fixes a pre-existing Workers-target defect where `handlers.ts` never imported `EventEnvelope` for a bare `on event(e: E)` handler under any `via schema(...)` clause."
---

## ADR: events-schema-range-dispatch
title: via schema(...) range dispatch ships inclusive-bound, reusing the .. and _ tokens already in the grammar, with no new ambiguity check
summary: Events slice 4b's design — the inclusive-bound convention, why no new lexer tokens or tree-sitter rule were needed, the `Wildcard` still-needs-the-envelope subtlety, and the unchanged no-cross-subscriber-ambiguity policy

**Context.** The last remaining slice of the Events track's dispatch work
(spine #936), following slice 4 (#985, ADR 0300). Slice 4 shipped `via
schema(N)` literal-value dispatch, deliberately narrowing out range
patterns: `design/bynk-design-notes.md`'s original worked example uses
them (`via schema(2..)` for "version 2 or later"), but no range-pattern
syntax existed anywhere in bynk at the time, so ranges were split to an
unfiled slice 4b. Proposal #990 files and ships that slice.

**Decision 1 — bounds are inclusive on both ends.** Matching the one
existing integer-range concept in bynk: the `InRange(lo, hi)` refinement
predicate lowers to `receiver >= lo && receiver <= hi`
(`bynk-emit/src/emitter.rs`), described as "must be in range `[lo, hi]`" —
bracket notation, inclusive. `via schema(v1..v2)` follows the identical
convention: `v1 <= schemaVersion <= v2`. `via schema(v..)` is
`schemaVersion >= v`; `via schema(..v)` is `schemaVersion <= v`.

**Decision 2 — reuse the pre-existing `..`/`_` tokens; no new lexer
tokens, no new tree-sitter rule.** The `..` (`DotDot`) token slice 1
already added (previously only a record-pattern's rest marker) and the `_`
(`Underscore`) token already used by match-pattern wildcards cover every
new shape without a lexer change. On the grammar side, `schema_dispatch_
clause`'s existing `version` field widens in place from `optional("-")
number_literal` to a `choice` of five arms (closed range ordered before
open-above so the generator prefers the longer alternative on their shared
`number_literal ".."` prefix); the wildcard arm reuses the already-named
`wildcard_pattern` rule. `tree-sitter generate` reported no conflicts.
Net +0 rules for `bynk-grammar`'s own coverage count.

**Decision 3 — `Wildcard` still triggers the synthetic-envelope-parameter
plumbing, but emits no runtime guard.** Slice 4 inserts a synthetic
`env: EventEnvelope` parameter into a generated subscriber method whenever
the protocol carries *any* `via schema(...)` clause and the handler didn't
declare its own second parameter — needed so the guard can read
`env.schemaVersion`. `via schema(_)` has no guard to evaluate (it matches
unconditionally, identical codegen to omitting the clause), but the
envelope parameter is still inserted: a service listing several sibling
`via schema(...)` clauses across a family of subscribers may rely on the
envelope being present uniformly, and the presence check
(`schema_dispatch: Some(_)`) is deliberately shape-agnostic — it does not
distinguish `Wildcard` from any other variant. The IR field
(`ProtocolIr::Events::schema_dispatch`) therefore widens from slice 4's
`Option<i64>` to `Option<SchemaDispatchIr>` rather than collapsing
`Wildcard` to `None`, which would have silently dropped this plumbing.

Testing this plumbing on the Workers target (slice 4 itself only ever
exercised it on Bundle, `bynkc/tests/fixtures/positive/1232_events_
envelope_schema_dispatch_bare`) surfaced a pre-existing defect, present
since slice 4 and unrelated to range dispatch itself: a bare `on event(e:
E)` handler's synthetic `env: EventEnvelope` parameter never appears in
the handler's own `h.params`, so `collect_external_references`
(`bynk-emit/src/emitter.rs`) — which walks raw AST params/body to decide
what a Workers module needs to import — never saw it, and `handlers.ts`
never imported `EventEnvelope`. `tsc --strict` catches it
(`Cannot find name 'EventEnvelope'`), but no prior fixture had a
Workers-target, all-bare-handler `via schema(...)` family to trip it — the
one existing schema-dispatch fixture is Bundle-target, and its lone bare
handler shares a module with a sibling that declares `env` explicitly,
incidentally satisfying the import for both. Fixed by hand-registering the
reference the same way `collect_external_references`'s own
`CommonsItem::Messages` arm already registers `LocaleTag`/`Message`/
`MessageArg` — a reference the generated code needs even though no
expression in the file's source names it.

**Decision 4 — the checker's positivity rule extends per-bound; a new
inverted-range rule reuses the existing diagnostic code.** Each bound
independently must be a positive `Int` literal — unchanged rule, now
applied to every bound of every shape. `Closed(lo, hi)` with `lo > hi` is
additionally rejected as an always-empty range, mirroring `InRange`'s own
inconsistent-bounds check. Both report `bynk.event.bad_schema_dispatch`
(slice 4's own code, not a new one) — the issue's own design section
frames both as "malformed," the same vocabulary that code already covers.

**Decision 5 — no new cross-subscriber ambiguity check.** Unchanged from
slice 4 and slice 1's own deliver-and-filter policy: sibling subscribers
with overlapping or gapped range coverage are not diagnosed. In
particular, `via schema(_)` fires on every version alongside whichever
other clause also matches — it is not "the one that gets picked when
nothing else matches," it is unconditional, same as omitting `via`
entirely.

**Verification.** Positive behavioural test extending slice 4's own
(`events_schema_dispatch_behaviour.rs`): four sibling subscribers
(`via schema(1)`, `via schema(2..4)`, `via schema(5..)`, `via schema(_)`),
compiled at three or more schema versions, asserting each version's
emission reaches both its version-specific subscriber and the always-on
wildcard subscriber. Negative fixture proves the inverted-closed-range
rejection (`via schema(5..2)`). Regression: existing literal-only
fixtures/tests unaffected — `Literal`'s own parse/check/emit path is
untouched, only additive match arms added around it.
