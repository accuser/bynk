---
level: minor
changelog: Capability-aware quick-fixes — add a missing `consumes`, fill missing record fields, and auto-`uses`/`consumes` an unresolved name (#852)
---

## ADR: capability-aware-quickfixes
title: Capability-aware quick-fixes for boundary/resolution diagnostics
summary: Where each capability-aware fix is computed, plus ambiguity, default-value, and clause-placement rules

**Context.** Before this increment `bynk-lsp` offered exactly two quick-fixes,
both `given`-clause edits authored as structured `Suggestion`s at the checker's
diagnosis site and rendered by `code_actions::quick_fixes` (ADR 0054). Every
other boundary/resolution error the compiler already reports — a missing
`consumes`, a record construction missing fields, an unresolved name that in
fact lives in a mixable commons or a consumable context — was a manual edit,
even though the compiler knows precisely what is missing. This widens the
quick-fix seam to those diagnostics (#852). No grammar, checker-semantics,
emitter, or runtime behaviour changes: the diagnostics are unchanged; this adds
fixes to them.

**Decision.** Add four fix families, split across two authoring sites by where
the information the fix needs actually exists:

- **Authored at the diagnosis site (checker `Suggestion`s), rendered by the
  existing pipeline:**
  - **Missing record field(s).** `check_record_field_set` now attaches, to each
    `bynk.resolve.missing_field` diagnostic, an "add field `x`" fix and — when
    more than one field is missing — an "add all missing fields" convenience
    **[DECISION B]**. Both insert a *valid default value* per field type
    (`Int`→`0`, `Float`→`0.0`, `String`→`""`, `Bool`→`false`, `Option`→`None`,
    `List`→`[]`), so the fixed buffer re-checks clean. A field that carries an
    inline refinement, or whose type is user-named (itself possibly refined, a
    sum, or opaque), has no synthesised default and is simply not offered; the
    "add all missing" convenience is withheld unless the whole missing set is
    defaultable. With existing fields the edit appends `, field: default` after
    the last one; an **empty** literal has no field span to anchor to and its
    interior spacing is unknown, so the whole ` { … }` tail (type-name end
    through the closing brace) is instead **replaced** with a canonical
    ` { … }` — fmt-stable however the empty braces were spelled (`{}`/`{ }`/
    `{  }`), both spans being available without the source text.

- **Computed in the LSP (`capability_fixes`), keyed on the diagnostic category,
  from the committed binding index and a fresh reparse of the buffer:** these
  cannot be authored at the per-unit checker diagnosis site — the fix's location
  is a *unit-header* edit that only exists once the buffer is reparsed, and the
  resolution is a *whole-project* query over the `ProjectIndex`, which is built
  after checking. The unresolved name / unconsumed chain is read from the
  source at the diagnostic's own span (never re-derived from the message).
  - **Add missing `consumes` (cross-context).** On `bynk.resolve.unconsumed_context`,
    insert `consumes <chain>` for the chain the diagnostic flagged.
  - **Auto-`uses`/`consumes` (the Bynk analogue of auto-import).** On
    `bynk.resolve.unknown_name` / `unknown_type`, query the index for every unit
    that declares the name and offer **one action per candidate [DECISION A]**,
    never a guess: a commons declaration → `uses <commons>`; a capability
    exported by a context/adapter → `consumes <context> { <name> }`. The env-free
    `bynk` surface capabilities (`Clock`, `Random`, `Logger`, `Fetch`, `Secrets`,
    `Locale`) are first-party synthetic symbols excluded from the index, so they
    are offered from an explicit list kept in sync with the adapter source.
    Commons-vs-context is inferred from the candidate unit's own index symbols (a
    context/adapter additionally declares a service/agent/actor/capability/
    provider); an unclassifiable unit is conservatively not offered for `uses`.

- **Clause placement [DECISION C].** A brand-new clause is inserted in fmt-stable
  position — appended after the last existing clause of its kind, else on a fresh
  line under the unit name (before any `uses`, the conventional consumes-first
  header order) — so the edited buffer round-trips through `fmt` unchanged. When
  a braced `consumes <unit> { … }` for the same target already exists, the
  capability is appended after the last listed one; an **empty** such clause
  (`{ }`/`{}`, interior spacing unknown) is instead **rebuilt** canonically from
  its target and the new capability, so the result is fmt-stable either way. The
  one residual is a non-empty literal/clause the author wrote with a trailing
  comma — the fix leaves that pre-existing comma in place (removing it needs the
  source text the checker does not hold), and `fmt` reconciles it exactly as it
  would the unfixed buffer.

Every fix is a **versioned `WorkspaceEdit`** (rejects a drifted buffer, like the
seed fixes); a fix is offered only when it is sound and not a no-op (a candidate
already in scope, or one the current unit cannot host — a commons cannot
`consumes` — is dropped). A capability auto-`consumes` resolves the name but
leaves the ordinary `given` requirement, whose existing add-`given` fix then
reaches clean: the two compose as the intended repair sequence.

**Consequences.** The "the LSP never re-derives a fix from a diagnostic" posture
of §3.10 now has a scoped exception: the capability-aware fixes are *computed* in
the LSP, keyed on category, because the header-insertion location and the
cross-project resolution exist only there. They stay honest by reading the
flagged name from the source at the diagnostic's own span rather than parsing the
message. The record-field fix is deliberately conservative (default-value fill
only), trading breadth — no snippet tab-stops, no fix for non-defaultable field
types — for the guarantee that a one-click apply always type-checks; snippet
placeholders and remove/rename fixes for unknown/duplicate fields are follow-ups.
Commons-vs-context classification is heuristic (no `UnitKind` is threaded into
the index); threading it is a later refinement if the heuristic proves too
coarse.
