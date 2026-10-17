---
level: patch
changelog: A new Worker's first `bynk deploy` now uploads its secrets with its code (`wrangler deploy --secrets-file`, from a 0600 temporary file deleted when the upload ends), so the Worker is never live without them; it used to set them about four seconds after the upload. A redeploy still sets them first with `wrangler secret put`. Each secret set now prints a `bynk: set secret` line naming it and the Worker (`api-staging` under `--env staging`), never its value, beside the "already set, skipping" line, and a wrangler older than 4.74.0 falls back to the old order and says so (#2055).
---
