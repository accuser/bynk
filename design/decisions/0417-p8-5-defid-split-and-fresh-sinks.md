# 0417 — P8.5's `DefId` — a split Fn/Handler enum, plain-`DefId` query signatures, and fresh per-call sinks

- **Status:** Accepted (v0.289.55). **Superseded by [ADR 0420](0420-phase-8-unadopted-query-layer-deleted.md)**
  (v0.289.65): the `DefId`-keyed `body`/`type_of` queries this decision shaped were deleted on
  2 September 2026 (#1537) — nothing outside their own test module ever called them, and no
  scheduler existed to. Decisions A–D here stand as the record a rebuild adapts to if R3.15's
  trigger (#1523) fires (the `HandlerKind` `Hash` derive Decision A motivated is kept for that
  reason); `incremental_query_types` now gates that a `DefId`-keyed query stays absent until then.

**Context.** #1516 proposed `Body(DefId)`/`TypeOf(DefId)` as real, callable query functions
wrapping `checker::check_body`/`checker::check_handler_body`, built but not wired into
`check_file_core`/`analyse_project`/`check_unit_files` (Decision C, followed as recommended — those
three entry points are untouched by this slice). It left four open decisions; this ADR records how
each was actually resolved during implementation, plus a fifth fork the issue's own text did not
anticipate.

**Decision.**

1. **[DECISION A] `DefId` is a split enum**, `DefId::Fn(FnDefId)` / `DefId::Handler(HandlerDefId)`,
   as recommended — `check_handler_body` returns `()` while `check_body` returns `Option<TyId>`, so
   a handler has no value for `TypeOf` to report. `FnDefId { unit: UnitId, owner: Option<String>,
   name: String }` (`owner` is the attached type's name for a method, `None` for a free function)
   and `HandlerDefId { unit: UnitId, owner: String, kind: HandlerKind, method_name: Option<String>
   }` both reuse `UnitTable`'s own existing `String`-keyed identity shape rather than inventing a
   fresh interned scheme, per Decision A's own recommendation. `HandlerKind` gained a `Hash` derive
   (additive, `bynk-syntax/src/ast.rs`) to support this — it was already body-free and span-free.

2. **[DECISION B] Both `body` and `type_of` allocate a fresh `CheckSinks` per call.** This turned
   out to need no invasive isolation work: `expr_types`/`callees` are plain `ExprId`-keyed maps with
   no accumulation semantics, and `refs`/`hints`/`locals`/`requirements` each already document that
   "a fresh sink records nothing until `enter_file` attributes it" — exactly the shape a per-call
   query needs, not a new assumption. `bynk-check/src/queries.rs`'s own test module includes a
   byte-for-byte fixture (`body_matches_check_handler_body_for_one_provider_op_in_isolation`)
   proving a fresh-sink run and a file-wide-shaped run agree on `errors`/`expr_types` for one
   isolated provider op — the one shape this slice actually exercises. It does not prove isolation
   safe for every accumulation pattern `CheckSinks`'s seven fields could ever see across a real
   multi-definition file; that remains a risk for whichever future slice wires these into
   production per Decision C.

3. **[DECISION C] Followed as recommended** — `check_file_core`, `analyse_project`, and
   `check_unit_files` are untouched; nothing in the tree calls `body`/`type_of` outside this
   slice's own tests.

4. **[DECISION D] Query signatures stay plain `DefId`** (`fn body(id: DefId, …) -> Body`, `fn
   type_of(id: DefId, …) -> Option<TypeOf>`), not a `DefId::Fn`-typed parameter, so `xtask`'s
   `defid_query_fn_present` probe keeps working unmodified. `type_of` returns `None` for a
   `DefId::Handler` without doing any checking work — the one place Decision A's "invalid states
   unrepresentable" ideal is deliberately relaxed for probe compatibility, documented in
   `queries.rs`'s own module doc comment rather than left implicit.

5. **A fork neither #1516 nor R3.13 examined: the probe requires `DefId` on the *same source line*
   as the `fn` needle** (`defid_query_fn_present`'s own doc comment states this explicitly — a
   wrapped multi-line signature was already a known, named limitation). `rustfmt` reflows any
   six-plus-parameter signature across multiple lines at this codebase's line width, which would
   have put `id: DefId` on its own line and left the probe reading `query_types` at 2/4 even after
   this slice landed. Fixed by bundling every parameter but `id`/`inputs` into a new `QueryCtx<'a>
   { input, tys, file, muted }`, shrinking both signatures to three parameters each — short enough
   to survive formatting on one line. This is a real, if narrow, precision requirement for any
   future query function this phase adds: a five-or-more-parameter signature risks the same
   silent probe miss, not just a `DefId`-naming risk.

**Consequences.** `bynk-check/src/queries.rs` is the one new module (`DefId`/`FnDefId`/
`HandlerDefId`, `Body`/`TypeOf`/`BodyInputs`/`FnBodyInputs`/`QueryCtx`, the `body`/`type_of`
functions), registered in `lib.rs`. `bynk-syntax::ast::HandlerKind` gained `Hash`. `cargo xtask
greenfield-status` now reads `query_types 4/4 (UnitSignature, ProjectGraph, Body, TypeOf)`. No
production call path changes; a future scheduler slice (R3.15, still deferred) is what would call
these from `check_file_core`/`analyse_project`, and per Decision B's own residual risk, should not
assume per-call sink isolation is safe for every accumulation shape without its own broader fixture.
