# `bynk.integration.*` errors

These diagnostics come from `test integration` blocks (multi-Worker integration
tests). See the [testing reference](../reference/testing.md#test-integration--multi-worker-integration-tests)
and [Test a flow across Workers](../guides/testing/integration.md).

## `bynk.integration.too_few_participants`

```text
[bynk.integration.too_few_participants] an integration test must wire at least two contexts
```

**Cause:** the `wires` clause lists fewer than two contexts.

**Fix:** an integration test exercises a flow *between* contexts. To test one
context in isolation, use a unit test (`test <context> { … }`) and `mocks` its
collaborators.

## `bynk.integration.unknown_participant`

```text
[bynk.integration.unknown_participant] `shop.nope` is not a declared context in this project
```

**Cause:** a name in `wires` is not a context the project declares (a typo, a
commons, or a missing file).

**Fix:** wire only declared contexts. Commons are brought in with `uses`, not
`wires`.

## `bynk.integration.duplicate_participant`

```text
[bynk.integration.duplicate_participant] context `shop.orders` is listed more than once in `wires`
```

**Fix:** list each participant once.

## `bynk.integration.unwired_dependency`

```text
[bynk.integration.unwired_dependency] participant `shop.orders` consumes `shop.payment`, which is not wired into this integration test
```

**Cause:** a participant `consumes` a context that is not itself wired. An
integration test runs each participant as a real Worker, so every consumed
context needs a Worker to route to.

**Fix:** add the named context to the `wires` clause. The set must be closed under
`consumes`.

## `bynk.integration.mock_in_integration`

```text
[bynk.integration.mock_in_integration] `mocks` is not allowed in an integration test
```

**Cause:** a `mocks` declaration appears inside `test integration`. Integration
tests wire participants with their **real** implementations — that is the point.

**Fix:** remove the mock. To substitute a collaborator, write a unit test
(`test <context> { mocks … }`) instead.

## `bynk.integration.duplicate_suite`

```text
[bynk.integration.duplicate_suite] integration test `"checkout"` is declared more than once
```

**Fix:** give each `test integration` a unique suite name.
