# Lay out a project

**Goal:** structure a multi-file project with source and tests, and build it.

A project is a directory containing a `bynk.toml` manifest plus `src/` and
`tests/` trees:

```text
my-project/
├── bynk.toml
├── src/
│   ├── counters.bynk       # context counters
│   └── quantities.bynk     # commons quantities
└── tests/
    ├── counters.bynk       # test counters
    └── quantities.bynk     # test quantities
```

## The manifest

`bynk.toml` names the project and its directory layout:

```toml
[project]
name = "my-project"
version = "0.1.0"

[paths]
src = "src"
tests = "tests"
```

See the [`bynk.toml` reference](../../reference/manifest.md) for every key.

## Path identity

A unit's path must match its qualified name. A file declaring `context counters`
must be `src/counters.bynk`; one declaring `context commerce.orders` must be
`src/commerce/orders.bynk`. A test file mirrors the name of the unit it tests
under `tests/`, so `test counters` lives in `tests/counters.bynk` — or, with the
optional self-identifying suffix, `tests/counters.test.bynk`. Both forms are
accepted (the `.test.bynk` suffix is what single-tree layouts use, and is handy
for grepping); use whichever you prefer.

> Mismatches are reported as `bynk.project.inconsistent_commons_name` (source) or
> `bynk.project.inconsistent_test_path` (tests).

## Build and test

```sh
bynkc compile . --output out      # compile the project
bynkc test .                      # compile and run the tests
bynkc check .                     # type-check only
```

## Related

- [Write tests and mock collaborators](../testing/write-tests.md).
- [Target Cloudflare Workers](cloudflare-workers.md).
- Reference: [`bynk.toml` manifest](../../reference/manifest.md).
