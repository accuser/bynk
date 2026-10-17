---
level: minor
probes: emit_diagnostics, diagnostic_coverage
changelog: A `stub` in a `system`-tier case, or at the scope of a suite that runs `system` cases, is now `bynk.stub.at_system` instead of being accepted and ignored, since each context runs as the Worker it deploys as and builds its own capability providers; keep a case that needs a stubbed seam at `unit` or `integration`
---
