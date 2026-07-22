---
level: minor
changelog: The `messages` construct compiles a locale's message bundle to a lookup and a bundle-aware `render`, wired to `bynk.locale`'s bundle-free `render` (ADR 0256) as its fallback
---

## ADR: messages-construct-slice-1
title: The `messages` construct, slice 1 — a single-locale bundle and a bundle-aware `render`
summary: Grammar, placement, the checked-catalogue floor, and the checker-visible render binding for message-bundles slice 1

**Context.** [Message bundles](../tracks/message-bundles.md) (spine
[#857](https://github.com/accuser/bynk/issues/857)), slice 1
([#859](https://github.com/accuser/bynk/issues/859)): the Locale capability's
shipped, bundle-free `render` ([ADR 0256](../decisions/0256-locale-capability-slice-1.md))
accepts `tag` but has never consulted it — there was nothing to look a
translation up in. This slice gives it one.

**Decision — the surface.** `messages <tag> @reference { "code" => "template" }`
is a new item, legal inside a `commons` (checker-enforced, not grammar-enforced
— see below). `tag` is a plain identifier; its `LocaleTag` refinement is a
checker concern. `@reference` reuses the existing `@name(args)` annotation
grammar (`store_annotation`, ADR 0111) rather than inventing new syntax. Both
sides of an entry are plain string literals — a template's `{name}` tokens are
resolved by a **compile-time string scan during lowering**, not parsed as
expressions (no new grammar for placeholders; Bynk's one existing
interpolation form, `\(expr)`, is the wrong shape — its holes evaluate eagerly
against lexical scope, the opposite of a token resolved later against a
`Message.params` map supplied at the call site).

**Decision — `messages` is a contextual keyword, not a hard one.** A first
pass made it a plain hard keyword; a parser unit test caught that this breaks
`commons app.messages { ... }` — the exact name this feature's own design
notes use as the running example — since a hard keyword can never appear as a
dotted-name segment. Added to `RESERVED_CONTEXTUAL` instead (alongside
`case`/`on`/`suite`): reserved at its own item-dispatch position, an ordinary
identifier everywhere else, via `expect_ident`'s existing exemption.

**Decision — placement is a checker rule, not a parser one.** `messages`
parses syntactically inside `commons`, `context`, and `adapter` bodies alike —
mirroring how `service`/`agent` already parse inside an `adapter` body purely
so the checker rejects them precisely. `bynk.messages.outside_commons` is
reported by a new per-unit validation pass
(`check_messages_bundles`, `bynk-emit/src/project/validate.rs`), not by the
parser. The same pass enforces exactly one `@reference` block per commons
(counted across every `messages` block — multiple blocks per commons are
allowed, forward-compatible with slice 2's multi-locale model; only one may be
the reference) and that the commons also `uses bynk.locale` (nothing in this
compiler auto-injects a `uses` clause, and the generated `render`'s fallback
needs it in scope). A within-block duplicate `code` is
`bynk.resolve.duplicate_message_code`.

**Decision — the generated `render` needs a checker-visible declaration, not
just emitted TypeScript.** Building the first fixture surfaced that a
`messages` block produces no `CommonsItem::Fn` AST node, so its generated
`render` was invisible to the resolver/checker — a Bynk-source
`render(tag, msg)` call would resolve to `bynk.locale`'s *imported*,
bundle-free `render` instead (same signature, so it "type-checked" while
silently calling the wrong implementation), defeating the slice's purpose.
Fixed by registering a synthetic `render(tag: LocaleTag, msg: Message) ->
String` `FnDecl` into a messages-bearing commons' own function table
(`build_unit_table`, `bynk-emit/src/project/symbols.rs`) — a
`bynk.resolve.duplicate_fn` conflict if the author separately declares their
own `render`. Its body is a placeholder, never inspected: body-checking walks
real AST items directly, and this entry is never added there; call-site
signature checking reads only params/return-type off the map entry. Ordinary
local-beats-`uses`-imported precedence and bare-call emission then require no
further changes — traced through `compose_unit_symbols`,
`phase_uses_name_conflicts`, and `lower_call` before relying on it.

This surfaced one more previously-unexercised bug: the test-scaffold's
used-commons destructuring (`emit_test_scope_setup`,
`bynk-emit/src/project/tests_emit.rs`) had no local-shadows-`uses` filter of
its own (unlike the production path), so a commons with its own `render` *and*
`uses bynk.locale` (which also declares `render`) destructured both into one
scope — `Cannot redeclare block-scoped variable 'render'` under `tsc`. Fixed
with the same filter.

**Decision — collision-avoidance at the TypeScript layer.** `bynk.locale`'s
exports import by plain name (confirmed against the shipped Locale fixture),
so a commons that both defines its own `render` and needs `bynk.locale`'s as a
fallback has a real naming collision. The emitter imports the fallback under a
private alias (`import { render as __bynkLocaleRender, renderArg } from
"...";`), injected as a hand-written extra import line
(`emit_unit`, `bynk-emit/src/project.rs`) rather than through the ordinary
reference-collection path, which has no per-name aliasing of its own.

**Consequences.** `render` for a messages-bearing commons now: looks up
`msg.code` in the bundle's lookup; if found, substitutes `{name}` tokens
(reusing `bynk.locale`'s `renderArg` for each present `MessageArg`, leaving an
unmatched token as literal text — total, never throws); else falls back to
`bynk.locale.render`'s existing code+params floor. `tag` is accepted but not
yet consulted for locale selection — there is only the one (reference) locale
until slice 2 (multi-locale bundles + completeness checking), which this
slice's `check_messages_bundles` and multi-block-per-commons allowance are
already shaped to extend without a breaking change. Construction-site checking
(does a `message(code).withText(...)` chain supply the parameter names a
code's template declares) remains a named, deliberately deferred gap — no
precedent in this checker for validating a runtime `String` against a
compile-time declared set (§7 M1 of the track doc).
