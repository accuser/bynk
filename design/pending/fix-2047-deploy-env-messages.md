---
level: patch
probes: diagnostic_coverage
changelog: Under `bynk deploy --env staging`, every command a deploy message suggests repeats the environment (`bynk deploy --env staging`, `bynk deploy --env staging --force`), where a plain `bynk deploy` would have deployed the default environment, and every deploy message that names a Worker (the contract-skew refusal, edge-key and secret notices, the progress line, a failed push, the foreign caller hand-off) names the deployed `-staging` Worker, as the plan does (#2047).
---
