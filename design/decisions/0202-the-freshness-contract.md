# 0202 — The freshness contract: an index-backed request answers against the buffer the client holds

- **Status:** Accepted (v0.179)
- **Provenance:** proposed in #665, a slice of the LSP foundations work (spine
  #640). The fork it settles — refresh vs. decline — was closed as Q3 (#663)
  before the slice was cut; this record fixes the mechanism.
- **Relates:** [[0052]] (project diagnostics — the round this gates), [[0201]]
  (the project model the round analyses), [[0156]] (the editor surface is a
  projection of the language — the principle a stale answer betrays), [[0023]]
  (each increment stays single-purpose).

## Context

The server answered index-backed requests from the *previous* analysis round.
`index_position` resolved the client's current cursor position against that
round's snapshot with no version check, so an edit that shifted byte offsets —
a line inserted above the cursor — made hover, definition, references, and the
rest land on the wrong symbol. Silently: the wrong answer is occasional and
self-corrects on the next round, so it is never loud enough to report. That is
the worst kind, because [[0156]] makes the editor a *projection of what the
checker knows* — and a projection that is quietly wrong erodes the one thing it
exists to build, trust that the tool knows what the user knows.

The exposure was not confined to one path. Freshness was split three ways:
`index_position(fresh: bool)` (eight position handlers cached, `rename` alone
fresh), `ensure_analysis` (cold-start only), and eight handlers reading
`state.analysis` raw. Re-auditing that surface against `main` for this slice
turned up an **eighth** stale-position reader the track doc's own table had
missed — `locals_completions`, reached through `completion`, resolving the
cursor against the cached snapshot exactly as `index_position` did. A per-handler
gate is how a count drifts; that miss is the argument for one gate, not nine.

Q3 measured the affordability question the decision turned on. A full round —
Bynk has no incremental layer, so refresh means whole-project — is single-digit
milliseconds for a realistic project and under ~100 ms at 200 files, an order
past the largest example in the tree. rust-analyzer and gopls both block a
request until the world is current and never serve stale; they afford it via
incrementality. Bynk affords it by being small. So the fork resolved to
**refresh**, uniformly — never a position against stale text.

## Decision

**(A) One gate. `Backend::analysis_for(uri)` is the single request-analysis
path.** It returns the analysis a request must answer from, *current for `uri`*:
cold start triggers a round; a round whose snapshot for `uri` predates the open
buffer triggers a refresh. The `fresh: bool` parameter, `fresh_analysis`, and
the eight raw `state.analysis` reads are gone; every index-backed handler —
including `locals_completions` — routes through it.

The client's request position refers to `uri`'s current document version;
messages are ordered, so `docs[uri].version` reflects every `didChange` sent
before the request. The gate returns an analysis only when it has analysed
*that* version of *that* file — so `position_to_offset` against its snapshot is
never resolved against text the user edited past.

*The subtlety a test caught, recorded because it is the load-bearing detail:*
`versions` is built from open **docs**, not analysed **files**. A file open but
outside every `include` root has a version entry and no snapshot. Gating on
version-match alone would hand such a file a round that never analysed it. The
gate requires the file to be an actual snapshot key **and** at the wanted
version. Two behaviour-driven tests exist because a static test cannot see this;
one of them found this bug.

**(B) Refresh, not decline, is the answer to staleness. Decline is reserved for
the unanswerable.** The gate returns `None` — the request answers empty — only
when it cannot reach the client's version: single-file mode (no project), a file
outside every `include` root, or a concurrent edit that raced the refresh (rare;
the next request is current). It never returns a snapshot older than the buffer.
Declining briefly is the honest failure; a confident wrong answer is not.

`completion` and `document_symbol` stay outside the contract — they resolve
against **live** buffer text, so there is no snapshot to be stale. `type_receiver`
(completion's member typing and signature help) keeps its own freshness path: it
analyses a *rewritten* overlay, not the live buffer, so it cannot gate on
`docs[uri].version` — folding it in was scoped as optional and declined for that
reason, verified by reading it rather than assumed.

**(C) The refresh coalesces; it does not race the debounce or itself.** A
request-driven refresh bumps the analysis generation, so the pending debounced
round (which checks the generation before running) bails rather than running the
same whole-project analysis 200 ms later. Concurrent requests after one edit
serialise behind a lock: the first runs the round, the rest wait and find it
already current. Five concurrent requests after one edit run **one** round, not
five — asserted, because the redundant-round bug is silent.

**(D) Diagnostics publish with the version they were computed at.** The
project-diagnostics publish carried `version: None`; it now carries the document
version the round analysed each file at (a disk-only file carries none), so a
client drops a range whose buffer has moved past it — the publish-side half of
the same contract.

## Consequences

- An index-backed request during or just after typing answers against the
  current buffer, or answers nothing — never against stale text. Every one of
  the ~ten position handlers and the seven-plus-one direct readers is covered by
  the single gate.
- **The tests are behaviour-over-time, not static shape** — the point of the
  track (§4.1). They drive a real `Backend` (`LspService::new(Backend::new)`)
  through `didChange` → request and assert the request resolves against the new
  text; they are mutation-checked (reverting the refresh reproduces the original
  wrong-symbol resolution). A static test could not have caught the defect, the
  `versions`-vs-`snapshots` bug, or the coalescing.
- **No incrementality.** Refresh is a full round, so its cost scales with project
  size where rust-analyzer's stays flat. Fine at Bynk's scale (measured); the
  named cliff that would revisit this is a many-hundred-file project.
- The behavioural change lands on all four ADR 0156 surfaces — hover, completion
  (its locals sub-path), semantic tokens, signature help (via `type_receiver`,
  already correct) — as *"refresh to the current buffer, then answer"* rather
  than *"answer from the old snapshot."* No author-facing surface, no grammar,
  no runtime change.
