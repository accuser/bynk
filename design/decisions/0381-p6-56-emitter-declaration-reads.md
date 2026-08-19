# 0381 — `called_consumed_services` reads `Callee::Cross`; six other proposed `emitter.rs` sites investigated and declined

- **Status:** Accepted (v0.249.33)

summary: One real defect closed; the rest of this plan row's own scope did not survive tracing against the tree — six honest declines, not force-fits

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b) named seven `emitter.rs`
sites as its Phase H conversion targets. Tracing each against the current tree, one at a time,
found one genuinely convertible and six that were not — for concrete, structural reasons, not
convenience.

**Landed.**

`called_consumed_services` reconstructed cross-context-ness syntactically: `ExprKind::MethodCall`'s
receiver flattened via `flatten_emit_ident_chain`, then resolved via `CrossContextInfo::
resolve_prefix`. This is the identical resolution the checker's own `Callee::Cross { unit, service }`
already performed once, at check time, per call site — the same shadowing-hazard class
`block_uses_emit` closed for `Events.emit` (#1202), and the same conversion `project::
called_cross_context_services` already made (this function's own doc comment calls itself "a copy"
of that one). Now reads `commons.callees.get(&e.id)` directly; the `ExprKind::MethodCall` match and
`flatten_emit_ident_chain` call are gone from this site entirely — this is the highest-value item in
the phase, a live correctness defect closed, not a tidy.

**Investigated and declined, each recorded in-place with the tracing that ruled it out** (so a future
reader does not re-attempt the same dead end):

- **`collect_json_codec_roots`** — `resolver.rs` resolves `Json.encode`/`Json.decode` as a
  no-declaration-needed built-in static (alongside `List.empty`/`Map.empty`/`Duration.millis`/…),
  never through the `Callee`-classification machinery. `commons.callees` carries no entry for this
  call site at all — there is no IR-native value to read.
- **`refined_or_opaque_base` / `emit_context_rebrands`** — `TypeShape::Refined`'s own `base` field is
  `bynk_syntax::ast::BaseType` (P6.41 ruled it stays, phase 7). This function's `Option<BaseType>`
  return type is identical whether read off `TypeBody` or `TypeShape` — converting adds a
  `CheckedProgram`/`TypedCommons` dependency for zero reduction in AST-type surface.
- **`sum_owner_of_variant` / `positional_field_name`** — both need only a variant's own *name*
  (membership or a payload field's own name string), answerable with a zero-cost, infallible string
  comparison against `TypeBody::Sum` today. Routing through `TypeShape::Sum` would resolve every
  payload field's own `TyId` for every variant just to answer that question — real resolution work
  plus a new `.unwrap_or_else(|| panic!(..))` panic path on any variant's field, not just the one
  being asked about, that neither call site has today.
- **`is_refined_is_check`** — reads only `TypeBody`'s own discriminant, never `base`/`refinement`;
  the same reasoning as `refined_or_opaque_base` applies unchanged.
- **`ts_binop`** — its sole caller (`emitter/lower.rs`) holds an AST `BinOp` and separately compares
  `op == BinOp::Eq` a few lines away; converting here would only relocate the AST read into that
  still-AST-walking caller. Net zero, as this plan's own row already flagged before landing.

**Consequences.** `ast_importers`: **unaffected (5)** — `emitter.rs` stays counted regardless of any
of this. One real R6.13 defect closed (`called_consumed_services`'s shadowing hazard); six proposed
conversions correctly declined rather than force-built for a probe number that would not have moved
either way. Verified: zero-diff bless over the full e2e fixture corpus, with
`1203_cross_context_call_shadowed_by_local` (the fixture pinning this exact shadowing-hazard class)
checked individually, `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt
--all -- --check`.
