# Compiler architecture

`bynkc` is a straight-line pipeline: **lex → parse → resolve → check → emit**.
Each stage has a module in `bynkc/src/`.

```text
  .bynk source
       │
   lex │  lexer.rs        →  tokens
       ▼
 parse │  parser.rs       →  AST (ast.rs)
       ▼
resolve│  resolver.rs     →  symbols
       ▼
 check │  checker.rs      →  typed, validated AST
       ▼
  emit │  emitter.rs      →  TypeScript
       ▼
  out/*.ts
```

## The pipeline

| Stage | Module | Role |
|---|---|---|
| Lex | `lexer.rs` | Tokenise source (built on `logos`); doc blocks via a hand-written scanner. |
| Parse | `parser.rs` | Build the AST (`ast.rs`); recover where possible for the LSP. |
| Resolve | `resolver.rs` | Name resolution and symbol-table construction. |
| Check | `checker.rs` | Type checking, refinement validation, effect rules. |
| Emit | `emitter.rs` | Generate TypeScript. |

Supporting modules: `span.rs` (source locations), `error.rs` (`CompileError`
and rendering via `ariadne`), `project.rs` (multi-file assembly), `fmt.rs` (the
formatter), `diagnostics.rs` (the diagnostic-code registry), `cli.rs` (the clap
CLI), and `keywords.rs` (the keyword registry).

**First-party sources** (the `bynk` surface + platform adapters, the Bynk-written
`bynk.list`/`bynk.map`/`bynk.string` commons, the per-platform TypeScript
bindings, and the emitted runtime) live as real `.bynk`/`.ts` files under
`bynkc/src/firstparty/` and are embedded at compile time via `include_str!`
(ADR 0086). **Edit the file, not a string literal.** They are checked standalone
(`tests/firstparty_sources.rs` parses + `bynk-fmt`-checks each `.bynk`;
`tsc_verify.rs` type-checks the embedded `runtime.ts`), and vendored into every
emitted project rather than published.

## Entry points

The library (`lib.rs`) exposes the flows the CLI and LSP build on:

- **`compile(source, filename)`** — single-file mode for a self-contained
  `commons`. Runs the five stages in order and returns the emitted TypeScript (or
  `Vec<CompileError>`). This is also what the [doc-example gate](documentation.md)
  uses for `commons` blocks.
- **`compile_project(root)`** / `compile_project_with_target` /
  `compile_project_with_split_paths` — multi-file projects. A two-pass design:
  first discover and parse every `.bynk` file and build a global symbol table;
  then resolve, type-check, and emit each unit with visibility of the units it
  `uses`/`consumes`.
- **`diagnose(source)`** — best-effort, never-fatal compilation with recovery
  that accumulates diagnostics. The language server uses this.

## Diagnostics

Every error has a stable `bynk.*` code (the `category` field of `CompileError`).
These codes are the user-facing contract, so they are catalogued in a central
registry, `diagnostics.rs`, which also generates the
[diagnostic index](../reference/diagnostics.md). A test asserts the registry
matches the codes actually emitted in the source — so adding a new code without
registering it fails the build. See [Testing & fixtures](testing.md).

## Targets

The emitter has two targets, selected by `BuildTarget`:

- **Bundle** (default) — a flat TypeScript tree; cross-context calls are direct
  function calls.
- **Workers** — one Cloudflare Worker per context; cross-context calls go over
  Service Bindings with boundary validation, and agents become Durable Objects.

See the [emission reference](../reference/emission.md) for what each construct
produces.

## The sibling tools

- `bynk-fmt` re-exports `bynkc::fmt`, so the formatter has one implementation
  shared by the CLI (`bynkc fmt`) and the LSP.
- `bynk-lsp` depends on `bynkc` for `diagnose` and on the formatter, adding the
  LSP protocol layer. See [`bynk-lsp`](../tooling/bynk-lsp.md).
