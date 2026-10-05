---
level: patch
changelog: Generated `wrangler.toml` files now pin `compatibility_date = "2026-07-01"` (it was `2024-11-01`), so deployed Workers run under current Workers runtime behaviour. Local `bynk dev` needs wrangler 4.107.0 or newer (July 2026); an older one refuses the date rather than falling back. The first review under the compatibility-date policy found no flag in between that changes how a Bynk program behaves
---
