# Working on the docs

This book lives in `docs/` and is built with [mdBook](https://rust-lang.github.io/mdBook/).
It is organised by [Diátaxis](../introduction/how-these-docs-are-organised.md):
tutorials, how-to guides, reference, and explanation, for language users, plus
the contributor and tooling sections.

## Build the book locally

```sh
cargo build --release -p mdbook-karn-highlight   # the highlighting preprocessor
cargo install mdbook mdbook-linkcheck            # one-time
mdbook build docs                                # html + linkcheck + highlighting
mdbook serve docs                                # live preview
```

`book.toml` wires in the highlighting preprocessor and link checking, so a broken
internal link fails the build.

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
