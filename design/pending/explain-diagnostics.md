---
level: minor
changelog: Diagnostic codes are teachable — curated codes carry a `codeDescription` link to their Book explanation in the editor, and `bynk explain <code>` prints the offline-complete blurb, an example, and the link
---

## ADR: explain-diagnostics
title: Diagnostic explanations are a compiler-owned mapping to Book anchors, surfaced by `bynk explain` and `codeDescription`
summary: A `code → { blurb, example, page/anchor }` table in the compiler links each curated diagnostic code to its Book concept page and prints an offline-complete explanation

**Context.** Every `bynk.*` diagnostic carries a stable machine code
(`bynk.resolve.unknown_type`, …), but the code was inert: the LSP built every
diagnostic with `code_description: None`, so an editor rendered the code as
dead text with nowhere to go, and there was no CLI analogue of Rust's
`rustc --explain`. A newcomer who hit a rule saw its name but had no path from
"you hit this" to "here is why this rule exists" — even though the Book already
teaches exactly that conceptual material, and the compiler alone owns the
code↔concept mapping no third-party tool can reconstruct.

**Decision.**

**(A) Source of truth — a compiler-owned mapping to Book anchors plus a short
inline blurb.** A `code → { blurb, example, page, anchor }` table
(`bynk_syntax::diagnostics::EXPLANATIONS`) lives next to the existing
diagnostic `REGISTRY`, the code owner. Each entry carries a longer-form blurb
and a minimal before/after example (the offline answer) plus a site-root-
relative Book `page`/`anchor` (the link target); it does **not** duplicate the
Book's prose. Both consumers read this one table — the CLI prints the
blurb/example/href, the LSP hangs the composed hosted URL off the code as a
`codeDescription` — so the two surfaces cannot drift, and the Book stays the
single home for the full explanation.

**(B) Coverage is incremental with a designed graceful fallback.** The feature
is not gated on explaining every code. The initial set curates the highest-
traffic, newcomer-facing codes (unknown type/name, missing/unknown record
field, undeclared/unknown capability); every other code simply has no entry.
An uncurated code carries **no** `codeDescription` (no link, never a broken
one) and `bynk explain` prints its one-line registry summary with a pointer to
the diagnostic index — both are documented, first-class states, not "unmapped =
broken". This lets coverage grow one PR at a time.

**(C) Link target is the hosted Book; the CLI blurb is the offline answer.**
`codeDescription.href` points at the stable hosted Book URL
(`https://bynk-lang.org` + the page + anchor), because an editor link wants a
canonical destination. The CLI's inline blurb and example are offline-complete,
so a developer without connectivity still gets the substance of the explanation
— `bynk explain` shells nothing and reads no network.

The `code → page` mapping is guarded against Book-page moves and anchor renames
two ways: the generated diagnostics reference page links every curated code at
its in-site page/anchor, so `astro build`'s link checker fails if a target
moves; and `tests/diagnostics_registry.rs` asserts each explanation's page file
exists on disk, its anchor matches a real heading slug, and its code is a real
`REGISTRY` code. `bynk explain` is a subcommand of the `bynk` driver only;
`codeDescription` flows through the standard LSP diagnostic, so the VS Code
client renders the link with no extension change.

**Consequences.** The tooling delta is confined: **hover changed** — a
diagnostic's code now renders as a link to its explanation via
`codeDescription`; completion, semantic tokens, and signature help are
unchanged. No grammar, AST, checker-semantics, emitter, or runtime change — what
is diagnosed is untouched; only how a code is explained is new. A curated
explanation is a small, reviewed prose commitment per code, so the table grows
deliberately; a `bynk check` "run `bynk explain <code>`" hint and richer,
per-code offline pages remain natural follow-ups, not part of this increment.
