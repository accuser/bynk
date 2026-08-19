# 0380 — `emit_provider` stops re-deriving `op.return_type`'s own effectfulness twice; the full `ProviderShapeIr` sketch scoped back down

- **Status:** Accepted (v0.249.32)

summary: Same correction P6.53 made for `AgentShapeIr` — traced each proposed read's own downstream consumer before building anything, found most of it unjustified

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) proposed a
signature-only `ProviderShapeIr` for `emit_provider`'s remaining `ProviderDecl`/`ProviderOp` reads
(`p.provider_name.name`, `p.capability.name`, `op.name.name`, `is_effectful_return`), reasoning that
the existing `lower_provider_item_ir` is the wrong target (it routes through `lower_provider_op_ir`,
which lowers every op body — the same unmeasured panic exposure P6.53 declined for agents) and
`lower_provider_item_ir` has zero production call sites today.

Tracing each proposed read's own downstream consumer first (P6.24a's discipline, the same correction
P6.53 made) found most of the scope unjustified: `p.provider_name.name`/`p.capability.name`/
`op.name.name` are plain field accesses with `p: &ProviderDecl`/`op: &ProviderOp` staying required
parameters regardless (`p.documentation`/`p.given`/`p.ops`/`op.params`/`op.return_type`/`op.body` all
still need them) — the identical "buys nothing" finding P6.53 made for `a.name.name`.

**The concretely justified defect was narrower**: `emit_provider`'s per-op loop called
`is_effectful_return(&op.return_type)` twice for the same op in the same iteration (`async_kw`, then
`async_tail` a few lines later) — the identical duplicate-computation pattern P6.54 fixed in
`emit_agent`, just not caught in that slice because it lives in a different function. And the
`HandlerShared::capabilities` field still re-derived `p.given.iter().map(|c| c.key().to_string())`
independently, when `lower_provider_given_ir(p)` was already being called two lines above for the
`deps_ty` construction — the same pattern P6.54 converted three times elsewhere in this file.

**Decision.** `is_effectful_return(&op.return_type)` computed once per op, reused for both
`async_kw`/`async_tail`. `HandlerShared::capabilities` now reads `lower_provider_given_ir(p)`'s own
`CapRefIr::name` instead of re-deriving via `CapRef::key()`.

**Consequences.** `ast_importers`: **unaffected (5)**. Two real duplications closed, both matching a
pattern this phase already established elsewhere in the same file — not a new class of defect, the
same one caught twice more. `lower_provider_item_ir` stays unwired to a production call site, as
P6.53 left `lower_agent_item_ir` — both remain real, tested (`ir/lower.rs`'s own unit test suite
exercises `lower_provider_item_ir` directly), but production-unreached IR, an accurate state rather
than a gap to force-close. Verified: zero-diff bless over the full e2e fixture corpus, `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
