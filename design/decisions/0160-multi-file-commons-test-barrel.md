# 0160 — `bynkc test` resolves a multi-file commons via a test-path barrel

- **Status:** Accepted (2026-07-03)
- **Realises:** parity between `bynkc test` and the two paths that already accept
  a directory-split commons (`bynkc check`, production emit). Provenance: the
  v0.132 multi-file-commons test-import increment, closing
  [#451](https://github.com/accuser/bynk/issues/451) (discovered building the
  `ilr` project against v0.118).

## Context

A `commons` may live as **one file** (`src/thing.bynk`) or as a **directory** of
files that each declare the same commons (`src/thing/a.bynk`, `src/thing/b.bynk`).
Both are supported source layouts — `unit_path_matches`
(`bynk-emit/src/project/paths.rs`) accepts a `single_file_match` (stem parts ==
name parts) or a `multi_file_match` (parent-directory parts == name parts) — and
both `bynkc check` and production emit honour the directory layout. Production
emits a multi-file commons **per file** (`out/thing/a.ts`, `out/thing/b.ts`) and
never an aggregate `out/thing.ts`; each importing context resolves symbols to the
defining file via `imported_decl_paths`.

The **test** emitter did not. Every test/integration import of a commons is a
single whole-module **namespace** import — `import * as thing from
"./thing.js"` — because lowered case bodies name symbols namespace-qualified
(`thing.Foo`). For a single-file commons that path is `out/thing.ts` (the file
itself) and resolves; for a multi-file commons no `out/thing.ts` exists, so `tsc`
reported `TS2307 Cannot find module`. This shape recurs at four sites (the suite
target, each `consumes` target, each `uses` commons, and the integration
harness's `uses` commons), and every one broke for the directory layout.

## Decision

**[A] Fix with an aggregating barrel, not per-symbol imports.** Emit
`out/<name>.ts` that re-exports the commons' directory, so the existing one-line
namespace import resolves unchanged. Per-symbol imports (the production shape)
fit production — which imports individual **rebranded** symbols (`X as
__CommonsX`) and never uses `import * as` — but fight the test path, whose
contract is *namespace imports + namespace-qualified references*; mirroring
production would mean synthesising a namespace object by hand or rewriting every
`ns.Symbol` reference, at all four sites. The barrel keeps the contract and
repairs every site uniformly.

**[B] Emit the barrel from the test path only, not the shared project emit.** All
four broken sites are test-path sites (`process_tests` and
`process_integration_tests`). A **global** barrel in the per-unit emit loop would
buy only `out/` self-consistency and a speculative future non-test consumer of
the namespace contract — neither exists today — at the permanent cost of shipping
an inert `out/<name>.ts` entrypoint into *every* build of *every* multi-file
commons, and of splitting the `import * as` requirement from the barrel that
guarantees it (~900 lines apart, a latent drift risk). Test-only co-locates the
namespace import and the barrel it needs. Its one cost is dedup: several
test/integration modules may import the same commons, so a `HashSet<PathBuf>` of
emitted barrel paths — shared across the unit- and integration-test passes —
keeps it to one emission. That trade (≈5 lines of dedup vs. a permanent
production-surface addition) favours test-only. *Named follow-on:* if a non-test
consumer ever materialises, promoting the barrel to the shared emit loop is a
mechanical move — the emission is written as a standalone function to make it a
call-site change.

**[C] The barrel is `export *` from each of the commons' source files — no symbol
enumeration.** A commons is one merged namespace, so every symbol is already
unique across its files (`bynk.resolve.duplicate_type`/`_fn`/`_method`); a flat
`export *` merge cannot collide regardless of symbol kind. The file set is the
commons' **source files** (its `parsed` indices in `groups`), *not* the values of
the `FileDeclIndex { types, fns, methods }` maps — those record only files that
declare a type, fn, or method, so keying off them could silently drop a file and
dangle a symbol. This rides on `tsc` treating `export *` as a re-export of both
values **and** types; the emitted `out/tsconfig.json` sets `isolatedModules` +
`strict` but **not** `verbatimModuleSyntax`, under which a star re-export resolves
type-vs-value automatically. That is the fix's one implicit assumption — a
fixture type-checks a barrel that re-exports a `type` so adding
`verbatimModuleSyntax` later fails loudly rather than breaking `bynkc test`
silently.

**[D] Emit a commons' barrel once, when a test/integration module imports a
commons whose layout is multi-file.** The trigger is the `multi_file_match`
branch (extracted as `is_multi_file_layout`, the single source of truth shared
with path validation) — the layout where no `out/<name>.ts` is otherwise
produced, so a single-file commons never gets a colliding barrel. The predicate
is keyed by **commons name** and evaluated per *imported* commons at every site
— the target and each `consumes`/`uses`/integration target — not just the suite
target, so a multi-file commons reached only indirectly is still resolved.
Dotted names compose: `commons a.b` split across `src/a/b/*.bynk` barrels at
`out/a/b.ts`, the slot the single-file form would occupy; specifiers run through
the emitter's `cross_commons_import_specifier_for_path` so the relative path is
correct for dotted names and forward-slash-normalised.

## Consequences

- **Production output is byte-for-byte unchanged** — the barrel ships only from
  the test path; a non-test build of a multi-file commons emits no barrel.
- The four import sites resolve for both layouts with no change to
  `emit_test_module` / `emit_integration_module` themselves.
- No grammar, checker, or AST change — the layout already parsed and type-checked;
  this was an emit-only defect.
- **Tooling delta (ADR 0156):** hover, completion, semantic tokens, and signature
  help are unchanged — this increment adds no construct and alters only the shape
  of emitted `out/*.ts`.
- *Named follow-on:* should a future consumer need per-symbol test imports (e.g.
  tree-shaking the test bundle), revisit Option A against the then-current
  namespace convention.
