---
level: patch
changelog: "A `RehydrationViolation` is now logged, as the docs said it was (#1827). The load-time gate threw it without a log line, unlike an `InvariantViolation`, and an HTTP or `on call` entry point then caught it. `rehydrationViolation` now logs `RehydrationViolation <Agent>` with the agent, the field path and the kind of failure (never the key or the offending value) wherever a gate builds it"
---
