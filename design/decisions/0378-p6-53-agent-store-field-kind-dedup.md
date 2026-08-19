# 0378 — `emit_agent`'s store-field-kind dispatch reads `StoreKindIr` instead of re-deriving it

- **Status:** Accepted (v0.249.30)

summary: Scoped down from the originally-planned full `AgentShapeIr` after tracing its actual downstream consumers — this slice keeps the concretely-justified defect closure and defers the rest

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) proposed a full
`AgentShapeIr`/`AgentHandlerShapeIr` pair mirroring an agent's declaration-level facts (`def`, `key`,
handler signatures, invariant/transition names) to replace roughly 55 `emit_agent` reads. Tracing each
of those reads against its actual downstream consumer before building anything (P6.24a's own
discipline — never land a mirror speculatively) found the plan's own scope too wide:

- `a.name.name` reads (~18 sites) are plain field accesses with no decision or duplication behind
  them — `a: &AgentDecl` stays a required parameter regardless (`a.documentation`/`a.store_fields`/
  `a.handlers`/`a.invariants`/`a.transitions` all still need it), so wrapping the name alone in a new
  type buys neither probe movement nor defect closure.
- `a.invariants`/`a.transitions` name reads sit inside loops that also need `inv.predicate`/
  `tr.predicate` (Q7 body-lowering, out of scope) — the loop needs the full `Invariant`/`Transition`
  struct regardless, so pre-computing just the names would need a parallel, order-matched list with no
  simplification to show for it. The one real consumer for name-only agent facts is `emitter.rs`'s
  `write_header` (`agent_needs_rehydrate`/`agent_has_held_storage`), already named as its own,
  separate, unproposed item in this track's own residual table (`the-ir.md` §7) — not this slice's.

**The concretely justified defect was narrower and different**: `emit_agent`'s five store-field
membership filters (`f.kind.head.name == "Cell"/"Map"/"Set"/"Cache"/"Log"`, string comparison against
a `TypeRef`'s own head name) and, for `Cache`/`Log` specifically, a *second* walk of `f.annotations`
to extract `@ttl`/`@retain` — both already computed once by `lower_store_field_shape_ir` and sitting
unused in `state: &[StoreFieldIr]`, which `emit_agent` was already threading in as a parameter
(`store_field_ty: HashMap<&str, &StoreKindIr>`, built at the top of the function, existed for exactly
one downstream site before this slice).

**Decision.** Every one of the five membership filters now checks `store_field_ty.get(name)` against
the typed `StoreKindIr` variant instead of the field's own string-compared head name. `store_cache_fields`'s
`ttl` and `store_log_fields`' `retain` now read `StoreKindIr::Cache(_, _, ttl)`/`StoreKindIr::Log(_,
retain)` directly instead of re-walking `f.annotations`/`ExprKind::DurationLit`. Each field's own
`&TypeRef` half is unchanged — still needed downstream by the rehydration check
(`serialisation.rs`'s `TypeRef`-driven boundary, P6.33's own excluded phase-7 territory).

**Consequences.** `ast_importers`: **unaffected (5)** — `emit.rs` stays counted regardless. Two real
defects closed: (1) a `Cell`/`Map`/`Set`/`Cache`/`Log`-named user type could previously mis-match a
store field's own string-compared kind (a name collision the checker doesn't reject, since `Map` etc.
are ordinary type names, not reserved words); typed dispatch cannot mis-fire this way. (2) The
Cache-`@ttl`/Log-`@retain` values were computed twice by two independent walks that could silently
diverge, emitting a wrong TTL/retain with nothing to catch it — now one computation, read twice.
Verified with extra care given the store-field-value touch: zero-diff bless over the full e2e fixture
corpus, with `227_store_cache_agent`/`228_store_log_agent` (the two fixtures whose source actually
declares `@ttl`/`@retain`) checked individually, `cargo test --workspace`, `cargo clippy --workspace
--all-targets`, `cargo fmt --all -- --check`.

**Deferred, not abandoned**: the originally-sketched `AgentShapeIr`'s remaining scope (agent-level
`def`/`key` and invariant/transition presence for `write_header`'s own remaining checks) stays
recorded in the residual table (`the-ir.md` §7) as its own, separately-proposable item — building it
now, with no consumer this slice actually found, would be exactly the speculative mirror P6.24a
argues against.
