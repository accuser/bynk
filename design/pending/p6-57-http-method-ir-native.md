---
level: patch
changelog: "P6.57: IrHttpMethod gains from_ident(); emitter/lower.rs's two HttpMethod::from_ident sites read it instead. Corrects P6.37's stated reason for leaving system_http_route_body's TypeRef field alone -- the resolution context does exist, the real reason is a lossy round-trip with no probe payoff. Closes Phase H of the #1137 retirement plan. ast_importers: unaffected (5)."
---

## ADR: p6-57-http-method-ir-native

title: `emitter/lower.rs`'s `HttpMethod::from_ident` sites read `IrHttpMethod`; P6.37's stated reason for `system_http_route_body` corrected

summary: Closes Phase H — after this slice, `emitter/lower.rs`'s residue is provably 100% Q7 body-rendering plus phase-7 codec, not untried

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase H) named
`emitter/lower.rs`'s two `bynk_syntax::ast::HttpMethod::from_ident` call sites (both classifying a
system-http test-address call's own verb) as tractable in isolation, and flagged that landing them
would make the file's residue *provably* Q7 + phase-7 — the evidence Phase I's re-settling needs to
argue the floor is structural rather than untried.

It also flagged a correction to record: P6.37's own slice-history entry (Forty-eighth,
`design/tracks/the-ir.md`) states `system_http_route_body`'s `TypeRef` field has "no `TypedCommons`/
`tys` resolution context in scope" at its construction site (`emit_system_http_support`,
`project/tests_emit.rs`). Tracing the call chain found this half of the claim inaccurate:
`emit_system_http_support` is called from `emit_integration_module`, whose own signature already
takes `tys: &Arc<Types>`, and six lines later that same function builds
`integration_typed_commons(...)` over the same intern arena the case bodies later lower against. The
resolution context genuinely exists, two frames up.

**Decision.** `IrHttpMethod` gains `from_ident(&str) -> Option<IrHttpMethod>` (`ir.rs`), a
field-for-field mirror of `bynk_syntax::ast::HttpMethod::from_ident`, alongside P6.51's `as_str()`.
Both `emitter/lower.rs` call sites now match `IrHttpMethod::from_ident(&method.name)` and pass the
result to P6.51's `http_handler_method_name_ir` instead of the AST-typed
`http_handler_method_name`.

`system_http_route_body`'s own `TypeRef` field is **not converted** — the correction is to the stated
*reason*, not the outcome. The context exists, but converting would still be a lossy round-trip:
`TypeRef → TyId` (at the construction site) then `TyId → TypeRef` (at the sole consumer,
`serialise_expr_via`, P6.33's own ruled-phase-7 codec renderer) passes through two `Option`-returning
functions, and a silent `None` at either end would downgrade emitted output from
`JSON.stringify(serialise_X(...))` to `String(...)` — a wrong-bytes bug, for a conversion that clears
no probe count regardless (the file stays locked by both its own Q7 import list and the `use
super::*;` rule pointing at `emitter.rs`).

**Consequences.** `ast_importers`: **unaffected (5)** — `bynk_syntax::ast::HttpMethod` no longer
appears anywhere in `emitter/lower.rs`, but the file stays counted on its Q7 import list and the
super-glob rule regardless. **Closes Phase H.** After P6.50–P6.57, `emitter/lower.rs`'s residue is
provably: the L10 23-name Q7 body-rendering import list (permanent under §3.7/Q7), and
`system_http_route_body`'s `TypeRef` field (permanent under P6.33's codec ruling, now for a
correctly-stated reason) — nothing untried remains. `emitter.rs`/`emitter/emit.rs` carry their own
residue, argued in each landed Phase H slice's own pending file. Phase I (re-settling + retirement)
is what remains of the #1137 retirement plan.

Verified: zero-diff bless over the full e2e fixture corpus, `tsc --strict` verification (all six
`tsc_verify.rs` checks — this changes an emitted key string's own construction path), `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
