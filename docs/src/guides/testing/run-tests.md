# Run your tests

**Goal:** run a project's `test` and `integration` blocks, read pass/fail, and —
in your editor — click straight from a failing assertion to the line that failed.

Once you've [written some tests](write-tests.md), run them with `bynkc test`:

```sh
bynkc test            # from the project root
```

`bynkc test` compiles the project (every generated `tests/*.test.ts` module
included), then runs the aggregated runner under Node — type-checking as it goes,
so a type error stops you with the usual diagnostics. It exits non-zero if any
case fails. You need `node` and `tsc` (or `tsx`) on your `PATH`; check with
[`bynk doctor --only test`](../editor-and-tooling/doctor.md).

```text
commerce.money:
  ✓ accepts positive
  ✗ deliberate failure
    assertion failed at tests/commerce/money.test.bynk:8:12

1 passed, 1 failed.
```

A failed `assert` reports the **`path:line:col`** of the assertion that failed,
for both unit and integration tests.

## Machine-readable results: `--format json`

For CI and tooling, `--format json` emits a single, stable JSON **document**
instead of the human lines:

```sh
bynkc test --format json
```

```jsonc
{
  "passed": 10,
  "failed": 1,
  "suites": [
    {
      "name": "commerce.money",
      "kind": "unit",
      "cases": [
        {"name": "accepts positive", "outcome": "pass"},
        {"name": "deliberate failure", "outcome": "fail",
         "message": "assertion failed at tests/commerce/money.test.bynk:8:12",
         "location": {"path": "tests/commerce/money.test.bynk", "line": 8, "col": 12}}
      ]
    }
  ]
}
```

Each suite carries a `kind` (`"unit"` or `"integration"`) and a clean `name`; a
failing case carries a `message` and a structured `location` for click-through.
The exit code is unchanged — `0` only if the project compiled and every case
passed.

There are three shapes a consumer should handle, distinguished by `error`:

- **a normal run** — `suites` present, no `error` (it may still have `failed >
  0`);
- **a compile failure** — no `suites`; `error.kind` is `"compile"` and
  `error.diagnostics` carries the `path:line:col: severity[category]: message`
  lines (the same shape the editor's problem-matcher reads);
- **a crashed run** — the `suites` observed before the crash, plus `error.kind`
  `"runtime"` with the captured `stderr`.

The exit code always follows the runner's own process status, so a crash is
never reported as success.

## In the editor: the Test Explorer

The [VS Code extension](../editor-and-tooling/editor-support.md) consumes that
JSON surface directly. Open the **Testing** view (the beaker icon) and run your
Bynk tests from the tree, or invoke **Bynk: Run Tests** from the command palette.
Results show inline; a failing assertion links to its `.bynk` line, and a
compile failure lands in the Problems panel exactly as
[`bynkc check`](../../reference/cli.md) does. The extension resolves `bynkc` the
same way the check task does — the `bynk.compilerPath` setting, else `bynkc` on
`PATH`.

## Related

- [Write tests and mock collaborators](write-tests.md) — the `test` block, `assert`, and `mocks`.
- [Test a flow across Workers](integration.md) — `integration` suites over the real wire.
- Reference: [CLI (`bynkc`)](../../reference/cli.md) — every `bynkc test` flag and exit code.
