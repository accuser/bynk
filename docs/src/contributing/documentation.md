# Working on the docs

This book lives in `docs/` and is built with [mdBook](https://rust-lang.github.io/mdBook/).
It is organised by [Diátaxis](../introduction/how-these-docs-are-organised.md):
tutorials, how-to guides, reference, and explanation, for language users, plus
the contributor and tooling sections.

## Build the book locally

```sh
cargo build --release -p mdbook-karn-highlight   # the highlighting preprocessor
cargo build --release -p mdbook-karn-grammar     # the {{#grammar}} include preprocessor
cargo install mdbook mdbook-linkcheck            # one-time
mdbook build docs                                # html + linkcheck + highlighting
mdbook serve docs                                # live preview
```

`book.toml` wires in the highlighting preprocessor and link checking, so a broken
internal link fails the build.

## Embedding a grammar production

A reference page can embed one grammar production by name. Put a line whose only
content is the directive:

```text
\{{#grammar http_handler}}
```

The `mdbook-karn-grammar` preprocessor replaces it with an `ebnf` block holding
that production, rendered from `tree-sitter-karn/src/grammar.json` (the same
source as the [grammar appendix](../reference/grammar.md)) so it cannot drift
from the parser. The rendered production is generated — never hand-edit it. An
unknown rule name fails the build, so a typo cannot silently vanish.

## Embedding a construct's static semantics

A production says what *parses*; the diagnostics say what is *legal beyond
parsing*. Embed the diagnostics that constrain a construct with:

```text
\{{#grammar-semantics http_handler}}
```

The same preprocessor replaces it with a bullet list of the governing
diagnostics, generated from `docs/grammar-semantics.json`. That file is itself
generated from the `grammar_symbol` field of each entry in
`karnc/src/diagnostics.rs` — the single source of the mapping — and regenerated
by the `diagnostics_registry` test (see below). A construct with no diagnostics
yields a neutral line rather than failing, since an unconstrained production is
legitimate; to add or change a mapping, edit `grammar_symbol` and re-bless.

The annotated reference (`reference/grammar.md`) must cover every production:
`karnc/tests/grammar_coverage.rs` asserts that each embeddable grammar rule has
exactly one `{{#grammar <rule>}}` entry with a matching `{#rule-<rule>}` heading
anchor, and that every directive argument names a real rule. So a new production
cannot ship without a documented entry, and the diagnostic index's **Construct**
column deep-links to `grammar.md#rule-<rule>` and always resolves.

## The guardrails

Four mechanisms keep the docs honest; all run in CI.

1. **Every example compiles.** `karnc/tests/doc_examples.rs` extracts every
   fenced ```` ```karn ```` block from `docs/src/**` and compiles it — `commons`
   blocks in-process, `context` blocks as a temp project. Annotate blocks that
   should not be compiled as-is:
   - ```` ```karn,ignore ```` — a fragment, a `test` block, or pseudo-syntax;
   - ```` ```karn,fail ```` — a negative example that must fail to compile.
   Karn uses `--` for comments, not `//` (the gate will catch `//`).

2. **Generated reference is generated.** Four reference pages are emitted from the
   compiler/grammar and guarded by tests, so they cannot drift:

   | Page | Source | Test |
   |---|---|---|
   | `reference/diagnostics.md` | `diagnostics.rs` registry | `diagnostics_registry.rs` |
   | `reference/keywords.md` | lexer keyword tokens | `keywords_reference.rs` |
   | `reference/cli.md` | the clap command tree (`cli.rs`) | `cli_reference.rs` |
   | `reference/grammar.md` | `tree-sitter-karn` grammar.json | `grammar_reference.rs` |

   Each test renders the page and asserts it matches the committed file.
   Regenerate after a relevant change:

   ```sh
   KARN_BLESS=1 cargo test -p karnc --test diagnostics_registry \
                                    --test keywords_reference \
                                    --test cli_reference \
                                    --test grammar_reference
   ```

   Never hand-edit a generated page — the header says so, and CI will revert you.

   The `diagnostics_registry` test also generates `docs/grammar-semantics.json`
   (the rule → diagnostics map behind `{{#grammar-semantics}}`) and checks every
   `grammar_symbol` names a real grammar rule, so a mistyped mapping fails the
   build.

3. **Link checking.** `mdbook-linkcheck` validates internal links on every build.

4. **British English.** `docs/tools/check-british-english.sh` scans prose
   (skipping code blocks) for US spellings. Extend the wordlist there as needed.

## Style

- **One Diátaxis mode per page.** No explanation inside a tutorial; no how-to
  steps inside reference. Link outward to siblings instead of duplicating.
- **British English**, enforced by the lint above.
- **Document the present.** Write what compiles today; mark planned features as
  planned.

## Docs ship with the feature

Treat docs as part of an increment's definition of done (see
[Testing & fixtures](testing.md)): update the affected reference (regenerating
the generated pages), add a changelog entry, and check that any touched tutorial
or how-to still compiles under the doc-example gate.
