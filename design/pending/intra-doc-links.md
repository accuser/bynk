---
level: minor
changelog: Doc comments resolve `[Name]`/`[Owner.member]` links against the project's binding index, navigable via document links and hover
---

## ADR: intra-doc-links
title: Intra-doc links resolve by naming convention, not a `kind@` prefix
summary: Bare/dotted bracket names in doc comments become navigable links, resolved by scope order with no disambiguation syntax

**Context.** Bynk doc comments (`--- … ---`) are Markdown, but had no way to
reference another declaration — `[Limiter]`, `[RateView.remaining]` rendered
as literal brackets, never linked, in both `textDocument/documentLink` and
`textDocument/hover`. Rustdoc's answer is intra-doc links resolved against the
whole crate's namespace, with an optional `kind@name` prefix for
disambiguation. Bynk's binding index (`bynk_check::index::ProjectIndex`) is
per-unit-scoped, not global, and Bynk's own naming conventions (PascalCase for
types/agents/capabilities/services/providers/actors, lowercase for
fns/commons members — collisions across kinds *sharing a name* are already
rare by convention) make a same-name-different-kind collision an edge case
rather than the common case a `kind@` syntax exists to solve.

**Decision.** A bare `[Name]` or dotted `[Owner.member]` in a doc comment
resolves against the *declaring unit's* doc-link scope order — the unit
itself, then its `uses` targets, then its `consumes` targets, in that order,
mirroring the bare-name qualification the checker itself already uses
(`IndexBuilder::qualify_with`). The **first** scope-order unit with any match
decides the answer; more than one candidate there is treated as unresolved,
never guessed — no `kind@` prefix syntax in this increment. An unresolved name
renders as plain text; there is no new diagnostic. `[text](url)` and
author-defined `[name]: url` references keep their ordinary Markdown meaning
and are never captured, nor is fenced code-block content ever scanned.
First-party/synthetic-unit symbols (the embedded `bynk` capability surface)
are out of scope — they carry no on-disk location to link to, and the
existing index already excludes their defs at assembly, so they resolve to
"unresolved" for free rather than needing a special case.

Hover's doc-link rendering is scoped to its binding-index-backed rung only
(the resolved-symbol-reference rung, tried first) — the one rung with a
resolved owning unit on hand. The same-file-lexical, cross-file, and
first-party lexical hover rungs keep plain doc rendering in this increment,
since none of them carries a resolved owning unit to search from.

**Consequences.** A same-name, *different*-kind collision within one unit
(e.g. a bare name matching both a `Fn` and a `Capability` in the same unit)
makes that name unresolved rather than guessing one — a `kind@`-prefixed
disambiguation syntax remains a natural follow-up if that proves too coarse in
practice. Link targets are file-only (no `#line,col` fragment), matching the
existing `uses`/`consumes` document-link precedent exactly; a
position-anchored link is a possible future enhancement, not introduced here.
`document_link`'s doc-link resolution runs under the same `committed_analysis`
(stale-while-revalidate) gate its existing `uses`/`consumes` links use —
consistent with `code_lens`, which already resolves against the full project
index under that same gate.
