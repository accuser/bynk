# 0353 — `RuntimeUse::json_codec_roots` carries `TyId`, not `TypeRef`; the `TypeRef` conversion moves from two push sites to the one drain site

- **Status:** Accepted (v0.249.5)

summary: Phase B of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.28) — the first slice to fully clear a file

**Context.** `emitter/runtime_use.rs`'s `RuntimeUse::json_codec_roots: RefCell<Vec<TypeRef>>` accumulates
the `Json.decode[T]`/`Json.encode` target types a test-scaffold module's bodies reach for, drained once
by `project/tests_emit.rs` to compute a codec closure via `bynk_check::wire::collect_codec_closure` (a
genuinely `TypeRef`-driven function, owned by `bynk-check`, out of this slice's scope). Both push sites
in `emitter/lower.rs`'s `lower_json_codec_call` already resolve the target as a `TyId`
(`expr_types.get(...).map(|te| &te.ty)` / `Ty::Result(t, _)`) before calling `ty_to_type_ref` — and that
`TypeRef` (`tref`) is genuinely needed at each site regardless, for `serialise_expr`/`ts_type_ref_qualified`/
`deserialise_expr`'s own codec rendering. The `note_json_codec_root(tref.clone())` push existed only to
satisfy `RuntimeUse`'s own field type, one call downstream of a conversion the function was already doing
for an unrelated reason.

**Decision.** `json_codec_roots` becomes `RefCell<Vec<TyId>>`; `note_json_codec_root`/
`take_json_codec_roots` change signature to match. Both push sites in `emitter/lower.rs` now push the
`TyId` they already hold (`arg_ty`/`t`, both `Copy`) — their own `tref` local, and everything downstream
of it in that function, is untouched. `project/tests_emit.rs`'s drain converts once,
`.filter_map(|ty| crate::emitter::ty_to_type_ref(ty, tys))`, immediately before
`collect_codec_closure` — the same filtering `ty_to_type_ref`'s `Option` return already did at push time
(functions/effects/type-vars silently dropped), just relocated. `emitter::ty_to_type_ref` becomes
`pub(crate)` so `project/tests_emit.rs` (a sibling module tree of `emitter`, not a descendant, so it
cannot reach a private `fn` there) can call it.

**Consequences.** `ast_importers`: **9 → 8** — `emitter/runtime_use.rs` no longer spells
`bynk_syntax::ast` anywhere, the first file this completion plan (§6a) has fully cleared. Verified by a
full zero-diff bless against the entire e2e fixture corpus — the risk this slice's own plan entry named
(filtering moving from push-time to drain-time potentially reordering `collect_codec_closure`'s input)
did not materialize; bless is byte-identical. `cargo check --workspace --all-targets` is clean with zero
warnings. Full `cargo test --workspace` green.
