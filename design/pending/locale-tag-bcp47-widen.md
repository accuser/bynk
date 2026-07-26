---
level: minor
changelog: `LocaleTag` widens past `language[-Script][-REGION]` to admit BCP-47 variants, extensions, and private-use subtags (`messages "ca-valencia"`, `messages "en-US-u-ca-buddhist"`, `messages "x-custom"`), while still rejecting grandfathered/irregular tags
---

## ADR: locale-tag-bcp47-widen
title: `LocaleTag` widens to the productive BCP-47 grammar — variants, extensions, extlang, and private-use
summary: The refinement's `Matches` pattern grows past language[-Script][-REGION] to admit variants/extensions/extlang/private-use, bounded to satisfy the ReDoS guard, while grandfathered/irregular tags stay rejected

**Context.** [#909](https://github.com/accuser/bynk/issues/909) grounded a
narrower gap ADR 0279 named out of scope: `LocaleTag`'s refinement —
`[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?` — admits only
`language[-Script][-REGION]`, so well-formed BCP-47 tags carrying **variants**
(`de-CH-1996`, `ca-valencia`, `sl-rozaj`), **extensions**
(`en-US-u-ca-buddhist`), **private-use** (`de-CH-x-phonebk`, `x-custom`), or
**extlang** (`zh-yue`) could never be declared in a `messages` block, even
though `negotiateLocale`'s RFC 4647 subtag truncation (ADR 0277) is already
agnostic to how many subtags a declared tag carries.

**Decision.**

- **[Decision A] Widen the regex for well-formedness; do not adopt registry
  validation.** A full BCP-47 validator against the IANA subtag registry is a
  data-carrying commitment out of proportion to a refined-type pattern, and
  against ADR 0276's "no CLDR data in the compiler" posture. The widened
  pattern follows the *shape* of `langtag = language ["-" script] ["-"
  region] *("-" variant) *("-" extension) ["-" privateuse]`, plus a
  standalone `privateuse` (`x-…`) — well-formedness only, and not a literal
  transcription of the RFC 5646 production: extlang position isn't
  restricted to a 2-letter primary the way the grammar restricts it, and (see
  below) each unbounded repetition carries a bounded cap instead of an open
  one. A syntactically valid but unregistered shape (`en-abc`, a made-up
  extlang) is accepted, the same way `LocaleTag` today accepts `xx-Yyyy`
  without checking `xx` or `Yyyy` are real IANA subtags.
- **[Decision B] Admit variants, extensions, extlang, and private-use;
  exclude grandfathered/irregular tags, by name.** `i-klingon`, `en-GB-oed`,
  and the other registered irregulars don't fit the productive grammar and
  are deprecated in the registry; the pattern rejects them by construction
  rather than special-casing them, and a fixture pins each explicitly.
- **[Decision C] Keep canonical casing per subtag type.** language/extlang
  lowercase, script Titlecase, region uppercase (or 3 digits),
  variant/extension-singleton/extension-subtag/private-use lowercase —
  consistent with #899's canonical-casing stance, so a locale still has one
  spelling.
- **The pattern caps each unbounded BCP-47 repetition (`*variant`,
  `*extension`, extension subtags) at a generous bounded count instead of an
  unquantified `+`/`{m,}`.** `bynk-check`'s `has_nested_unbounded_quantifier`
  guard (#724) rejects a pattern where one unbounded quantifier's content
  itself carries another — the shape that makes JS `RegExp` backtrack
  exponentially. An extension's `*("-" subtag)` written as `(?:-subtag)+`
  nested under the outer `*("-" extension)` trips exactly that guard; written
  as `(?:-subtag){1,8}` it does not, and no realistic tag needs more than a
  handful of subtags per extension. This is a bound on repetition count, not
  on subtag content, so every example named above still matches.
- The pattern is unchanged in its use: still `String where Matches(pattern)`,
  read once from `bynk.locale.types.bynk` by `bynk-check`'s
  `locale_tag_pattern`/`locale_tag_accepts` and lowered to the same
  `new RegExp(...)` by every emitter call site — the single-source-of-truth
  wiring ADR 0279 built is unchanged, only the pattern string itself moves.

**Consequences.** `messages "ca-valencia"`, `messages "de-CH-1996"`,
`messages "en-US-u-ca-buddhist"`, and `messages "x-custom"` are now
declarable and flow through to `messagesLocales` for negotiation to match;
`messages "i-klingon"` remains `bynk.messages.invalid_locale_tag`. Every
project-form fixture that pulls in `bynk.locale.types` carries the pattern
string in its emitted `LocaleTag.of` guard, so this change reblesses ~30
fixtures' `expected/bynk/locale/types.ts` snapshots with no behavioural
change to those fixtures. Checking a well-formed-but-unregistered tag like
`en-abc` still succeeds — registry validation remains a named, deferred gap,
as it was before this increment. Negotiation itself is unchanged: RFC 4647
basic filtering still truncates a variant/extension/private-use tail away
like any other subtag, so a caller shouldn't expect negotiation to weight an
extension-bearing tag differently.
