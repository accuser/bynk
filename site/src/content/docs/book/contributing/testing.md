---
title: Testing & fixtures
---
The compiler's correctness rests on a large fixture suite plus a TypeScript
type-check gate. Both live under `bynkc/tests/`.

## The fixture suite

`tests/e2e.rs` discovers every directory under `tests/fixtures/positive/` and
`tests/fixtures/negative/` (currently 143 positive, 105 negative) and runs each
as one fixture. There are two shapes:

**Single-file**
- `input.bynk` — the source (a self-contained `commons`).
- `expected.ts` — the exact emitted TypeScript (positive), **or**
- `expected_error.txt` — expected diagnostics (negative).

**Project**
- `src/` — a source tree (one or more `context`/`commons` units).
- `expected/` — the emitted output tree to match (positive), **or**
  `expected_error.txt` (negative).
- `target.txt` — optional; `workers` selects the Workers target (default bundle).
- `bynk.toml` — optional; marks a project and configures `[paths]`.

`runtime.ts` and `tsconfig.json` are excluded from per-fixture comparison (they
are checked separately).

### How matching works

- **Positive** fixtures compare emitted files byte-for-byte against `expected/`
  (or `expected.ts`).
- **Negative** fixtures match by **substring**: each non-blank, non-`#` line of
  `expected_error.txt` must appear somewhere in the concatenated
  `"{code} {message}"` of the diagnostics. So a line is usually just a code, e.g.
  `bynk.refine.literal_violates`.

## The bless workflow

When you change the emitter (and the new output is correct), regenerate the
positive fixtures' expectations rather than editing them by hand:

```sh
BYNK_BLESS=1 cargo test -p bynkc bless_positive_fixtures
```

The `bless_positive_fixtures` test is a no-op unless `BYNK_BLESS` is set; with it
set, it recompiles each positive fixture and overwrites `expected/`. **Always
review the resulting diff** — blessing is how a regression silently becomes the
new "expected" if you are not careful.

`BYNK_BLESS` is the project's shared regenerate switch: the same run also
refreshes the generated reference pages (see [Working on the docs](/book/contributing/documentation/)).
Scope it to a specific test when you only mean to bless one thing.

## The `tsc` verification gate

`tests/tsc_verify.rs` (`emitted_typescript_passes_tsc_strict`) compiles every
project-form positive fixture and runs `tsc --strict --noEmit` over the output.
It is a backstop for emitter bugs that produce TypeScript which round-trips our
own comparison but does not actually type-check.

It needs `tsc` on `PATH`, or falls back to `npx -p typescript@5 tsc`. Behaviour
when neither is available:

- locally — it logs a warning and passes (so a missing toolchain does not block
  you);
- in CI — set **`BYNK_REQUIRE_TSC=1`** to make a missing `tsc` a hard failure.

## Adding a feature: the definition of done

A grammar increment is not complete until:

1. positive **and** negative fixtures cover it (and pass);
2. emitted output type-checks under the `tsc` gate;
3. any new diagnostic code is added to the registry in `diagnostics.rs`;
4. the **docs** are updated in the same change — see
   [Working on the docs](/book/contributing/documentation/).
