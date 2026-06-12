# 0054 — Quick-fixes are structured suggestions authored at the diagnosis site

- **Status:** Accepted (v0.26)
- **Spec:** `design/karn-lsp-spec.md` §3.10

## Context
Karn's diagnostics are prescriptive — "remove the capability from the
`given` clause", "add `B.Cap` to the handler's `given` clause" — but the
prescription lived in prose `notes`: human-readable, not
machine-applicable. With A-0 complete (project-wide diagnostics reach
context files; the analysis round is retained), the LSP could turn those
prescriptions into one-click fixes — *if* it had fix data. Two ways to
get it: re-derive fixes in the LSP, keyed on the diagnostic category
(re-parse, pattern-match, compute spans), or have the checker author
them where the diagnostic is raised.

The headline fixes are **`given`-clause list edits**, the classic
comma/whitespace trap — and their edits land **away from the
diagnostic's span** (the unused-capability diagnostic anchors on the
return type, the undeclared-capability one on the usage site in the
body; both edits land in the clause). Two further constraints surfaced
during verification: the diagnosis sites had only capability *names*
(`Vec<String>`), not their clause spans, and the retained `Analysis`
kept only diagnostic *categories* — the full `CompileError`s were
published and dropped.

## Decision
Quick-fixes are **structured `Suggestion`s on `CompileError`** —
`message`, span→replacement `edits` (empty replacement deletes; empty
span inserts), and a rustc-style `Applicability` — attached via
`.with_suggestion(…)` (mirroring `.with_note`) **at the diagnosis
site**, the only place the exact spans and replacement are known.
LSP-side re-derivation is rejected: co-location keeps the fix correct
as its diagnostic evolves, unit-testable in `karnc` with no LSP, and
consumable by a future CLI `--fix`.

- **List-aware `given` edits, authored in the checker.** The checker now
  receives the clause's `CapRef`s (spans included) and an anchor for the
  absent-clause case: removal spans the entry plus one adjacent comma
  and surrounding space; removing the *only* entry deletes the `given`
  keyword too (anchored on the return type — sound because unused-cap
  only reports at handler sites); adding inserts `, Cap` after the last
  entry or synthesises ` given Cap` after the handler's return type. No
  anchor exists for provider op bodies (their clause lives on the
  `provides` line), so an absent-clause insertion is not offered there.
  Cross-context entries insert the **canonical context path** (the
  diagnosis site sees the resolved name, not an `as` alias spelling) —
  valid alongside alias-style calls.
- **The LSP keys on the diagnostic's span, not the edit's.** Clients
  request actions at the cursor/squiggle; for both seed fixes the edit
  is elsewhere. A suggestion is offered when the requested range
  intersects its **owning diagnostic's** span.
- **Served from the cached analysis round.** `Analysis` retains the
  round's per-file diagnostics (the categories-only field collapsed
  into a derivation); `codeAction` never runs a fresh analysis — slow,
  and it can disagree with the squiggles the client shows. No cached
  round (pre-first-analysis, non-project file) → the empty list. Edits
  are **versioned** `TextDocumentEdit`s against the analysed snapshot's
  captured versions, as rename's are.
- **`Applicability` gates application.** Only `MachineApplicable`
  suggestions surface as quick-fixes; `HasPlaceholders` is reserved for
  fixes a human must complete (unused in v0.26) and gates the future
  `--fix`.
- **Deferred: the InRange bound-swap.** `PredKind::InRange(i64, i64)`
  carries no bound spans or lexemes, so "swap the two arguments" is not
  computable at the diagnosis site without a predicate-AST extension —
  it returns as a fast-follow once bounds carry spans.

Test strategy: fix correctness is pinned in `karnc` — each seed
diagnostic carries its expected suggestion, the list-aware fixtures
assert exact text across the first/middle/last/only position matrix,
and **every applied fix re-diagnoses clean**. The LSP side is the pure
`code_actions` computation, exercised end-to-end over a real temp
project (keying, versioned edit, applied round-trip); the capability
advertisement is a unit check over the extracted `server_capabilities()`.

## Consequences
Karn's prescriptive `given` diagnostics are one-click fixes — the
differentiator the A-0 sequencing built toward. The catalogue grows
per-diagnostic with its correctness pinned beside the diagnostic itself;
the same suggestions back a future CLI `--fix` with no LSP involvement.
The checker's handler entry points now carry the clause spans, which is
the plumbing later same-area fixes need.
