# 0240 — LSP `codeAction` gains extract-variable; extract-function deferred to a track

- **Status:** Accepted (v0.214)

**Context.** Issue #303 asked for two `codeAction` refactors — extract-variable
and extract-function — to fill the gap left by `code_action_provider`
advertising only `CodeActionKind::QUICKFIX` (rename is the only existing
structural refactor). Investigating extract-function surfaced a blocker:
`given Cap` capability clauses exist only on `Handler` and `Provider` in the
AST — `FnDecl` carries no such field, so lifting a capability-using body into
a plain `fn` isn't expressible without a language change. That question is
multi-increment and its surface isn't settled (does `fn` gain `given`? does
extraction stay capability-free-only? does it target a different shape
entirely?) — the ADR 0076 trigger for a feature track, not a single
increment. It is split out to track issue #800.

Extract-variable has no such blocker: it never crosses a capability boundary,
so it ships here as an ordinary increment.

**Decision.** Two implementation choices, both scoped to keep the increment
small and consistent with existing `codeAction` precedent:

1. **Freshness gate: `committed_analysis`, not a whole-buffer refresh.**
   Extraction is single-file and selection-driven, exactly like the existing
   quick-fix path (`code_actions.rs`) — it never needs rename's
   `analysis_covering_open_buffers` gate, which exists only because rename
   emits versioned edits across multiple files. Reusing the non-refreshing
   decoration gate (ADR 0235) keeps `codeAction` from forcing a whole-project
   re-analysis on every selection change.
2. **Placeholder naming: a whole-file word-boundary text scan, not
   scope-aware binding analysis.** `extracted`, `extracted2`, … — the first
   candidate that doesn't appear as a whole word anywhere in the file. This
   is a stronger-than-necessary collision guarantee (file-wide rather than
   scope-local) chosen for simplicity; the issue's own caveat expects the
   client's rename-on-extract to supply the real name immediately after.

The extraction algorithm itself: parse the live buffer (no cached AST is
retained in `Analysis`, matching `structure.rs`'s folding/selection-range
posture), find the smallest AST expression node whose span fully contains the
selection, and insert `let <name> = <selected>` immediately above the
enclosing statement or block tail — descending through `if`/`match`/block-
expression boundaries resets the insertion point to that nested block's own
statement/tail, so extracting from inside an `if` branch inserts there, not
above the whole `if`. The AST-child walk reuses `bynk_syntax::ast::
expr_children`/`statement_exprs` (the exhaustive-by-construction iterators
introduced to stop hand-rolled partial `ExprKind` walks from silently
skipping variants) rather than a second hand-rolled match, for exactly the
kinds of block-only special cases (`Block`/`If`/`Match`) that need their own
insertion-point tracking.

**Consequences.** `textDocument/codeAction` gains `CodeActionKind::
RefactorExtract` (and the parent `Refactor`) alongside `QuickFix`. No
grammar, checker, or emitter change — the extraction never alters what a
program means, only rearranges its text. Extract-function stays open,
tracked at #800, gated on settling the capability-propagation surface.
