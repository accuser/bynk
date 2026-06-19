# 0057 — Semantic tokens read the index; first-party references via a tokens-only side table

- **Status:** Accepted (v0.28)
- **Spec:** `design/bynk-lsp-spec.md` §3.14, §4.3

## Context
Tree-sitter / the extension's TextMate grammar colour syntax, but cannot
tell a type from a capability from a value at a use site — the binding
index (ADR 0053) already classified every occurrence. One gap blocks a
pure index read: first-party (`bynk.*`) references are **deliberately
dropped** at assembly (synthetic defs point at files not on disk, which
definition/rename/workspace-symbol must never surface) — yet the
platform-native distinction on exactly those symbols (`Kv`, `Clock`,
`Fetch`) is the Bynk-specific payoff of type-aware highlighting.

## Decision
Semantic tokens are a **pure read of two sources** in the cached round,
additive over the client's syntactic layer:

- **`ProjectIndex.symbols`** — user-defined defs + refs; the def site
  carries the `declaration` modifier. Modifiers are recorded on the
  `SymbolEntry` at assemble time: **`refined` only when a refinement is
  present** (`type X = Int` is `Refined { refinement: None }`, a plain
  alias — neither modifier), **`opaque`** orthogonal (an
  `opaque … where` type carries both), `platform_native` from
  `firstparty::platform_of` on the declaring unit.
- **`ProjectIndex.foreign_refs`** — a **tokens-only side table**: a
  second qualification pass (the same merged-table layering) routes the
  edges the first pass drops into `(site, kind, modifiers)` entries when
  they match a first-party declaration; genuinely unresolved targets
  stay dropped. Synthetic units remain out of `symbols` — every v0.25
  navigation invariant stands.

**The legend is frozen.** Array order is the wire encoding: token types
`[type, function, capability, service, agent, provider]`, modifiers
`[declaration, refined, opaque, platformNative]` — append-only, pinned
by a stability test. `capability`/`service`/`agent`/`provider` are
custom types; theme defaults are a B-1 extension item, and unthemed
clients fall back to the syntactic colour.

`full` + `range` ship; **`delta`** is deferred. Locals, params, and
generic type parameters are not in the index — deferred to a follow-up
(resolver scopes or a harvest); their identifiers keep the syntactic
colour meanwhile.

## Consequences
Tokens are exactly as correct as the index — binding-resolved, never
name-matched — and first-party surfaces light up (with `platformNative`
on platform adapters) without perturbing rename/references. Test files
get tokens for free (their references are namespace-targeted into the
index). The side table grows nothing else: it is invisible to every
other query. When the index gains the deferred kinds, tokens inherit
them by appending legend entries.
