---
level: minor
changelog: LSP decoration requests (semantic tokens, inlay hints, code lenses, document links, code actions) serve the last committed round instead of forcing a whole-project re-analysis on every keystroke, revalidating via workspace/*/refresh
---

## ADR: lsp-decoration-stale-while-revalidate
title: Pull-based decorations serve the committed round and revalidate, not force a refresh
summary: Why the freshness contract's strict refresh is kept for cursor requests but relaxed for the decorations the editor auto-fires on every edit

**Context.** The freshness contract (#665, ADR 0202) routes every index-backed
request through one strict gate, `analysis_for`: a request for a file whose open
buffer has moved past the last committed round forces a synchronous whole-project
round (discover + parse + check + a diagnostics publish) *on the request path*
before answering. That guarantee — a position is never resolved against text the
user edited past — is exactly right for requests that resolve the client's live
cursor.

But editors auto-fire five *pull-based decoration* requests after **every**
`didChange`: `semanticTokens`, `inlayHint`, `codeLens`, `documentLink`, and
`codeAction`. While the user types, the debounced round (default 300 ms) always
lags the buffer, so each of these requests missed the version match and forced a
full re-analysis — defeating the debounce and making per-keystroke latency grow
with project size (#733). #776/#777 removed the redundant re-parse and added a
line index, but the request-path re-analysis itself remained.

The decisive observation: none of these five handlers resolve anything against
the client's *live* cursor. Every range and span they emit converts against the
round's own snapshot (`Analysis.snapshots`); `documentLink` computes ranges
against live text and reads only the project-level `unit_sources` map. So a
committed round lagging the buffer by at most one debounce cycle is internally
consistent — the strict version match is stronger than these requests need.

**Decision.** Add a second, non-refreshing gate, `committed_analysis`, that
returns the last committed round for the file's project as-is (requiring only
that the file is a snapshot key), never triggering a refresh. Route the five
decoration handlers through it; keep `analysis_for` / `index_position` strict for
every cursor request (hover, completion, goto/type definition, references,
signature help, document highlight, call hierarchy, implementation, prepare
rename) and `analysis_covering_open_buffers` for `rename`. This is
stale-while-revalidate: the already-scheduled debounce round is the revalidation,
and on its commit `run_project_diagnostics` nudges the client to re-pull each
decoration via `workspace/{semanticTokens,inlayHint,codeLens}/refresh` — sent
only for a decoration whose `refresh_support` the client advertised at
`initialize`. (`documentLink` has no `workspace/*/refresh` in the protocol; the
editor re-requests it on change, and its ranges already track live text.)

Also, off the request path: `prune_orphaned_projects` now resolves the open
buffers' project roots (a canonicalising filesystem walk per buffer) *before*
taking `state.write()`, holding the write lock only to mutate `projects`; and the
name-resolution project enumeration in `signature_help` and `completion_resolve`
moves onto the blocking pool (mirroring #776's treatment of `complete()`).

**Consequences.** Typing stays responsive on large projects — decoration requests
no longer run analysis. Decorations can lag the buffer by at most one debounce
cycle and then catch up on the refresh nudge; a client that advertises no
`refresh_support` re-pulls on its own next request instead. Cursor-position
correctness is unchanged: the strict contract still governs every request that
resolves the live cursor. A client that never re-pulls after a nudge would keep a
one-cycle-stale decoration until its next request — acceptable, and the standard
posture of production language servers. The remaining low-frequency, borrow-heavy
enumerations (`hover`'s cross-file fallback rung, goto-definition's symbol
fallback) stay on the async worker: they are parse-cached (#776) so their cost is
a file stat, and hoisting their borrowed analysis tables across a `spawn_blocking`
boundary would cost more than it saves.
