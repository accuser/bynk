---
level: patch
changelog: A test now pins that every internal path a Worker answers (`/_bynk/call/`, `/_bynk/event/`) checks the caller's signature before anything else that can answer, so an unauthenticated request gets a bare `401` and never the `409` `ContractMismatch` body that names the expected hash; that body is unchanged (#2054).
---

## ADR: contract-mismatch-body-keeps-expected
title: The 409 ContractMismatch body keeps `expected`, because only an authenticated caller can reach it
summary: The signature check precedes the contract check on every internal path; the published 409 body is unchanged

**Context.** A `/_bynk/call/<service>` request without the right `X-Bynk-Contract` header gets a `409` whose body is `{"kind":"ContractMismatch","service":…,"expected":…,"actual":…}`. Before ADR 0479 (the deployment call secret) and ADR 0485 (per-edge signatures), anyone who could reach a Worker could read the expected hash with a single request (#2054). The hash is not a credential (ADR 0479), but the body made finding it trivial. The issue asked for two things: confirm that the caller check precedes the contract check on every internal path, and decide whether the body should still carry `expected`.

**Decision.** Signature verification (`__verifyCall`) runs first on every internal path a Worker entry answers. On `/_bynk/call/`, only the `405` method gate comes before it. On `/_bynk/event/`, nothing does, and that path makes no contract check at all. A request that does not verify gets a bare `401` before the service lookup, the wire-version check, the contract comparison or the body parse. `/_bynk/agent/` and `/_bynk/fanout` are Durable Object paths reached only through a stub, not Worker routes. A golden-wide test (`every_internal_path_verifies_the_caller_before_any_other_answer`) now pins this ordering, and it also fails on any new `/_bynk/` prefix until that prefix is gated. The workerd tests already pin it end to end (`wire_reference.rs` check 3, `cross_context_caller.rs` case 9).

The `409` body keeps `expected`. Three reasons:

- Only a caller holding the edge key (or a foreign caller's secret) can reach the `409`, so `expected` tells nobody anything they couldn't already get.
- Removing the field changes an existing error body. Wire format version 1 forbids that: "anything else is a new version" (ADR 0483, the wire-format reference). The published `ContractMismatch` type in the TypeScript client declares `expected` as required, so a foreign v1 client built against it would break.
- The caller learns the callee's hash only from this field. The runtime's `409` handling logs the parsed body (`ContractMismatch <caller> -> <service>`, #1826), and that is how the caller's own log line holds both hashes. Dropping `expected` would leave the caller with only its own hash.

**Consequences.** The wire format stays at version 1, with no change. An unauthenticated sender learns nothing from the internal paths beyond a `401`. If a later version wants a smaller `409` body, it does so as wire version 2, under the reference page's versioning rules, and moves the caller's two-hash log to the callee's line alone.
