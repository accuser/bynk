# `tree-sitter-bynk`

The [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar for Bynk.
It is the source of truth for structural tooling — syntax highlighting (including
[this book's](../contributing/documentation.md) code blocks) and any editor that
speaks tree-sitter.

## Layout

| Path | What it is |
|---|---|
| `grammar.js` | The grammar definition — the file you edit. |
| `src/parser.c`, `src/scanner.c` | Generated parser plus a hand-written external scanner (for `--- … ---` doc blocks). |
| `src/grammar.json`, `src/node-types.json` | Generated grammar metadata. |
| `queries/highlights.scm` | Highlight queries (the highlight groups). |
| `queries/injections.scm` | Language-injection queries (currently none). |
| `test/corpus/` | Versioned parse-tree test cases. |

The generated files in `src/` are committed. The book's
[grammar reference](../reference/grammar.md) is generated from `src/grammar.json`.

## Build and test

With the [tree-sitter CLI](https://tree-sitter.github.io/tree-sitter/cli):

```sh
npm install          # once
npm run build        # tree-sitter generate  → regenerates src/
npm test             # tree-sitter test      → runs the corpus
```

After editing `grammar.js`, run `npm run build` to regenerate the parser, then
`npm test` to check it against the corpus. A corpus case is a named section: the
source, a `---` separator, and the expected S-expression parse tree.

## Highlight groups

`highlights.scm` maps grammar nodes to standard highlight groups — `@keyword`
(and `@keyword.declaration`, `.import`, `.modifier`, `.operator`), `@type`,
`@type.builtin`, `@string`, `@number`, `@comment`, `@function`, `@variable`,
`@operator`, `@punctuation.*`, and more. The book's highlighting preprocessor
([`mdbook-bynk-highlight`](../contributing/documentation.md)) compiles this
grammar and renders ```` ```bynk ```` blocks through these groups, so doc
highlighting stays correct as the grammar evolves.

## Keeping it in sync

`grammar.js` is regenerated to `src/` via `tree-sitter generate`; commit the
generated files together with the grammar change so downstream consumers (the
docs preprocessor, editors) stay consistent.
