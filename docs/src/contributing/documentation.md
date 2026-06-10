# Working on the docs

This book lives in `docs/` and is built with [mdBook](https://rust-lang.github.io/mdBook/).
It is organised by [Diátaxis](../introduction/how-these-docs-are-organised.md):
tutorials, how-to guides, reference, and explanation, for language users, plus
the contributor and tooling sections.

## Build the book locally

```sh
cargo build --release -p mdbook-karn-highlight   # the highlighting preprocessor
cargo build --release -p mdbook-karn-grammar     # the {{#grammar}} include preprocessor
cargo build --release -p mdbook-karn-visuals     # diagrams + callouts preprocessor
cargo install mdbook --version "=0.4.51" --locked  # pinned (linkcheck targets 0.4)
cargo install mdbook-linkcheck --locked            # one-time
mdbook build docs                                # html + linkcheck + highlighting
mdbook serve docs                                # live preview
```

`book.toml` wires in the preprocessors and link checking, so a broken internal
link fails the build.

## Diagrams and callouts

These render through the in-house `mdbook-karn-visuals` preprocessor (chosen over
external plugins to stay pinned to mdBook 0.4.51, offline, and CDN-free).

**Diagrams.** Write a fenced ` ```mermaid ` block; it renders client-side via the
vendored `theme/mermaid.min.js`. **Accessibility rule — required:** every diagram
carries a *caption* and a *text equivalent* in the surrounding prose. No
information may live only in a picture; a reader who cannot see the diagram must
still get the full meaning from the text.

**Callouts.** Write a GitHub-style alert blockquote. Exactly four kinds, each
with a fixed meaning — use them for what they say, not for decoration:

| Callout | Means |
|---|---|
| `> [!NOTE]` | an aside or clarification |
| `> [!TIP]` | a better or faster way |
| `> [!WARNING]` | easy to get wrong; proceed carefully |
| `> [!DANGER]` | will break, or is forbidden |

```text
> [!WARNING]
> Body text — ordinary Markdown, rendered normally.
```

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

## Showing a real diagnostic

To show what the compiler actually says when it refuses a program — verbatim, not
paraphrased — add a deliberately failing fixture and `{{#include}}` both it and
its captured transcript:

1. Write a standalone failing program at `docs/diagnostics/<id>.karn` (a
   `commons` or `context` block, like a doc example, but one that must error).
2. Run `KARN_BLESS=1 cargo test -p karnc --test doc_diagnostics`. This compiles
   the fixture, asserts it fails, and writes the real diagnostic — colour-free,
   with a stable `<id>.karn` label — to `docs/diagnostics/<id>.txt`.
3. On the page, show the source in a `karn,fail` fence and the transcript in a
   `text` fence, each holding a single mdBook `#include` line pointing at
   `docs/diagnostics/<id>.karn` and `docs/diagnostics/<id>.txt` (the path is
   relative to the page). See [the agent model](../explanation/the-agent-model.md)
   for a live example to copy.

The `.txt` transcripts are **generated — never hand-edit them**;
`doc_diagnostics` (run in CI) re-derives them from `karnc` and fails if the
committed copy drifts, and fails if a fixture ever starts compiling. The fixtures
live outside `docs/src/`, so the doc-example gate skips a fenced block whose body
is only an `{{#include}}` (it is display-only; the fixture's own compile is what
`doc_diagnostics` checks).

### The before/after device

On **explanation** pages, pair the refusal with the bug it prevents — the most
persuasive shape in the book. Two panels:

- **The bug that ships.** A short, idiomatic `typescript` block that genuinely
  compiles *with* the exact bug Karn targets. Tag it `typescript` (the
  doc-example gate ignores it) and keep it honest — it must really compile.
- **The program that won't build.** The Karn equivalent via the mechanism above:
  a `karn,fail` fixture include, then the generated transcript.

Weave it into the prose where the page already *asserts* the bug, so the
demonstration replaces the assertion rather than bolting on. Keep it to
explanation pages; reference stays dry. See
[the type-system philosophy](../explanation/type-system-philosophy.md) for the
device in use.

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

### The four voices

"One mode per page" is also a rule about *voice*: the same fact should sound
different in each mode. Here is "an agent's state must be zeroable", written four
ways — read them as a tuning fork before you draft a page.

- **Tutorial** (warm, "we", a guaranteed path): "We'll give the counter a
  `count` field. Karn needs a starting value for a brand-new key, so every state
  field must have a zero — `Int`'s is `0`, so we're set. Run it and watch a fresh
  counter read `0`."
- **How-to** (imperative, goal-first): "Keep every agent state field zeroable:
  use a type with an implicit zero (`Int`, `Bool`, `String`, `Option[T]`), or
  give the field an initialiser (`field: T = value`)."
- **Reference** (neutral, terse): "Each agent `state` field must be zeroable — it
  has an implicit zero value or an initialiser. A non-zeroable field without an
  initialiser is rejected (`karn.agents.non_zeroable_state_field`)."
- **Explanation** (discursive, a view): "Why insist on a zero? A fresh key has no
  stored state, and no constructor was required first, so the agent must still
  come into being with a defined value. Zeroability is how Karn makes 'never seen
  before' honest instead of undefined."

## Glossary first-use linking

The [glossary](../reference/glossary.md) gives each load-bearing term a stable
anchor, `#term-<slug>` (e.g. `#term-refined-type`). On a reader-facing page, link
the **first** occurrence of a glossary term to its entry —
`[refined type](../reference/glossary.md#term-refined-type)` — and only the first;
never inside a heading, a code fence, or on the glossary page itself.

`docs/tools/check-glossary-links.sh` is an **advisory** lint: it lists, per page,
glossary terms that appear with no link to their entry, so first-use linking can
be caught up page by page. It prints findings and exits 0 (set
`GLOSSARY_LINK_STRICT=1` to exit non-zero on findings). It deliberately does not
auto-link — terms are common words, so a human decides each first use; an
auto-linking preprocessor is a possible future if the false-positive risk can be
contained.

## The language specification

The [Karn Language Specification](../spec/index.md) lives in `docs/src/spec/`. It
is the **normative** definition of the **current language**, updated in place
per increment, distinct in register from the friendly
[grammar reference](../reference/grammar.md): the reference is per-construct
lookup, the spec is the complete citable definition. The two share their
generated facts.

It is **translation-defined** — syntax by the grammar, static semantics by the
`karn.*` well-formedness rules, dynamic meaning by emission plus the runtime
contract — and it **reuses the existing machinery**: it embeds `{{#grammar}}`
productions and `{{#grammar-semantics}}` diagnostics just like the reference (the
rendered output is shared from one source, so there is no drift), and every
example is covered by the doc-example gate. It adds no preprocessor of its own.

**Keeping the spec current.** A language or grammar increment updates the
**affected spec chapters** and records each language-defining call as a
**decision record** in `design/decisions/` — it does *not* spawn a standalone
instalment document. The per-increment-file practice is retired and the old
instalments have been **removed** (their history is in version control;
Appendix B records the lineage); the spec, with the
[changelog](../reference/changelog.md) and the decision records, is the record.
An increment's *design draft* is a **transient proposal** in
`design/proposals/`: merged for sign-off before implementation, consumed by it,
and deleted by the PR that lands the increment (the lifecycle is documented in
that directory's README). Much of the spec stays current for free:
the `{{#grammar}}` productions (§3/§4/§11) and the `{{#grammar-semantics}}`
diagnostic links (§5) re-render from the grammar and the registry, so syntax and
the diagnostic catalogue never drift. The **prose** is hand-maintained — when
behaviour changes, review §5 (static semantics), §6 (the type system), §7 / §7.4
(emission and the runtime library), §8 (compilation), §10 (conformance), and
Appendix B (version history).

**Verify against the compiler.** Specification and reference claims are checked
against the **actual compiler** — the emitter, the checker, and the fixtures —
never against the older design documents. Three are **not normative** and have
drifted: `karn-design-notes.md` (rationale and history; still refers to a Go
compiler — Karn's compiler is Rust), `karn-type-system.md` (aspirational v1), and
the retired `karn-runtime-spec.md`. Cite them for *intent* only, never for current
behaviour.

## Docs ship with the feature

Treat docs as part of an increment's definition of done (see
[Testing & fixtures](testing.md)): update the affected reference (regenerating
the generated pages), update the affected [specification](../spec/index.md)
chapters (per *Keeping the spec current* above), add a changelog entry, bump the
version references (the introduction banner, `tooling/index.md`, and
`explanation/versioning-and-roadmap.md`) when the increment changes it, and check
that any touched tutorial or how-to still compiles under the doc-example gate.
