# 0064 — Locals in scope: scope ranges from the checker, resolved per-file

- **Status:** Accepted (v0.31)
- **Spec:** `design/bynk-lsp-spec.md` §§3.4, 3.8, 3.12 (and §3.14/§3.15 as later slices land)
- **Relates to:** the recurring deferral — ADR 0053 (index), 0056 (hints), 0057 (tokens), 0061 (completion)

## Context
Local bindings (`let`/`let <-`, fn/handler/lambda params, match-arm patterns)
have been deferred at every A-tier LSP increment because **no scope-at-offset
query existed**: the index tracks only top-level symbols (`index.rs:13`), and
the checker's local scopes are a transient `Vec<HashMap<String, Ty>>` discarded
after use. So references/rename, semantic tokens, inlay-hint *navigation*, and
completion all stopped at top-level names.

## Decision
Record local bindings with their **lexical scope ranges**, homed per-file on
the analysis, and resolve everything else from there:

- **Scope ranges come from the checker, not the AST.** A `LocalsSink` (mirroring
  the v0.27 `HintSink`) records each binding at its checker binding site with the
  enclosing block/body span the checker already has — `let`: `[stmt end .. block
  end]`; params: the body span. **Nesting** therefore falls out of the checker's
  recursive block-checking (an inner block's binding gets the inner block's
  span), and **shadowing** is resolved in the query (latest in-scope def wins).
  This makes scope correctness the checker's already-tested scoping, not new
  logic — the proposal's central risk.

- **Home: per-file `FileLocals` on the analysis, not the cross-file index.**
  Locals are file-local and never cross-file, so the `ProjectIndex` (def + global
  refs) is the wrong shape. `ProjectAnalysis.locals` / `ProjectDiagnostics.locals`
  parallels `hints`/`expr_types`. Only synthetic files are muted (locals serve
  test files too, unlike inlay hints).

- **Use sites are recovered in the LSP, not recorded in the checker.** The
  foundation records *bindings* (def + scope + type); a binding's *uses* are
  found by lexing the snapshot and keeping the identifier tokens of the name
  within the binding's scope that resolve back to it (excluding shadowing inner
  uses and every binding's own def token). This keeps the checker change to the
  binding sites only — no use→def edge recording — and the resolution
  (`locals_nav`) is a pure function over the snapshot, like `index_queries`.

- **Consumers fall back to locals.** references / go-to-definition /
  document-highlight try the index first, then the locals resolver, so a local
  resolves scope-correctly (the legacy string-matching definition fallback
  can't tell scopes apart, so locals are tried before it).

## Consequences
The recurring deferral is lifted from one foundation: navigation lands now
(slice 2); semantic-token colouring (a new legend token — the frozen ADR-0057
legend is append-only, plus the vscode contract) and expression-position
completion (a new lexical context) follow as their own slices, both reading the
same `FileLocals`. **Match-arm pattern bindings and `is`-narrowing bindings**
(subtler scopes) are deferred — they need more binding-site instrumentation, not
new machinery. Recording use→def edges in the checker (instead of the LSP lexer
scan) remains an option if a future consumer needs cross-file local edges, which
none do today.
