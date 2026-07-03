# 0157 — Editor scaffolds cannot drift: each catalogue compiles in CI, independently

- **Status:** Accepted (doc-ADR; 2026-07-03)
- **Spec:** `design/bynk-lsp-spec.md` §3.15 (Completion — snippet insert-text)
- **Realises:** the editor-currency track (retired), ADR 2 of 2 — front-loaded ahead of every slice.

## Context

The tooling ships two scaffold catalogues: the LSP's `SNIPPETS` const
(`bynk-lsp/src/completion.rs`) and VS Code's static
`vscode-bynk/snippets/bynk.json`. They are largely disjoint — sharing only
`context`/`capability`/`service` — and serve different consumers with
different insert-text dialects (`bynk.json` is ahead on current constructs,
carrying `provides`/`agent`/`type record`/`type enum`/`fn`; `SNIPPETS` carries
`adapter`/`on call`). Neither is checked against the grammar: `SNIPPETS` still
ships `("test", "test \"${1:description}\" { … }")` for a keyword the grammar
retired, and nothing failed when it stopped compiling. This is the same
posture gap the docs track closed for the Book with its snippet-verification
harness and the existing `keywords_reference` drift test — scaffolds are
enumerable and checkable, and nothing was checking them.

## Decision

**Each scaffold set is independently lexed and parsed against the current
grammar in CI**, with `${N:default}` / `${N|a,b,c|}` / bare `$N`/`$0` tab
stops substituted to a compilable skeleton first (default text, first choice,
and empty, respectively). A scaffold that no longer parses fails the build.

This is a **per-set compiles assertion, not a set-equality/parity check across
the two catalogues.** The two consumers keep distinct catalogues and
insert-text dialects deliberately — forcing symmetry between `SNIPPETS` and
`bynk.json` would churn both for no user benefit, since they serve different
completion surfaces (server-side snippet items vs. static editor snippets).
Filling each set's own gaps against the current grammar is a later slice's
job, not this ADR's.

## Consequences

- This is the test that would have caught the retired `test` snippet; the
  track's first proposal (slice 0) deletes that snippet as its first casualty
  and adds this test to prove the remaining scaffolds parse.
- Revisit the no-parity stance only if the two catalogues' independent
  divergence starts causing a real coverage gap a user notices (e.g. VS Code
  offering a construct the LSP's own completion never mentions) — not merely
  because the two lists differ in membership.
