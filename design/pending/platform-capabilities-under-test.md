---
level: minor
changelog: A context that consumes platform capabilities can now be tested. Under `bynkc test`, each one (`Clock`, `Random`, `Secrets`, `Locale`, `Logger`, `Events`, `Idempotency`, `Kv`, `Fetch`) is a deterministic test double, fresh per case. A `stub` overrides just the operations it names, and observation (`expect Logger.info called once`) works on platform seams. A case-scoped `stub` now applies to its own case only; before, it leaked into every case of the suite. `sessions`, `webhook-relay` and `event-log` gain handler tests, and every example's suite now runs in CI (#291)
---

## ADR: deterministic-platform-test-doubles
title: Platform capabilities get deterministic test doubles under `bynkc test`
summary: A consumed platform capability is a fixed, I/O-free double at the unit and integration tiers; `stub` layers over it per operation, and case-scoped stubs no longer leak

**Context.** A context that consumed a platform capability
(`consumes bynk { Clock }`) could not be tested. The test module wired the
capability as an `undefined` placeholder, so any handler that used it threw.
Most examples kept their testable logic out of platform-touching contexts for
this reason, and three shipped no tests at all. The capability also wasn't a
seam for observation, and a `stub` replaced the whole capability, not just the
named operation. Separately, case-scoped `stub` clauses were folded into one
first-match chain shared by every case, so one case's stub answered in its
siblings, contradicting the documented "case-scoped: overrides for this case".

**Decision.**
1. **A double per platform capability.** At the `unit` and `integration` tiers,
   each consumed platform capability is a deterministic, I/O-free double, built
   fresh for each case:
   - `Clock.now` reads the epoch;
   - `Random.uuid` counts up, and `Random.int(lo, hi)` draws from a fixed-seed
     linear congruential generator in `[lo, hi)`, matching the production range;
   - `Secrets.get` is `None`, and `Locale.current` is `"en"`;
   - `Logger` and `Events` do nothing;
   - `Idempotency` and `Kv` are in memory. A TTL (`Kv.putTtl`) or
     `expiresAfter` (`Idempotency.remember`) is accepted and ignored: modelling
     expiry would tie these doubles to the `Clock` double. A test of expiry
     stubs the read instead.

   `Fetch.send` **faults**, with a message naming the missing stub: a test never
   reaches the network, so a handler that fetches must say what the upstream
   answers.
2. **`stub` overrides per operation.** A stubbed capability is the stub layered
   over its tier default (the double, or the context's own provider). An
   operation the stub names answers from the stub, and every other operation
   reaches the default. The precedence is case `stub` > suite `stub` > the tier
   default, operation by operation.
3. **Case-scoped clauses are scoped.** Each clause records its owning case.
   `makeTestDeps` receives the running case's name, so a case-scoped clause
   matches only in its own case. An operation stubbed only by other cases
   reaches the tier default.
4. **Platform capabilities are seams.** A flattened platform capability
   (`consumes bynk { Logger }`) is observable like a capability the context
   declares, including `with` predicates over record parameters
   (`with req.url == …`) and `trace`. A platform capability that isn't flattened
   is not a seam.

**Alternatives.** Requiring an explicit `stub` for every platform operation a
test reaches was rejected as noise: most handlers only need the clock to tick
or the logger to stay quiet. Running the real Node providers was rejected
because they are non-deterministic (`Date.now`, `crypto.randomUUID`) or do real
I/O.

**Consequences.**
- A suite that passed only because a sibling case's stub leaked into it now
  fails honestly.
- `system`-tier cases still run the participants as Workers; this decision
  covers the in-process tiers.
- A record literal of a context-rebranded `uses` type still fails `tsc`, in
  tests and in production (#1704). `event-log`'s tests drive its read routes
  until that is fixed.

Proved by:
- the behavioural fixture `291_platform_test_doubles`, which shows each double,
  observation on a platform seam, a partial stub falling through, case
  isolation, a stubbed `Fetch`, the unstubbed `Fetch` fault, and the integration
  tier;
- the behavioural fixture `291_stub_precedence`, which pins case `stub` >
  suite `stub` > the double on the same operation, and that a suite stub
  answers again in the case after a case-scoped override;
- the example suites, now run by `bynkc/tests/example_tests_behaviour.rs`
  (`sessions` stubs the clock to test expiry, and `webhook-relay` stubs
  `Secrets` and `Fetch`).
