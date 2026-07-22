---
level: minor
changelog: "`messages` templates gain ICU `plural`/`select`/`number`/`date` placeholders, formatted via the host `Intl`"
---

## ADR: messages-icu-format-slice-3
title: The ICU MessageFormat template format, slice 3 — plural/select/number/date over MessageArg's typed args
summary: A bynk-emit-only mini-parser adds four ICU dispatch forms to a message template, checked and emitted with no bynk-syntax grammar change

**Context.** [Message bundles](../tracks/message-bundles.md) (spine #857),
slice 3 (#878, sub-issue of the spine): slice 1 (#859, ADR 0272) shipped the
`messages` construct and a single-locale bundle; slice 2 (#874, ADR 0273)
shipped multi-locale completeness and placeholder-*name*-set agreement. Both
left templates positional-only (`{name}`), deferring ICU MessageFormat per
`locale-capability.md` §4.5/L4 and this track's own §4.4 — settled rationale
("adopt ICU MessageFormat"), not re-derived here; this ADR settles *how*.
`MessageArg`'s four-variant shape (`Text`/`Whole`/`Num`/`Moment`, ADR 0256)
was fixed specifically so this slice could read each argument's value for
plural/select/number/date dispatch, needing no breaking change to `Message`.

**Decision — the ICU sub-grammar lives entirely inside a template's
`String`, parsed by a new, self-contained module (`bynk-emit/src/emitter/icu.rs`),
not a `bynk-syntax` grammar/lexer/AST change.** Consistent with slice 1's own
Decision D (`{name}` resolved by a Rust-side string scan, not new grammar):
the ICU mini-grammar (keyword dispatch, nested arms, `#`, `''`-quoting) is a
wholly different grammar from Bynk's own, and the existing `\(expr)`
string-interpolation lexer machinery is Bynk-expression-aware by
construction (it re-invokes Bynk's own tokenizer on a hole) — it does not
transfer to a foreign grammar. `MessageEntry.template`/`template_span` are
unchanged; `bynk-syntax` gains nothing.

**Decision — runtime formatting delegates entirely to the host `Intl`
object (`Intl.PluralRules`/`Intl.NumberFormat`/`Intl.DateTimeFormat`); no
CLDR data is bundled in the compiler or its runtime.** No precedent existed
either way in this codebase before this slice. Re-implementing CLDR plural
rules and locale-aware formatting in Rust-emitted TS would commit Bynk to
carrying and maintaining CLDR data itself, for something the JS runtimes it
already targets carry natively. All three target platforms document `Intl`
support (Node bundles full-ICU by default; browsers must per ECMA-402;
Cloudflare Workers' docs list `Intl` under supported runtime web-standard
APIs). Three shared runtime helpers (`selectPluralArm`, `formatIcuNumber`,
`formatIcuDate` — `bynk-emit/runtime/src/messages.ts`) wrap the `Intl`
constructors; emitted `messages`-bundle code imports them only when a
template actually uses `plural`/`select`/`number`/`date` (mirrors the
existing `Bytes` runtime-helper injection mechanism, generalised).
Consequently, no compile-time CLDR-category-reachability validation is
performed either: the checker validates only that `plural`/`select` arm
keywords are drawn from the fixed vocabulary and that `other` is present,
never whether a declared arm (e.g. `few`) is reachable for a given locale's
real plural rule — that's a runtime fact (`Intl.PluralRules`), and
validating it at compile time would mean bundling the exact CLDR data this
decision avoids.

**Decision — no `MessageEntry` position-map field; a malformed-syntax
diagnostic derives its span by byte-offset arithmetic against the existing
`template_span`.** `template_span` covers the raw quoted source token;
`template` is the decoded value (only `\n \t \" \\` are decoded, each
shrinking 2 raw bytes to 1). Rebasing is `template_span.start + 1` (skip the
opening quote) plus the parser's own byte offset into the decoded text —
exact unless an escape sequence occurs *earlier in the same template*, in
which case the derived span under-shoots by the number of such escapes. A
named, accepted approximation (real templates essentially never contain an
escape before an ICU placeholder), not a claim of general precision, and it
keeps this slice's `bynk-syntax`/AST footprint at zero.

**Decision — two new diagnostics.** `bynk.messages.format_mismatch`: the
same placeholder name in the same code must agree on ICU format *kind*
(plain/plural/select/number/date) across every declared locale — kept
separate from the existing `bynk.messages.placeholder_mismatch` (name-set
only) so that check's meaning doesn't silently broaden underneath slice 1/2
authors. `bynk.messages.malformed_icu_syntax`: one code covering every
parse failure (unbalanced arm braces, an unknown format keyword, `#`
outside a `plural` arm, a missing mandatory `other` arm, or an explicitly
out-of-scope construct) — matching `split_template`'s own established
philosophy that malformed input is one class of author mistake, not many
near-duplicate codes.

**Decision — the concrete surface is deliberately capped**, each excluded
construct diagnosed rather than silently mishandled: no `selectordinal`, no
`plural`'s `offset:`/`=N` exact-value arms, no CLDR skeleton strings beyond
the fixed style keywords (`number`: `integer`/`percent`; `date`:
`short`/`medium`/`long`/`full`), and no nesting a second `{arg, …}` dispatch
inside a sub-message (one dispatch level per placeholder). Construction-site
argument-type checking (does a `Message.withWhole(...)` call site pass the
`MessageArg` variant a code's ICU usage expects) also stays out of scope —
it would require threading a per-code "expected argument kind" map out to
every construction site across the program, a separate flow-sensitive
checker feature, not incidental to landing ICU formatting itself.

**Consequences.** A `messages` template can now express plural/gender/
number/date-aware text without an author hand-rolling locale-specific
branching. The compiler takes on no CLDR-data maintenance burden — that
stays with the JS host. The named exclusions (`selectordinal`, `offset:`,
skeletons, nesting, call-site argument checking) are real gaps, each with a
diagnosable "not supported" signal rather than silent misbehaviour, and are
candidate follow-ons if real demand appears; none block this slice's own
scope. `workerd`'s exact ICU data completeness relative to Node/browsers was
verified only by a manual smoke test at implementation time (Cloudflare's
own docs list `Intl` as supported but do not document data completeness in
detail) — a residual, named risk, not a blocking unknown.
