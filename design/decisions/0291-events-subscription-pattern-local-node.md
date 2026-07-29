# 0291 — A subscription filter is an Events-local pattern node, amending ADR 0286's "no bespoke matching engine" claim

- **Status:** Accepted (v0.239)

**Context.** `design/tracks/events.md` §3.3 and
[ADR 0286](../decisions/0286-events-pattern-dispatch-deliver-and-filter.md)
(Accepted) both asserted that subscription pattern filtering would "reuse
auth/refined-pattern machinery" and that "no bespoke matching engine is
introduced for Events." Neither claim survives direct inspection of the
code. The shared `Pattern` AST (`bynk-syntax/src/ast.rs`) has exactly six
variants — `Wildcard`, `Binding`, `Literal`, `Variant`, `Refined`, `Or` —
none a record/field pattern.
[ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md)
Decision F explicitly deferred record patterns as their own future slice
("Field patterns on a record type... are a distinct destructuring surface,
not a payload nesting"), and nothing since has built them. `Pattern::Variant`
requires a sum-type tag; its checker path (`variants_of`) returns `None` for
a record type, and its emitter lowering unconditionally tests `.tag` on the
*scrutinee itself*. An `event` is a plain record (`EventDecl { body:
RecordBody }`), not a sum — `Pattern::Variant` categorically cannot
represent "match some of this record's fields."

Extending the shared `Pattern` enum to fit would touch parser, checker,
emitter, `bynk-fmt`, tree-sitter, and LSP call sites, and would drag in
match-exhaustiveness semantics a delivery filter does not need — a
subscription pattern is a boolean guard evaluated once per delivery, not a
`match` arm that must be proven to cover every case.

Separately, the design notes' own worked example implies `e.region` is
*statically* narrowed to the matched value (e.g. `Region.Domestic`) inside a
matching handler body. That is a distinct claim from filtering, and it has
no existing mechanism either: the closest shipped analogue,
[ADR 0253](../decisions/0253-refined-patterns.md)'s refined patterns,
states plainly that "matching it does not change the static type of
anything in the arm's body" — static narrowing "waits on §2.5.4 (refinement
propagation), which is still the specification's largest open question."
This increment does not attempt it; see the companion "no narrowing" note in
`design/tracks/events.md` §3.3.

**Decision.** Introduce an Events-local pattern node —
`EventPattern`/`EventPatternField`/`EventPatternValue` in `bynk-syntax`,
placed near `EventDecl` rather than near `Pattern` to signal it is
deliberately not part of the shared pattern surface. It is checked by new
code in `bynk-emit/src/project/validate.rs` (field/type/variant resolution
against the event's declared record shape) and lowered by a small dedicated
`event_pattern_guard` function in `bynk-emit/src/emitter/lower.rs`, parallel
in shape to the shared `pattern_match_tests` but not calling it (that
function's `Refined` arm panics off a non-literal-kind scrutinee, and its
`Variant` arm tests the wrong path shape for a flat record field).

This amends ADR 0286's "no bespoke matching engine is introduced for
Events" claim. **ADR 0286's deliver-and-filter decision is unchanged and
remains correct** — the fan-out mechanism still delivers every emission of
`E` to every subscriber unconditionally; the guard this increment adds lives
entirely inside the subscriber's own generated handler method, not in
routing. Precedent for correcting an Accepted ADR from within a later
increment: issue #939's own DECISION B corrected §7's `Events.emit` call
shape, and ADR 0288's Decision D corrected `given Clock, Idempotency` against
the shipped mechanism.

**The surface, concretely.**

```
service OnDomesticPayment from Events(PaymentConfirmed { region: Region.Domestic, .. }) {
  on event(e: PaymentConfirmed) -> Effect[()] {
    -- e.region is NOT narrowed to Region.Domestic — e is PaymentConfirmed
    -- as always. Only delivery is filtered.
  }
}
```

- `from Events(E)` (no braces) remains the pattern-less form, matching
  everything, exactly as slice 0 shipped.
- Listing any field requires a trailing `..` — `from Events(E { })` is
  rejected, steering toward the bare form.
- A field's value is a literal (`Int`/`String`/`Bool`) or a nullary
  sum-variant reference, bare (`Domestic`) or qualified
  (`Region.Domestic`) — both accepted, since there is no binding form at a
  field's value position for a bare identifier to be ambiguous with.
- A payload-carrying variant is rejected (`bynk.event.pattern_variant_payload`):
  testing only the tag while a variant carries a payload would silently
  ignore the payload, an over-broad filter with no diagnostic.
- Multiple listed fields compose with AND. No nested record sub-patterns in
  v1.

**A genuinely new grammar surface: the `..` token.** No `..` (two-dot) token
existed anywhere — the lexer had `Dot` (one) and `DotDotDot` (three, record
spread). A real `DotDot` lexer token was added rather than reusing two
adjacent `Dot` tokens, because the latter would let the hand-written parser
accept a whitespace-split `. .` while tree-sitter (which declares `".."` as
one literal) rejects it — the exact class of Rust/tree-sitter conformance
divergence [ADR 0253](../decisions/0253-refined-patterns.md) Decision D4
was written to prevent. Verified directly: a unit test confirms `..` lexes
as one `DotDot` token and `. .` lexes as two separate `Dot` tokens.

**A latent slice-0 gap, closed here, independent of whether a pattern is
present.** Nothing previously checked that `on event(e: E)`'s declared
parameter type equals the header's `event_type`
(`check_service_protocols`). Harmless while no code depended on it — load-
bearing now, because the pattern is checked against the header's `E` while
the handler body sees `e` at its own declared type; if those could diverge,
the emitted guard would test fields the body doesn't believe exist. New
diagnostic: `bynk.event.handler_param_type_mismatch`.

**Tree-sitter never had `event`/`Events` grammar at all — a slice-0
prerequisite, not scope creep this increment introduces.** `grammar.js` had
zero occurrences of either token; slice 0 (#939) shipped the compiler-side
grammar only. This increment adds the missing slice-0 baseline (`event_decl`,
`Events(E)` in `service_protocol`, the `on event` handler) *and* the slice-1
pattern extension together, with a new corpus file
(`tree-sitter-bynk/test/corpus/events.txt`) and highlighting query entries.

**Consequences.**

- Deliver-and-filter (ADR 0286's still-correct half) means
  `discover_event_subscribers` and the fan-out DO's routing table need zero
  changes — verified directly by a Workers-target wiring fixture asserting
  the routing table is byte-identical whether or not a subscriber's header
  carries a pattern.
- The guard is inserted once, as the first line of the generated handler
  method body (`emit_service`), covering all three delivery paths
  (Cloudflare Workers, Bundle/node, Bundle/browser) in one edit, since all
  three call into this same generated method.
- `bynk-fmt`'s roundtrip guard (`fmt.rs`'s `roundtrip_divergence`) compares
  two canonical forms produced by the *same* renderer, so it is structurally
  incapable of catching a renderer bug that silently drops the pattern — a
  dedicated golden fixture (`bynk-fmt/tests/fixtures/36-events-pattern/`)
  asserts the rendered *text* contains the pattern syntax, which the
  roundtrip guard alone cannot.
- Verified end to end: `bynkc/tests/events_pattern_behaviour.rs` (Bundle
  target, real `tsc`+`node` run) proves a matching subscriber runs and a
  non-matching sibling subscriber of the same event does not, for two
  emissions of opposite value, in the same run — the positive and negative
  are each other's control. `bynkc/tests/events_workers_wiring.rs` gained a
  second fixture proving the Workers-target routing/wiring is unaffected and
  the emitted guard passes `tsc --strict`. Seven new static negative
  fixtures cover every new diagnostic.
- Tree-sitter corpus and VS Code integration tests are CI-only jobs — the
  grammar addition is not verified by the local dev loop, named here rather
  than silently assumed covered.
- Deferred, explicitly: nested record sub-patterns, static narrowing of the
  handler's parameter (needs the §2.5.4 refinement-propagation design
  question settled first), and server-side pre-filtering on Cloudflare
  (deliver-and-filter remains the shipped mechanism, per ADR 0286).
