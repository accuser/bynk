# 0412 — `UnitSignature` is a new type wrapping `combined_types_for`'s existing output unchanged, plus fresh fn/handler/storage/capability-set projections read from `UnitTable`, compared through a canonical span/trivia-erased rendering rather than raw AST values

- **Status:** Accepted (v0.289.49)

**Context.** R3.14 needs `UnitSignature` to cover four categories design notes §15 already names
as required annotations: function/handler declarations, agent storage declarations, cross-context
type references, and capability sets via `given`. ADR 0200's `combined_types_for`
(`bynk-check/src/symbols.rs:1147`) was this track's opening candidate for "the query already
exists in substance," but settling found it computes exactly one of the four categories
(cross-context type references, as a plain `HashMap<String, Arc<TypeDecl>>`) and is structurally
incapable of the other three — it never reads `UnitTable.fns`, `.agents`, `.services` or
`.capabilities` at all. It has 7 real call sites across `bynk-check` and `bynk-emit` (confirmed
fresh against the current tree: `symbols.rs:862`, `analysis.rs:666`, `check_pipeline.rs:284`,
`bynk-emit/src/project.rs:927,2227,2402,2428`), every one depending on its current, narrow,
types-only return shape for cross-context resolution and the contract hash itself.

Settling also checked the next-broadest candidate — `UnitTable` (`symbols.rs:295`), the per-unit
table `combined_types_for` itself reads from — to see whether *it* was already close to
signature-shaped. It is not: `UnitTable.fns: HashMap<String, Arc<FnDecl>>` and every `Handler`
reachable through `UnitTable.agents`/`.services` both carry a full `body: Block`
(`bynk-syntax/src/ast.rs:2005,1208`) alongside their declared signature — an edit to a function or
handler body changes the `FnDecl`/`Handler` value itself. `StoreField` (`ast.rs:945`) is closer —
`name`/`kind: StoreKind` are body-free — but carries `init: Option<Expr>`, an initialiser
expression, which is not. No existing type is `UnitSignature`-shaped without stripping something
that would break R3.14's own stability requirement.

One direct, contemporaneous in-repo precedent bears on the "widen vs. build" choice itself:
`combined_types_for_unit_info` (`symbols.rs:1170`) is a sibling function, deliberately
reimplemented against `UnitInfo` rather than calling `combined_types_for` directly, because the
per-unit emission prologue it serves has a genuinely different shape (a caller already holding
`UnitInfo`, not the flat project-wide `unit_tables`/`unit_uses` maps) than `combined_types_for`'s
own callers. This codebase already treats "build a parallel function alongside, rather than widen
one whose contract genuinely differs" as the right move when call-context shapes diverge — the
same reasoning this decision applies at the type level.

**Decision.** `UnitSignature` is a new type. It contains `combined_types_for`'s own output as one
field, completely unmodified — `combined_types_for` itself is not touched, and its 7 call sites
keep their current contract exactly as today. The remaining three categories are built fresh,
directly from `UnitTable`, each stripped to signature shape:

- Function/handler declarations: `FnDecl`'s `name`/`type_params`/`params`/`return_type`/`has_self`
  (not `body`, not `requires`/`ensures`) and every `Handler`'s
  `method_name`/`params`/`return_type`/`given` (not `body`).
- Capability sets via `given`: the same `Handler.given`/`ProviderDecl.given`/
  `ServiceDecl.default_given` fields, plus `UnitTable.exported_capabilities` copied as-is.
- Agent storage declarations: `StoreField`'s `name`/`kind: StoreKind` only — not `init`, not
  `annotations`.

`Artefacts` (phase 7's typed emit-side document set, R7.8) gets no signature concept of its own in
this phase. Design notes §15's annotation policy — the firewall's own foundation — is a check-side
contract; nothing in this phase's scope proposes an emit-side query to key one against.

**A field-exclusion list is not enough on its own: every fragment above carries a `Span`, and
several (`TypeDecl`, reused unchanged from `combined_types_for`'s own output) also carry `trivia`/
`documentation`.** `Ident` (`ast.rs:7`), `Param` (`ast.rs:2179`), every `TypeRef` variant
(`ast.rs:2187` onward) and `TypeDecl` (`ast.rs:1529`) each carry their own `Span`. Editing a
function body changes that file's byte length, shifting the `start`/`end` of every span *after*
the edit point in the same file — including every later declaration's own name, params and return
type, and every field of `combined_types_for`'s reused `TypeDecl` values. Comparing
`UnitSignature` as literal AST values would make it unstable under exactly the edit R3.14 says it
must survive. This is not a new erasure scheme to invent: `bynk-check/src/contract.rs`'s
`canon_type`/`service_normal_form` (ADR 0200) already solves the identical problem — a canonical
form that must compare equal across two semantically-identical builds regardless of source layout
— by rendering each `TypeRef` to a plain `String` and discarding every span (`contract.rs:103`
onward, every match arm binds `_` where a `Span` would be). `UnitSignature`'s own stability
comparison reuses that technique, extended to cover `FnDecl`/`Handler`/`StoreField` shapes
`canon_type` doesn't reach today: every field is rendered through a canonical form, and R3.14's
proof (see the accompanying track-doc §3.4/Q4) is defined over that rendering, never over the raw
AST values.

**Consequences.** Every one of `combined_types_for`'s 7 existing callers is unaffected by this
track landing — no widened signature to thread through them, no risk to `bynkc/tests/contract_hash.rs`'s
own no-false-positive guarantee (which never called the function directly, only named it in a doc
comment, so it wouldn't have caught a signature change either way). `UnitSignature` composes
`combined_types_for`'s output rather than duplicating it, satisfying phase 1's "no fact in two
hand-synced copies" invariant by construction. Reusing `canon_type`'s own technique for the span
erasure means R3.14's proof rests on machinery this codebase already trusts (guarded by
`contract_hash.rs`'s own no-false-positive fixture) rather than a new, unproven erasure path. P8.1
(the track doc's own §6) is the slice that builds this; P8.2–P8.5 build against its exact field
list and its canonical rendering.
