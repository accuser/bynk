---
level: patch
changelog: "P6.48: emitter::walk_unit_table_bodies (emitter.rs, already counted) replaces two hand-rolled copies of the same service/agent/provider body walk in project.rs's unit_table_uses_emit and called_cross_context_services. ast_importers: unaffected (6) -- project.rs's Block/Expr sites cleared, but the file was already counted on other names."
---

## ADR: p6-48-shared-unit-table-body-walk

title: `emitter::walk_unit_table_bodies` replaces two hand-rolled `project.rs` body walks

summary: Continues Phase G of the #1137 retirement plan — one behavioural change surfaced and verified safe against the full fixture corpus, not assumed

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase G) named `project.rs`'s
`unit_table_uses_emit`/`called_cross_context_services` — both walking every service handler, agent
handler, and (in one case) provider op body over a `&UnitTable` — as sharing one walk with
`emitter.rs`'s own `called_consumed_services` (which does the same walk over a `TypedCommons`'s
`CommonsItem` list instead). Only the two `project.rs` copies are addressed here — `emitter.rs` is
already counted by `ast_importers` regardless, so unifying its own copy buys no probe movement and
isn't attempted in this slice.

**Decision.** Added `emitter::walk_unit_table_bodies(table: &UnitTable, f: &mut impl FnMut(&Expr))`
in `emitter.rs` (already counted, so no probe cost to giving it the `&Expr` parameter type) — visits
every service handler, agent handler, and provider op body via the existing `walk_block_exprs`.
`called_cross_context_services`'s own walk already covered exactly this same three-collection scope
in the same order, so its own local loop is a direct, zero-risk replacement. `unit_table_uses_emit`'s
own `body_uses_emit` inner function covered only services and agents (no providers) — using the
shared, wider walker here is a real behavioural widening, not a pure relocation.

**Verified, not assumed, that the widening is safe.** Zero-diff bless over the full ~827-fixture e2e
corpus with the change applied confirms no emitted output differs — no fixture has a provider op body
that calls `Events.emit`, so the wider scan finds nothing new in practice today. This is corpus
evidence, not a proof that a provider op can never call `Events.emit` in principle; if one does in the
future, `unit_table_uses_emit` would now (correctly, arguably) also gate emission on it, where before
it silently would not have. Recorded here per §5's own "confirm, don't assume" discipline rather than
left implicit. A minor, deliberately-accepted perf change accompanies this: the original's `.any()`
short-circuited across handlers once `found` became `true`; the shared walker always visits every
remaining body (as `called_cross_context_services`'s own walker always did) — negligible at compose
time, not a hot path.

**Consequences.** `ast_importers`: **unaffected (6)** — `Block`/`bynk_syntax::ast::Expr` both gone
from `project.rs`'s own local scope (its import list already dropped `Block` as of this slice; the
crate stays counted regardless on its remaining names, `FnDecl`/`TypeDecl`/`TypeRef`/`Visibility`).
Phase G's remaining and final slice is P6.49. Verified: zero-diff bless over the full e2e fixture
corpus (see above), `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt
--all -- --check`.
