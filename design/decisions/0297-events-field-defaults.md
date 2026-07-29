# 0297 — Event field defaults lower to their wire form, not a value-level reference — amending #972's Decision E

- **Status:** Accepted (v0.241)

**Context.** Proposal #972 (Events slice 3a) scoped field-default expressions
on `event`-declared record fields — `RecordField.init`, already parsed for
every record field (including an event's) via `parse_record_field`, but never
validated or used outside agent `store` fields. The proposal's Decision E
recommended reusing `BodyMode::StaticInit` (`emit.rs`'s agent-fresh-state
factory lowering path) to compile a default's expression once, verbatim, the
same way an agent's `store colour: Cell[Light] = Red` is lowered today.

Compiling a real fixture before implementing surfaced why that reuse is
wrong for this case specifically. `bynkc/tests/fixtures/positive/
155_state_sum_machine`'s `store colour: Cell[Light] = Red` — a bare,
unqualified sum-variant reference — lowers via `BodyMode::StaticInit` to `{
colour: Light.Red }`: a **qualified value-level reference** into `Light`'s
generated namespace. That is correct and necessary for agent state, which is
always lowered inside its own declaring module. It is wrong for an event
field default: since ADR 0295 (#973), a subscriber's own regenerated codec
for a consumed event type only ever imports the publisher's *types*
(`import type * as commerce_order from "..."`), never a value-level binding —
a qualified `Region.Domestic`-shaped reference spliced into that codec would
not resolve at `tsc` time. The proposal's own open-risk #2 anticipated a
version of this ("confirm the codec-generation context has the same
commons/cross-context info `emit.rs`'s call site does") but framed it as a
*scope-plumbing* question; the actual problem is that the reused lowering
path itself produces the wrong *kind* of reference, not merely that it might
run somewhere lacking the right imports.

**Decision.**

1. **A defaulted field's default lowers to its wire (JSON) form, not its
   in-memory form — a new, dedicated `lower_field_default_wire`
   (`bynk-emit/src/emitter/serialisation.rs`), not `BodyMode::StaticInit`
   reuse.** Nothing in a wire-JSON literal is ever a qualified reference — a
   sum variant is `{ kind: "Domestic" }`, not `Light.Red` — because
   `emit_field_deserialise` already builds exactly this shape when reading
   real JSON off the wire. Splicing a defaulted field's fallback in *before*
   that same function runs needs no import and no qualification, sidestepping
   the problem entirely rather than working around it with additional
   plumbing.
2. **The lowering is type-directed, not `expr_types`-directed — because no
   cross-unit `expr_types` store exists or can cheaply exist.**
   `bynk-emit/src/project.rs`'s check-and-emit loop checks and emits each
   unit's `TypedCommons` within the same iteration and drops it; the only
   project-wide `expr_types` aggregation runs in `Mode::Analyse`, for the LSP
   only. `lower_field_default_wire` instead takes the field's expected
   `TypeRef` plus the visible `types: &HashMap<String, Arc<TypeDecl>>` table
   already in scope at codec emission, and resolves every syntactic
   ambiguity (a bare `Ident` vs. `FieldAccess` vs. a qualified
   `MethodCall`/`ConstructorCall`) the same way the checker does, narrowed to
   the field's specific expected type.
3. **Validation gates constructibility, not just type/purity.** Beyond
   Decision C's static/pure check (reused via a new `check_static_initialiser`
   refactored out of `check_state_initialiser`, both now thin wrappers around
   one private helper), `check_context_declarations` also calls
   `lower_field_default_wire` itself against the narrower
   `subscriber_visible_types` table (local + direct `uses` — the same,
   deliberately-narrower view a consuming subscriber's own codec generation
   actually sees, computed via the existing `symbols::combined_types_for`) —
   an `Err` here is folded into `bynk.event.bad_field_default` too, so
   nothing that fails to lower ever reaches emission having passed
   validation.
4. **Opaque-`unsafe` refinement smuggling is checked explicitly.**
   `T.unsafe(lit)` parses as `ExprKind::MethodCall { receiver: Ident(T),
   method: "unsafe", args: [lit] }` — not `ConstructorCall`, confirmed by
   direct AST inspection of real parsed source (a wrong assumption made and
   fixed twice, in both the checker's admission check and the emitter's
   lowering, before the correct shape was confirmed this way). A default
   using `T.unsafe(lit)` where `lit` fails `T`'s own refinement is rejected
   with a dedicated note: a default is spliced into the same codec that
   validates a real wire value on receipt, so a refinement-violating
   `.unsafe(lit)` default would compile cleanly and only fail at runtime the
   first time an old event actually triggers it.

Decisions A–D and C' from #972 are unchanged: the slice-3a/3b split (A),
deserialise-only (B), event-only scope (C) with non-event rejection as a new
diagnostic (C'), and the wire-key-absence-vs-`Option`-`None`-presence
distinction (D) — implemented via `"fname" in obj` (not `!== undefined`),
the only test that actually distinguishes "key never sent" from "key sent."

**Consequences.** `emit_record_codec`'s field tuple grows a third,
pre-lowered wire-JSON element (`Option<String>`); `serialise_<T>` is
unaffected (Decision B holds); `deserialise_<T>` splices `const __d_{fname} =
"{fname}" in obj ? obj["{fname}"] : {default};` ahead of the existing
per-field deserialise call. A generic-record instantiation (`RecordInst`)
never carries a default — events are never generic (`parse_event_decl`
always builds zero type params) — so that emission path is unaffected.

**A pre-existing, unrelated defect was found and deliberately not fixed
here.** A consumed boundary type whose field is a built-in generic
(`Option[T]`/`Result[T,E]`/`List[T]`) wrapping *another* consumed named type
loses that inner type's qualification in a subscriber's regenerated codec,
specifically when the type is reached only through
`emit_consumed_context_helpers`'s per-consumed-context `qual` map — producing
`tsc`-rejected TypeScript (`Cannot find name 'Region'`). Found via this
slice's own `Option[Region]`-defaulted-field fixture; not scoped to Events or
to defaults (the same `qual`-construction machinery backs an ordinary
cross-context service call with the same field shape), so fixing it here
would have bundled an unannounced, wider-blast-radius emitter change into a
feature PR. Reported as a standalone defect rather than fixed here; this
slice's own `Option`-default fixture uses `Option[Int]` instead, which needs
no named-type qualification and still proves the two-absences distinction
(Decision D) end to end.
