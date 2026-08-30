<!-- GENERATED FILE — do not edit by hand.
     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).
     Regenerate with: cargo xtask greenfield-status --apply -->

# Greenfield status

Track slice T0.0 (#999); `ts_writes`/`ts_any` added by P7.0 (#1296); `verbatim_origins`/`verbatim_sites` added by P7.5 (#1307); `incremental_query_types`/`keystroke_latency` added by P8.0 (#1510). Fourteen probes are gated — a disagreement between this file and a fresh run fails `greenfield_status_table_is_current` (`xtask/tests/greenfield_status.rs`). Five are trend probes, reported only.

| Probe | Gated | Reads |
|---|---|---|
| `workspace_lints` | yes | present — wildcard_enum_match_arm = "warn" |
| `fs_below_driver` | yes | 0 files (bynk-emit=0, bynk-ide=0, bynk-fmt=0) — 0 named floor, 0 residual total |
| `options_sources` | yes | present |
| `hoist_sinks` | yes | 0 |
| `span_keyed_maps` | yes | 4 |
| `emit_diagnostics` | yes | bynk-emit=4/6, bynk-check=389/397 (true/naive) |
| `ide_emit_edge` | yes | absent |
| `ast_importers` | yes | 5 |
| `emit_abi_shapes` | yes | 1 (bynk-cloudflare.ts:negotiateLocale) |
| `ts_writes` | yes | 809 |
| `ts_any` | yes | 26 |
| `verbatim_origins` | yes | 1 |
| `verbatim_sites` | yes | 2 |
| `incremental_query_types` | yes | query_types 1/4 (UnitSignature); shared_cache not migrated (PROJECT_UNIT_CACHE still bynk-ide-local); stability_test absent |
| `wildcard_arms` | no (trend) | 320 |
| `keep_in_sync` | no (trend) | 235 |
| `test_density` | no (trend) | bynk=13.6%, bynk-check=9.6%, bynk-driver=22.3%, bynk-emit=11.8%, bynk-fmt=15.6%, bynk-grammar=33.2%, bynk-ide=40.9%, bynk-ir=0.0%, bynk-lower=66.6%, bynk-lsp=35.7%, bynk-project=33.6%, bynk-render=41.8%, bynk-strip=53.5%, bynk-syntax=10.7%, bynk-testkit=0.0%, bynk-ts=59.6%, bynk-wasm=45.1%, bynkc=0.0%, xtask=39.2% |
| `fixture_kinds` | no (trend) | contains=3, absent=2, diagnostics=5, error=424 |
| `keystroke_latency` | no (trend) | not measured — no scheduler exists yet (R3.15, deferred whole this phase) |

## Rules closed

See [`design/greenfield-status-rules.md`](greenfield-status-rules.md) for rule ids closed so far (written by `cargo xtask stamp --apply` at merge; may not exist yet if no increment has cited `closes_rule`).
