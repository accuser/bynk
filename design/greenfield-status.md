<!-- GENERATED FILE — do not edit by hand.
     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).
     Regenerate with: cargo xtask greenfield-status --apply -->

# Greenfield status

Track slice T0.0 (#999); `ts_writes`/`ts_any` added by P7.0 (#1296); `verbatim_origins`/`verbatim_sites` added by P7.5 (#1307); `incremental_query_types`/`keystroke_latency` added by P8.0 (#1510); `unconsumed_ir_items` added by Slice D3 of the IR cutover (#1542); `diagnostic_coverage` added by #1662. Sixteen probes are gated — a disagreement between this file and a fresh run fails `greenfield_status_table_is_current` (`xtask/tests/greenfield_status.rs`). Five are trend probes, reported only.

| Probe | Gated | Reads |
|---|---|---|
| `workspace_lints` | yes | present — wildcard_enum_match_arm = "warn" |
| `fs_below_driver` | yes | 0 files (bynk-emit=0, bynk-ide=0, bynk-fmt=0) — 0 named floor, 0 residual total |
| `options_sources` | yes | present |
| `hoist_sinks` | yes | 0 |
| `span_keyed_maps` | yes | 4 |
| `emit_diagnostics` | yes | bynk-emit=4/8, bynk-check=395/403 (true/naive) |
| `ide_emit_edge` | yes | absent |
| `ast_importers` | yes | 5 |
| `emit_abi_shapes` | yes | 1 (bynk-cloudflare.ts:negotiateLocale) |
| `ts_writes` | yes | 835 |
| `ts_any` | yes | 26 |
| `verbatim_origins` | yes | 2 |
| `verbatim_sites` | yes | 11 |
| `incremental_query_types` | yes | unit_signature present; shared_cache migrated; stability_test present; definition/project levels absent (deleted by #1537) |
| `unconsumed_ir_items` | yes | 0 |
| `diagnostic_coverage` | yes | unasserted=4 (asserted 468/472) |
| `wildcard_arms` | no (trend) | 326 |
| `keep_in_sync` | no (trend) | 202 |
| `test_density` | no (trend) | bynk=15.9%, bynk-check=10.8%, bynk-driver=23.3%, bynk-emit=12.4%, bynk-fmt=15.9%, bynk-grammar=33.2%, bynk-ide=43.3%, bynk-ir=0.0%, bynk-lower=66.1%, bynk-lsp=37.3%, bynk-project=37.5%, bynk-render=47.3%, bynk-strip=53.5%, bynk-syntax=14.1%, bynk-testkit=0.0%, bynk-ts=53.7%, bynk-wasm=45.7%, bynkc=0.0%, xtask=40.1% |
| `fixture_kinds` | no (trend) | contains=3, absent=2, diagnostics=7, error=632, warnings=6, run=107 |
| `keystroke_latency` | no (trend) | not measured — no scheduler exists yet (R3.15, deferred whole this phase) |

## Rules closed

See [`design/greenfield-status-rules.md`](greenfield-status-rules.md) for rule ids closed so far (written by `cargo xtask stamp --apply` at merge; may not exist yet if no increment has cited `closes_rule`).
