---
level: minor
changelog: A second, non-reference `messages` locale now actually renders — completeness and cross-locale placeholder-agreement checking, plus the bundle's declared-locale set exported for Locale's own negotiation
---

## ADR: messages-checked-catalogue-slice-2
title: The checked-catalogue model, slice 2 — multi-locale bundles, completeness, and placeholder agreement
summary: Reference-bundle coverage, cross-locale placeholder-set agreement, and the multi-locale render-dispatch/export surface

**Context.** [Message bundles](../tracks/message-bundles.md) (spine
[#857](https://github.com/accuser/bynk/issues/857)), slice 2
([#874](https://github.com/accuser/bynk/issues/874)): slice 1
([#859](https://github.com/accuser/bynk/issues/859), [ADR 0272](0272-messages-construct-slice-1.md))
shipped the `messages` construct with exactly one consulted locale (the
`@reference` block) — a second, non-reference block parsed and was
checker-validated for its own internal correctness, but contributed nothing
to the generated `render`, and `tag` was accepted by `render(tag, msg)` but
never read.

**Decision — completeness (`bynk.messages.incomplete`) is one diagnostic per
missing `(locale, code)` witness.** `check_messages_bundles`
(`bynk-emit/src/project/validate.rs`) diffs every non-reference locale
against the `@reference` block's own codes, once cardinality confirms
exactly one reference exists (0 or 2+ already report their own diagnostic;
"the reference" isn't well-defined otherwise). Mirrors
`bynk.types.non_exhaustive_match`'s own one-witness-per-diagnostic
convention (ADR 0169) rather than one aggregated diagnostic — a reviewer
fixing several missing codes gets several separate, addressable
diagnostics, each anchored at the locale block that needs the addition.

**Decision — placeholder agreement (`bynk.messages.placeholder_mismatch`)
compares placeholder-name *sets*, not order.** Only for codes present in
*both* the reference and another locale (a missing code is `incomplete`'s
job). Sets, not sequences: a translation may legitimately reorder
placeholders for the target language's grammar (`"{age} ans, bonjour
{name}"` agrees with `"Hello, {name}, you are {age}"` — same two
placeholders, reordered) — comparing positional order would incorrectly
reject every idiomatically-reordered translation. Reuses the existing
`split_template` (slice 1) via a new narrow `placeholder_names(template) ->
BTreeSet<&str>` helper, rather than exposing `TemplateSegment` itself to
the checker.

**Decision — `render`'s dispatch is two-level, built by emitting every
declared locale together, once.** `emit_messages` (per-block, guarded on
`@reference`) becomes `emit_messages_bundle(blocks)`: one `code ->
renderer` table per locale, one shared `messagesByLocale` dispatch object,
and one `render(tag, msg)` trying the resolved `tag`'s own table, else the
reference's, else falling to `bynk.locale.render`'s existing floor — with
explicit `undefined` checks at each rung (this codebase's plain `Record`
indexing is not `noUncheckedIndexedAccess`-safe, matching how slice 1's own
single-table lookup was already written). The emitter's per-item dispatch
loop gathers every `CommonsItem::Messages` in the commons up front and
calls `emit_messages_bundle` once, rather than once per block.

**Decision — the bundle exports its declared-locale set, un-prefixed.**
`export const messagesLocales: readonly LocaleTag[]` and `export const
messagesReferenceLocale: LocaleTag` — deliberately without the
double-underscore prefix this file's other internals use (`__messages_en`,
`__bynkLocaleRender`), since these two are the concrete form of "the
bundle's declared locales" the track doc names as the precondition Locale's
own slice 2 (negotiation) needs before it can start (design/tracks/
message-bundles.md §9) — they are meant to be imported by that future work,
not treated as private. Wiring them into Locale's own negotiation provider
is that track's proposal to cut, not this one's.

**Decision — two blocks declaring the same locale tag are rejected
(`bynk.resolve.duplicate_message_locale`), not last-write-wins (PR #875
review).** The emitter has no dedup of its own — every block in
`emit_messages_bundle`'s `blocks` slice gets its own `const
__messages_<tag>` declaration unconditionally — so a silent "later block
wins" at the checker level would let a duplicate tag through to a hard
`tsc` redeclare error instead. Mirrors `bynk.resolve.duplicate_fn`'s own
shape: only the first occurrence of a tag is recorded, so every later
occurrence reports against the original.

**Consequences.** Construction-site checking (a
`message(code).withText(...)` chain vs. a code's declared parameter names)
remains the same named, deliberately deferred gap slice 1 named (§7 M1 of
the track doc) — completeness/placeholder-agreement are both bundle-internal
checks, neither needs or provides construction-site checking. Slice 3 (ICU
MessageFormat) is next; Locale's own slice 2 (negotiation) may now be cut as
a separate proposal against that track.
