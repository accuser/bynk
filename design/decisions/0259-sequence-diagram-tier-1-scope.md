# 0259 — Sequence diagram Tier 1 — on-demand/static, not live-synced

- **Status:** Accepted (v0.223)

**Context.** Two increment shapes were considered: a command/CodeLens that
builds a static diagram from the committed round (Tier 1), and a panel that
follows the active handler with cursor↔message highlighting (Tier 2).

**Decision.** Ship Tier 1 only. `bynk/sequenceModel` is served from the
`committed_analysis` gate (the #733 stale-while-revalidate mechanism the
same 5 pull-based decoration handlers already use) and is re-issued fresh
by the client every time the command/lens fires — there is **no** refresh-
push mechanism for it. This corrects the issue's own phrasing ("re-pull via
`workspace/*/refresh` nudge"): no generic "refresh a custom method" exists
in the LSP spec or in `tower_lsp::Client` (the #733 nudge only wraps three
typed built-in methods — `semanticTokensRefresh`/`inlayHintRefresh`/
`codeLensRefresh`), and Tier 1's on-demand model does not need one.

**Consequences.** The diagram does not update while the user keeps editing
with the panel open; re-invoking the command/lens gets a fresh render.
Tier 2 (live sync + cursor↔message highlighting) is deferred, tracked as a
follow-up, not scoped here.
