# 0278 — Split `bynk.locale`'s types into a leaf commons, `bynk.locale.types`

- **Status:** Accepted (v0.232)

**Context.** Locale-negotiation slice 2 (#882, `design/pending/locale-negotiation-slice-2.md`)
shipped Cloudflare `Accept-Language` negotiation wiring that auto-detects a
context's message bundle via its direct `uses` — but named a real, load-bearing
gap in its own Consequences: `Locale.current()` requires `uses bynk.locale`
(for `LocaleTag` to resolve at all), and every message-bundle commons
synthesises its own `render` function (ADR 0272). The two collide the moment
a context `uses` both, under the pre-existing, unaliased `uses`-clause
name-conflict check (`bynk.uses.name_conflict`) — regardless of whether the
handler body ever calls `render`. No context in the compiler could combine
real negotiation with rendering a bundle's message in one place; slice 2's
own auto-detection mechanism (Decision B) could never fire in any compiling
program. Confirmed by direct construction: every attempt at an end-to-end
fixture combining the two failed to compile.

An initial proposal (#886, first revision) tried loosening the checker's
`exports transparent` validator so the `bynk` adapter could re-export
`LocaleTag` from `bynk.locale` without a full `uses`. Review found this
approach had a real bug: `merge_consumed_exports` resolves an exported type's
declaration from the consumed unit's *local-only* table
(`unit_tables.get(name)`, never composed with that unit's own `uses`), so
`LocaleTag` would be silently dropped even if the checker accepted the wider
clause. The review proposed the alternative this ADR adopts instead: fix the
actual cause, not route around a symptom of it.

**Decision.** Split `bynk.locale` into two firstparty commons:

- **`bynk.locale.types`** — a new, dependency-free leaf carrying `LocaleTag`,
  `MessageArg`, and `Message`. No `uses` clauses; these three types have no
  dependency on `bynk.list`/`bynk.string` (only `render`'s own
  fold/join implementation does).
- **`bynk.locale`** — unchanged value-level API (`render`/`renderArg`/
  `message`/`withText`/`withWhole`/`withNum`/`withMoment`), now `uses
  bynk.locale.types` for those three type names instead of declaring them
  itself.

The `bynk` adapter's own `uses bynk.locale` (previously needed only for
`LocaleTag`, for `capability Locale { fn current() -> Effect[LocaleTag] }`)
repoints to `uses bynk.locale.types`. A context calling `Locale.current()`
now only ever needs the leaf directly, never the value-level commons — so
it has nothing left to collide with a message-bundle commons's own
synthesised `render`.

**A wider migration than the original proposal scoped.** The synthesised
`render` a messages-bearing commons gets (`synthetic_render_fn`,
`bynk-emit/src/project/symbols.rs`) types its parameters as real, resolved
name references — `TypeRef::Named("LocaleTag")`/`TypeRef::Named("Message")` —
not bypassed. Every message-bundle commons therefore now needs `uses
bynk.locale.types` too, not only the contexts calling `Locale.current()`
directly. The existing `bynk.messages.missing_locale_dependency` diagnostic
(`check_messages_bundles`, `bynk-emit/src/project/validate.rs`) widens to
require both `uses bynk.locale` and `uses bynk.locale.types`, reporting
whichever is absent (or both) — one diagnostic, not two, since a message
bundle always needs both together. All 13 existing fixtures that `uses
bynk.locale` needed the new `uses` line added and their goldens re-blessed.

A second, unplanned fix surfaced during that re-bless: the three
platform-specific `LocaleProvider` bindings
(`bynk-check/src/firstparty/bindings/bynk-{node,browser,cloudflare}.ts`)
imported `LocaleTag` from `./bynk/locale.js` directly — a hardcoded path, not
resolved through the ordinary checker-driven import mechanism, so the split
didn't move it automatically. All three now import from
`./bynk/locale/types.js`; caught by `tsc --strict` failing on the type-check
step of a manual end-to-end smoke test, not by any golden-diff (the golden
files are exactly what was under test).

**A side benefit, not the point of this change.** Because the `bynk` adapter
previously depended on `bynk.locale` (which itself `uses bynk.list` and `uses
bynk.string`), *every* fixture consuming any `bynk` capability — not just
`Locale` — had `bynk/list.ts`, `bynk/locale.ts`, and `bynk/string.ts`
injected into its output, unused. Fifteen such fixtures lost that dead
weight, now emitting only the minimal `bynk/locale/types.ts` leaf.

**Consequences.** Closes the gap named in `locale-negotiation-slice-2.md`'s
Consequences — see that file's own Update note. Verified end-to-end, not
only at the unit level: a context combining a real `Locale.current()` call
with a message bundle's `render` (via a hand-written wrapper function, since
`uses` stays one-level/non-transitive — a message-bundle commons's own
`uses bynk.locale`-derived names are not further re-exported to a consumer
of *that* commons), with no `uses bynk.locale` anywhere in the consuming
context, compiles, passes `tsc --strict`, and runs correctly under node
(`bynkc/tests/fixtures/positive/817_locale_bundle_wrapper_e2e`). This is a
point-fix for this one pairing (`Locale.current()` plus a message bundle),
not a general fix for `uses`-clause aliasing — that broader gap, and the
non-transitivity constraint above, remain open language questions for a
future track.
