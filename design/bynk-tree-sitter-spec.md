# Bynk Tree-sitter Grammar — Specification

The tree-sitter grammar provides syntactic structure for Bynk source files. It drives syntax highlighting in editors that support tree-sitter (VS Code, Neovim, Helix, GitHub web view, etc.) and is the foundation for any future tooling that needs cheap, accurate parsing without invoking the full Bynk compiler.

This specification covers the grammar's coverage requirements, the highlighting groups, and the test corpus structure. Implementation details (the actual `grammar.js` file) follow standard tree-sitter conventions; this spec defines what the grammar must recognise and how.

> **Status (18 June 2026, v0.54).** This spec — and the grammar it describes —
> are scoped to **v0–v0.5** and have not been brought forward to the current
> language. Newer surface (`from <protocol>` / `on http`, the `assert`
> expression, `test`/`mocks` units, `HttpResult`, the actor `by` clause, string
> interpolation) is **not** covered, so a modern Bynk file produces ERROR nodes
> and broken highlighting in tree-sitter-driven editors. Bringing the grammar up
> to the current surface is tracked in
> [`bynk-engineering-roadmap.md`](bynk-engineering-roadmap.md) and noted in
> [`bynk-status-and-roadmap.md`](bynk-status-and-roadmap.md) §5. (This file is
> still referenced by `tree-sitter-bynk/queries/highlights.scm` §2 for the
> highlight-group inventory, so it stays in place.)

---

## 1. Scope

### What the grammar must cover

All Bynk syntactic forms from v0 through v0.5:

- Comments — line (`-- ...`) and doc blocks (`--- ... ---`).
- Literals — integer, string, boolean, the unit literal `()`.
- Identifiers, keywords, and operators (including multi-character ones: `->`, `<-`, `==`, `!=`, `<=`, `>=`, `&&`, `||`, `?`).
- Top-level declarations: `commons`, `context`. Both brace and fragment forms.
- Project-level clauses within declarations: `uses`, `consumes`, `exports`.
- Type declarations: refined values (`type T = Int where ...`), records (`type T = { ... }`), sums (pipe form and `enum` form), opaque types (`type T = opaque Int`), generic type references (`Result[T, E]`, `Option[T]`, `Effect[T]`).
- Function declarations: free functions and methods (dotted form `Type.method`).
- Capability declarations.
- Provider declarations.
- Service declarations with handler blocks.
- Agent declarations with key, state block, handlers.
- Expressions: the full v0–v0.5 expression grammar including `match`, `is`, record construction (with spread), method calls, field access, `if`/`else`, function calls, constructor calls, `Ok`/`Err`/`Some`/`None`, `Effect.pure(...)`, literals, identifiers, parenthesised expressions.
- Statements: `let` (pure and effectful via `<-`), `commit`.
- The `given` clause on handler declarations.

### What the grammar need not cover

The grammar is *syntactic*, not *semantic*. It identifies the shape of source code; it does not:

- Validate types or refinements.
- Resolve names.
- Enforce semantic rules (exhaustiveness in match, effect propagation, `given` matching, etc.).

Those are the LSP's job (via the compiler), not the grammar's. The grammar should accept syntactically-well-formed code that the type checker might still reject.

### Error recovery

Tree-sitter has built-in error recovery — when the parser hits an unparseable section, it produces ERROR nodes but continues parsing the rest of the file. The grammar should make reasonable choices about token boundaries so that recovery is good. Specifically:

- Top-level declaration keywords (`commons`, `context`, `type`, `fn`, `capability`, `provides`, `service`, `agent`) serve as natural recovery points.
- Inside expressions, semicolons and closing braces are recovery points.
- Doc blocks (`---`) are atomic — partial doc blocks are an error but don't cascade into the following declaration.

---

## 2. Highlighting groups

Tree-sitter highlighting uses named capture groups in the grammar's `queries/highlights.scm` file. Bynk's grammar maps source elements to these standard groups (with the `@` prefix as is conventional):

### Keywords

- `@keyword` — control-flow and binding keywords: `if`, `else`, `let`, `match`, `is`, `commit`, `on`, `given`.
- `@keyword.import` — import-related: `uses`, `consumes`.
- `@keyword.declaration` — declaration introducers: `commons`, `context`, `type`, `fn`, `capability`, `provides`, `service`, `agent`, `state`, `exports`.
- `@keyword.modifier` — visibility and modifiers: `opaque`, `transparent`.
- `@keyword.operator` — keyword operators: `where`, `and`.

### Types

- `@type` — user-defined type names (capitalised identifiers in type position): `Money`, `OrderError`, etc.
- `@type.builtin` — language built-in types: `Int`, `String`, `Bool`, `Result`, `Option`, `Effect`, `ValidationError`.
- `@type.qualifier` — type modifiers/constructors in type expressions: `record`, `enum`.

### Functions, methods, and variables

- `@function` — free function names (in declarations and call positions).
- `@function.method` — method names (the part after the dot in `Type.method`).
- `@function.builtin` — built-in functions / constructors: `Ok`, `Err`, `Some`, `None`, `Effect.pure`.
- `@variable` — parameter names, `let` binding names.
- `@variable.builtin` — `self`.
- `@variable.parameter` — function parameters (subset of `@variable`, for editors that distinguish).

### Fields and members

- `@field` — record field names (in declarations and accesses).
- `@property` — capability operation names.

### Constants and literals

- `@constant` — variant names in sum types: `Pending`, `Placed`, `Declined`, etc.
- `@constant.builtin` — boolean literals: `true`, `false`.
- `@string` — string literals.
- `@string.escape` — escape sequences within strings: `\n`, `\t`, `\"`, `\\`.
- `@number` — integer literals.

### Operators and punctuation

- `@operator` — binary and unary operators: `+`, `-`, `*`, `/`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!`, `?`, `<-`, `=`, `->`, `.`.
- `@punctuation.bracket` — `(`, `)`, `{`, `}`, `[`, `]`.
- `@punctuation.delimiter` — `,`, `:`, `;` (if added), `|` (in sum variants).
- `@punctuation.special` — `=>` (in match arms), `...` (in record spread), `---` (doc block markers).

### Comments

- `@comment` — line comments (`-- ...`).
- `@comment.documentation` — doc blocks (the content between `---` markers).

### Attributes (refinement predicates)

- `@attribute` — refinement predicates: `Matches`, `InRange`, `MinLength`, `MaxLength`, `Length`, `NonNegative`, `Positive`, `NonEmpty`.

### Module references

- `@module` — qualified-name segments (the parts of a path like `commerce.money`).

### Errors

- `@error` — applied to ERROR nodes from tree-sitter's error recovery.

---

## 3. Specific highlighting rules

A few cases worth being explicit about:

### `self` and its fields

`self.state` and `self.<key>` (in agent handlers) should highlight `self` as `@variable.builtin` and the field name as `@field`. This makes agent handler bodies visually distinct from regular method bodies.

### Variant construction vs type reference

`Ok(value)` and `Pending` are variants. In expression position, they highlight as `@constant` (or `@function.builtin` for `Ok`, `Err`, `Some`, `None` specifically). In type-ref position (e.g., `Result[T, E]`), the type name `Result` highlights as `@type.builtin`.

The grammar must distinguish position-by-context. Tree-sitter handles this naturally via different grammar productions.

### Refinement predicates

`Matches(...)`, `InRange(...)`, `NonNegative`, etc., appear only in `where` clauses. They look syntactically like function calls / identifiers but should highlight as `@attribute` to mark their special role.

### Doc blocks

The `---` markers themselves are `@punctuation.special`. The content between markers is `@comment.documentation`. This lets editors render doc blocks distinctly (e.g., italicised or in a doc-comment colour).

### Capability operation calls

A call like `Logger.log(message)` has two interesting elements:
- `Logger` highlights as `@type` (it's a capability name, which is a type-like declaration).
- `log` highlights as `@property` (capability operations get this group, distinguishing them from method calls).

### Effect operations

`Effect.pure(value)` has:
- `Effect` as `@type.builtin`.
- `pure` as `@function.builtin`.

### Record construction shorthand

In `Money { minorUnits, currency }` (shorthand form where the binding name matches the field name), the identifier serves dual duty. Highlight as `@field` since the record-construction context is what matters semantically.

---

## 4. Grammar structure

The grammar should follow tree-sitter conventions:

- A `grammar.js` file declaring the grammar via the JavaScript DSL.
- Production rules for each syntactic form.
- Precedence and associativity for operators (matching the spec — expression precedence cascade).
- External scanner (in C) for any context-sensitive tokens. Bynk's tokens are mostly context-free; doc blocks are the only candidate for an external scanner because they span multiple lines with non-trivial delimiter rules.

### Doc block handling

Doc blocks (`---` line ... `---` line) are the trickiest part of the grammar. Recommendations:

- Use an external scanner for the doc-block content. The scanner reads until the closing `---` marker.
- Doc blocks have leading whitespace stripped (per the v0.3 spec amendment). The scanner records the common indent and emits the content with it stripped.
- Doc blocks attach to the following declaration; this is a parser-level concern, not a lexer one.

If implementing the external scanner is too much for this increment, an alternative is to use a regex-based token that matches `---\n([^-]|-(?!--))*---\n` (or similar) as a single multi-line token. Less precise but simpler.

### Multi-character operators

Some operators must be recognised greedily:
- `->` (function return type arrow) — not `-` then `>`.
- `<-` (effect bind) — not `<` then `-`.
- `==`, `!=`, `<=`, `>=` — comparison operators.
- `&&`, `||` — boolean operators.
- `=>` (match arm arrow) — not `=` then `>`.
- `...` (spread) — three dots.

Tree-sitter handles greedy matching naturally if the longer operators are declared first or with appropriate precedence.

### Reserved keywords

All Bynk keywords (per the various grammar specs) must be tokenised as keywords, not as identifiers. The tree-sitter grammar declares them as terminals; identifiers are matched only after the keyword tokens fail.

Reserved keywords across all versions:
```
agent, and, Bool, capability, commit, commons, consumes, context,
Effect, else, enum, Err, exports, false, fn, given, if, Int, is,
let, match, None, Ok, on, opaque, Option, provides, record, Result,
self, service, Some, state, String, transparent, true, type, uses,
ValidationError, where
```

---

## 5. Test corpus

The grammar's test corpus mirrors the compiler's fixture organisation. Each test is an input source followed by the expected parse tree (in tree-sitter's S-expression format).

Test file structure:

```
test/corpus/
├── v0.txt          -- refined types, pure functions, simple expressions
├── v0.1.txt        -- let, if/else, ?, Ok/Err, constructor calls
├── v0.2.txt        -- records, sums, methods, match, is, Option
├── v0.3.txt        -- opaque, multi-file headers, doc blocks, uses
├── v0.4.txt        -- contexts, exports (opaque/transparent), consumes
├── v0.5.txt        -- Effect, capabilities, providers, services, agents,
│                      commit, given, record spread, <-
├── doc-blocks.txt  -- doc block edge cases (indented, multi-line, etc.)
├── errors.txt      -- recovery tests for malformed input
```

Each test entry has the structure:

```
================================================================================
<test name>
================================================================================

<source code>

--------------------------------------------------------------------------------

(<expected parse tree>)
```

The corpus must cover at minimum:
- Every top-level declaration form.
- Every expression form.
- Every statement form.
- Edge cases in operator precedence.
- Doc blocks attached to each kind of declaration.
- Both forms of commons and context declarations (brace and fragment).
- Error recovery cases (malformed input that should produce ERROR nodes plus valid trees for the rest).

### Specific corner cases worth testing

- `match` with all variants having different bodies.
- Method calls chained: `value.method1().method2()`.
- Record spread inside `commit`.
- `<-` followed by `?`: `let x <- effectfulOp()?`.
- `if`/`else` as a value (in let RHS, as function return).
- `is` operator binding flowing into `&&` right-hand side.
- Nested record construction: `Order { items: Items { ... } }`.
- Generic type references in deeply nested positions: `Result[Option[Money], Error]`.

---

## 6. Implementation notes

### 6.1 Tree-sitter setup

The grammar lives in its own directory (`tree-sitter-bynk/`), separate from the compiler workspace. Standard tree-sitter project layout:

```
tree-sitter-bynk/
├── grammar.js                 -- the grammar definition
├── package.json
├── tree-sitter.json
├── queries/
│   ├── highlights.scm         -- highlighting query
│   ├── injections.scm         -- (probably empty for Bynk)
│   └── locals.scm             -- scope/binding analysis (optional)
├── src/                       -- generated by tree-sitter generate
│   ├── parser.c
│   ├── grammar.json
│   └── ...
└── test/
    └── corpus/                -- the test corpus
```

The `src/` directory is auto-generated; check it into git so consumers can build without `tree-sitter generate`.

### 6.2 Building

`tree-sitter generate` regenerates `src/` from `grammar.js`. `tree-sitter test` runs the corpus tests. `tree-sitter parse <file>` parses a file and prints the tree (useful for debugging).

For VS Code integration, the grammar is compiled to a native library (`.so`/`.dylib`/`.dll`) and shipped with the extension. VS Code's tree-sitter integration loads it at runtime.

### 6.3 Highlight query development

The `queries/highlights.scm` file declares which AST nodes get which highlighting groups. Develop iteratively:

1. Start with the obvious mappings (keywords, identifiers, literals).
2. Run `tree-sitter highlight <file>` to see what the editor would render.
3. Refine based on what looks right.
4. Test against the corpus.

Editors apply theme colors to highlighting groups; the same grammar produces different visual output in different themes. The grammar's job is to identify *what* each piece of code is, not *how* it should be coloured.

### 6.4 Versioning

The grammar version tracks the language version. The grammar for v0.5 (the current compiler version) is `tree-sitter-bynk v0.5.x`. Patch updates fix bugs in the grammar without changing what's recognised; minor updates add language features (after a compiler release adds new productions).

The grammar's `tree-sitter.json` declares its version. VS Code's extension manifest depends on a specific version.

---

## 7. Future grammar work

After this increment, the grammar covers v0–v0.5. Future versions will add productions as the language grows:

- v0.6: cross-context service call syntax, test contexts as a kind, wire-format expressions (if any).
- v0.7+: additional handler kinds (`on http`, `on queue`), provider composition syntax, state-machine sum types, saga/compensation syntax.

Grammar updates follow language updates — when the compiler ships a new version, the grammar gets a corresponding update.
