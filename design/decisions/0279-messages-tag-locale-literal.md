# 0279 — A `messages` block's tag is a checked `LocaleTag` literal

- **Status:** Accepted (v0.233)

**Context.** The `messages` construct (slice 1, ADR 0272) shipped its locale
tag as a bare identifier: `messages en @reference { … }`. Grounding [#899](https://github.com/accuser/bynk/issues/899)
surfaced two defects rooted in that one decision.

- **A region- or script-bearing tag could not be declared at all.** `messages
  pt-BR { … }` is a hard parse error (a bare identifier can't contain `-`), so a
  bundle could never declare `pt-BR`, `en-GB`, or `zh-Hans-CN` — the exact
  shapes `LocaleTag`'s own refinement admits, and the shapes
  `negotiateLocale`'s RFC 4647 subtag truncation (ADR 0277) exists to resolve.
  Because a bundle's emitted `messagesLocales` set could therefore only ever
  hold single-subtag tags, the exact-match-over-truncation branch of
  negotiation was structurally unreachable end to end.
- **The tag was never validated.** ADR 0272 stated "its `LocaleTag` refinement
  is a checker concern", but no checker checked it. `messages Klingon { … }`
  compiled clean and emitted `("Klingon" as string) as LocaleTag` — a double
  cast that also bypasses `tsc`. Combined with slice 3's ICU formatting, an
  invalid tag reached the runtime as `new Intl.PluralRules("x")`, which throws
  `RangeError` — the opposite of `render`'s documented totality.

**Decision.** The tag is a `LocaleTag` **string literal**, checked against
`LocaleTag`'s own refinement.

- **The tag is a string literal (`messages "pt-BR"`).** `parse_messages_decl`
  swaps `expect_ident` for `expect_str_lit` — the identical primitive an
  entry's `code`/`template` already use. `MessagesDecl.tag` becomes a
  `String` + `tag_span`, mirroring `MessageEntry.code`/`code_span`. Bynk's one
  interpolation form (`\(expr)`) is rejected here as it is for any
  `expect_str_lit` position.
- **The check reads `LocaleTag`'s own declared refinement, not a hand-copied
  pattern.** A new `bynk.messages.invalid_locale_tag`, raised from
  `check_messages_bundles` (`bynk-emit/src/project/validate.rs`) alongside the
  other `messages` checks, evaluates the tag against the `Matches` predicate on
  `bynk.locale.types`'s `LocaleTag` declaration — read once from the firstparty
  source via a small `bynk-check` helper (`locale_tag_accepts`/
  `locale_tag_pattern`), so the pattern has exactly one definition and the
  check agrees by construction with the `new RegExp(...)` the emitter lowers.
  Checking lives in `bynk-emit`, not `bynk-check`: every other `messages` check
  already does, including `bynk.resolve.duplicate_message_code` — despite slice
  1's proposal saying that one would live in the resolver, it did not — so a
  first check in `bynk-check` would split the construct's checking across two
  crates for no author-visible gain. Canonical casing falls out of the pattern
  (`pt-BR`, not `pt-br`), giving a locale one spelling across a bundle.
- **The emitter drops the per-locale `const __messages_<tag>` binding.** A tag
  like `"pt-BR"` is not a valid TS identifier, so `__messages_pt-BR` would be a
  syntax error, and `ts_ident` sanitises only reserved words. Each locale's
  `code -> renderer` table is inlined directly into the `messagesByLocale`
  object literal, which is keyed by the tag *string* and needs no binding — the
  emitted output is also shorter. The `render` dispatch and the
  `messagesLocales`/`messagesReferenceLocale` exports are otherwise unchanged.
- **`CommonsItem::name()` returns `Option<&Ident>`, with `Messages` the sole
  `None`.** A `messages` block no longer has an identifier to name it by;
  synthesising an `Ident` from a string tag would be a lie any
  identifier-shaped consumer (rename, go-to-definition) would surface. The two
  in-workspace callers (`collect_external_references`, `find_declaration_span`)
  correctly skip a nameless item.
- **The migration is complete in this increment — no transition period.** The
  construct shipped four days earlier and the entire affected corpus is
  in-repository: 19 negative fixtures, 9 positive fixtures, one example, the
  guide, and the tree-sitter grammar/corpus. Accepting a bare identifier "for
  now" would mean carrying two grammars for one field and leaving the
  unvalidated path — which throws at runtime — live.

**Consequences.** `messages "pt-BR"` and `messages "zh-Hans-CN"` are
declarable, and a bundle's `messagesLocales` can now carry a subtag'd tag for
negotiation to match — the previously-unreachable end-to-end path, covered by a
new positive fixture. An invalid tag is a compile error at the tag's own span,
naming `LocaleTag`'s pattern, instead of a runtime `Intl` throw. This is a
breaking surface change (`messages en` → `messages "en"`), the cheapest it will
ever be. The `select`-arm prototype-chain defect noted alongside #899 is
independent and remains open ([#900](https://github.com/accuser/bynk/issues/900)),
as does the `bynk.locale` IDE-surface gap ([#901](https://github.com/accuser/bynk/issues/901)).
Construction-site catalogue checking and code namespacing remain the named,
deferred gaps the message-bundles retirement summary records.
