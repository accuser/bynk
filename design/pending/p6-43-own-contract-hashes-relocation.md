---
level: patch
changelog: "P6.43: own_contract_hashes moves into bynk-check::contract, sharing one cross_context_service_for projection with build_cross_context_info -- the caller-side and callee-side CrossContextService views were two hand-written copies agreeing only by convention; now one function. ast_importers: unaffected (6)."
---

## ADR: p6-43-own-contract-hashes-relocation

title: `own_contract_hashes` relocates into `bynk-check::contract`, sharing one `CrossContextService` projection with `build_cross_context_info`

summary: Fixes a real duplication hazard (two independently-maintained copies of the same projection, agreeing only by convention) while also clearing the probe-visible AST reference it forced into `bynk-emit`

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b) named `project.rs`'s
`own_contract_hashes` as one of the file's remaining `ast_importers` sites — its
`own_types: &HashMap<String, Arc<bynk_syntax::ast::TypeDecl>>` parameter spelled the AST module
directly. §6a's own P6.33 re-settling had already ruled the *hash derivation* permanently AST-bound
(a context's `X-Bynk-Contract` hash is compared at runtime between separately compiled, separately
deployed binaries — potentially built by different compiler versions — so its normal form needs
record field names *and* types, sum variant payload lists, refined-predicate sets, the opaque/refined
distinction, and generic type-parameter names for `subst`; none of which a bare `TyId` carries).

But the probe-visible reference is a different question than the derivation, and re-reading the
function against `bynk-check/src/symbols.rs`'s `build_cross_context_info` found something P6.33 did
not: the two functions build the *same* `CrossContextService` projection from a `ServiceDecl`'s `on
call` handler — `project.rs:3628-3651` (deleted by this slice) and `symbols.rs:868-889` were
field-for-field identical eight-line blocks, kept in sync only by whoever remembered to update both
when one changed. A divergence between them is not cosmetic: it is exactly the skew the contract-hash
mechanism exists to catch, except invisible to it, because both sides would agree with themselves
while disagreeing with each other.

**Decision.** Extract the shared projection into one function,
`bynk_check::resolver::cross_context_service_for(name, sdecl) -> Option<CrossContextService>`, next
to `CrossContextService` itself. `build_cross_context_info` (`symbols.rs`) now calls it in its own
per-service loop instead of its inline block. `own_contract_hashes` moves in its entirety into
`bynk_check::contract`, beside `service_contract_hash`/`contract_hash` — the functions it was already
calling into another crate to reach — and calls the same shared projection. `bynk-emit/src/project.rs`
keeps only its three call sites, retargeted to `bynk_check::contract::own_contract_hashes`.

This is not a re-export (the case decision 2 restricts to an already-public API surface): the whole
function relocates, because its natural owner is the crate that already owns `CrossContextService`,
`service_contract_hash`, and `UnitTable`. `bynk-emit` never read a `TypeDecl` field in this path — it
only forwarded the combined type table `bynk_check::symbols::combined_types_for` had already built.

**Consequences.** `ast_importers`: **unaffected (6)** — `project.rs` still imports the AST on its
remaining names (`Block`, `CommonsItem`, `FnDecl`, `FnName`, `HandlerKind`, `ServiceProtocol`,
`TypeDecl`, `TypeRef`, `Visibility`); Phase G continues in P6.44–P6.49. `symbols.rs` loses its own
`HandlerKind` import (down to the shared projection's use inside `resolver.rs`, which already does
`use bynk_syntax::ast::*;`).

The hash derivation itself is unchanged byte-for-byte — `service_contract_hash`/`contract_hash`/
`service_normal_form` are untouched, and the caller-side/callee-side projections now literally share
one code path instead of two that happened to agree. Verified: zero-diff bless over the full e2e
fixture corpus, with the `X-Bynk-Contract`-emitting fixtures (`137_agent_instantiation_workers`,
`254_multi_file_commons_workers_codec`, `170_cross_ctx_capability_workers`, and others) inspected by
eye per this slice's own extra caution — a contract-hash change is exactly the kind a green bless
could hide behind an unchanged byte count if the projection diverged subtly. `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check` all pass.
