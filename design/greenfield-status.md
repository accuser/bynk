<!-- GENERATED FILE — do not edit by hand.
     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).
     Regenerate with: cargo xtask greenfield-status --apply -->

# Greenfield status

Track slice T0.0 (#999). Nine probes are gated — a disagreement between this file and a fresh run fails `greenfield_status_table_is_current` (`xtask/tests/greenfield_status.rs`). Four are trend probes, reported only. No rule-id (`Closes-Rule:`) column yet — that provenance is a deferred follow-on slice (#999 Decision B).

| Probe | Gated | Reads |
|---|---|---|
| `workspace_lints` | yes | absent |
| `fs_below_driver` | yes | 6 files (bynk-emit=3, bynk-ide=2, bynk-fmt=1) |
| `options_sources` | yes | present |
| `hoist_sinks` | yes | 31 |
| `span_keyed_maps` | yes | 27 |
| `emit_diagnostics` | yes | bynk-emit=200/206, bynk-check=206/212 (true/naive) |
| `ide_emit_edge` | yes | present |
| `ast_importers` | yes | 13 |
| `emit_abi_shapes` | yes | 1 (bynk-cloudflare.ts:negotiateLocale) |
| `wildcard_arms` | no (trend) | 296 |
| `keep_in_sync` | no (trend) | 146 |
| `test_density` | no (trend) | bynk=13.8%, bynk-check=8.3%, bynk-driver=14.4%, bynk-emit=10.7%, bynk-fmt=15.5%, bynk-grammar=33.2%, bynk-ide=41.3%, bynk-lsp=36.0%, bynk-render=41.8%, bynk-strip=41.5%, bynk-syntax=11.3%, bynk-wasm=45.1%, bynkc=0.0%, xtask=27.2% |
| `fixture_kinds` | no (trend) | contains=3, absent=2, diagnostics=1, error=419 |
