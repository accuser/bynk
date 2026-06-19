# Tooling track — LSP: complete the editor experience

- **Phase:** **✅ Completion arc complete (slices 0–4); slice 5 landed.** The
  surface-contract ADR (slice 0, ADR 0093) plus slices 1 (G1–G3), 2 (G4), 3 (G5),
  and 4 (G6, ADR 0094) wired the eight-context surface — registry-complete,
  coverage-tested, error-tolerant. Slice 5 added `completionItem/resolve` (lazy
  docs) + typed-signature detail polish, and slice 9 surfaced first-party symbol
  docs through hover/completion. Slices 6a (go-to-type-definition) and 6b
  (document links, ADR 0095) have landed; slice 7 (editor polish) is done — most
  of it pre-shipped with the v0.54 extension, the genuine gaps (inlay-hint
  granularity + default-formatter config) now closed. 6c (type hierarchy) is
  **closed as won't-do** — `typeHierarchy` is an OO feature Bynk's type model
  doesn't fit (see the slice row). What remains is slice 8 (editor-agnostic docs,
  doable now; marketplace/Open VSX publishing, gated on Tier 4 release work) —
  plus the known upstream resolve-gate limitation noted in ADR 0094. The navigation and refactor table-stakes (references, rename,
  hover, code actions, signature help, code lens, inlay hints, semantic tokens,
  workspace symbols, document highlights, call hierarchy, implementation nav,
  folding/selection) **already shipped** across v0.24–v0.37; this track picks up
  the work that remains:
  **completion is still narrow and debt-laden**, the clean-file ceiling caps the
  type-aware features, and a tail of editor-experience polish is unscheduled.
  This doc is the connective plan and the decision log for that arc; concrete
  slices become `proposals/vX.Y-<slug>.md` entries when scheduled.
- **Realises:** the standing "keep tooling current with the language" rule
  (`bynk-tooling-roadmap.md` §5) and the completion/editor-experience remainder of
  roadmap items **A‑1 → A‑3** and **B‑1/B‑2**.
- **Depends on / sequences after:** the v0.25 binding index (ADR 0053) and the
  v0.24 project-diagnostics pipeline (ADR 0052) — both shipped; this track queries
  them, it does not rebuild them. **Refreshes:** `bynk-tooling-roadmap.md` §1–§2
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

> **The hard infrastructure already exists.** Bynk's LSP already has the
> authoritative compiler diagnostics, a cross-file binding index, scope-correct
> locals, kernel-method tables, and a typed-receiver overlay. Completion is thin
> not because the data is missing but because the **context→candidate wiring was
> built conservatively, one slice at a time** (ADRs 0061–0064), and stopped short.
> This track is mostly *wiring up data that's already enumerable* — and then
> lifting the one structural limit (the clean-file ceiling) that caps the
> type-aware features.

Every gap below was confirmed by reading the engine, not inferred: the
context-dispatch in `bynk-lsp/src/completion.rs::complete` (lines 96–134), the
handler's locals/value-member fallbacks in `bynk-lsp/src/main.rs::completion`
(lines 960–1003), and the advertised `trigger_characters` (line 1515).

## Current state (refreshed — corrects the roadmap)

**Implemented and shipped** (`server_capabilities()`, `main.rs:1506`): live
diagnostics, hover, go-to-definition, **references, rename/prepareRename, code
actions, signature help, code lens, inlay hints, semantic tokens (full + range),
workspace symbols, document highlights, call hierarchy, implementation nav,
document symbols, folding + selection ranges**, and document/range formatting
(wired to `bynk-fmt`). The roadmap's §1 "not implemented" list predates all of
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

After `uses bynk.list` / `bynk.map` / `bynk.string`, the combinators (`map`,
`filter`, `find`, `values`, `getOr`, `join`, …) are never completable, nor are
user-declared top-level `fn`s in scope. All are enumerable from the firstparty
`.bynk` sources + the project parse. **Decision:** add a free-function candidate
producer keyed off the in-scope `uses` set and the current unit's `fn` exports.

### G6 — The clean-file ceiling caps the type-aware features

`value_member_completions` and `locals_completions` both depend on a clean
re-analysis / last-good cached analysis (`main.rs:384`, `:331`). Any parse or type
error silences value-receiver methods, record fields, **and** locals — exactly
mid-edit, when they're wanted most. **Decision:** relax to error-tolerant receiver
typing (type the longest clean prefix / fall back to the last good snapshot per
binding). The largest slice; its own ADR — see below.

## Beyond completion — the desirable-feature survey (VS Code context)

A scan of the standard LSP surface against what `bynk-lsp` advertises, curated for
what's genuinely valuable *for Bynk* (debugger- and color-oriented providers are
omitted as N/A — Bynk has no debugger and no color literals).

**Worth doing (tracked here):**

- **`completionItem/resolve`** — lazy docs/detail on the focused item. Pairs
  naturally with the richer completion above; keeps the initial list cheap.
- **Go-to-type-definition + type hierarchy** — deferred at ADR 0068 pending a
  unit→file map. High value for Bynk's refined/opaque/sum types and the
  `given Cap → provider` relationship; the index already has most of the edges.
- **Document links** — make `uses bynk.list`, `consumes B`, and split-path unit
  references click through to their source. Cheap given the binding index.
- **On-type formatting** — light auto-format on `}`/newline, deferring to
  `bynk-fmt`. Modest, but removes a manual format step.

**Editor polish (roadmap B‑1/B‑2, folded in):**

- Inlay-hint granularity + format-on-save + server-trace **settings**.
- **Snippets** beyond the six completion scaffolds; **scaffolding commands** (new
  context/adapter) — note these overlap the `bynk` driver's `new` (roadmap §5.1),
  which supersedes the CLI path.
- A **problem matcher** so `bynkc` builds surface in the Problems panel.
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
enumerates from the static `bynkc` registries or a recovery parse of the project).
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
   variants (`HttpResult`/`QueueResult`, from the `bynkc::ast` registries), plus
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
   the embedded `bynk.list`/`bynk.map`/`bynk.string` stdlib, now in
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
6. **Navigation round-out.** Three sub-features of very different cost:
   - **6a — go-to-type-definition.** ✅ **Landed.** Value → its type's
     declaration: reads the value's type from the round's `expr_types` (now
     cached in `Analysis`), unwraps single-param containers to a `Named`, and
     returns that `Type` symbol's def site(s). No new index surface, no ADR.
   - **6b — document links** (`uses`/`consumes` → unit source). ✅ **Landed
     ([ADR 0095](../decisions/0095-unit-source-map.md)).** The analysis now
     exposes `unit_sources` (qualified unit name → its project files), built from
     non-synthetic parsed units; `document_link` parses the live buffer for
     `uses`/`consumes` spans and resolves each to the unit's first source file.
     The same map unblocks the deferred consumed-context half of 6a.
   - **6c — type hierarchy.** ❌ **Won't do — not a fit.** `typeHierarchy` is an
     OO-inheritance feature; Bynk has none. Its relationships are refinement over
     a base *builtin* (`type Email = String where …` — `String` is not a navigable
     symbol, so supertypes point at nothing), opaque-over-builtin (same), structural
     records (no hierarchy), and actor-over-actor (`actor Admin = User` — the one
     real type→type case, but actors aren't indexed and are niche). The model
     assumes every node is a navigable symbol; Bynk's main case violates that. The
     navigation needs Bynk *does* have are already served (go-to-definition,
     go-to-type-definition 6a, find-references). **Possible lightweight follow-up
     if a real need surfaces:** "refinement families" — *all refined/opaque types
     over base T* (a base→refinements index + a CodeLens or workspace-symbol
     filter), which is the one genuinely useful query buried in here — delivered
     without the full protocol, the actor indexing, or the ADR.
7. **Editor polish (B‑1/B‑2).** ✅ **Substantially pre-shipped; gaps closed.**
   Most of B‑1/B‑2 already landed with the v0.54 `vscode-bynk` extension —
   settings (`executablePath`/`trace.server`/`inlayHints.enable`/`compilerPath`),
   the 10-scaffold snippet file, `newProject`/`newContext` commands, the
   getting-started walkthrough, the `bynkc` build task + problem matcher,
   semantic-token theming, and the inlay-hints toggle. The genuine remaining gaps:
   **inlay-hint granularity** (per-kind `types`/`parameterNames` toggles filtering
   on the server-tagged `kind`) and a **default-formatter config** (`[bynk]` →
   `bynk.bynk-vscode`, so format-on-save works out of the box). `newAdapter`
   deferred — an adapter scaffold needs a `binding` + a matching `.ts` stub, more
   than mirroring `newContext`.
8. **Editor-agnostic + publishing.** Neovim/Helix/Zed docs; marketplace + Open VSX
   (sequences with Tier 4 release work).
9. **First-party symbol docs.** ✅ **Landed.** Hover and completion-doc resolution
   walked only the project's files, so stdlib/surface symbols (`uses bynk.list`
   combinators, the `bynk` capabilities/types) had no hover at all. A
   `symbols::describe_firstparty_symbol` fallback scans the embedded sources after
   the project scan, surfacing their signature — and any `---` doc block, once the
   sources carry one. No new ADR. (Adding the doc blocks themselves is a separate
   content pass: doc blocks emit as JSDoc, so it re-blesses the first-party emit
   goldens.)

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

1. **`bynk-lsp-spec.md`** — the feature's behaviour, in place, the way the
   normative spec is. (Slice 0 added the §3.15 surface-contract paragraph; slices
   1–4 update §3.15's as-built table as each cell lands.)
2. **This track's *Decision log*** — a dated entry with the slice's ADR link(s)
   and the one-line decision, mirroring the actors track.
3. **This track's *Phase* bullet and the relevant *slice-decomposition* row** —
   marked ✅ with the version, so the doc never overstates what's shipped.
4. **`bynk-tooling-roadmap.md`** — only if the slice changes its A‑/B‑item status
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
  cell is registry-sourced (`bynkc::{keywords, builtin_names, kernel_methods,
  firstparty}` + AST sum-variant tables) and **complete**, enforced by a
  coverage test; the clean-file ceiling is confined to the value-receiver cell
  (D4); `.` is a trigger character; expression position offers values +
  constructors + functions + type names (D3, the G4 call). A doc-ADR (no code,
  no version tag); the spec contract lives at `bynk-lsp-spec.md` §3.15.
- **Slice 1 — completion quick wins, G1–G3 (2026-06-18):** no new ADR (implements
  0093 for the member/static cells). Registered `.` as a completion trigger char
  (D1); completed the builtin-statics table with `List.empty`/`Map.empty`/
  `Effect.pure` (D2/G2); sourced builtin sum-type variants for `HttpResult`/
  `QueueResult` from the `bynkc::ast` `HTTP_VARIANTS`/`QUEUE_VARIANTS` registries
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
  embedded stdlib commons (`bynk.list`/`bynk.map`/`bynk.string`) joined
  `for_each_unit` (harmless to other contexts — fns only — and a free win for
  signature help). Signatures reuse `symbols::type_ref_str` (one renderer, like
  hover/signature help). Coverage: `free_functions_offered_for_own_unit_and_used_
  modules` (registry-driven over `BYNK_LIST_SRC`) + `free_functions_require_a_uses_
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
- **Slice 9 — first-party symbol docs (2026-06-18):** no new ADR. The hover and
  completion-doc paths resolved symbols only through `walk_bynk_files` (project
  files), so first-party symbols (stdlib combinators, the `bynk` surface) had no
  hover at all. Added `symbols::describe_firstparty_symbol`, scanning the embedded
  sources (`BYNK_ADAPTER_SRC`/`CLOUDFLARE_ADAPTER_SRC`/`BYNK_LIST_SRC`/
  `BYNK_MAP_SRC`/`BYNK_STRING_SRC`) via the existing `describe_symbol`, wired as the
  final fallback in both hover and `completion_resolve`. Surfaces their signature
  now and any `---` doc block — the `describe_*` renderers already append
  `documentation`. Coverage:
  `first_party_symbols_describe_their_signature_and_doc`.
- **Slice 9 (content pass) — document the first-party sources (2026-06-18):** the
  follow-up that makes the wiring visible. Added `---` doc blocks to every
  first-party combinator, capability, and surface type (`bynk.list`/`map`/
  `string`, the `bynk` adapter's capabilities + transparent types, `bynk.cloudflare`
  `Kv`). Doc blocks emit as JSDoc, so the first-party emit goldens were re-blessed
  via the existing `BYNK_BLESS=1 … --test e2e` path (468 insertions, 0 deletions —
  pure JSDoc, no semantic change); `tsc_verify` confirms the annotated output still
  type-checks. The docs now surface in hover, completion, *and* the generated
  TypeScript.
- **Slice 6a — go-to-type-definition (2026-06-18):** no new ADR (the value→type
  half ADR 0068 deferred — but, it turns out, only the *consumed-context* half
  needed the unit→file map; value→type needs no new index surface). Cached the
  round's `expr_types` in the LSP `Analysis`; `goto_type_definition` reads the
  value's type at the cursor, `named_type_target` unwraps single-param containers
  (`Option`/`Effect`/`List`/`HttpResult`) to a `Named`, and `type_definitions_named`
  returns that `Type` symbol's def site(s) by bare-name match (cross-unit
  ambiguity → multiple locations, the LSP-conventional resolution). Coverage:
  `type_definitions_named_collects_type_defs_by_bare_name`,
  `named_type_target_unwraps_single_param_containers`, `advertises_type_definition`.
  Document links (6b) and type hierarchy (6c) remain — each earns an ADR.
- **Slice 6b — document links + the unit→source map (2026-06-18):**
  [ADR 0095](../decisions/0095-unit-source-map.md). The analysis (`ProjectAnalysis`
  → `ProjectDiagnostics`) now exposes `unit_sources: HashMap<String, Vec<PathBuf>>`,
  built in one pass over the non-synthetic parsed files on the structurally-
  analysed path (empty on a parse bail). `document_link` parses the live buffer
  for `uses`/`consumes` target spans (`symbols::unit_reference_spans`) and resolves
  each unit to its first source file via the cached map; first-party/unknown units
  yield no link. The map is the shared enabler for the deferred consumed-context
  navigation half of 6a. Coverage: `unit_sources_maps_project_units_excluding_
  synthetic` (bynkc), `unit_reference_spans_finds_uses_and_consumes_targets`,
  `advertises_document_links`. §3.21 added.
- **Slice 6a follow-up — consumed-context go-to-definition (2026-06-18):** no new
  ADR (rides ADR 0095's map). `goto_definition` gained a `unit_reference_definition`
  fallback: a cursor on a `uses`/`consumes` unit name resolves to that unit's first
  source file (after the index/locals paths, before the name-matching path, so a
  unit segment can't be mistaken for a like-named type). Closes the deferred
  consumed-context half of 6a for unit *declarations*; `B.Cap`-in-expression is not
  yet a nav source. §3.21 updated.
- **Slice 7 — editor polish (2026-06-18):** no new ADR; `vscode-bynk` only.
  Investigation found B‑1/B‑2 was **already ~90% shipped** with the v0.54 extension
  (settings, snippets, `newProject`/`newContext`, the walkthrough, the `bynkc`
  problem matcher, semantic-token theming, the inlay-hints toggle). Closed the two
  genuine gaps: **inlay-hint granularity** — `bynk.inlayHints.types` /
  `.parameterNames` settings + a client middleware filter on the server-tagged
  `InlayHintKind`; and a **default-formatter config** — `configurationDefaults`
  maps `[bynk]` to `bynk.bynk-vscode` so the formatter (hence format-on-save) works
  without manual setup. `newAdapter` deferred (adapters need a `binding` + a `.ts`
  stub). Validated: tsc + esbuild + `vsce package` clean.
- **Slice 6c — type hierarchy: closed won't-do (2026-06-18):** `typeHierarchy` is
  an OO-inheritance feature; Bynk has no class inheritance. Its candidate
  relationships are refinement-over-builtin (`Email = String where …`; the base
  isn't a navigable symbol), opaque-over-builtin, structural records (no
  hierarchy), and actor-over-actor (real, but actors aren't indexed and are
  niche). The protocol assumes every node is a navigable symbol — Bynk's main case
  violates that — so the result would be a shallow graph mostly pointing at
  builtins. The navigation needs Bynk has are already served (go-to-definition,
  go-to-type-definition, find-references). Decided in discussion with the user.
  Recorded follow-up *if a real need appears:* "refinement families" (a
  base→refinements index surfaced via CodeLens / workspace-symbol filter) — the
  one useful query here, without the protocol or actor indexing.

- `bynk-tooling-roadmap.md` — the parent roadmap; this track refreshes its §1–§2
  status and owns the completion + B‑1/B‑2 remainder.
- `bynk-lsp-spec.md` — the authoritative LSP feature spec; updated in place as
  slices land, the way the normative spec is.
- ADR [0052](../decisions/0052-lsp-project-diagnostics.md) (diagnostics),
  [0053](../decisions/0053-lsp-binding-index.md) (binding index),
  [0061](../decisions/0061-completion-sliced-positional-first.md)–0064 (the
  completion slices this track continues),
  [0068](../decisions/0068-implementation-navigation.md) (the deferred type-nav).
