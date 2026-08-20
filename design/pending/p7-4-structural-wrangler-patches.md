---
level: patch
changelog: "P7.4: `bynk deploy`/`bynk dev --remote` and `--emit js` now read/write a generated `wrangler.toml`'s `id`/`main` fields through a real TOML parse (`bynk-emit::emitter::wrangler`'s new `materialise_kv_namespace_id`/`set_wrangler_main`/`wrangler_needs_kv_materialisation`) instead of matching the literal text those fields happen to appear as today (#1305). Immune to reformatting; no observable output change."
---
