# 0268 — Documentation view Tier 1 — on-demand, not live-synced

- **Status:** Accepted (v0.225)

**Context.** As with #846, two shapes were possible: a command that builds a
static page from the committed round (Tier 1), and a panel that follows the
active file and highlights on cursor (Tier 2).

**Decision.** Ship **Tier 1**. `bynk/documentationModel` is served from the
`committed_analysis` gate (the #733 stale-while-revalidate mechanism the
pull-based decoration handlers use) and is re-issued by the client each time the
"Bynk: Show Documentation" command fires. There is **no** refresh-push — no
generic "refresh a custom method" exists in the LSP spec or `tower_lsp::Client`,
and Tier 1 does not need one (same reasoning as `bynk/sequenceModel`).

**Consequences.** The page does not update while the user keeps editing with it
open; re-invoking the command gets a fresh render. Tier 2 (live follow +
cursor↔declaration highlighting) and context-aggregation are the deferred
follow-ups.
