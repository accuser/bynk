---
level: patch
changelog: "Behavioural fixtures: a positive fixture's `suite`s now *run* when it carries an `expected_run.txt` (`bynkc/tests/behaviour_fixtures.rs`; 71 fixtures marked). A strict marker pins known defects as expected failures. The bundle runtime's `InMemoryStorage` now copies stored data on `put`/`get` (sharing live connection handles), so `bynkc test` sees workerd's copy semantics and a commit refused by an invariant no longer leaks an in-place store write (ADR 0109)."
---
