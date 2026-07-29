# 0298 — Event schema versioning ships as manual @schema(N) pinning, not the auto-detecting registry — narrowing #978's own scope

- **Status:** Accepted (v0.242)

**Context.** Proposal #978 (Events slice 3b) scoped an optional `@schema(N)`
annotation on `event` declarations, embedding `N` into `env.schemaVersion` at
emission. This is itself a narrower slice of what `design/tracks/events.md`
§3.5/§7 originally envisioned for "additive versioning": a schema registry
that automatically detects a version bump from an event's structural shape
changing across builds, with an evolution report and `@schema(N)` as an
*override* for edge cases — not, as this proposal ships it, the *sole*
mechanism.

Confirmed before writing the proposal: `bynkc compile`/`check` has zero
persisted cross-build state anywhere. The only committed, cross-run state in
the whole repo is `bynk.deploy.lock` (`bynk/src/deploy/ledger.rs`), and it is
owned exclusively by `bynk deploy` — read/written only inside
`bynk/src/deploy/plan.rs::run`, never by the compiler itself. Building the
full registry as originally envisioned would make `bynkc` the first-ever
compiler command with committed state — new, architecture-wide
infrastructure, out of proportion to one increment, and genuinely unsettled
in the ways §3.5 already named (where the registry lives, how a version is
computed, what counts as a bump).

**Decision.**

1. **Ship manual `@schema(N)` pinning only; split automatic detection out to
   a new, unfiled, future slice 3c** — the same narrow-and-name-the-deferral
   move #972 already made once for this exact track-doc bullet (splitting
   the original slice 3 into 3a/3b). Slice 4 (`via schema(...)`) is not
   renumbered and does not depend on 3c: it needs `schemaVersion` to be a
   real, author-controlled small integer, which this slice already supplies.
2. **`@schema(N)` is optional; absence means version `1`** — byte-identical
   to every event compiled before this annotation existed. Zero migration.
3. **Placement mirrors `messages "tag" @reference { ... }`**: `event Name
   @schema(N) = { body }`, annotations parsed after the declaration's
   identity and before its body-opening token — not a leading decorator line
   (the shape `@cache` uses on a `Handler`). `event Name = { ... }` reads
   structurally like a type alias; `messages`'s trailing placement is the
   closer shape match. Reuses the existing generic `store_annotation`
   grammar rule (no new rule, no new `{{#grammar}}` doc entry — the same as
   when `messages_decl` first reused it).
4. **`N` must be a positive (`>= 1`) integer literal, positional, at most
   once per event** — checked by a new closed-registry validator mirroring
   `validate_store_annotations`'s shape, scaled to a registry of one legal
   name (`schema`). Every malformed surface `parse_annotation` can actually
   produce is covered under one diagnostic, `bynk.event.bad_schema_version`:
   non-positive, non-literal (including a negative literal, which parses as
   `UnaryOp(Neg, IntLit)`, not a bare `IntLit`), wrong arity, a labelled
   argument, and more than one `@schema` on the same event (a case the
   proposal itself did not name, and which the grammar's `repeat` would
   otherwise admit with no diagnostic at all — first-wins silently would
   have been the wrong default). Any annotation name other than `schema` is
   `bynk.event.unknown_annotation`.
5. **No cross-build drift detection of any kind** — an accepted gap, not an
   oversight, named the same way idempotency's in-memory-only provider and
   per-publisher FIFO's narrower-than-asserted guarantee are named elsewhere
   in this track. The compiler cannot warn "you changed this event's shape
   but forgot to bump `@schema`" without persisted state to compare against,
   which Decision 1 explicitly defers.

**The plumbing problem this decision set forced, and its resolution.**
`Events.emit[E](payload)`'s lowering site (`lower_method_call`'s `Events.emit`
arm, `bynk-emit/src/emitter/lower.rs`) only ever has `E`'s bare name — from
the call's turbofish type argument — never its declaration. None of
`LowerCtx`, `ModuleCtx`, `TypedCommons`, or `CrossContextInfo` carried an
events-by-name map. The nearly-correct-looking shortcut — scanning
`TypedCommons.commons.items` for the matching `CommonsItem::Event` at the
lowering site — was rejected: that `Commons` is a **per-file** synthetic
structure (`ParsedFile::as_synthetic_commons`), so an event declared in a
different file of the same multi-file context would silently resolve to the
default `1` with no diagnostic. The resolution reuses `UnitTable::events` (a
unit-**merged** map, already used identically for `actors` at the same
`EmitProjectCtx` construction site): resolved once per unit into
`EmitProjectCtx.event_schema_versions: HashMap<String, i64>`, threaded to
`ModuleCtx` as a default-empty field assigned post-construction at the same
five sites `agent_method_givens` already is (not a required constructor
parameter, unlike `runtime_use` — a miss here safely degrades to
`schemaVersion: 1`, exactly every event's pre-existing behaviour, not a hard
failure), and read through a total `LowerCtx::event_schema_version` accessor
that never panics.

**Consequences.** `env.schemaVersion` is no longer a permanent, hardcoded
`1` — it reflects an author's own `@schema(N)` when declared. This is
assertion, not verification: a wrong or unbumped version is not a validation
failure, since nothing compares it against a prior build. `EventEnvelope`'s
own doc comment in `bynk-check/src/firstparty/bynk.bynk`, previously
describing `schemaVersion` as reserved and always `1`, is corrected. Proven
behaviourally in-process (not via a live-`workerd` boundary test, which
POSTs a hand-authored envelope directly to the receiving route and so cannot
observe the mint site this slice changes) across both a plain service
handler and an agent handler — the two distinct emitter body kinds the
`event_schema_versions` threading passes through.
