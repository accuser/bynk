# Karn LSP — Specification

A Language Server Protocol implementation for Karn. Provides syntax highlighting (via tree-sitter, specified separately in `karn-tree-sitter-spec.md`), live diagnostics, hover, go-to-definition, formatting, and a status-bar integration. Initial scope is VS Code only.

This is the first tooling increment for Karn — a pause from language development to make the language usable in practice. The compiler reaches v0.5 (intra-context behavioural layer); the LSP makes that capability accessible through an editor.

---

## 1. Scope

### In scope

- **Project discovery** via `karn.toml` at the project root.
- **Tree-sitter syntax highlighting** (specified in `karn-tree-sitter-spec.md`).
- **Live diagnostics** — compile errors and warnings shown as the user types, with debouncing. Configurable to on-save for users on slow machines.
- **Hover** — type information, declarations, and doc blocks shown on cursor hover.
- **Go-to-definition** — F12 / Cmd-click jumps from a name to its declaration, across files in a project.
- **Format-on-save** — canonical formatting applied when files are saved; available as a manual command.
- **`karnc fmt` CLI command** — format Karn source files from the command line.
- **Document symbols** — outline view of a file's declarations, shown in VS Code's outline pane.
- **Status-bar integration** — VS Code status bar shows the project name and Karn compiler version.
- **VS Code extension** packaged for local sideload.
- **References & rename** *(v0.25, §3.8–3.9)* — binding-index-backed, project-wide.
- **Quick-fix code actions** *(v0.26, §3.10)* — from the diagnostics' structured suggestions.
- **Workspace symbol search & document highlights** *(v0.26, §3.11–3.12)* — index queries.
- **Inlay hints** *(v0.27, §3.13)* — inferred-type hints from the analysis round's harvested set.
- **Semantic tokens** *(v0.28, §3.14)* — resolution-aware highlighting from the binding index.

### Out of scope (deferred to later tooling increments)

- General autocomplete at every cursor position (substantial work). *(v0.17 adds a
  scoped completion for the adapter surface: consumable units after `consumes `,
  a unit's exported capabilities inside `consumes U { … }`, and in-scope
  capabilities after `given `. Broader completion remains deferred.)*
- Inlay hints (showing inferred types inline).
- Code lenses (e.g., "show service handlers" markers).
- Refactorings beyond rename (extract function, inline); non-quick-fix code-action kinds.
- Semantic tokens (type-aware highlighting beyond tree-sitter's syntactic level).
- Editor commands beyond what LSP standard provides (no "Karn: Build" / "Karn: Run tests" yet — those come later).
- Marketplace publication.
- Editor support beyond VS Code.
- Configuration formats other than `karn.toml` (no `.yaml`, no `.json` for now).

---

## 2. Project model

### 2.1 Project discovery

A Karn project is a directory containing `karn.toml` at its root. The LSP discovers the project root by walking upward from any open `.karn` file until it finds `karn.toml`. If none is found before the filesystem root, the LSP treats the file as a single-file project (no workspace features) and shows a warning.

### 2.2 `karn.toml` schema

```toml
[project]
name    = "my-karn-project"     # required
version = "0.1.0"               # optional, free-form

[paths]
src = "src"                     # source root (default: "src")
out = "out"                     # compiled TypeScript output (default: "out")

[fmt]
indent             = "tab"      # "tab" (default) or "spaces"
indent_width       = 1          # number of tabs / spaces per level (default: 1 for tab, 2 for spaces)
max_line_width     = 100        # soft target for line wrapping (default: 100)
trailing_comma     = true       # trailing comma in multi-line lists (default: true)

[lsp]
diagnostics_mode   = "live"     # "live" (default) or "on_save"
diagnostics_debounce_ms = 300   # debounce delay for live mode (default: 300)
```

All fields are optional. A minimal `karn.toml` is:

```toml
[project]
name = "my-karn-project"
```

The LSP applies defaults for everything else. The file's *presence* is what marks the project root; its contents are advisory.

Future configuration alternatives (`.yaml`, `.json`) are not supported in this increment but the schema is designed to translate cleanly.

### 2.3 Workspace discovery

Once the project root is found, the LSP discovers all `.karn` files under the configured `src` directory (recursive). These files form the project's source corpus. The LSP loads, parses, resolves, and type-checks them all on startup.

File watching is enabled for the `src` directory. Changes (file added, removed, modified externally) trigger re-discovery and re-resolution of affected files.

---

## 3. LSP capabilities

### 3.1 Syntax highlighting

Provided via a tree-sitter grammar (see `karn-tree-sitter-spec.md`). The VS Code extension ships the compiled tree-sitter grammar. VS Code natively supports tree-sitter for syntax highlighting when configured.

The tree-sitter grammar covers all Karn syntactic forms from v0 through v0.5. Highlighting groups follow standard tree-sitter conventions (`@keyword`, `@type`, `@function`, etc.) mapped to VS Code theme tokens.

### 3.2 Live diagnostics

The LSP server runs the existing Karn compiler on the project's source corpus and reports errors and warnings as LSP diagnostics back to the editor.

**Behaviour (REVISED v0.24, ADR 0052 — project-wide):**
- With a project root (`karn.toml`): every change triggers a **debounced
  whole-project analysis** via `karnc::diagnose_project` — non-bailing
  (every file's diagnostics, not the first failure's), **overlay-aware**
  (open buffers layered over disk, so unsaved edits are diagnosed), and
  **file-attributed** (collection-point tagging; no `Span` change).
  Context files get full resolve/check diagnostics — the pre-v0.24 server
  resolved/checked `Commons` units only.
- Publish: **all-and-clear** — every file with diagnostics is published;
  every file that carried diagnostics last round and is now clean gets an
  **empty publish**. The publish/clear diff is a pure function
  (`karn-lsp/src/publish.rs`), unit-tested without a transport.
- **Positions convert against the analysed snapshot** — `diagnose_project`
  returns the per-file text it analysed; spans never convert against a
  newer buffer (the analyse→publish window is real; debounce narrows but
  does not close it).
- Project-level diagnostics with no single owning file (group/cycle/
  directory validations) surface on `karn.toml` at position 0:0.
- Single-file mode (no `karn.toml`): the per-buffer `diagnose` path,
  unchanged.
- Debounce: 200ms, generation-counter based (a typing burst produces one
  analysis). Incremental/salsa-style recompute is deferred — full
  re-analysis is acceptable at current scale.
- Reported back via `textDocument/publishDiagnostics`.

**Severity levels:**
- *Error* — compile errors that prevent valid output (type errors, unresolved references, etc.).
- *Warning* — compile warnings (unused capabilities in `given`, orphan doc blocks, etc.).
- *Hint* — suggestions (not currently used; reserved for future).

**Error recovery:** The LSP shows *all* errors in a file, not just the first one. This requires the parser and checker to recover and continue after errors rather than bail. If the existing compiler doesn't do this fully, extending it is part of this increment.

**Diagnostics include:**
- Primary message (the error description).
- Primary range (the source span).
- Secondary ranges where helpful (e.g., the conflicting declaration, the type mismatch source).
- Error category code (e.g., `karn.types.if_branch_mismatch`) included in the diagnostic for filterability.

**Configuration:** Users can set `[lsp].diagnostics_mode = "on_save"` to disable live diagnostics. In on-save mode, diagnostics run only when the file is saved.

### 3.3 Hover

On cursor hover over a name, the LSP returns information via `textDocument/hover`. Specifically:

**On a type name:**
- The type's declaration (the source form, formatted).
- Any attached doc block.
- For exported context types: the visibility (`opaque` or `transparent`).
- For opaque types: a note that the representation is hidden.

```
type Money = { minorUnits: Int where NonNegative, currency: CurrencyCode }

---
The Money type represents an amount in a specific currency...
---
```

**On a function or method name:**
- The full signature.
- The doc block.
- For methods: whether instance or static.
- For service handlers: the `given` clause.

**On a variable / `let` binding:**
- The inferred or declared type.

**On a field access:**
- The field's declared type, plus any inline refinement.

**On a capability name:**
- The capability's declaration with all operations.
- The doc block.
- The available providers in the current context.

**On a keyword:** nothing (hovers only fire on identifiers).

Hover content is rendered as Markdown by the editor. The LSP returns `MarkupContent` with `MarkupKind.Markdown`.

**Markdown layout.** Hover content follows a consistent structure:

1. A fenced code block (```` ```karn ```` ) containing the declaration's source form (formatted via the canonical formatter so it matches the project's style).
2. A blank line.
3. The attached doc-block content (if any), as plain Markdown.
4. For types with additional metadata (exported visibility, opaque representation note, etc.), a short list below the doc content.

Example for a `Money` record type:

````markdown
```karn
type Money = {
  minorUnits: Int where NonNegative,
  currency:   CurrencyCode,
}
```

The Money type represents an amount in a specific currency. The minorUnits
field is the smallest indivisible unit of the currency.

— exported transparently from `commerce.payment`
````

Hover content stays compact — typically under twenty lines. For declarations that are long (e.g., a capability with many operations), the full declaration is rendered; the editor's hover popup handles overflow with scrolling.

### 3.4 Go-to-definition

`textDocument/definition` returns the location of a name's declaration.

**Resolution:**
- Type names → the `type` declaration.
- Function names → the `fn` declaration.
- Method names (`value.method()`) → the method's `fn TypeName.method` declaration.
- Field names (`record.field`) → the field's declaration in the record type.
- Variant names (`Pending`, `Ok`, etc.) → the variant's declaration in the sum type.
- Capability names → the `capability` declaration.
- Service operation names → the service's `on call` handler (the `on` keyword's location).
- Agent names → the `agent` declaration.

**Cross-file (required).** Definitions in other files within the same project must be resolved. The returned location points to the correct file and source range. This is a hard requirement — the language explicitly supports multi-file commons (v0.3) and context consumes graphs (v0.4); navigation that doesn't cross file boundaries is unusable for any non-trivial project. The LSP's project module (which loads all `.karn` files at startup) already has the symbol tables needed; the definition lookup walks those tables, not just the open file's local tables.

**Binding-correct via the index (v0.25, ADR 0053).** Definition (and hover) resolve through the project **binding index** first — the use→def edges recorded at the compiler's own resolution sites — so duplicate names in different units navigate to the *bound* declaration, not the first name match. The legacy name-matching walk remains only as a fallback for symbol kinds the index defers (locals, methods, record fields, capability ops).

**Imported names:** When a context uses a commons, names from the commons resolve back to the commons declaration (not to the context's rebranded copy — the original source location is more useful).

### 3.5 Formatter

The Karn formatter applies canonical style to source files.

**Style rules (the defaults; all configurable via `karn.toml`):**

- **Indentation:** tabs by default. One tab per nesting level. (This is for accessibility — users set their preferred tab width in VS Code; tab-based indentation respects that, space-based indentation does not.)
- **Brace style:** K&R / same-line. `if cond {` on one line, not `if cond` then `{` on the next.
- **Trailing commas:** present in multi-line records, sums, parameter lists, argument lists.
- **Blank lines:** one blank line between top-level declarations (types, functions, services, agents, capabilities, providers). No blank lines between fields within a record or arms within a match.
- **Doc blocks:** immediately above the declaration they document, no blank line between. Doc content has the common indent stripped (per v0.3's clarification).
- **Spacing:** one space around binary operators (`a + b`, not `a+b`); one space after commas; no space inside parens.
- **Line width:** soft target of 100 columns; the formatter wraps long lines where natural (after commas in long parameter lists, at `&&`/`||` boundaries in long expressions). For lists with delimiters (parameter lists, argument lists, record fields, exports clauses), the formatter emits single-line form when it fits within the line width and multi-line form otherwise. Multi-line form uses trailing commas; single-line form does not.

**Idempotency:** Running the formatter twice produces the same result. This is a requirement.

**Preservation:**
- Comments preserve their position relative to surrounding code.
- Doc blocks preserve their content verbatim (only the indentation is normalised).
- The semantic meaning of the code is preserved (same AST after parse → format → re-parse).

**Comment-preservation implementation requirement.** The formatter must not drop line comments (`-- ...`). This is a hard requirement — dropping user comments is data loss, which destroys trust in format-on-save and ultimately in the canonical-style discipline. Implementing this requires the lexer to emit comments as trivia tokens (or similar) so that the parser/formatter can track their positions and emit them in the formatted output. A side-pass scan of original source is acceptable as a fallback but trivia-tracking is the principled approach.

Specifically:
- Comments before a top-level declaration go above the declaration in the formatted output (with the doc block, if any, between the comment and the declaration).
- Comments at the end of a line stay on that line (`expr  -- note`).
- Comments on their own line within a block preserve their position relative to surrounding statements.
- Multi-line groups of comments stay together.

Doc blocks (`---`) are separate from line comments and are already preserved via the AST.

**Integration:**

- **Format-on-save:** the LSP responds to `textDocument/formatting` requests. VS Code with `editor.formatOnSave: true` calls this on every save. The LSP returns the formatted document as a single text edit.
- **Range formatting:** `textDocument/rangeFormatting` formats a selected range. Useful for "format this function." Implemented best-effort — the formatter operates on whole declarations, so the returned range may be slightly wider than requested.
- **CLI:** `karnc fmt [file...]` formats files in place. `karnc fmt -` reads from stdin, writes to stdout.

### 3.6 Status-bar integration

The VS Code extension shows two status-bar items when a Karn file is open:

- **Project name** — from `karn.toml`'s `[project].name`. Clicking opens `karn.toml`.
- **Compiler version** — the version of the bundled `karnc` binary. Clicking does nothing (informational).

If `karn.toml` is missing, the project-name slot shows "no project" (clicking suggests creating one).

The status bar items only appear when the active editor has a `.karn` file open.

### 3.7 Document symbols

The LSP responds to `textDocument/documentSymbol` requests with a hierarchical outline of the file's declarations. This populates VS Code's "Outline" pane (in the explorer sidebar) and powers the "Go to Symbol in File" command (Cmd-Shift-O).

**Symbols and their kinds:**

The LSP maps Karn declarations to LSP `SymbolKind` values:

- `commons` declaration → `Module` (top-level container).
- `context` declaration → `Module`.
- `type T = ...` → `Struct` (for records), `Enum` (for sums), `Class` (for opaque types), `TypeParameter` (for refined values).
- `fn name(...) -> T` (free function) → `Function`.
- `fn TypeName.method(...)` → `Method` (nested under the type).
- `capability X { ... }` → `Interface`, with operations as `Method` children.
- `provides X = Y { ... }` → `Object`, with operations as `Method` children.
- `service X { ... }` → `Class`, with handlers as `Method` children.
- `agent X { ... }` → `Class`, with the state block as `Property` children and handlers as `Method` children.
- Record fields → `Field` children of their type.
- Sum variants → `EnumMember` children of their type.

**Hierarchy:**

The top-level container (commons or context) is the root. All other declarations are children. Methods nest under their type; record fields nest under the record; variants nest under the sum.

For multi-file commons or contexts, each file has its own document symbol tree — the LSP returns symbols for the current file only. The outline view shows the contents of the current file.

**Ranges:**

Each symbol carries two ranges:
- `range` — the full extent of the declaration (from the `type`/`fn`/etc. keyword to the closing brace).
- `selectionRange` — the identifier itself (the name being declared).

Clicking a symbol in the outline jumps to the `selectionRange`. The `range` is used for highlighting and breadcrumb display.

**Documentation:**

If a declaration has an attached doc block, its content (truncated to one line if multi-line) appears as the symbol's detail. VS Code shows this alongside the symbol name in the outline.

### 3.8 References (v0.25)

`textDocument/references` returns every reference to the symbol under the cursor, project-wide, from the **binding index** (ADR 0053): the index is assembled from use→def edges recorded at the compiler's resolution sites during the project analysis pass — binding-correct by construction, never name-matched. Two same-named symbols in different units never conflate.

**Coverage.** Top-level named declarations: types, free `fn`s, capabilities, services, agents, providers. Reference sites include every way such a symbol is named — annotation and static-receiver type positions (`T.of`, `T.Variant`, `T { … }`, pattern qualifiers, `Mock[T]`), fn calls and first-class fn values, `given` clauses (bare, dotted `B.Cap`, flattened), capability op-call receivers, cross-context service calls, the clause lists (`exports opaque/transparent { T }`, `exports capability { Cap }`, `consumes U { Cap }` selections), and references from **test and integration units** (including a test body's `svc.call(…)`). Spans cover the **name segment only** — in `shop.billing.Pay` the reference is `Pay`.

**Deferred kinds** (no index entries yet): local bindings, instance methods, record fields, capability op names. First-party `karn.*` units are excluded — they are not user-editable.

Positions convert against the **analysed snapshot** (§3.2's rule); `includeDeclaration` is honoured (the definition site is first). Requests outside a project (no `karn.toml`) return no results.

### 3.9 Rename (v0.25)

`textDocument/rename` renames a symbol project-wide; `textDocument/prepareRename` (declared via `prepareProvider: true`) validates the position first and **refuses** (returns null) on anything the index does not cover — locals, methods, fields, op names, and unit/context names (renaming a unit implies a file move; that is the A-3 file-operations increment).

**Plan.** The edit set is exactly the index's sites for the symbol — definition plus every reference, name segments only — built against a **fresh analysis** of the current buffers. The new name must lex as a single identifier (keywords refuse).

**Validation — two checks, both correct-by-construction (ADR 0053):**

1. **Collisions by re-analysis.** The candidate edits are applied to an in-memory overlay and the project is re-analysed; any **new** diagnostic (per file + category) refuses the rename. This catches every collision class — same-unit name clash, `uses` import conflicts, flattened-capability clashes — without enumerating them.
2. **Capture/escape by index equality.** Re-analysis alone misses *silent re-binding* (a rename can make an existing name resolve somewhere new with no diagnostic — declared fns shadow fn-typed locals in call position). The re-built index must equal the pre-rename index **modulo the rename**: the renamed symbol's sites are exactly the edited ones, every other symbol's reference set is unchanged (after remapping the edit deltas). Any difference refuses.

A refused rename surfaces as an LSP request error with the reason — never a partial edit, and never a `karn.*` diagnostic.

**Versioned edits.** The `WorkspaceEdit` uses `documentChanges` with `TextDocumentEdit`s carrying the document **versions captured when the analysed snapshots were taken** (disk-only files carry none). A buffer that drifted past its analysed version makes the client reject the rename rather than mis-apply it.

### 3.10 Code actions (v0.26)

`textDocument/codeAction` offers **quick-fixes** (`CodeActionKind.QuickFix`, the only kind advertised) computed from the **structured suggestions** riding on the diagnostics (ADR 0054): `karnc` attaches machine-applicable `Suggestion`s — message, span→replacement edits, an `Applicability` — at the diagnosis site, the only place the exact spans and replacement are known. The LSP never re-derives a fix from a diagnostic's category or message.

**Keying.** A diagnostic's suggestions are offered when the requested range intersects the **diagnostic's span** — never the edits' spans, which can land far from the squiggle (both `given` fixes do: the diagnostic sits on the usage site or the return type; the edit lands in the clause).

**Serving.** Actions are computed from the **cached analysis round** (the same retained snapshots/versions that back references and rename, extended to retain the round's per-file diagnostics), never a fresh analysis — a fresh run is slow and can disagree with the squiggles the client is showing. A request arriving **before the first analysis round**, or for a file **outside the project**, returns the empty list. The request range converts against the analysed snapshot (§3.2's rule); each action's `WorkspaceEdit` is a **versioned** `TextDocumentEdit` against the analysed document version, so a drifted buffer rejects the edit rather than mis-applying it.

**Applicability.** Only `MachineApplicable` suggestions surface as quick-fixes; `HasPlaceholders` exists for a future CLI `--fix` and is never offered as a one-click edit.

**The seed catalogue (v0.26):** remove an unused capability from the `given` clause (`karn.given.unused_capability`) and add an undeclared one (`karn.given.undeclared_capability`, bare and cross-context `B.Cap` — the cross-context entry inserts the canonical context path). Both edits are **list-aware**, authored in the checker: removal takes one adjacent comma and surrounding space with it, removing the *only* capability deletes the `given` keyword too, and adding inserts `, Cap` after the last entry or synthesises ` given Cap` after the handler's return type when the clause is absent. The result never double-commas, leading-commas, or leaves a dangling `given`.

### 3.11 Workspace symbols (v0.26 rider)

`workspace/symbol` enumerates the binding index's **definitions** (ADR 0055) — the same coverage as §3.8: types, free fns, capabilities, services, agents, providers. The query is a case-insensitive substring match on the symbol name (an empty query lists all), results ordered by (name, unit) with the owning unit as the container name. Positions convert against the analysed snapshot.

### 3.12 Document highlights (v0.26 rider)

`textDocument/documentHighlight` returns the symbol-at-cursor's occurrences **within the active file** — the §3.8 references query, file-scoped, definition included (ADR 0055). The index does not distinguish read from write references, so highlight `kind` is omitted. Requests on uncovered symbol kinds (locals, methods, fields, op names) return no highlights.

### 3.13 Inlay hints (v0.27)

`textDocument/inlayHint` returns **inferred-type** hints for the request's visible range: `let` and `let <-` bindings and lambda parameters whose annotation is **absent** (an explicit annotation needs no hint; `_` binds nothing and gets none). Each hint anchors at the **end of the binding name**, label `: T` with `T` in Karn surface syntax via the checker's display rendering (`List[Int]`, `Option[String]`, `Int -> Int`); a `let <-` hint shows the **peeled `Effect[T]` payload** — the binding's actual type. `kind` is `Type`. No padding is requested: the separator is part of the label, so the hint reads as source syntax (`x: Int`). *(The proposal sketched padding-left; implementation drops it — a gap before the `:` would break the source-syntax reading.)*

**Harvesting (ADR 0056).** Hints are a curated per-file set recorded by the **checker** at each binding site as it computes the binding's final type — never a tool-side re-inference, and not the raw typed model (which cannot position a hint). The sink is a `&mut` parameter (the `RefSink` shape), so recorded hints **survive a transient type error** at every site the checker still reaches; a fn-body error short-circuits that file's v0.5 declaration pass, so its **handler-body** hints are suppressed until the error clears. Synthetic and test/integration files record nothing.

**Serving.** Hints are served from the **cached analysis round** only, like code actions (§3.10): a request before the first round, or for a file outside the project, returns the empty list — as does a file whose group's composition failed (no analysed model). The visible range and the hint positions convert against the analysed snapshot (§3.2's rule). The server always produces hints; visibility is the client's (the editor's built-in inlay toggle; a `karn.inlayHints.enable` extension setting is a B-1 surface item). `inlayHint/resolve` is not declared — labels are computed eagerly.

### 3.14 Semantic tokens (v0.28)

`textDocument/semanticTokens/full` (and `…/range`) returns resolution-aware tokens for the **index kinds** — the classification tree-sitter / the extension's TextMate grammar *can't* make (is this `Foo` a type, a capability, or a value?). Tokens are **additive over the client's syntactic layer**: keywords, literals, comments, and the uncovered identifiers (locals, params, generic type parameters — not in the index) keep their syntactic colour.

**The legend (frozen — ADR 0057).** The legend's array order **is the wire encoding**; entries are append-only and never reordered (a stability test pins both arrays):

| | |
|---|---|
| **Token types** | `type`, `function`, `capability`, `service`, `agent`, `provider` |
| **Token modifiers** | `declaration`, `refined`, `opaque`, `platformNative` |

`type`/`function` are the standard LSP types; the other four are **custom** (theme defaults ship with the extension — a B-1 item; unthemed clients fall back to the syntactic colour). `declaration` marks a `def` site (references carry none); `refined` requires a refinement **present** (`type X = Int` is a plain alias and carries neither `refined` nor `opaque`); `opaque` is orthogonal, so `opaque B where …` carries both; `platformNative` marks symbols declared by a platform unit (e.g. `karn.cloudflare`'s `Kv`).

**Sources (ADR 0057).** A pure `index_queries` producer reads **two** sources from the cached round: `ProjectIndex.symbols` (user-defined defs + refs) and **`ProjectIndex.foreign_refs`** — references to first-party (`karn.*`) symbols, which `symbols` deliberately drops (synthetic defs point at files not on disk; definition/rename/workspace-symbol must never surface them). The side table is tokens-only; the v0.25 navigation invariants on `symbols` are untouched. `test`/`integration` files' references are in the index, so semantic tokens light up test files too.

**Serving & encoding.** Tokens are served from the **cached analysis round** only (no cached round / non-project file → empty); positions and the `range` request convert against the analysed snapshot (§3.2's rule). The token array is delta-encoded per the protocol — relative line/char, position-sorted (name segments never overlap), lengths in UTF-16 code units. The **`delta`** request variant is not declared (a later optimisation).

**Client theming (v0.29, ADR 0058).** The custom token types (`capability`/`service`/`agent`/`provider`) and modifiers (`refined`/`opaque`/`platformNative`) render with **no colour** under default themes unless the *client* declares them. The VS Code extension therefore declares them in `contributes.semanticTokenTypes` / `semanticTokenModifiers` (each custom type with a standard `superType` — `interface`/`type`/`class`/`function` — so semantic-highlighting themes colour it) and maps fallback TextMate scopes in `contributes.semanticTokenScopes` for theme without semantic rules. The declared **names are a cross-component contract** with the server's frozen legend — they must match exactly, or those tokens silently go unthemed — enforced by a `karn-lsp` test that parses the extension's `package.json` against `semantic_tokens_legend()` (the single source of truth). Token *visibility* is the client's: the built-in `editor.semanticHighlighting.enabled`, with no Karn-specific toggle.

### 3.15 Completion (v0.17; positional in v0.30)

`textDocument/completion` returns context-keyed candidates. **Context detection is lexical** — it keys off the line up to the cursor, not the parse tree, because completion fires mid-edit when the buffer rarely parses; **candidates are semantic** (drawn from parsing the *other* project files with recovery, plus the static `karnc` registries). The `completion::complete(line_prefix, doc_text, src_root)` producer is a pure function, fully unit-tested; the handler builds `line_prefix` and maps `CompletionKind` → `CompletionItemKind`.

The recognised contexts and their candidate sources:

| Context (lexical trigger) | Candidates | Item kind |
|---|---|---|
| `consumes <prefix>` (v0.17) | consumable units (contexts/adapters + `karn`) | `MODULE` |
| `consumes U { … ` (v0.17) | the capabilities `U` exports | `INTERFACE` |
| `given … ` (v0.17) | in-scope capabilities (local, flattened, `U.Cap`) | `INTERFACE` |
| **type position** (`: T`, `-> T`, `[ … ]` type args) (v0.30) | built-in types + `karn`-surface transparent types + project `type` decls | `STRUCT` |
| **keyword position** (a bare word at a decl/statement start) (v0.30) | reserved keywords (with registry docs) + declaration snippets | `KEYWORD` / `SNIPPET` |
| **name-receiver member** (`UpperIdent.`) (v0.30.1) | sum-type variants; refined/opaque `of`/`unsafe`; capability ops; built-in type statics (`Int.parse`/`Json.decode`) | `ENUM_MEMBER` / `METHOD` |

**Built-ins/surface come from static registries, not the index (ADR 0061).** Because first-party symbols aren't indexed (§3.14's finding), the built-in types (`Int`/`Bool`/`Float`/`String`/`Option`/`Result`/`Effect`/`List`/`Map`), keyword docs, the `karn`-surface transparent types, and the built-in type statics (`Int.parse`/`Float.parse`/`Json.encode`/`decode`) are sourced from `karnc::{keywords, builtin_names, firstparty}` (and a small static statics table) — the index (here, the project parse) supplies only *project* symbols. Keyword candidates are the lowercase-initial reserved words (declaration/statement keywords); uppercase type/value names belong to type/expression position. Snippets carry LSP `${n:…}` tab stops (`InsertTextFormat::SNIPPET`).

**Name-receiver members (v0.30.1, ADR 0062).** The `.`-member context is split by *what sits before the dot*. A **name** receiver — a single uppercase-initial identifier (`Color.`, `Email.`, `Clock.`) — is resolved from the project/surface parse to a sum/refined/opaque type or a capability, and its members are enumerated from the AST (no typed model, no scope query — the same mid-edit-safe recovery parse). A *plain* alias `type Id = Int` **does** carry `of`/`unsafe` (the emitter brands every `Refined` body), so they are offered; a **record** type has no name-receiver members (its fields are value-receiver). A **value** receiver — a lowercase `x.` — is deferred to slice 3.

**Conservative detection.** Type-position triggers exclude a list-literal `[` (its bracket isn't preceded by a type constructor); the one accepted false positive is a record *construction* value (`Order { id: ` — lexically identical to a record field-type declaration), where offering type names is mild noise. Name-receiver detection requires a *single* uppercase-initial segment, excluding the decimal `1.` and the `.`-qualified `a.B.`. Out-of-context prefixes (e.g. `let x = `) yield `[]`.

**Deferred to slice 3 (ADR 0062).** **Value `.method`/`.field`** completion needs the receiver's type: `expr_types` is computed during checking but discarded on the LSP `Analyse` path, keyed by span not offset, and — fundamentally — completion fires mid-edit when the buffer doesn't parse, so the checker never reaches the cursor expression (the scout's #1 risk; warrants its own design spike). **Locals/params in scope** need a scope-at-offset query the index doesn't have (it tracks only top-level symbols). Signature help, auto-import/add-`consumes` resolve, and postfix expansion are later still.

---

## 4. Implementation architecture

### 4.1 Component layout

The tooling project consists of four components:

```
karn-tooling/
├── tree-sitter-karn/        -- Tree-sitter grammar (separate sub-project)
│   ├── grammar.js
│   └── ...
├── karn-lsp/                -- LSP server binary (Rust, in the compiler workspace)
│   ├── src/main.rs
│   ├── src/handlers/        -- LSP request handlers
│   └── ...
├── karn-fmt/                -- Formatter (Rust, in the compiler workspace; used by both LSP and CLI)
│   └── src/lib.rs
└── vscode-karn/             -- VS Code extension (TypeScript)
    ├── package.json
    ├── src/extension.ts
    └── ...
```

The tree-sitter grammar lives in its own repo / subdirectory because tree-sitter has its own build tooling (`tree-sitter generate`, `tree-sitter test`).

The LSP server and formatter live in the existing compiler workspace as new Rust crates. They depend on the compiler's existing modules (parser, resolver, checker).

The VS Code extension is a minimal TypeScript project that activates the LSP and ships the tree-sitter grammar.

### 4.2 Dependencies

**For `karn-lsp`:**
- `tower-lsp` — LSP server framework for Rust. Handles protocol plumbing.
- `tokio` — async runtime (tower-lsp uses it).
- The existing compiler modules (in-tree dependency).

**For `karn-fmt`:**
- The existing compiler's AST and parser (in-tree dependency).
- `std::fmt` for output rendering.

**For the tree-sitter grammar:**
- `tree-sitter-cli` (npm package; used for development and code generation).
- No runtime dependencies beyond tree-sitter itself.

**For the VS Code extension:**
- `vscode` (`@types/vscode`) — VS Code API.
- `vscode-languageclient` — LSP client for VS Code.
- `tree-sitter-karn` (the compiled grammar).

### 4.3 LSP protocol surface

The LSP server declares support for these capabilities:

```
textDocument.synchronization: Full
textDocument.publishDiagnostics
textDocument.hover
textDocument.definition
textDocument.formatting
textDocument.rangeFormatting
textDocument.documentSymbol
textDocument.references            (v0.25, §3.8)
textDocument.rename                (v0.25, §3.9; prepareProvider: true)
textDocument.codeAction            (v0.26, §3.10; kinds: [quickfix])
textDocument.documentHighlight     (v0.26, §3.12)
textDocument.inlayHint             (v0.27, §3.13)
textDocument.semanticTokens        (v0.28, §3.14; full + range)
workspace.symbol                   (v0.26, §3.11)
workspace.workspaceFolders
workspace.didChangeWatchedFiles
```

(Completion was added in v0.17 for `consumes`/`given` surfaces.)

Not declared (out of scope so far):
- completionItem/resolve
- codeLens, inlayHint/resolve
- semanticTokens/delta
- signatureHelp

### 4.4 Error recovery for diagnostics

The LSP needs the parser and checker to recover and continue after errors so multiple errors can be reported. The existing compiler may currently bail at first error in each phase; extending it to recover is part of this increment.

**Parser recovery:** Skip to the next synchronisation point (closing brace, semicolon, top-level declaration keyword) and continue. Each recovered region produces a separate parse error but doesn't prevent parsing the rest.

**Resolver recovery:** Continue resolving even when individual names fail (mark them as unresolved in the AST). The checker then sees an annotated AST with both resolved and unresolved nodes.

**Checker recovery:** Continue type-checking even when individual expressions fail. Unresolved types propagate but don't cascade — once an unresolved type is encountered, further errors on the same expression are suppressed to reduce noise.

The result: a parse-resolve-check pipeline that produces a complete list of errors per file, not just the first.

### 4.5 Performance targets

- **Project load:** under 2 seconds for projects with ~100 source files. Larger projects are out of scope for performance optimisation in this increment.
- **Live diagnostics:** under 100ms for a single-file change (post-debounce). The 300ms debounce plus 100ms diagnostics = response within ~400ms after the user pauses typing.
- **Hover:** under 50ms for any hover query.
- **Go-to-definition:** under 100ms.
- **Format:** under 200ms for a 1000-line file.

These are targets, not requirements. If real-world performance falls short, the on-save fallback for diagnostics is the immediate mitigation; deeper optimisations (incremental compilation, persistent caches) come later.

---

## 5. The VS Code extension

### 5.1 Activation

The extension activates when:
- A workspace folder contains a `karn.toml` file, OR
- A `.karn` file is opened.

On activation:
1. Locate the `karnc-lsp` binary (bundled with the extension or installed separately — for first cut, bundled).
2. Start the LSP server as a child process.
3. Connect via stdio.
4. Register file watchers on `**/*.karn` and `karn.toml`.
5. Register the tree-sitter grammar for `.karn` files.
6. Show status-bar items.

### 5.2 Configuration

The extension reads VS Code settings under the `karn.*` namespace:

```json
{
  "karn.executablePath": "karnc-lsp",    // path to the LSP server binary
  "karn.trace.server": "off"             // "off" | "messages" | "verbose"
}
```

VS Code's built-in `editor.formatOnSave` is honoured for format-on-save behaviour.

### 5.3 Logging

LSP server logs to `~/.karn-lsp.log` at warning level by default. The `karn.trace.server` setting can crank this to verbose for debugging.

The extension exposes a "Karn LSP" output channel in VS Code for protocol-level traces.

---

## 6. Testing strategy

### 6.1 Tree-sitter grammar tests

The tree-sitter project includes its own test infrastructure (`tree-sitter test`). Tests are organised by syntactic form:

- `corpus/v0.txt` — refined types, pure functions (v0 syntax).
- `corpus/v0.1.txt` — let, if/else, Result, etc.
- `corpus/v0.2.txt` — records, sums, methods, match, is, Option.
- `corpus/v0.3.txt` — opaque, multi-file, doc blocks, uses.
- `corpus/v0.4.txt` — contexts, exports, consumes.
- `corpus/v0.5.txt` — Effect, capabilities, providers, services, agents, commit.

Each test is an input source plus the expected parse tree. `tree-sitter test` verifies.

### 6.2 LSP tests

The LSP server has unit tests for individual handlers (hover, definition, formatting) using synthetic in-memory documents. End-to-end tests use the existing fixture corpus from the compiler — verifying that fixtures produce the expected diagnostics, hover content, and so on.

The v0.25 references/rename surface is tested as **pure functions** (`index_queries`): the `karnc` fixture matrix proves the index captures every reference kind with name-segment spans and no same-name conflation, and the rename pipeline (plan → apply → re-analyse → validate) is exercised over real multi-file temp projects, including a collision refusal and a genuine capture refusal. The handlers are thin position/packaging shims over that core; the JSON-RPC round-trip harness remains deferred to the first feature that needs it.

The v0.26 code-action surface follows the same pattern: fix **correctness** is pinned in `karnc` (each seed diagnostic carries its expected suggestion; the list-aware `given` fixtures assert exact text for first/middle/last/only positions, and every applied fix re-diagnoses clean), and the pure quick-fix computation (`code_actions`) is exercised end-to-end over a real temp project — diagnostic-span keying, the versioned `WorkspaceEdit`, and the applied-edit round-trip. The capability advertisement is a unit check over the extracted `server_capabilities()`; no transport round-trip is claimed. The riders are `index_queries` unit tests.

### 6.3 Formatter tests

The formatter is tested via snapshot tests using `insta` (same pattern as the existing compiler). For each input file, the expected formatted output is checked in. The formatter is run on the input; the result is diffed against the snapshot.

Idempotency tests: for each input, run the formatter twice and verify the output is identical between runs.

### 6.4 VS Code extension tests

Minimal — VS Code's testing infrastructure is verbose. For first cut, manual smoke tests:
- Open a fixture in VS Code with the extension installed.
- Verify highlighting appears.
- Verify diagnostics show.
- Verify hover works on a few identifiers.
- Verify go-to-definition works.
- Verify format-on-save works.

A more thorough test suite can come later if the extension grows complex.

---

## 7. Build and distribution

### 7.1 Building

The Rust components build via standard `cargo build --release` from the compiler workspace. Outputs:
- `target/release/karnc` — the compiler CLI (already exists).
- `target/release/karnc-lsp` — the LSP server.
- The formatter lives as a library used by both `karnc` (for `karnc fmt`) and `karnc-lsp` (for the formatting capability).

The tree-sitter grammar builds via `tree-sitter generate` in the grammar directory. Output: `tree-sitter-karn.so` (or `.dylib`/`.dll` per platform).

The VS Code extension builds via `npm run package` in its directory. Output: `karn-vscode-<version>.vsix`.

### 7.2 Installation

For local sideload:

1. Build all three components.
2. Place `karnc-lsp` in a location on `PATH` (or configure `karn.executablePath` in VS Code).
3. Install the VS Code extension: `code --install-extension karn-vscode-<version>.vsix`.

A bundled installer (single script or installer package) is not in scope for this increment; manual install is acceptable for first cut.

### 7.3 Updates

Updates are manual: rebuild, reinstall. Auto-update via a marketplace is deferred.

---

## 8. What's deferred (future tooling increments)

After this increment, the language has working tooling for the v0.5 surface. Future tooling work:

**Tooling v2 (probably after v0.6):**
- Autocomplete (the substantial missing feature).
- Semantic tokens (semantic highlighting).
- Editor commands ("Karn: Build", "Karn: Run tests").

Document symbols are shipped in v1.1 (§3.7) — they're cheap to implement
(AST walk) and unlock VS Code's outline view.

**Tooling v3:**
- Workspace symbol search.
- Refactorings (rename, extract).
- Inlay hints.
- Code actions / quick fixes.

**Tooling v4:**
- Marketplace publication.
- Auto-update infrastructure.
- Editor support beyond VS Code (Neovim, Helix, etc. — the tree-sitter grammar enables this; the LSP works in any LSP-capable editor; the question is which extensions to package and maintain).

These come as practice surfaces the need.
