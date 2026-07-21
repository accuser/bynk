# Locale — the `Locale` capability: ambient locale reads and a pure render seam for user-facing text

- **Status:** Settling. Direction not yet merged; no slice authorised. Live state
  on the track's **spine issue**, [#838](https://github.com/accuser/bynk/issues/838)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)).
- **Realises:** Bynk's first i18n requirement — a handler-authored or
  boundary-surfaced validation message reaching a caller in their own language
  with no handler code. No prior design-notes section; a stub lands with the
  settling PR that merges this doc.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on two of the three axes: **multi-increment** (the capability +
  `LocaleTag` + render seam, then negotiation/fallback, then ICU-shaped
  formatting are genuinely separable slices — a delete-on-merge proposal
  cannot carry the connective contract, the resolved-tag shape and the render
  signature, across them) and **surface not yet settled** (the effectful/pure
  split, where negotiation lives, missing-translation semantics, and the ICU
  dependency are all open, §7). The security/safety-boundary axis is present
  but minor — see the Threat model (§6) — so it is not the primary trigger.
- **Front-loaded ADRs (named, not numbered):** the **`Locale` capability shape**
  (the effectful/pure split — an ambient `current()` read paired with a pure,
  total `render` — and why this needs no new language mechanism beyond the
  existing capability system, ADR 0018); the **`Message` type and its
  ownership boundary** (this track defines and owns a plain `{code, params}`
  record now, rather than waiting on the unrelated, unbuilt `predicate`-
  declaration work #838 named as motivation). Each is created and numbered by
  the slice that lands it (§8) — this doc deliberately does not pre-allocate
  numbers, since concurrent tracks would collide.

## 1. Motivation

Bynk has one built-in error type carrying a free-text message today:
`ValidationError { field, message, value }`
([`bynk-emit/runtime/src/errors.ts:1-5`](../../bynk-emit/runtime/src/errors.ts)),
produced when a refined type's constructor rejects a value and — for a value
arriving across a context boundary — nested inside a `BoundaryError`'s
`RefinementViolation` variant
([`bynk-emit/runtime/src/boundary.ts:14-27`](../../bynk-emit/runtime/src/boundary.ts)).
`message` is an English string baked in at the point the refinement fires.
There is no capability that could localise it: predicates are pure (no
`given` capabilities in scope), and nothing downstream currently has a
concept of *which language* to render into.

Issue [#838](https://github.com/accuser/bynk/issues/838) frames this as
downstream of a **`predicate` declaration** feature that would turn
`ValidationError.message` from a string into a `Message { code, params }`
descriptor. That feature, and a "sibling message-bundle track" issue #838
names for missing-translation completeness checking, **do not exist** — no
filed issue, no design-notes section, no code (`grep -rl LocaleTag` and
`grep -rn predicate.declaration` across the repo are both empty; confirmed
during this track's settling). This doc treats that absence as load-bearing,
not a detail to paper over: see §2 and §7 Q0.

Three forces converge on `Locale` regardless of that gap:

1. **The capability system already has the shape this needs.** `Clock.now()
   -> Effect[Instant]` ([`bynk-check/src/firstparty/bynk.bynk:39-41`](../../bynk-check/src/firstparty/bynk.bynk))
   is a zero-argument, ambient-read capability op — it does not take "which
   clock" as a parameter, it reads the platform's one wall-clock. `Locale`'s
   `current() -> Effect[LocaleTag]` is the identical shape, reading "the
   current request's resolved locale" instead of "the current time." No new
   language mechanism is needed; ADR 0018 already forecloses a second,
   parallel ambient-context channel outside `given`-capabilities.
2. **A pure render half is buildable and useful today, independent of the
   predicate-declaration gap.** A handler can already construct a small
   `Message { code, params }` value by hand and call a pure
   `render(tag, msg) -> String` — nothing about that requires the compiler to
   auto-produce `Message` values from failed refinements. That automatic
   wiring is real, desired, future work; it is not a precondition for the
   capability and render seam to exist and be useful.
3. **`Uuid = String where Matches(...)`** ([`bynk-check/src/firstparty/bynk.bynk:8`](../../bynk-check/src/firstparty/bynk.bynk))
   is exact, proven precedent for `LocaleTag` as a refined `String` — same
   mechanism, same adapter, already exported as a transparent type.

## 2. Scope and non-goals

**In scope.**

- The **`Locale` capability**: `fn current() -> Effect[LocaleTag]`, following
  the `Clock`/`Random`/`Logger`/`Fetch`/`Secrets` pattern exactly
  ([`bynk-check/src/firstparty/bynk.bynk`](../../bynk-check/src/firstparty/bynk.bynk)) —
  a `given`-declared, per-platform-provided op, never a second ambient
  mechanism.
- **`LocaleTag`**, a refined `String` transparent type over a pragmatic BCP-47
  subset (§4.1) — language + optional script + optional region, not the full
  grammar.
- **A pure, total `render(tag: LocaleTag, msg: Message) -> String`** — the
  render half never requires a capability, so it also covers non-ambient
  cases a capability-only design could not: a receipt rendered in a buyer's
  *stored* locale preference (not the request's), a batch job emailing each
  user in their own language from a queue consumer with no HTTP request in
  scope at all.
- **The `Message` type**, defined and owned by this track:
  `{ code: String, params: Map[String, String] }` — a plain record, not
  contingent on the unbuilt `predicate` declaration feature (§7 Q0).
- **A `FixedLocale` test provider** (`provides Locale = FixedLocale`
  test-tier override), so a `case`/`property` can assert rendered output
  without a real request.
- Per-platform providers (`bynk-cloudflare.ts`, `bynk-node.ts`,
  `bynk-browser.ts`), matching the existing `Fetch`/`Secrets` divergence
  pattern — see §4.2 for what "browser" does.

**Non-goals (and why).**

- **The `predicate` declaration feature and the `ValidationError` → `Message`
  upgrade.** Named by #838 as this track's motivation, but it is a language
  surface change (a new declaration kind, or a new field on the existing
  refinement-violation path) entirely orthogonal to the capability/render
  seam this track builds. This track makes that upgrade *possible* — once it
  lands, `RefinementViolation`'s payload becomes a `Message` and the boundary
  codec can call `render` automatically — but does not itself implement it.
  Tracked as an explicit, currently-unfiled dependency (§7 Q0), not silently
  assumed.
- **The message-bundle / translation-catalogue format and its completeness
  checker.** #838's L3 explicitly "couples to the sibling message-bundle
  track" — which also does not exist. This track does not invent a bundle
  format; `render`'s totality (§4.3) is designed to hold regardless of
  whether or when that track is opened.
- **Locale-aware number/date/currency formatting beyond slice 3's ICU
  adoption decision** — the actual CLDR data tables, plural-rule engines,
  etc. are a large surface; this track commits to *whether* to adopt ICU
  MessageFormat as the bundle format (L4/§7), not to shipping a full
  formatting library.
- **Full BCP-47** (extended language subtags, variants, extensions,
  private-use subtags per RFC 5646 §2.2.6-2.2.7). `LocaleTag`'s v1 pattern
  (§4.1) covers language, script, and region — the overwhelming common case —
  and follows this repo's own refinement discipline (design notes §15: "start
  with a small set, evolve slowly"; `Scale(N)` was rejected on the same
  reasoning). Full BCP-47 is a named follow-on (§9), not a v1 requirement.

## 3. The core problem: what "ambient" means for a capability, and who owns `Message`

Two load-bearing calls, both already answerable from existing precedent
rather than needing new invention:

**"Ambient" is not a new mechanism — it's `Clock.now()`'s shape, reused.**
`Clock`, `Random`, and `Logger` are already implemented identically across
every platform ([research: `bynk-cloudflare.ts`/`bynk-node.ts`/`bynk-browser.ts`
agree for these three]); `Fetch` and `Secrets` diverge, and where a platform
has no sensible implementation, the provider **throws** a descriptive error
naming the capability and which platforms do provide it, rather than
fabricating a plausible-looking fallback
(`bynk-check/src/firstparty/bindings/bynk-browser.ts`, `FetchProvider`/
`SecretsProvider`). `Locale.current()` sits in the second group: cloudflare
and node read the resolved locale from request-scoped context exactly the
way `SecretsProvider` reads `env` at construction
(`SecretsProvider(env?: unknown)`); browser needs a considered stance — see
§4.2, one of the few genuinely new calls this track makes rather than
inheriting.

**`Message` needs an owner now, and it does not have to be the
predicate-declaration feature.** A `{code, params}` record is not itself
new language surface — it is an ordinary record type, declarable and usable
today. This track defines it and gives `render` a real, useful signature
*today*, so that when (if) the predicate-declaration work lands, it has a
`Message` shape to target rather than needing to invent one alongside a
compiler feature. This is the single choice this doc most wants a reviewer
to push on: is decoupling ownership this way actually cleaner, or does it
risk a competing/incompatible `Message` shape when predicate-declaration
eventually lands? (Named explicitly in §7 Q0 rather than asserted as
obviously right.)

## 4. Internal architecture

### 4.1 `LocaleTag` and the render seam (slice 1)

```
type LocaleTag = String where Matches("^[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?$")
```

Language subtag (2-3 lowercase letters) + optional script (title-case,
4 letters — `Hans`, `Hant`, `Latn`) + optional region (2 uppercase letters,
ISO 3166-1, or 3 digits, UN M49). Covers `en`, `en-US`, `pt-BR`, `zh-Hans`,
`sr-Latn-RS` — the common real-world shapes — and follows the exact
`Uuid`-style precedent already in the adapter
([`bynk-check/src/firstparty/bynk.bynk:8`](../../bynk-check/src/firstparty/bynk.bynk)).
`Matches` already supports the alternation this pattern needs (confirmed
against existing fixtures exercising alternation and lookbehind in the
refinement engine).

```
type Message = { code: String, params: Map[String, String] }

capability Locale {
  fn current() -> Effect[LocaleTag]
}

fn render(tag: LocaleTag, msg: Message) -> String
```

`render` is an ordinary pure function, not a capability method — it needs no
`given` clause and can be called from a predicate, a pure helper, or a
handler alike. This is L1's resolution (§7): a capability that itself
rendered (`render(msg) -> Effect[String]`, ambient-only) would only ever
render for *the current request's* locale, which cannot express a receipt
rendered in a stored preference or a batch job iterating many users' locales
in one non-request context. A pure, explicitly-tagged `render` covers both;
the ambient case is just `let tag <- Locale.current()` followed by an
ordinary `render(tag, msg)` call.

### 4.2 Per-platform providers and the browser question

`Locale.current()` needs request-scoped context (the `Accept-Language`
header, or a resolved value the routing layer already worked out) —
cloudflare and node providers read it the way `SecretsProvider` reads `env`,
via a constructor parameter the platform binding supplies, never leaking
into application code (ADR 0018). Browser has no request at all in the
playground/REPL sense that `Fetch`/`Secrets` withhold against — but unlike
those two, a *plausible* default is not obviously wrong for a locale (unlike
a secret value or an outbound request, "assume `en`" is not a safety
problem). Two options, neither yet chosen:

- **Throw**, matching `Fetch`/`Secrets`'s existing withheld-capability
  convention exactly — consistent, but arguably wrong here since nothing
  unsafe is being avoided.
- **Default to a fixed reference locale** (e.g. `en`) — useful for the
  playground, but is the *first* first-party capability to diverge from the
  established throw-on-withheld convention, and needs its own justification
  if chosen.

Left open for the settling review to pick (folded into L1's ADR, not a
separate question — see §7).

### 4.3 Render totality and missing translations (slice 1, extended by the message-bundle gap)

`render` must be **total** — it never panics, never returns `Result`/`Effect`
— because a rendering failure on a fail-closed validation path is exactly the
"a list that's usually right is worse than none" failure mode this repo's
own ADR 0195 D2 (secrets-at-deploy) already names for a structurally
identical reason. Fallback order, independent of whether a message-bundle
track ever exists: the tag's own translation, else the reference locale's
(a bundle always has one, by construction — it's what "reference locale"
means), else the literal `code` string. The last rung means `render` is
total even against an **empty** bundle — a context that declares no
translations at all still gets `code`-as-string back, never a panic. This
survives the message-bundle track never being opened.

### 4.4 Negotiation (slice 2)

The default provider resolves `Accept-Language` against the bundle's
declared locale set using RFC 4647 basic filtering (exact match, then
successive subtag truncation — `pt-BR` falls back to `pt` before the
reference locale), not full BCP-47 language-range matching (extended
filtering, wildcard ranges) — the same "pragmatic subset, not the full
grammar" call as §4.1's `LocaleTag` pattern. `current()` returns an
**already-resolved** tag; negotiation is entirely the provider's concern, so
`render`'s callers never see a raw `Accept-Language` string.

### 4.5 ICU MessageFormat (slice 3)

Adopting ICU MessageFormat as the bundle format is recommended (L4)
specifically because Bynk's own `Money`/`Instant`/`Duration` kernels
(design notes §15, `Money` as `{minorUnits, currency}`) need locale-aware
formatting eventually, and a positional-only format would need its own,
narrower plural/gender handling reinvented later. This is a data-format
commitment, not new language surface — deferred to its own slice so it
never blocks slice 1/2 shipping.

## 5. Tooling delta (the standing rule)

Driver-only impact is minimal (no new CLI verb); the language-surface impact
is a new capability + refined type + one pure function, so LSP/fmt/tree-sitter
pick it up through the existing first-party-adapter machinery with no new
tooling code — `Locale`, `LocaleTag`, and `render` get hover/completion for
free exactly as `Clock`/`Uuid` do today. Each slice states this explicitly
rather than by omission, per the tooling roadmap's standing rule.

## 6. Security & threat model

Not a primary security boundary (the ADR 0076 trigger is ticked for
completeness, not because this track handles credentials or provisions
infrastructure), but one real asset is in scope: **user-facing validation
text crosses a boundary to an external caller**, and a poorly-written
predicate message could leak internal detail (a field name, an internal
identifier, an implementation detail phrased carelessly into the message).

- **Mitigation: the stable `code` is the externally-safe surface; free text
  stays internal by default.** `Message.code` is a short, stable identifier
  (`"order.total.non_negative"`, not a sentence) suitable for external
  exposure and for driving `render`'s lookup; nothing about this track
  requires exposing arbitrary free text to a caller. Once the
  predicate-declaration work lands and starts producing `Message` values
  from real predicates, *that* work owns keeping author-written prose out of
  the externally-rendered path by default — named here so it is not
  rediscovered as a defect later.
- **No new credential or provisioning surface.** `Locale.current()` reads a
  request header or an already-resolved context value; it authenticates to
  nothing, creates nothing, and holds no secret.

## 7. Open questions (settle before slicing)

- **Q0 (new, surfaced during settling) — the predicate-declaration and
  message-bundle dependencies are unfiled. Confirm before slice 1 whether
  that's acceptable, or whether either should be filed as its own track
  first.** Neither exists as an issue, a design-notes section, or code
  (confirmed exhaustively during this settling pass — see §1). This track's
  scope is written to be useful and shippable without either
  (§2 non-goals, §4.3's bundle-optional totality), but the boundary-codec
  auto-render behaviour #838 names as the payoff ("a validation error
  escaping a boundary reaches the caller in their language with no handler
  code") **cannot** land until the predicate-declaration work does. Slice 1
  therefore ships the capability + render seam with *manual* `Message`
  construction and `render` calls; automatic boundary integration is
  explicitly deferred, not silently assumed solved.
- **L1 — the effectful/pure split. Recommended: settled per §4.1/§4.2** — a
  pure `render(tag, msg) -> String` for any explicit tag, paired with the
  one ambient op `Locale.current() -> Effect[LocaleTag]`, needing no new
  language mechanism beyond the existing capability system (ADR 0018). The
  browser-provider question (throw vs. fixed default) is the one genuinely
  open sub-call, folded into this same front-loaded ADR.
- **L2 — where negotiation lives, and the resolved-tag contract. Recommended:
  settled per §4.4** — the default provider resolves at the seam via RFC 4647
  basic filtering against the bundle's declared locales; `current()` returns
  an already-resolved tag, never a raw header.
- **L3 — missing-translation semantics / render totality. Recommended: settled
  per §4.3** — `render` is total by construction (tag → reference locale →
  literal `code`), independent of whether a message-bundle completeness
  checker (Q0) ever exists. If that checker does land later, it becomes a
  compile-time *addition* on top of this runtime fallback, not a replacement
  for it — the runtime must stay total regardless of what compile-time
  completeness checking is layered on.
- **L4 — the ICU/CLDR runtime dependency. Recommended: adopt ICU
  MessageFormat (§4.5), deferred to slice 3** — a data-format choice that
  commits the runtime to CLDR data, justified by `Money`/`Instant`/`Duration`
  needing locale-aware formatting eventually; not new language surface.

## 8. Slice decomposition (ordered)

Each slice is an ordinary [increment proposal](../proposals/README.md) — an
issue opened as a sub-issue of the track's spine
([#838](https://github.com/accuser/bynk/issues/838)) citing this doc and its
ADRs; accepting the proposal authorises the build. Slice 1 is standalone;
later slices build on the negotiation/formatting surface, not on each other's
internals.

- **Slice 1 — the `Locale` capability, `LocaleTag`, `Message`, and a pure,
  total `render`.** The `FixedLocale` test provider; per-platform providers
  including the settled browser stance (§4.2). Explicitly **excludes**
  automatic boundary-codec integration (Q0) — a handler calls `render`
  manually. Lands the capability-shape ADR.
- **Slice 2 — locale negotiation & fallback.** The default provider resolves
  `Accept-Language` → `LocaleTag` against the bundle's declared locales, RFC
  4647 basic filtering, fallback chain to the reference locale (§4.4).
- **Slice 3 — ICU MessageFormat.** Plurals, gender, and locale-aware
  number/date/currency formatting (§4.5), landing the ICU/CLDR dependency
  decision.

## 9. Risks

- **The predicate-declaration work never lands, or lands with an
  incompatible `Message` shape.** This track's whole "no handler code"
  payoff depends on it. *Mitigation:* slice 1 ships a capability + render
  seam useful on its own (manual `Message` construction), so the track's
  value does not evaporate if the dependency stalls; Q0 names the risk
  rather than hiding it.
- **A fixed browser-locale default (§4.2) turns into a silent, wrong
  assumption for a real deployed context.** *Mitigation:* if chosen, scoped
  explicitly to the playground/REPL, documented as such, and named as the
  one first-party capability that diverges from the throw-on-withheld
  convention, so a reader does not mistake it for the general rule.
- **Full BCP-47 turns out to be needed sooner than expected** (extended
  subtags, variants, private-use). *Mitigation:* `LocaleTag`'s pattern is a
  single refined type, not deep architecture — widening the regex is a
  small, isolated change, not a redesign.
- **Scope creep into the message-bundle format or a general i18n framework.**
  *Mitigation:* §2's non-goals name both explicitly; §4.3's totality is
  designed to hold with or without either ever existing.

## 10. Relationship to the north star

This track opens Bynk's first i18n surface without waiting for the
language-surface work (`predicate` declarations) that would make it fully
automatic — it proves the runtime seam (an ambient capability + a pure,
total render) is buildable and useful on its own terms, using precedent
(`Clock`, `Uuid`, ADR 0018) that already exists rather than inventing a new
mechanism. When the predicate-declaration work does land, it targets a
`Message` shape and a `render` seam that already exist, tested, and shipped
— rather than the two being designed simultaneously and risking a mismatch.
