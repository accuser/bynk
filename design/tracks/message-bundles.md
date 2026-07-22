# Message bundles — the `messages` construct, the checked catalogue, and the bundle `render` consumes

- **Status:** Slicing — slice 1 (the `messages` construct, a single
  `@reference` bundle, and render wiring, #859) shipped; slices 2–3 follow,
  each cut as a proposal sub-issue of the track's **spine issue**,
  [#857](https://github.com/accuser/bynk/issues/857)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)).
- **Realises:** the Locale capability track's own unfiled sibling — its
  [§2 non-goal and Q0](locale-capability.md#7-open-questions-settle-before-slicing)
  named "the message-bundle / translation-catalogue format and its completeness
  checker" as a dependency and left it unfiled. This track is that sibling: it
  turns [ADR 0256](../decisions/0256-locale-capability-slice-1.md)'s shipped,
  bundle-free `render` — which accepts `tag` but does not consult it — from
  inert into localising.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on two of the three axes, exactly as the spine issue's own trigger
  checklist ticks: **multi-increment** (the construct + a reference bundle,
  then multi-locale completeness, then the ICU template format are ordered,
  separable slices — a delete-on-merge proposal cannot carry the
  construct/lookup/checker contract across them) and **surface not yet
  settled** (where a `code` becomes checkable, the completeness model, where a
  bundle lives, the template format, and code identity are all open, §7). The
  security/safety-boundary axis is explicitly left unticked, as on the spine:
  translations are deployment data, never part of a contract hash, and a
  bundle compiles to a lookup, never executes (§6).
- **Front-loaded ADRs (named, not numbered):** the **`messages` construct and
  the bundle-lookup contract** (grammar, commons placement, compilation to a
  `(tag, code)` lookup, and how it composes with — not replaces —
  ADR-0256's `render`); the **checked-catalogue model** (reference-locale
  coverage and cross-locale template agreement, and what is explicitly *not*
  checked yet); the **reference-locale designation**. Each is created and
  numbered by the slice that lands it (§8) — this doc deliberately does not
  pre-allocate numbers, since concurrent tracks would collide.

## 1. Motivation

[ADR 0256](../decisions/0256-locale-capability-slice-1.md) shipped
`Locale`/`LocaleTag`/`Message`/`MessageArg`/`render` in a first-party commons,
`bynk.locale` (`bynk-check/src/firstparty/bynk.locale.bynk`). Its own Context
paragraph is explicit about what slice 1 deliberately left undone: "`tag` is
accepted but unused" — `render(tag, msg)` always returns `msg.code` plus a
deterministic, sorted `"{k=v, k=v}"` suffix, regardless of which locale `tag`
names, because there is nothing to look a translation up in. The doc-comment
on the shipped function says the same thing directly: "Slice 1 has no
bundle/lookup mechanism… `tag` is accepted and reserved for a later slice — it
is not yet consulted."

That gap was not an oversight; it was named and deliberately deferred.
[`locale-capability.md`](locale-capability.md) §2 lists "the message-bundle /
translation-catalogue format and its completeness checker" as a non-goal, and
its own §7 Q0 says the automatic, no-handler-code payoff Locale's spine issue
promises "cannot land until" both that gap and the unrelated
predicate-declaration gap are filled — and confirms, by exhaustive search at
settling time, that **neither existed anywhere in the repo**: no issue, no
design-notes section, no code. This track closes the message-bundle half of
that gap. (The predicate-declaration half — turning a failed refinement into
a `Message` automatically — stays exactly where Locale's Q0 left it: a
separate, still-unfiled dependency, orthogonal to this track's scope.)

Two things make this buildable now, independent of that other gap:

1. **`Message`/`MessageArg`'s shape is already ICU-ready.** ADR 0256 shipped
   `params: Map[String, MessageArg]` with a small typed-argument sum
   (`Text`/`Whole`/`Num`/`Moment`) specifically so a later template format
   could read each argument's *value*, not a pre-stringified blob
   ([`locale-capability.md` §4.1](locale-capability.md#41-localetag-and-the-render-seam-slice-1)).
   This track's slice 3 is the "later template format" that shape was held
   open for.
2. **The compiler already has two of the three mechanisms this needs, just
   applied to different problems.** A `commons` split across a directory of
   files and merged by the resolver into one logical unit
   ([ADR 0160](../decisions/0160-multi-file-commons-test-barrel.md)) is
   exactly the shape "one locale's bundle per file, several files, one
   logical bundle" needs — no new file-layout mechanism required. And bounded
   structural coverage — "every declared case of a known set is covered,
   report a concrete witness for the first gap" — already exists for `match`
   exhaustiveness ([ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md))
   and is the closest available analogue for reference-bundle completeness
   (§4.3).

## 2. Scope and non-goals

**In scope.**

- The **`messages <tag> { code => template }` construct** — an item declared
  inside a `commons`, one block per locale, positional-only templates (no ICU
  yet — that is slice 3). Compiles to a `(tag, code) -> template` lookup
  table, not to a runtime data structure the author manipulates.
- **A reference-locale designation** — exactly one `messages` block per
  bundle is marked as the reference; every other locale's completeness is
  checked against it (§4.3/§7 M2).
- **A bundle-scoped `render` companion**, generated alongside the bundle,
  that consumes the lookup and falls back — through the tag's own
  translation, the reference locale's, and finally to
  [`bynk.locale.render`](../decisions/0256-locale-capability-slice-1.md)'s
  existing code+params floor — so it stays total even against an empty or
  partial bundle, exactly as `locale-capability.md` §4.3 already specifies
  (§4.2).
- **Reference-locale completeness checking** (`bynk.messages.incomplete`) and
  **cross-locale placeholder-name agreement**, derived mechanically from the
  bundle's own declared templates — no separate catalogue declaration (§4.3).
- **The ICU MessageFormat template format**, adopted per `locale-capability.md`
  §4.5/L4's already-settled rationale, re-homed to this track and deferred to
  slice 3 (§4.4).

**Non-goals (and why).**

- **Checking a `message(code).withText(...)` builder chain's supplied
  parameter names against a bundle at the *construction* site.** This is the
  full form of M1b (§7 M1) — it would require the checker to trace a `code`
  argument back to a compile-time string literal and check the accumulated
  builder calls against a derived catalogue entry, a class of check with no
  existing precedent in this compiler (every existing "declared shape,
  checked at use" mechanism — the type table, ADR 0224's shared field-set
  validator — keys on an *identifier*, resolved by the resolver, never on a
  runtime `String` value). Real, wanted, and explicitly **not** committed to
  by any slice below — a genuine open follow-on (§7 M1), not silently
  assumed solved.
- **The predicate-declaration feature and the automatic `ValidationError` →
  localised-text path.** Exactly the other half of Locale's Q0, orthogonal to
  this track (§1).
- **Namespacing `code` under a packaging identity model.** §7 M5: the
  packaging track this would depend on does not exist as a doc or a spine
  issue anywhere in the repo — confirmed at settling time (§7 M5). Waiting on
  unstarted work indefinitely is not a real option; v1 ships a bare dotted
  `String` and names the gap explicitly.
- **Locale negotiation.** Owned entirely by the Locale track (its slice 2);
  this track only needs negotiation's *output* — an already-resolved
  `LocaleTag` — the same contract `bynk.locale.render` already consumes.
- **Full BCP-47, a general i18n framework, or a runtime `Json`-shaped
  argument type.** Out of scope for the same reasons `locale-capability.md`
  §2 gives for its own analogous non-goals — this track inherits `LocaleTag`
  and `MessageArg` as ADR 0256 shipped them and does not revisit either.

## 3. The core problem: two load-bearing calls, both genuinely open

**Is a `code` checkable at all, and against what?** `Message.code` is a bare,
unconstrained `String` field ([ADR 0256](../decisions/0256-locale-capability-slice-1.md)).
Nothing today binds a specific `code` value to a specific set of expected
parameter names — `params` is a dynamic `Map[String, MessageArg]`, not a
typed record. A bundle's own templates are the first thing in the compiler
that says anything about a code's shape at all, so the question is not "is
there a catalogue" but "does the bundle's own reference-locale declaration
double as the catalogue, or does a separate declared-catalogue construct need
inventing." §4.3 resolves this: the former — the reference bundle mechanically
*is* the catalogue for cross-locale checking, and construction-site checking
against it is named as a real gap rather than built (§2).

**Does a bare `code` stay safe once contexts start depending on each other?**
Nothing today stops two unrelated contexts from picking the same `code`
string for unrelated messages, or from a code string leaking wire-portability
assumptions it cannot honour. This is not a defect to fix now — it is a
dependency on work (the packaging identity model) that has not started
(§7 M5) — but it is a real, load-bearing absence this doc treats the way
`locale-capability.md` treated its own Q0: named, not silently assumed away.

## 4. Internal architecture

### 4.1 The `messages` construct: an item inside a `commons`, one file per locale

```
commons app.messages {
  messages en @reference {
    "order.total.non_negative" => "The order total must not be negative."
    "order.item.out_of_stock" => "{item} is out of stock."
  }
}
```

with a sibling `messages fr { ... }` in a second file in the same directory.
`messages` is a new **item** parseable inside a `commons` body — not a new
top-level source-unit kind alongside `commons`/`context`/`suite`/`adapter`
([`SourceUnit`](../../bynk-syntax/src/ast.rs), which is exactly those four
file-level kinds today, no fifth). `context` is the wrong home regardless: it
is "a deployable context (services, agents, capabilities)"
([`bynk-syntax/src/keywords.rs`](../../bynk-syntax/src/keywords.rs)), and a
message bundle is exactly what `commons` already means — "a pure, stateless
module of types and functions" — plus data.

Granularity — "one locale per block vs one block per locale" (the issue's own
M3 framing) — is not a real fork: **one locale per file**, several files, one
logical bundle, using the multi-file-commons directory merge
[ADR 0160](../decisions/0160-multi-file-commons-test-barrel.md) already ships
generically (`src/app/messages/en.bynk`, `src/app/messages/fr.bynk`, merged by
the resolver exactly as two files of the same `commons` name merge today — no
new file-layout mechanism). A `messages` block does not itself need to be
splittable across files; a bundle (the commons containing one or more
`messages` blocks) already is, for free.

**The reference-locale designation.** Exactly one `messages` block per bundle
carries `@reference` — an annotation, not new syntax class: `@indexed`
(query-algebra track) is existing precedent for a field/item-level annotation
carrying compiler-checked meaning. The checker enforces exactly one
`@reference` block per bundle (an arity check, not a new mechanism class —
narrower than, and simpler than, ADR 0169's bounded-coverage check used
elsewhere in this doc). Left as this doc's one deliberately-open sub-call for
the settling review, the way `locale-capability.md` §4.2 left its browser-
default question open: is `@reference` the right marker, or should the first
declared block in file/directory order be implicit? `@reference` is
recommended — implicit-by-order is fragile under the same multi-file merge
that makes this construct useful in the first place, since file processing
order is not itself a stable, authored contract.

**Naming note.** `on message` (a queue/WebSocket handler kind,
`bynk-syntax/src/parser/declarations.rs`) and `Message`/`MessageArg` (the
ADR-0256 types) are already load-bearing vocabulary in this compiler. There is
no mechanical collision — `on message` is a contextual-identifier match,
`Message`/`MessageArg` are capitalised type names, `messages` is a new
lowercase hard keyword confirmed free in `bynk-syntax/src/keywords.rs` (not
in `KEYWORDS`, `CONTEXTUAL_KEYWORDS`, `RESERVED_CONTEXTUAL`, or
`BUILTIN_TYPE_NAMES`) — but a reader will reasonably ask "is `messages` about
`on message` handlers?" on first encounter. Named here, deliberately, rather
than left for a reader to rediscover.

### 4.2 Compiling a bundle to a lookup, and how it composes with ADR-0256's `render`

A bundle compiles to a `(tag, code) -> template` lookup table, not a runtime
value the author manipulates directly. Alongside it, the compiler generates a
`render(tag: LocaleTag, msg: Message) -> String` function **scoped to that
bundle's own commons** (`app.messages.render`, reached via
`uses app.messages`) — this is a new, bundle-owned function, **not** a
modification of `bynk.locale.render` itself. Two structural reasons force
this, both direct consequences of ADR 0256's own placement decision:

1. `bynk.locale` is a first-party commons; a first-party commons cannot
   `uses`/`consumes` an application-authored one — the dependency direction
   only ever runs outward from first-party code, never back into it (ADR
   0256 already establishes this for the opposite direction: "a commons
   cannot `uses`/`consumes` an adapter"). `bynk.locale.render` structurally
   cannot see an app's bundle, so it cannot be the thing that gains bundle-
   awareness.
2. Two unrelated bundles in two unrelated contexts must never resolve each
   other's codes. A single global override of `render` would blur exactly
   that boundary; a per-bundle, per-commons `render` keeps lookup scoped to
   the bundle the caller actually `uses`, the same namespacing discipline
   `bynk.list`/`bynk.map`/`bynk.string`/`bynk.locale` already follow.

`bynk.locale.render` is therefore unchanged by this track — still directly
callable for bundle-free rendering exactly as ADR 0256 shipped it — and
becomes the **terminal fallback rung** a bundle's generated `render` calls
into, never bypassed:

1. the resolved `tag`'s own locale, if that locale declares the code;
2. else the `@reference` locale's template for that code (a bundle always
   has one, by construction — that is what `@reference` means);
3. else `bynk.locale.render(tag, msg)`'s existing code+params floor — total
   even against a code the bundle never declares at all, or an empty bundle.

This is exactly `locale-capability.md` §4.3's fallback chain, now given a
real implementation instead of always terminating at rung 3 vacuously — no
change needed to that chain's design, only to what actually happens at
rungs 1 and 2.

### 4.3 The checked-catalogue and completeness model (§7 M1/M2)

**M2 — completeness is reference-bundle coverage, not usage-site coverage.**
Every code declared in the `@reference` bundle must be covered by every other
declared locale; a locale missing a code the reference declares is
`bynk.messages.incomplete`, with the same concrete-witness diagnostic
convention [ADR 0169](../decisions/0169-nested-payload-patterns-and-match-arm-guards.md)
already established for `match` exhaustiveness ("a missing nested variant
reports `non_exhaustive_match` with a nested witness" — here, a missing
`(locale, code)` pair). This is a decidable, bounded, one-declared-set-against-
another check with a direct structural analogue already shipped in this
compiler. Usage-site coverage (every code *actually called* from some
`render` must be covered) has no analogue anywhere in the checker — nothing
here does call-site reachability analysis today — and would need to be built
from nothing; reference-bundle coverage is both what the issue leans toward
and the only one of the two with real prior art to build on.

**M1 — the reference bundle mechanically doubles as the catalogue for
cross-locale checking; construction-site checking is a named, deferred gap.**
Every `code => template` entry in the `@reference` bundle implicitly declares
that code's expected placeholder-name set, extracted from the template itself
(positional-only in slices 1–2, no type inference — that needs slice 3's ICU
adoption). Checking is therefore two things, not one:

- **Cross-locale template agreement** (in scope, slice 2): every other
  locale's template for the same code must reference the same placeholder-
  name set the reference template does. This is bundle-internal — it never
  needs to see a call site — and reuses the same declared-set-vs-actual-
  coverage shape as M2.
- **Construction-site checking** — does a `message(code).withText(k, v)…`
  chain actually supply the parameter names that code's reference template
  declares? — is the true M1b, and it is **not** committed to by any slice
  here (§2 non-goal). It would need the checker to resolve `code` back to a
  compile-time string literal at the builder-call site and check the
  accumulated chain against the derived catalogue, a mechanism this compiler
  has no precedent for: every existing "declared shape, checked at every use"
  discipline — the ordinary type table (`bynk-check/src/checker.rs`'s
  `types: HashMap<String, TypeDecl>`), and
  [ADR 0224](../decisions/0224-service-record-field-validation.md)'s shared
  `check_record_field_set` reached from both the resolver and the checker —
  keys on an *identifier*, resolved through ordinary name resolution, never
  on a runtime `String` value threaded through a builder chain. Building this
  well would conflate a new checker mechanism with the bundle construct
  itself; named here as a genuine, real follow-on question rather than
  scheduled into a slice, the same discipline `locale-capability.md` §7 Q0
  used for its own unscheduled dependency.

### 4.4 ICU MessageFormat (slice 3) — re-homed from Locale's L4, not re-derived

Fully settled already by `locale-capability.md`
[§4.5](locale-capability.md#45-icu-messageformat-slice-3)/[§7 L4](locale-capability.md#7-open-questions-settle-before-slicing):
adopt ICU MessageFormat as the bundle-template format, justified by `Money`/
`Instant`/`Duration` needing locale-aware plural/gender/number/date formatting
eventually, and by `Message.params`'s `MessageArg` shape already being fixed
to support it. This track now **owns** that decision going forward, per the
spine issue's own dependency note: Locale's slice 3 (L4) either retires in
favour of this track's slice 3, or is re-scoped to depend on it — recorded as
a one-line update to `locale-capability.md` §8 when this track's spine opens
its slice-3 sub-issue, not duplicated here.

### 4.5 Code identity (§7 M5) — a named, unresolved dependency, not a silent gap

A `code` ships as a bare dotted `String`
(`"order.total.non_negative"`, matching ADR 0256's own examples) with **no**
cross-context collision protection. The packaging identity model
(`organisation.package.unit`) this would need does not exist anywhere as a
committed doc or a spine issue — confirmed at settling time:
`design/bynk-adoption-sequencing.md` names it "referenced but unwritten (local
draft only, no spine issue)", and `design/archive/retired-tracks.md`'s
`deploy.md` retirement summary already flags the same packaging gap as an
"unresolved, load-bearing risk" for its own, unrelated naming cutover. This
track cannot sequence against a track that has not started; shipping a bare
`String` now and naming the gap explicitly (rather than either blocking
indefinitely or quietly assuming a bare string is fine forever) mirrors
exactly how `locale-capability.md` treated its own unfiled dependency (§7 Q0).
**When the packaging track is eventually written, this doc's §7 M5 is a
concrete forward-reference for it to pick up** — the same role Locale's own
Q0 plays for this track today.

## 5. Tooling delta (the standing rule)

New surface: one item kind (`messages`, and its `@reference` annotation)
inside a `commons`, plus one generated per-bundle function (`render`). No new
CLI verb. LSP/fmt/tree-sitter pick up `messages` through the existing
first-party-item and commons-item machinery — hover/completion/go-to-def for
a `messages` block and its `@reference` marker follow the same path existing
commons items (types, functions) already get, with no new tooling code beyond
grammar/parser entries for the new item kind. Each slice states this
explicitly per the tooling roadmap's standing rule, rather than by omission.

## 6. Security & threat model

Not a trust boundary, matching the spine issue's own unticked
security/safety-boundary axis. Translations are **deployment data**: adding a
locale or a code changes no contract hash, and a bundle compiles to a lookup
table, never to executed code (no resolve-time or deploy-time code
execution from bundle content). One asset carries over from
`locale-capability.md` §6 unchanged: a rendered message can cross a boundary
to an external caller, so a careless *template* could leak internal detail.
The mitigation is the same code-vs-free-text split Locale's own doc names —
`Message.code` is the externally-safe, stable surface; a bundle's authored
template text is content whose exposure is a review concern, not a mechanism
this track needs to enforce. No credential, provisioning, or authentication
surface; the packaging-identity gap named in §4.5 is a collision/portability
risk, not a trust-boundary one.

## 7. Open questions (settle before slicing)

- **M1 — how a `code` becomes checkable. Settled per §4.3**: the
  `@reference` bundle mechanically doubles as the catalogue for cross-locale
  template-placeholder agreement (slice 2); checking a builder-chain
  construction site against it is real, wanted, and **explicitly deferred** —
  no precedent exists in this compiler for checking a runtime `String` value
  against a compile-time declared set (§4.3), and building that mechanism is
  not scheduled into any slice below.
- **M2 — completeness model. Settled per §4.3**: reference-bundle coverage
  (`bynk.messages.incomplete`), following ADR 0169's bounded-structural-
  coverage shape directly. Usage-site coverage has no analogue in this
  compiler and is not pursued.
- **M3 — where a `messages` block lives and its granularity. Settled per
  §4.1**: an item inside a `commons`, one locale per file, using the existing
  multi-file-commons merge (ADR 0160) for free. The reference-locale marker
  (`@reference` vs. implicit-first-declared) is the one genuinely open
  sub-call left for the settling review — recommended: `@reference`,
  explicit, since file-processing order is not itself a stable authored
  contract.
- **M4 — the bundle-template format. Settled per §4.4**: adopt ICU
  MessageFormat, deferred to slice 3, re-homing `locale-capability.md`
  §4.5/L4's already-settled rationale rather than re-deriving it.
- **M5 — code identity / cross-context safety. Named, not settled — blocked
  on unstarted work.** Per §4.5: ship a bare dotted `String` in v1; the
  packaging identity model this would depend on has no doc and no spine issue
  anywhere in the repo today. Revisit when (if) that track opens.

## 8. Slice decomposition (ordered)

Each slice is an ordinary [increment proposal](../proposals/README.md) — an
issue opened as a sub-issue of this track's spine
([#857](https://github.com/accuser/bynk/issues/857)) citing this doc and its
ADRs; accepting the proposal authorises the build.

- **Slice 1 — shipped (#859).** The `messages` construct, a single
  `@reference` bundle, and render wiring: `messages <tag> @reference { "code"
  => "template" }` as a commons item (`messages` is a contextual keyword, not
  a hard one — a hard keyword broke `commons app.messages { ... }`, this
  doc's own example name); the `@reference` annotation and its
  exactly-one-per-bundle check (`bynk.messages.missing_reference`/
  `multiple_reference`), counted across every `messages` block in the commons
  (multiple blocks are allowed — forward-compatible with slice 2); a
  within-block duplicate code (`bynk.resolve.duplicate_message_code`); a
  `code -> renderer` lookup and a generated, bundle-scoped `render` composing
  with `bynk.locale.render` per §4.2's fallback chain, with a real checker-
  visible signature (not just emitted TS — resolving a Bynk-source
  `render(...)` call to the bundle-aware implementation, not a same-signature
  `bynk.locale` import, needed a synthetic function-table entry, ADR
  `messages-construct-slice-1`). `uses bynk.locale` is required
  (`bynk.messages.missing_locale_dependency`) since nothing auto-injects it.
  Positional templates only, one locale (the reference). **Makes `render`
  actually emit declared text for the reference locale.**
- **Slice 2 — multi-locale bundles, reference-bundle completeness, and
  cross-locale placeholder agreement.** Additional non-reference `messages`
  blocks in the same bundle; `bynk.messages.incomplete` reference-coverage
  checking; cross-locale template-placeholder agreement (§4.3). Lands the
  checked-catalogue-model ADR. **Produces "the bundle's declared locales" —
  the precondition the Locale track's own spine names for its slice 2, which
  must not start before this slice lands** (carried forward from the spine
  issue's own dependency note, §9).
- **Slice 3 — ICU MessageFormat.** Plural/gender/number/date over
  `MessageArg`'s typed args, the CLDR-data commitment; re-homes
  `locale-capability.md`'s L4/slice 3 (§4.4).

## 9. Slice dependencies (Locale ↔ message-bundles) — carried from the spine

Preserved from the spine issue's own "do not lose again" section, since
`locale-capability.md` has no equivalent section to point at:

- **Locale slice 1 (shipped, ADR 0256) → this track's slice 1.** This
  track's slice 1 gives the shipped `render` a bundle to read; until it
  lands, `tag` stays unused and nothing localises (§1).
- **This track's slice 2 → Locale slice 2.** Locale's slice 2 negotiates
  `Accept-Language` against "the bundle's declared locales" — not a checkable
  set until this track's completeness checking (slice 2 here) lands. **Locale
  slice 2 must not start before this track's slice 2.**
- **This track's slice 3 ↔ Locale slice 3 (L4).** One decision, owned here
  (§4.4). Locale's doc gets a one-line update to its own §8 when this
  track's slice-3 sub-issue opens, rather than the decision being re-derived
  or duplicated in two places.

## 10. Risks

- **The deferred construction-site check (§4.3, §7 M1) turns out to be load-
  bearing sooner than expected** — e.g. once the predicate-declaration work
  Locale's own Q0 names eventually lands and wants to check a predicate's
  emitted `Message` against a bundle statically. *Mitigation:* naming it now
  (rather than silently assuming M1a alone suffices forever) means a future
  track inherits a clearly-scoped, well-understood gap, not a rediscovery.
- **`@reference`'s exactly-one-per-bundle enforcement interacts badly with
  the multi-file merge** if two files in the same directory each declare
  `@reference` — an authoring mistake, not a design flaw, but the diagnostic
  needs to name both files' spans clearly (the same "cross-file secondary-
  label" class of issue
  [ADR 0227](../decisions/0227-attribute-project-level-diagnostics.md)'s
  project-diagnostics-attribution work already had to solve once).
- **A bare, unnamespaced `code` collides across two contexts that later start
  depending on each other**, once packaging exists. *Mitigation:* §4.5 names
  this explicitly rather than treating a bare string as a permanent design
  decision; it is a v1 floor, not a claim that namespacing is unnecessary.
- **Scope creep into a general i18n framework or the predicate-declaration
  feature.** *Mitigation:* §2's non-goals name both explicitly, mirroring
  `locale-capability.md`'s own discipline.

## 11. Relationship to the north star

This track completes the seam `locale-capability.md` opened but deliberately
left half-built: a pure, total `render` that had nothing to read from. It
does so using mechanisms this compiler already has — the multi-file commons
merge, bounded structural coverage — rather than inventing new ones, and it
draws a clear, honest line around the one mechanism that would be genuinely
new (construction-site catalogue checking, §4.3) rather than quietly building
it in under a different name. When it lands, `render` selects real, checked,
localised text against a multi-locale bundle, and the Locale track's
remaining slices — negotiation and formatting — have a real bundle to
negotiate against and format, exactly as its own spine issue named as the
payoff.
