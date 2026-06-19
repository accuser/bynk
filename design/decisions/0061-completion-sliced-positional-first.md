# 0061 — Completion is sliced; positional first, lexical contexts, static built-ins

- **Status:** Accepted (v0.30)
- **Spec:** `design/bynk-lsp-spec.md` §3.15

## Context
Completion was still the narrow v0.17 surface — three lexical contexts
(`consumes`/`given`) emitting only `Unit`/`Capability` items. Making it
comprehensive spans three machinery tiers of very different cost:

1. **positional** (type-position type names, keyword completion, snippets)
   — needs only the project parse + static name registries;
2. **`.`-member** (`x.method`/`x.field`, `Type.of`, `Cap.op`) — needs
   resolving *what is before the dot* and, for value receivers, the
   **typed model mid-edit**;
3. **locals/params in scope** — needs a **scope-at-offset** query that
   does not exist (the index's `in_scope` is span-range filtering, not
   lexical scope; it overlaps the deferred index kinds).

## Decision
**Slice the increment; ship tier (1) — positional completion — first**, and
fix two cross-cutting design points for all of completion:

- **Context detection is lexical; candidates are semantic.** Completion
  runs on an unparseable buffer, so *where am I?* is decided from the line
  prefix (as v0.17 already did), while *what fits here?* is drawn from
  parsing the *other* project files with recovery + the static registries.
  No reliance on the current file parsing.
- **Built-ins/surface come from static registries, not the index.**
  First-party symbols aren't indexed (ADR 0057's finding — synthetic defs
  aren't on disk), so the built-in types, keyword docs, and the
  `bynk`-surface transparent types are sourced from
  `bynkc::{keywords, builtin_names, firstparty}`; the index (here, the
  project parse) supplies only *project* symbols. One source of truth —
  reuse the registries, don't hardcode a parallel list that can drift.

Slice 1 contexts: **type position** (`: T`, `-> T`, `[ … ]` type args) →
built-ins + surface transparent types + project `type` decls; **keyword
position** (a bare word at a declaration/statement start) → the
lowercase-initial reserved keywords (with registry docs) + declaration
snippets; plus the unchanged v0.17 `consumes`/`given`. Detection is kept
**conservative** (a list-literal `[` is excluded; out-of-context prefixes
yield `[]`) — better to offer nothing than wrong items — with one accepted
false positive: a record *construction* value (`Order { id: `) is lexically
identical to a record field-type declaration.

**Tiers (2) + (3) are slice 2.** `.`-member is the daily-driver users will
most expect, so slice 2 should follow promptly — but it is the part that
needs receiver typing + a scope-at-offset query, so it is a separate
increment, not a bolt-on.

## Consequences
Type/keyword/snippet completion lands now, reusing the existing
lexical+project-parse approach, fully unit-tested at the pure-function
level (`complete()` over crafted line prefixes). The lexical/static-registry
decisions carry forward to slice 2. The accepted cost is the known
record-construction false positive and the absence of `.`-member/locals
until the receiver-typing + scope-at-offset machinery is built.
