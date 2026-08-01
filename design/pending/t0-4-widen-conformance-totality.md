---
level: patch
changelog: "`tree-sitter-bynk/tests/conformance.rs` widens from a fixed case list to totality: `examples/`, the vendored first-party `.bynk` sources, and every `bynkc` positive fixture must parse clean under tree-sitter, and every parse/lex-time negative fixture must be rejected by both parsers"
---

Closes-Rule: R11.7

T0.4′ (design/tracks/compiler-architecture.md, Tier A). Widens the
cross-parser conformance test from its original fixed `CASES` list to the
full corpus: `examples/`, `bynk-check/src/firstparty`, and every
`bynkc/tests/fixtures/positive` fixture must have zero `ERROR`/`MISSING`
nodes under tree-sitter, and every `bynkc/tests/fixtures/negative` fixture
whose `expected_error.txt` names a `bynk.parse.*`/`bynk.lex.*` category must
be rejected by both parsers.

Running the widened corpus surfaced four real drifts, all fixed in
`tree-sitter-bynk/grammar.js`:

- Predicate arguments (`InRange`, `MinLength`, `MaxLength`, `Length`) can be
  negative (`InRange(-1_000, 1_000_000)`); the grammar admitted only an
  unsigned literal.
- Handler-position annotations (`@cache(maxAge: …)`, `@limit(maxBody: …)`,
  ADR 0163/0165) were not in the grammar at all — only the store-field
  `@name(args)` shape existed.
- Only `on call` is a valid agent handler
  (`bynk.parse.handler_in_agent`); the grammar let cron/queue/HTTP/WebSocket
  handlers appear inside an `agent` too.
- A refined pattern's inner form must be `_` (`bynk.parse.refined_pattern_inner`);
  the grammar accepted any pattern there.

Three categories are structurally inexpressible in a context-free grammar
(`bynk.lex.float_literal_overflow`, `bynk.parse.nesting_too_deep`) or would
require a disproportionate rewrite/an unguarded duplicate source of truth for
this slice (`bynk.parse.non_associative`, `bynk.parse.reserved_keyword`,
`bynk.parse.uses_after_decls`); each is named individually in
`EXCLUDED_PARSE_TIME_CATEGORIES` with its reason, and the test fails if an
excluded category stops appearing in the fixture corpus (a dead exclusion).

Regenerating the parser after the grammar changes updated the committed
`tree-sitter-bynk/src/{grammar.json,node-types.json,parser.c}`, the two
generated docs goldens (`site/src/generated/grammar.json`,
`site/src/content/docs/book/reference/grammar-appendix.md`), and five
`tree-sitter-bynk/test/corpus/*.txt` snapshots (an agent's `on call` handler
now parses directly as `call_handler`, without a redundant wrapping `handler`
node, since `call_handler` is now the only alternative reachable inside an
`agent_decl`).
