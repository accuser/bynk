# 0300 — via schema(N) version-aware dispatch ships literal-only, nested in the Events protocol grammar, with no cross-subscriber ambiguity check

- **Status:** Accepted (v0.244)

**Context.** The last remaining slice of the Events track (spine #936).
Per `design/bynk-design-notes.md`'s "Version-aware dispatch via envelope
patterns" section, a subscriber should be able to filter delivery by the
event envelope's `schemaVersion`, using a `via` clause parallel to slice
1's existing payload pattern:

```
service OnPaymentV1
      from Events(commerce.order.PaymentConfirmed { region: Domestic, .. })
      via schema(1)
      given ... { ... }
```

Proposal #985 scoped this to literal matching only, resolved directly with
the repo owner before filing: the design notes' worked example uses range
patterns (`via schema(2..)`), but **no range-pattern or range-literal
syntax exists anywhere in bynk** — the `..` token (added by slice 1) has
exactly one use, a record-pattern's trailing rest marker; it has never
appeared as an infix operator between two values. Shipping ranges here
would mean inventing an entire new grammar/AST/parser/checker/emitter
surface for integer ranges from scratch, disproportionate to bundle with
the much smaller literal case.

**Decision 1 — literal + wildcard only; ranges split to an unfiled slice
4b.** This slice ships `via schema(N)` (exact match) and no `via` clause
at all (matches any version, unchanged from today). Open ranges (`v..`,
`..v`), closed ranges (`v1..v2`), and an explicit `_` wildcard token are
named as future slice 4b, not built.

**Decision 2 — `via schema(N)` is nested inside the `Events(...)` protocol
grammar arm, not a free-standing clause.** Written after the `Events(...)`
header's closing `)` (matching the design notes' own form) but still part
of that one choice arm of `service_protocol`, exactly the way slice 1's
payload pattern is nested inside it. This makes `via` on `http`/`cron`/
`queue`/`websocket` a syntax error, not a checker diagnostic — there is no
grammar production admitting it there, so nothing needs validating. `via`
and `schema` are matched as plain `Ident` text, the same way `websocket`/
`Events` already are — both names were confirmed unused anywhere else in
the language before reusing them this way, so neither costs a lexer
reservation.

**The central plumbing problem, and its resolution.** Slice 1's payload-
pattern guard is inserted as the first line of the generated subscriber
method body (`emit_service`), reading the payload parameter that's always
present. The envelope is different: it only reaches that generated method
today when the subscriber itself declares `env: EventEnvelope` (slice 2's
optional second parameter) — confirmed at both forwarding sites
(`bynk-emit/src/emitter/workers.rs`'s `compose.ts` wrapper, `bynk-emit/
src/project.rs`'s Bundle dispatch closure), both of which only forward the
already-in-scope envelope value when `h.params.len() == 2`. A `via
schema(N)` guard needs `schemaVersion` regardless of whether the
subscriber declared `env`.

**Decision 3 — a synthetic envelope parameter, not a required
declaration.** When a service's protocol carries `via schema(...)` and its
`on event` handler did *not* declare a second parameter, `emit_service`
inserts a synthetic `__bynkSchemaEnv: EventEnvelope` parameter into the
*generated* method's own signature — in the same position a real `env`
would occupy — under a name no user-written identifier could ever collide
with. Both forwarding sites widen their condition from `h.params.len() ==
2` to `h.params.len() == 2 || <protocol has a via-schema clause>`, so the
envelope value lines up positionally with whichever parameter, real or
synthetic, is expecting it. This was chosen over requiring every `via
schema(...)` subscriber to also write `env: EventEnvelope`: the version
being dispatched on is compiler-computed information the clause itself
already states, not something the handler body should need to re-declare
just to make dispatch work. The checker's own param-count/type validation
(`bynk.event.bad_params`) is untouched — it only ever sees the user's
declared params, never the synthetic one, so a bare `on event(e: E)`
handler with a `via schema(...)` clause type-checks exactly as it always
did.

**Decision 4 — no cross-subscriber ambiguity check.** Verified against the
already-shipped precedent: two sibling subscribers to the same event with
overlapping or gapped *payload* patterns already both fire independently
today (deliver-and-filter, ADR 0286 — the fan-out mechanism delivers every
emission to every subscriber regardless, unconditionally). `via schema(N)`
follows the identical policy: two sibling subscribers with the same
literal `N`, or no subscriber covering a given version at all, are both
legal and undiagnosed. The design notes' `OnPaymentV1`/`via schema(1)` +
`OnPaymentV2OrLater`/`via schema(2..)` worked example *reads* mutually
exclusive but was never statically enforced as such, before or after this
slice — named explicitly here so it isn't assumed away by a future reader.

**Consequences.** `bynk.event.bad_schema_dispatch` (a non-positive or
otherwise malformed `via schema(...)` argument) mirrors `@schema(N)`'s own
positivity check almost verbatim, under its own code since the two are
unrelated syntax positions. `bynk.service.unknown_via_clause` closes the
`via` keyword itself as a registry of one name, mirroring `@schema`'s own
"any other annotation name is rejected" precedent. Proven behaviourally
(`bynkc/tests/events_schema_dispatch_behaviour.rs`): the same two
subscribers, compiled once at each of two schema versions, each version's
single emission reaching only its matching `via` clause — with the
version-1-matching subscriber declaring no `env` at all, the only way to
prove the synthetic-parameter plumbing fix actually threads the value
through a bare handler, not just one that already declared it.
