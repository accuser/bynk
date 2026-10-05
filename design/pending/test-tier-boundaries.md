---
level: minor
changelog: A `unit` or `integration` test case can't reach another context, and now says so. A case that calls another context's service, directly or through a service or agent handler that calls one, is `bynk.tier.cross_context_needs_system` (#1737); it used to compile and then crash at runtime, since no consumed context runs in-process and `stub` doubles capabilities only. A suite that mixes `system` cases with lower-tier ones is `bynk.tier.mixed_system_suite` (#1738); it used to emit invalid TypeScript, because a `system` case addresses services by context path and one suite can't be both. Keep each target's in-process cases and its `system` cases in separate suites
---
