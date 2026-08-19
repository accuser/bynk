---
level: patch
changelog: "P6.35: plan_agent_given_deps (project.rs) no longer spells an explicit &AgentDecl type annotation -- the loop body's own a.handlers field access already pins the element type through inference, the same fully-mechanical finish the completion plan named for this function. Investigated and confirmed, no code change needed: unit_table_uses_emit/called_cross_context_services's own remaining Block/Expr sites are walk_block_exprs's own callback signature, the same Q7-deferred body-walking-plumbing shape as every other still-AST-typed rendering parameter in this track -- no IR-native body walker exists to route through instead until a future P6.2 emitter-side cutover lands, so this is not fixable within this slice's scope. instantiate_provider_expr confirmed already AST-free, as the plan itself already found. ast_importers unaffected (7) -- project.rs retains other, still-open AST surface (own_contract_hashes, build_output's declaration reads, etc)."
---

## ADR: project-plumbing-finish

title: `plan_agent_given_deps` drops its last explicit `&AgentDecl` annotation; `unit_table_uses_emit`/`called_cross_context_services`'s `Block`/`Expr` residue found structural, not fixable here

summary: Phase E of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.35) — the mechanical part of this row lands; the rest is investigated and found out of reach, not silently converted

**Context.** This row named three `project.rs` functions as nearly finished: `plan_agent_given_deps`
(only an `&AgentDecl` annotation left), `unit_table_uses_emit`/`called_cross_context_services` (only
`Expr`/`Block` types left), and `instantiate_provider_expr` (claimed already AST-free).

**`plan_agent_given_deps`: the mechanical fix, landed.** `agents: Vec<(&String,
&bynk_syntax::ast::AgentDecl)>` no longer names the AST type — `Vec<_>` infers it from the loop body's
own `a.handlers` field access two lines down. The `CapRef` walk itself was already `CapRefIr` (#1187's
slice 6); this was the one remaining literal spelling.

**`unit_table_uses_emit`/`called_cross_context_services`: investigated, found structural, not
converted.** Both functions' own *decisions* already read `Callee::Capability`/`Callee::Cross` — real,
prior work. What remains is `body: &Block` and the `Fn(&Expr)` closure parameter `walk_block_exprs`
itself requires — `crate::emitter::walk_block_exprs` is a plain AST-tree walker with no IR-native
equivalent, because bodies are not lowered to IR at any call site reachable from these two functions
(the emitter-side `Call`/`Lambda` cutover this would need — P6.2's own remaining half, `lower_method_call`/
`lower_call` — has not landed). This is the identical "no IR-native alternative exists yet" shape P6.33
just ruled the codec renderer's own AST surface into phase 7 for — `Block`/`Expr` here are Q7-deferred
body-walking plumbing, not a fixable R6.13 declaration read. Left as raw AST, named rather than
silently left looking unconverted.

**`instantiate_provider_expr`: confirmed, no change needed.** Re-verified: its signature carries no
AST type at all — its only "provider" contact is `UnitTable::providers: HashMap<String, ProviderDecl>`,
a `bynk-check` type reached through `unit_tables: &HashMap<String, UnitTable>`, invisible to this
probe by construction. The plan's own claim holds; stated here explicitly so this function does not
read as accidentally-still-open.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — `project.rs` retains other, still-open AST
surface (`own_contract_hashes`, `build_output`'s own declaration reads, and others not in this row's
scope) regardless of this slice landing. Verified by a full zero-diff bless against the entire e2e
fixture corpus (a type-annotation removal cannot alter emitted output) and a full `cargo test
--workspace`.
