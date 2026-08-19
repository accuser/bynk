---
level: patch
changelog: "P6.40: ProtocolIr::Events::schema_dispatch flattened from Option<SchemaVersionPattern> to Option<i64> -- SchemaVersionPattern has exactly one variant (Literal(i64)), so mirroring the payload directly was simpler than introducing a one-variant IR-native enum purely to re-wrap it. Cuts over its one real reader (emitter/emit.rs's via schema(N) guard prologue), which now reads the i64 directly instead of destructuring the AST enum -- removing SchemaVersionPattern from both emit.rs's and ir.rs's own explicit AST import lists. Pinned by 1232_events_envelope_schema_dispatch_bare. ast_importers unaffected (7) -- emit.rs retains its own much larger remaining AST surface."
---

## ADR: schema-dispatch-ir-native

title: `ProtocolIr::Events::schema_dispatch` flattens to `Option<i64>`, cutting over its one real reader

summary: Phase F of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.40) — R6.13's own field-level gap, this one with a real consumer to prove the conversion against

**Context.** `ProtocolIr::Events::schema_dispatch: Option<SchemaVersionPattern>` reused the AST
wrapper verbatim, per its own doc comment's `[DECISION B]`: a one-variant `Clone` enum "reused, not
adapted," explicitly deferred until a real consumer needed to match on it without spelling
`bynk_syntax::ast`. `emitter/emit.rs`'s `via schema(N)` guard prologue is that consumer: it already
destructured `SchemaVersionPattern::Literal(version)` to interpolate `version` into the generated TS
guard — the exact trigger P6.24a's own precedent named for converting a reused-verbatim field.

**Decision.** `SchemaVersionPattern` has exactly one variant, `Literal(i64)` — introducing a
one-variant IR-native mirror enum purely to re-wrap a single `i64` would be pure ceremony, so
`schema_dispatch` flattens directly to `Option<i64>` instead of gaining an `IrSchemaVersionPattern`
sibling. `ir/lower.rs`'s construction site destructures the AST enum once, at the `Ast → Ir` boundary
(an excluded file — this is exactly its job), and `emit.rs`'s consumer now binds the `i64` directly
from the `Some(version)` pattern, dropping its own `let SchemaVersionPattern::Literal(version) =
dispatch;` line entirely. A future range pattern (`via schema(2..)`) widens this field's own shape
when it lands, not before — named in the field's own doc comment, matching the original's own framing.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — `emitter/emit.rs` retains its own much
larger remaining AST surface (Phase E/F's still-open rows). Removing `SchemaVersionPattern` from both
`emit.rs`'s and `ir.rs`'s own explicit import lists is real, if probe-invisible for `ir.rs` (excluded)
and probe-neutral for `emit.rs` (many other names remain), progress. Verified by a full zero-diff bless
against the entire e2e fixture corpus, including `1232_events_envelope_schema_dispatch_bare` — the
fixture that specifically pins this exact guard — and a full `cargo test --workspace`.
