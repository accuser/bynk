<!-- GENERATED FILE — do not edit by hand.
     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).
     Regenerate with: cargo xtask greenfield-status --apply -->

# Greenfield status

Track slice T0.0 (#999). Nine probes are gated — a disagreement between this file and a fresh run fails `greenfield_status_table_is_current` (`xtask/tests/greenfield_status.rs`). Four are trend probes, reported only.

| Probe | Gated | Reads |
|---|---|---|
| `workspace_lints` | yes | present — wildcard_enum_match_arm = "warn" |
| `fs_below_driver` | yes | 6 files (bynk-emit=4, bynk-ide=2, bynk-fmt=0) |
| `options_sources` | yes | present |
| `hoist_sinks` | yes | 0 |
| `span_keyed_maps` | yes | 27 |
| `emit_diagnostics` | yes | bynk-emit=201/207, bynk-check=206/212 (true/naive) |
| `ide_emit_edge` | yes | present |
| `ast_importers` | yes | 13 |
| `emit_abi_shapes` | yes | 1 (bynk-cloudflare.ts:negotiateLocale) |
| `wildcard_arms` | no (trend) | 293 |
| `keep_in_sync` | no (trend) | 146 |
| `test_density` | no (trend) | bynk=13.8%, bynk-check=8.5%, bynk-driver=14.4%, bynk-emit=8.6%, bynk-fmt=15.6%, bynk-grammar=33.2%, bynk-ide=41.6%, bynk-lsp=34.9%, bynk-render=41.8%, bynk-strip=41.5%, bynk-syntax=10.8%, bynk-wasm=45.1%, bynkc=0.0%, xtask=33.6% |
| `fixture_kinds` | no (trend) | contains=3, absent=2, diagnostics=5, error=421 |

## Rules closed

See [`design/greenfield-status-rules.md`](greenfield-status-rules.md) for rule ids closed so far (written by `cargo xtask stamp --apply` at merge; may not exist yet if no increment has cited `closes_rule`).
