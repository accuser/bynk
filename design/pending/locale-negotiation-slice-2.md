---
level: minor
changelog: The Cloudflare `Locale` provider negotiates Accept-Language against a context's message bundle (RFC 4647 basic filtering)
---

## ADR: locale-negotiation-slice-2
title: Cloudflare Accept-Language negotiation, slice 2 — RFC 4647 basic filtering against a context's message bundle
summary: The default Locale provider on Cloudflare Workers negotiates a real request header instead of returning a fixed tag, wired by the emitter with no new language surface

**Context.** [Locale capability](../tracks/locale-capability.md) (spine
[#838](https://github.com/accuser/bynk/issues/838)), slice 2 (#882): slice 1
(#844, ADR 0256) shipped `Locale.current(): Effect[LocaleTag]` with all three
platform providers (`bynk-node.ts`/`bynk-cloudflare.ts`/`bynk-browser.ts`)
hardcoding `LocaleTag.of("en")` — no header, env, or request read anywhere.
The track's own §4.4/§7 L2 already settled the design: the default provider
resolves `Accept-Language` against the bundle's declared locale set via RFC
4647 basic filtering (exact match, then successive rightmost-subtag
truncation — `pt-BR` falls back to `pt` before the reference locale);
`current()` returns an already-resolved tag, negotiation never leaking a raw
header to callers. This ADR settles *how*.

**Decision — negotiation ships for Cloudflare Workers only; Node/Browser are
unchanged.** Confirmed: neither Node nor Browser has any inbound-HTTP-request
entry point in this compiler at all (`BuildTarget` is `Bundle`/`Workers`
only; the `fetch` boundary is Cloudflare-Workers-specific; every non-Workers
fixture uses `on call()` RPC, never `from http`). There is nothing to
negotiate from on either platform — not a deviation from the settled design,
the same "nothing platform-specific to refuse" reasoning slice 1's own
browser binding already used for the opposite case.

**Decision — which message bundle a context negotiates against is
auto-detected from its *direct* `uses` (one level, not transitive).** Zero
message-bundle commons in a context's own `uses` list → unchanged
fixed-default behaviour, silently, not a regression (nothing before ever
negotiated either). Exactly one → that bundle's declared locale set and
reference locale are threaded into the provider. Two or more, **and** the
context also `consumes bynk { Locale }` → a new diagnostic,
`bynk.locale.multiple_message_bundles` (a context depending on two
independent message catalogues has no principled single answer for "the
current locale") — matching this codebase's own established preference for a
loud diagnostic over an implicit pick. Two-or-more-but-`Locale`-never-consumed
is fine: nothing ambiguous to diagnose if `Locale.current()` is never called.

**Decision — the bundle's data reaches the provider via constructor
parameters threaded by the emitter**, mirroring the existing
`SecretsProvider(env)` precedent (`bynk_check::firstparty::provider_takes_env`)
rather than inventing a new capability-config grammar:
`new bynk__binding.LocaleProvider(request, declaredLocales, referenceLocale)`,
all three optional so every unaffected call site (Bundle mode, a bundle-less
Workers context) still calls `new LocaleProvider()` unchanged. `compose`'s
own generated signature widens to take an optional `request?: Request`
exactly when needed, mirroring how `env`/`ctx` were already conditionally
threaded (ADR 0025 D1) — `fetch` alone has a `Request` in scope; `scheduled`/
`queue` never do, so they get their own, narrower `compose(...)` call.

**Decision — the RFC 4647 basic-filtering algorithm lives in a new shared
runtime helper**, `bynk-emit/runtime/src/locale.ts`'s `negotiateLocale`,
mirroring the message-bundles-slice-3 precedent for `messages.ts`: pure,
total, never throws, always returns either a member of the declared set or
the reference locale verbatim.

**Decision — no caching of `current()` this slice.** Every call re-parses
the header and re-runs filtering; accepted as a small, bounded cost (an
`Accept-Language` header is short) rather than built speculatively.

**A real bug found and fixed during implementation, not by review this
time:** the first `LocaleProvider` implementation used TypeScript
constructor parameter properties (`constructor(private request?: Request,
...)`), which are not strip-removable — they implicitly declare *and* assign
a field, a construct `bynkc/tests/tsc_verify.rs`'s
`all_emitted_typescript_strips_under_node` gate exists specifically to catch,
since it breaks both `--inspect` debug sessions and the in-browser eval
path. De-sugared to a declared field plus a plain assigning constructor
before this landed on the 15 existing fixtures whose golden
`bynk-cloudflare.ts` this change ripples through.

**Consequences — a real, load-bearing limitation, named here rather than
silently left for someone to rediscover.** The pre-existing `uses`-clause
name-conflict check (`bynk.uses.name_conflict`) has no aliasing or renaming
mechanism. `Locale.current()` requires `uses bynk.locale` in the calling
context (for `LocaleTag`'s type to resolve at all — confirmed this holds
regardless of what the caller does with the result). Every message-bundle
commons synthesises its own `render` symbol (ADR 0272). The two collide the
moment a context `uses` both — **regardless of whether the handler body ever
calls `render` itself**, since the conflict is a declaration-level check,
not a usage-level one. The practical consequence: **no context in the
current compiler can both call `Locale.current()` directly and `uses` a
message-bundle commons in the same scope** — the exact combination this
slice's own auto-detection (Decision B) exists to wire up. This was
confirmed by direct construction, not assumed: every attempt at an
end-to-end fixture combining the two failed to compile via the pre-existing
collision, independent of this slice's own correctness.

This is not a defect in what this slice built — `negotiateLocale` and
`detect_context_message_bundle` are both correct and unit-tested in
isolation (`bynk-emit/runtime/test/locale.test.ts`;
`bynk-emit/src/project/symbols.rs`'s `detect_context_message_bundle_tests`,
covering the 0/1/2+ cases, the missing-`@reference` case, and the
non-transitivity case), and the wiring is real, dormant code that activates
correctly the moment the separate limitation is lifted. But it means this
slice cannot be verified end-to-end today, only at the unit level, and it
ships genuinely unusable for its stated purpose (negotiating a tag *and*
rendering a localised message in one place) until `uses`-clause aliasing (or
another fix to the same class of collision) exists. That is a separate,
general language feature — not scoped to this slice, and not silently
assumed solved. A future track picking up `uses` aliasing should treat this
as a concrete forward-reference, the same role several other named,
deferred dependencies play elsewhere in this track and its siblings.
