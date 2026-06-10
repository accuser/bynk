# karnc — Karn v0 compiler

A Rust implementation of the v0 Karn compiler. Takes a `.karn` commons file,
parses and type-checks it, and emits a TypeScript module.

## Pipeline

```text
lex  →  parse  →  resolve  →  check  →  emit
```

Each phase lives in its own module under `src/`:

- `lexer.rs` — `logos`-driven token stream, line comments skipped.
- `parser.rs` — hand-written recursive descent. One function per precedence level.
- `resolver.rs` — builds the per-commons symbol table; flags duplicates,
  name overlap, unresolved references, arity mismatches.
- `checker.rs` — type checks every declaration and expression; validates
  refinement predicate-base compatibility and detects contradictory
  combinations.
- `emitter.rs` — walks the typed AST and writes TypeScript.

Diagnostics flow through `error.rs` and `ariadne`. Every error carries a
dotted category (`karn.parse.expected_token`, `karn.types.invalid_regex`, …),
a source span, and a primary message; many carry secondary labels and notes.

## Building

Requires Rust stable ≥ 1.85 (edition 2024).

```bash
cargo build --release
```

## Using the CLI

```bash
karnc compile path/to/input.karn -o out.ts
karnc check   path/to/input.karn
```

The emitted module imports `./runtime.js`. Copy `runtime/runtime.ts` (built
into the crate at `runtime/runtime.ts`) into the same output directory.

## Tests

```bash
cargo test
```

Two test sets:

- Unit tests inside the lexer and parser modules.
- An end-to-end fixture-driven harness in `tests/e2e.rs` that runs the
  17 positive and 15 negative fixtures from `tests/fixtures/`.

To verify emitted TypeScript externally:

```bash
cp runtime/runtime.ts /tmp/karn-ts-check/
cp tests/fixtures/positive/*/expected.ts /tmp/karn-ts-check/
(cd /tmp/karn-ts-check && tsc --noEmit --strict --target es2020 \
   --module nodenext --moduleResolution nodenext *.ts)
```

## The language

The normative definition of the language this compiler accepts is the
specification in `docs/src/spec/` (rendered in the Karn Book), kept current
per increment. The decisions behind the increments are recorded in
`design/decisions/`.
