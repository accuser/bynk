# Tooling track — LSP: complete the editor experience

- **Phase:** **✅ Completion arc complete (slices 0–4); slice 5 landed.** The
  surface-contract ADR (slice 0, ADR 0093) plus slices 1 (G1–G3), 2 (G4), 3 (G5),
  and 4 (G6, ADR 0094) wired the eight-context surface — registry-complete,
  coverage-tested, error-tolerant. Slice 5 added `completionItem/resolve` (lazy
  docs) + typed-signature detail polish. What remains is the post-completion tail
  — slices 6–8 (navigation round-out, editor polish, editor-agnostic +
  publishing) — plus the known upstream resolve-gate limitation noted in ADR 0094. The navigation and refactor table-stakes (references, rename,
  hover, code actions, signature help, code lens, inlay hints, semantic tokens,
  workspace symbols, document highlights, call hierarchy, implementation nav,
  folding/selection) **already shipped** across v0.24–v0.37; this track picks up
  the work that remains:
  **completion is still narrow and debt-laden**, the clean-file ceiling caps the
  type-aware features, and a tail of editor-experience polish is unscheduled.
  This doc is the connective plan and the decision log for that arc; concrete
  slices become `proposals/vX.Y-<slug>.md` entries when scheduled.
- **Realises:** the standing "keep tooling current with the language" rule
  (`karn-tooling-roadmap.md` §5) and the completion/editor-experience remainder of
  roadmap items **A‑1 → A‑3** and **B‑1/B‑2**.
- **Depends on / sequences after:** the v0.25 binding index (ADR 0053) and the
  v0.24 project-diagnostics pipeline (ADR 0052) — both shipped; this track queries
  them, it does not rebuild them. **Refreshes:** `karn-tooling-roadmap.md` §1–§2
  (whose "not implemented" list is now stale) and §4 B‑1/B‑2.

## Why a track and not a single proposal

This is a **tooling track**, *not* an ADR 0076 feature track — the LSP is neither
a language-surface change nor a security boundary, so it carries no threat model
and no `where`-type design tension. What it shares with a feature track, and why
it earns a persistent doc rather than a lone delete-on-merge proposal, is the
other 0076 trigger: **it is unavoidably multi-increment**, and the increments are
*connected* — they share one completion engine, one clean-file ceiling, and one
"surface inventory" contract. A single proposal would settle the completion
contract for one slice and discard the reasoning the next five slices need; the
slice decomposition and decision log below are that connective tissue.

## The throughline

> **The hard infrastructure already exists.** Karn's LSP already has the
> authoritative compiler diagnostics, a cross-file binding index, scope-correct
> locals, kernel-method tables, and a typed-receiver overlay. Completion is thin
> not because the data is missing but because the **context→candidate wiring was
> built conservatively, one slice at a time** (ADRs 0061–0064), and stopped short.
> This track is mostly *wiring up data that's already enumerable* — and then
> lifting the one structural limit (the clean-file ceiling) that caps the
> type-aware features.

Every gap below was confirmed by reading the engine, not inferred: the
context-dispatch in `karn-lsp/src/completion.rs::complete` (lines 96–134), the
handler's locals/value-member fallbacks in `karn-lsp/src/main.rs::completion`
(lines 960–1003), and the advertised `trigger_characters` (line 1515).

## Current state (refreshed — corrects the roadmap)

**Implemented and shipped** (`server_capabilities()`, `main.rs:1506`): live
diagnostics, hover, go-to-definition, **references, rename/prepareRename, code
actions, signature help, code lens, inlay hints, semantic tokens (full + range),
workspace symbols, document highlights, call hierarchy, implementation nav,
document symbols, folding + selection ranges**, and document/range formatting
(wired to `karn-fmt`). The roadmap's §1 "not implemented" list predates all of
this and is wrong — A‑1/A‑2/A‑3 are substantially done.

**What completion offers today** (eight contexts across two dispatch sites — the
first six are arms of `complete()` in lexical priority order; the last two are
**handler-side fallbacks** in `main.rs::completion`, gated by the typed-receiver
overlay — the seam G6 touches):

| Context | Site | Trigger (line prefix) | Items |
|---|---|---|---|
| `consumes U { … }` | `complete()` | inside the brace | capabilities `U` exports |
| `consumes <prefix>` | `complete()` | after `consumes ` | consumable unit names |
| `given …` | `complete()` | in a `given` list | in-scope capabilities |
| `UpperIdent.` | `complete()` | name receiver + `.` | sum variants; refined/opaque `of`/`unsafe`; capability ops; 4 builtin statics |
| type position | `complete()` | after `:` `->` `[` | builtin + surface + project types |
| keyword position | `complete()` | bare word at stmt start | 60 keywords + 6 decl snippets |
| `value.` (lowercase) | handler, overlay-gated | lowercase receiver + `.` | kernel methods + record fields *(clean-file only)* |
| locals | handler, overlay-gated | keyword pos, or expr pos when empty | in-scope `let`/param bindings *(cached analysis only)* |

## The gaps (settled design, by issue)

Six confirmed gaps, each a "wire up enumerable data" or "lift one limit" — none
needs new analysis.

### G1 — No `.` trigger character

`trigger_characters` is `[" ", "{", ","]` (`main.rs:1515`). Both member contexts
(`Type.` and `value.`) only fire when the editor *explicitly re-requests*
completion; typing a dot shows nothing. This single omission is the dominant cause
of "members are missing." **Decision:** add `"."`. One line; highest impact.

### G2 — Builtin statics table is incomplete

`BUILTIN_STATICS` (`completion.rs:307`) lists only `Int.parse`, `Float.parse`,
`Json.encode`, `Json.decode`. The resolver also admits `List.empty`, `Map.empty`,
and the `Effect.pure` special form — so `List.‹dot›` completes nothing.
**Decision:** add the three missing statics. Data-only.

### G3 — Builtin sum types don't complete their variants

`member_candidates` (`completion.rs:334`) scans only *project* `type` decls, so
`HttpResult.‹dot›` and `QueueResult.‹dot›` yield nothing, though their variants
are a fixed table (`ast.rs` `http_variant`/`queue_variant`). **Decision:** seed
the name-receiver path from the builtin sum tables alongside project types.

### G4 — Expression position offers locals only

At `let x = ‹here›` (`is_expression_position`, `completion.rs:260`) only cached
locals fire. The constructor keywords (`Ok`/`Err`/`Some`/`None`/`true`/`false`),
in-scope **type names**, and **type statics** are all valid there and all
enumerable, but unoffered. **Decision:** broaden the expression-position candidate
set beyond locals. The load-bearing call (what *exactly* belongs at each cursor
context) is the completion-surface contract — see *Foundational ADRs*.

### G5 — No free-function / stdlib completion anywhere

After `uses karn.list` / `karn.map` / `karn.string`, the combinators (`map`,
`filter`, `find`, `values`, `getOr`, `join`, …) are never completable, nor are
user-declared top-level `fn`s in scope. All are enumerable from the firstparty
`.karn` sources + the project parse. **Decision:** add a free-function candidate
producer keyed off the in-scope `uses` set and the current unit's `fn` exports.

### G6 — The clean-file ceiling caps the type-aware features

`value_member_completions` and `locals_completions` both depend on a clean
re-analysis / last-good cached analysis (`main.rs:384`, `:331`). Any parse or type
error silences value-receiver methods, record fields, **and** locals — exactly
mid-edit, when they're wanted most. **Decision:** relax to error-tolerant receiver
typing (type the longest clean prefix / fall back to the last good snapshot per
binding). The largest slice; its own ADR — see below.

## Beyond completion — the desirable-feature survey (VS Code context)

A scan of the standard LSP surface against what `karn-lsp` advertises, curated for
what's genuinely valuable *for Karn* (debugger- and color-oriented providers are
omitted as N/A — Karn has no debugger and no color literals).

**Worth doing (tracked here):**

- **`completionItem/resolve`** — lazy docs/detail on the focused item. Pairs
  naturally with the richer completion above; keeps the initial list cheap.
- **Go-to-type-definition + type hierarchy** — deferred at ADR 0068 pending a
  unit→file map. High value for Karn's refined/opaque/sum types and the
  `given Cap → provider` relationship; the index already has most of the edges.
- **Document links** — make `uses karn.list`, `consumes B`, and split-path unit
  references click through to their source. Cheap given the binding index.
- **On-type formatting** — light auto-format on `}`/newline, deferring to
  `karn-fmt`. Modest, but removes a manual format step.

**Editor polish (roadmap B‑1/B‑2, folded in):**

- Inlay-hint granularity + format-on-save + server-trace **settings**.
- **Snippets** beyond the six completion scaffolds; **scaffolding commands** (new
  context/adapter) — note these overlap the `karn` driver's `new` (roadmap §5.1),
  which supersedes the CLI path.
- A **problem matcher** so `karnc` builds surface in the Problems panel.
- A **getting-started walkthrough**; **marketplace + Open VSX publishing** (ties
  to the Tier 4 release work — see `release_and_ci_posture`).

**Editor-agnostic (standing goal, roadmap §5):** documented Neovim / Helix / Zed
setup so "rival a modern language server" isn't VS-Code-only.

**Deferred / low-value (named so they're not silently dropped):**
semantic-tokens **delta** (perf optimisation; the full pass is fast enough),
`codeLens/resolve` and `inlayHint/resolve` (both computed eagerly today),
linked-editing ranges, monikers, inline values (debugger), color provider.

## Internal architecture

The completion engine is a **lexical context-dispatcher** (`complete` picks one
context from the line prefix) feeding **semantic candidate producers** (each
enumerates from the static `karnc` registries or a recovery parse of the project).
Two producers — value-members and locals — additionally need a **typed-receiver
overlay**: the buffer is rewritten so it parses, re-analysed, and the receiver
typed via the retained `expr_types` (`type_receiver`, `main.rs:408`, shared with
signature help). That overlay is the **clean-file ceiling**: it returns nothing
when the file doesn't check clean. G1–G5 extend the *producers* (no ceiling
involved); G6 is the ceiling itself. Nothing here is a re-architecture — the
shape is "more candidate producers + one error-tolerant typing path."

The drift guard that matters: the published-tarball exclusion and the legend/
encoding freezes already protect semantic tokens; completion has no equivalent
contract test, which the surface ADR below should establish.

## Slice decomposition (proposed)

0. **The completion-surface ADR.** ✅ **Landed (ADR 0093).** A standalone doc-ADR,
   not stapled to a code slice — see *Foundational ADRs*. Fixes the whole *context
   × candidate-kind* matrix (eight contexts), the completeness + coverage-test
   guarantee, the ceiling boundary (D4), and `.` as a trigger char, so slices 1–4
   implement against a settled contract rather than discovering it piecemeal.
1. **Completion — quick wins (G1–G3).** ✅ **Landed.** The `.` trigger char + the
   missing statics (`List.empty`/`Map.empty`/`Effect.pure`) + builtin sum-type
   variants (`HttpResult`/`QueueResult`, from the `karnc::ast` registries), plus
   the D5 coverage test (`builtin_sum_variants_are_complete`,
   `builtin_statics_are_reachable`). Data-and-config only; no new ADR (implements
   the slice-0 contract for the member/static contexts).
2. **Completion — expression-position surface (G4).** ✅ **Landed.** The value
   constructors (`Ok`/`Err`/`Some`/`None`/`true`/`false`) + in-scope type names at
   value positions, via a new `complete()` expression arm + a `Constructor` kind;
   locals/params still appended handler-side. No new ADR (implements D3). Exercised
   the contract's hardest partition (what belongs at an expression position).
3. **Completion — free functions & stdlib (G5).** ✅ **Landed.** A
   `free_function_candidates` producer (a `Function` kind) offering the current
   unit's own `fn`s + the combinators of every `uses`-imported module (project +
   the embedded `karn.list`/`karn.map`/`karn.string` stdlib, now in
   `for_each_unit`), gated on the `uses` set. No new ADR (implements D3); signature
   help gained stdlib labels for free.
4. **Completion — lift the clean-file ceiling (G6).** ✅ **Landed
   ([ADR 0094](../decisions/0094-error-tolerant-receiver-typing.md)).**
   Error-tolerant receiver typing for the value-receiver path (value members +
   signature help): Analyse mode records the checker's best-effort partial
   `expr_types` at every per-file check exit, so the receiver types whenever it
   itself checks, despite an unrelated type error elsewhere. Build stays Ok-only
   (codegen untouched). The structural slice; the one remaining limit is the
   upstream resolve gate. (Locals are out of this ceiling — see 0094.)
5. **`completionItem/resolve` + detail polish.** ✅ **Landed.** `resolveProvider`
   advertised; items stash their doc URI in `data`, and `completion_resolve` fills
   in hover-quality `documentation` lazily (reusing `symbols::describe_symbol`,
   local then cross-file) so the initial list stays cheap. Detail strings are now
   typed signatures (params + return) for capability ops as well as free fns. No
   new ADR. Auto-import via resolve deferred.
6. **Navigation round-out.** Go-to-type-definition + type hierarchy (the ADR 0068
   deferral) + document links. *May earn an ADR if the unit→file map is new
   surface.*
7. **Editor polish (B‑1/B‑2).** Settings, snippets, scaffolding-or-`new`, problem
   matcher, walkthrough.
8. **Editor-agnostic + publishing.** Neovim/Helix/Zed docs; marketplace + Open VSX
   (sequences with Tier 4 release work).

Each slice except 0 is an ordinary `vX.Y-<slug>.md` proposal citing this doc;
slice 0 is the standalone surface-contract ADR (no code, no version tag). Status
tracked here as slices land. Slice 0 + slices 1–4 are the completion arc the user
asked for; 5–8 are the broader editor-experience remainder. **Every completion
slice (1–4) ships fixture tests for the new contexts it adds** — the contract
test the surface ADR establishes is the standing guard the rest extend (today
completion has no such test; see §"Internal architecture").

## On merge — each slice updates

The forward-looking claims in this doc (the spec is "updated in place," the
roadmap is "refreshed," the log is "seeded as slices land") are contracts, not
aspirations. On landing, each slice's PR updates, in the same change:

1. **`karn-lsp-spec.md`** — the feature's behaviour, in place, the way the
   normative spec is. (Slice 0 added the §3.15 surface-contract paragraph; slices
   1–4 update §3.15's as-built table as each cell lands.)
2. **This track's *Decision log*** — a dated entry with the slice's ADR link(s)
   and the one-line decision, mirroring the actors track.
3. **This track's *Phase* bullet and the relevant *slice-decomposition* row** —
   marked ✅ with the version, so the doc never overstates what's shipped.
4. **`karn-tooling-roadmap.md`** — only if the slice changes its A‑/B‑item status
   (§1–§2 were already corrected to match what shipped through v0.37; later slices
   tick off the completion line and B‑1/B‑2).
5. **The fixture tests** for any new completion context (slices 1–4), extending
   the surface contract test from slice 0.

A slice that touches none of 1–4 (rare — most change spec or status) still owes
its decision-log entry.

## Foundational ADRs to land

- **The completion surface contract** — ✅ **landed as
  [ADR 0093](../decisions/0093-completion-surface-contract.md)** (slice 0, ahead
  of slice 1): the canonical matrix of *cursor context × candidate kinds* — which
  symbols complete where, the keyword/expression/type/member/value-member
  partition, and a coverage test so the surface can't silently regress as the
  language grows. The one genuinely design-bearing call in the arc; G4's "what
  belongs at an expression position" falls out of it (D3). A standalone doc-ADR,
  so the trivial slice 1 stays trivial and the contract is baked once, up front.
- **Error-tolerant receiver typing** — ✅ **landed as
  [ADR 0094](../decisions/0094-error-tolerant-receiver-typing.md)** (slice 4): how
  the clean-file ceiling is relaxed. The speculated fork (longest-clean-prefix
  **vs.** last-good-per-binding snapshot) dissolved on reading the checker — the
  types are already computed per-expression and merely withheld by a final
  `errors.is_empty()` gate, so a third option dominates both: **record best-effort
  partial `expr_types` in Analyse mode** (Build stays Ok-only, so codegen is
  untouched). Monotonic — never *worsens* on a broken buffer.

Both are tooling ADRs in the 0052–0072 LSP lineage, not language ADRs. Numbers are
assigned on landing (next free is 0093 as of this writing) — left unpinned here so
the prose can't go stale if an unrelated ADR lands first, matching the actors
track's forward-ADR convention.

## Decision log (track-level)

- **Slice 0 — completion surface contract (2026-06-18):**
  [0093](../decisions/0093-completion-surface-contract.md) — completion has one
  canonical *context × candidate-kind* matrix (eight contexts); every populated
  cell is registry-sourced (`karnc::{keywords, builtin_names, kernel_methods,
  firstparty}` + AST sum-variant tables) and **complete**, enforced by a
  coverage test; the clean-file ceiling is confined to the value-receiver cell
  (D4); `.` is a trigger character; expression position offers values +
  constructors + functions + type names (D3, the G4 call). A doc-ADR (no code,
  no version tag); the spec contract lives at `karn-lsp-spec.md` §3.15.
- **Slice 1 — completion quick wins, G1–G3 (2026-06-18):** no new ADR (implements
  0093 for the member/static cells). Registered `.` as a completion trigger char
  (D1); completed the builtin-statics table with `List.empty`/`Map.empty`/
  `Effect.pure` (D2/G2); sourced builtin sum-type variants for `HttpResult`/
  `QueueResult` from the `karnc::ast` `HTTP_VARIANTS`/`QUEUE_VARIANTS` registries
  (D2/G3); added the registry-driven coverage tests
  (`builtin_sum_variants_are_complete`, `builtin_statics_are_reachable`) — the
  first instances of the D5 guard. §3.15's as-built table updated.
- **Slice 2 — expression-position surface, G4 (2026-06-18):** no new ADR
  (implements 0093 D3). Added a `complete()` expression-position arm offering the
  six value constructors (`CompletionKind::Constructor`, docs reused from the
  `keywords` registry) + in-scope type names (reusing `type_candidates`); reworked
  the handler so locals/params attach at keyword *or* expression position (the old
  `items.is_empty()` proxy no longer holds). Free functions (the in-scope-values
  group) are deferred to slice 3 / G5. Coverage:
  `expression_position_offers_constructors_and_types`. §3.15 row added; module-doc
  header refreshed to the eight-context contract.
- **Slice 3 — free functions & stdlib, G5 (2026-06-18):** no new ADR (implements
  0093 D3). Added a `free_function_candidates` producer (`CompletionKind::Function`)
  at expression position offering the current unit's own free `fn`s + the
  combinators of every `uses`-imported module, gated on the `uses` set; the
  embedded stdlib commons (`karn.list`/`karn.map`/`karn.string`) joined
  `for_each_unit` (harmless to other contexts — fns only — and a free win for
  signature help). Signatures reuse `symbols::type_ref_str` (one renderer, like
  hover/signature help). Coverage: `free_functions_offered_for_own_unit_and_used_
  modules` (registry-driven over `KARN_LIST_SRC`) + `free_functions_require_a_uses_
  import`. §3.15 row + module-doc header updated.
- **Slice 4 — error-tolerant receiver typing, G6 — ADR (2026-06-18):**
  [0094](../decisions/0094-error-tolerant-receiver-typing.md) — lift the clean-file
  ceiling by recording best-effort partial `expr_types` in Analyse mode. Diagnosis:
  `check_record` computes types for every well-typed sub-expression, then discards
  the whole file's map on a final `errors.is_empty()` gate; the Analyse recorder
  sits past the per-file `Err → continue`. The track's speculated fork
  (longest-clean-prefix vs. last-good-snapshot) is rejected — both are dominated by
  surfacing the partial map (no new machinery, never stale, positionally exact).
  Build stays Ok-only, so codegen is untouched. **Implementation:** `check_record`
  returns a `RecordCheck { result, partial_expr_types }`; Analyse mode records the
  partial map via a shared `record_analyse_types` helper at all four per-file
  exits (`check_record` + the two context/declaration checks + the clean path).
  Coverage: `expr_types_capture` — the old ceiling test inverted, plus a
  handler-body fixture for the declaration-check exit. The completion arc is
  complete.
- **Slice 5 — `completionItem/resolve` + detail polish (2026-06-18):** no new ADR.
  Advertised `resolveProvider`; the completion handler stamps each item's `data`
  with its doc URI, and `completion_resolve` attaches hover-quality `documentation`
  lazily on the focused item only — reusing `symbols::describe_symbol` (local then
  cross-file, like hover §3.4), a graceful no-op for items naming no declared
  symbol. Capability-op detail strings now render typed signatures (params +
  return via `type_ref_str`), matching free fns / signature help. Coverage:
  `capability_member_suggests_ops` (typed detail) +
  `advertises_completion_with_dot_trigger_and_resolve`. Auto-import via resolve
  deferred. §3.15 updated.

## Cross-references

- `karn-tooling-roadmap.md` — the parent roadmap; this track refreshes its §1–§2
  status and owns the completion + B‑1/B‑2 remainder.
- `karn-lsp-spec.md` — the authoritative LSP feature spec; updated in place as
  slices land, the way the normative spec is.
- ADR [0052](../decisions/0052-lsp-project-diagnostics.md) (diagnostics),
  [0053](../decisions/0053-lsp-binding-index.md) (binding index),
  [0061](../decisions/0061-completion-sliced-positional-first.md)–0064 (the
  completion slices this track continues),
  [0068](../decisions/0068-implementation-navigation.md) (the deferred type-nav).
