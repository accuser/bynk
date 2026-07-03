# Editor currency — hover, completion & menus in step with the language

Persistent design doc for a **multi-slice** effort to close the drift between what
the Bynk language *is* and what the editor surface (`bynk-lsp` + `vscode-bynk`)
*shows*. The surface is broad but shallow: it advertises the full LSP capability
set, yet several responses render a v0.25-era subset of a language now at v0.119.
This doc is the living map the per-slice proposals are cut from. It **settles
direction, not build** — each slice is still an ordinary `vX.Y-<slug>.md` proposal
that cites this doc and the foundational ADRs.

- **Status:** Draft — settling. This is a **tooling track** (cf. the retired
  `lsp.md` and `crate-decomposition.md`), admitted under
  [ADR 0076](../decisions/0076-feature-track-posture.md) on the "spans several
  increments" + "surface not yet settled" limbs. Two load-bearing ADRs land up
  front (see [ADRs up front](#adrs-up-front)); remaining ADRs are numbered
  per-slice at authoring time.
- **Sharpens:** the LSP feature spec ([`../bynk-lsp-spec.md`](../bynk-lsp-spec.md),
  §3 LSP capabilities — §3.3 hover, §3.15 completion) and the editor/tooling guide
  (`site/src/content/docs/book/guides/editor-and-tooling/`). Extends the retired
  `lsp.md` plan — the completion-surface contract
  ([0093](../decisions/0093-completion-surface-contract.md)), its error-tolerant
  receiver-typing ceiling lift ([0094](../decisions/0094-error-tolerant-receiver-typing.md)),
  and the unit-source map ([0095](../decisions/0095-unit-source-map.md)) — from
  *breadth* to *depth-and-freshness*.
- **Posture:** catch-up + guardrail. We refresh the stale responses **and** install
  a mechanism so the next language slice cannot silently re-open the gap. Migration
  is nil — this is additive over an existing surface.

## The thesis

The editor is a *projection of the language*. Every construct the compiler
understands should be legible in hover, offerable in completion, and reachable
through the UI. Today three of those four projections have fallen behind the
language, and one never existed:

- **Hover** renders only top-level declarations, by name, from a lossy formatter
  (`bynk-lsp/src/symbols.rs::describe_symbol`). It says nothing about parameters or
  locals, drops a refined type's `where` predicate, and knows nothing of the
  testing track's contracts, step invariants, or history properties (v0.115–v0.119).
- **Completion** is structurally complete (the ADR 0093 matrix holds) but ships a
  scaffold that **no longer compiles** — the `test "…" { }` snippet
  (`completion.rs::SNIPPETS`), a keyword the grammar retired — and has no scaffold
  for `suite`/`case`/`property`/`expect`, `actor`, contracts, or refined/opaque types.
- **Menus** do not exist: `vscode-bynk/package.json` contributes **no `menus` block
  and no `keybindings`**, so all eight commands are Command-Palette-only and appear
  in every workspace, Bynk or not.
- **Codelens** is the healthiest — reference counts (`index_queries.rs::code_lenses`)
  and a test-run lens (`vscode-bynk/src/testCodeLens.ts`) — but narrow.

The one-sentence redesign:

> **The editor surface is rendered from the checker's own tables, and every
> language slice carries a tooling-delta obligation — mechanically checked where a
> construct is enumerable, backstopped by a proposal-template checklist where it
> is not.**

This moves currency off the honour system *as far as the enumerable constructs
reach*. Keywords, semantic-token types, and shipped scaffolds are finite, listable
sets: CI can assert every one of them has a completion doc, a hover path, a token
mapping, and compiles. The open-ended part — that a *new shape* of hover content is
warranted (say, rendering a contract clause well) — is judgement, and stays a
checklist obligation. The claim is therefore bounded, not absolute: the drift that
*can* be counted is caught by a test; the drift that must be *designed for* is
caught by a gate a reviewer applies. The documentation track drew the same line for
snippets (its verification harness makes *"the Book documents what compiles today"*
mechanically true; what the Book should *say* stays editorial).

## The root cause — a missing gate

This drift is not a series of oversights; it is a structural gap in the increment
recipe. The tooling roadmap already predicted it: *"each language increment's
tooling delta must explicitly enumerate LSP (completion, hover, semantic tokens for
the new constructs), not just tree-sitter and fmt"*
(`design/bynk-tooling-roadmap.md`). The testing track (v0.115–v0.119) landed
`requires`/`ensures`, `transition`, `expect`/`trace`, the tier dial and history
properties with a full grammar, checker, fmt and tree-sitter delta — but no hover
or completion delta. The proposal template's "tooling deltas" line
(`design/proposals/README.md`) is too soft to catch it: it does not name the LSP
responses, and nothing fails when they are skipped.

So the track's spine is a **guardrail plus a catch-up**. Fix today's arrears, then
make the *countable* arrears un-accruable (a failing test) and the rest hard to
skip (a named checklist) — the split ADR-1 draws explicit.

## Internal architecture — where the surface lives

The refresh touches four seams, three of them already holding the data the stale
responses omit:

- **Hover / symbol rendering** — `bynk-lsp/src/main.rs::hover` (both the
  binding-index path and the lexical fallback) → `symbols.rs::describe_symbol` /
  `describe_*`. The formatters re-parse the defining file and render a summary.
- **Locals & types (already captured, under-used)** —
  `bynk-ide::diagnose_project` already returns `locals` (`LocalBinding` with an
  inferred `ty` and scope), `expr_types`, `hints`, and `requirements`
  (`bynk-ide/src/lib.rs`). `bynk-lsp/src/locals_nav.rs` already resolves the binding
  under the cursor positionally for go-to-definition / references / highlight, and
  semantic tokens already colour locals. **Hover is the only navigation-class
  response that ignores this table.** Parameter/variable hover is therefore mostly
  a wiring job, not new analysis.
- **Completion** — `bynk-lsp/src/completion.rs` (the ADR 0093 context matrix) plus
  its `SNIPPETS` const, and — separately — the VS Code static scaffolds in
  `vscode-bynk/snippets/bynk.json`. These are **two largely-disjoint sets**, not
  mirrors: `SNIPPETS` has 6 entries (`context`, `adapter`, `capability`, `service`,
  `on call`, and the stale `test`); `bynk.json` has 11 (`context`, `commons`, `type
  record`, `type enum`, `fn`, `capability`, `provides`, `service`, the http/cron
  handlers, `agent`), sharing only `context`/`capability`/`service`. `bynk.json` is
  in fact *ahead* on current constructs (it carries `provides`/`agent`/`type`), while
  `SNIPPETS` is the one shipping a scaffold the grammar retired. Keyword *names*
  already track the language (they read the live `bynk-syntax::keywords::KEYWORDS`
  registry); scaffolds and non-keyword contexts do not.
- **Manifest (VS Code)** — `vscode-bynk/package.json` `contributes.*`. Commands are
  registered in `extension.ts`; only the manifest wiring is missing.

The design principle across all four: **render from the analysis tables, not from a
re-parse-and-guess**, so hover, signature help and completion-doc cannot diverge
(they already share `symbols::type_ref_str`; this generalises that discipline).

## ADRs up front

Two decisions are load-bearing and hard to reverse; they land before slicing (per
the track lifecycle). Numbers assigned at authoring — 0156/0157 are the next free.

1. **The editor surface tracks the language — with a mechanical floor.** Hover and
   completion render from the checker's captured tables (single source, so they
   cannot diverge from each other or signature help). The gate has two teeth:
   - a **mechanical coverage test** over the enumerable construct sets — every
     lowercase-initial `KEYWORDS` entry must have a completion doc *and* a hover
     path, and every construct that carries a dedicated semantic-token type/modifier
     must have a legend mapping. This is the tooth that would have caught the actual
     drift: the testing-track keywords (`requires`/`ensures`/`transition`/`expect`)
     landing with no hover or completion-doc coverage becomes a failing test, not a
     silent gap. The check keys off the same `KEYWORDS` registry and semantic-token
     legend the code already owns, so it is a pure cross-table assertion.
   - a **proposal-template checklist** naming the LSP responses a language slice
     must consider (hover, completion, semantic tokens, signature help), for the
     *non-enumerable* residue — a genuinely new *shape* of content that no coverage
     test can demand. Silence is an oversight, mirroring the existing docs-delta rule.

   The checklist alone is an honour-system gate (Risks acknowledges slices route
   around process gates); the coverage test is what makes the load-bearing claim
   real for everything countable.
2. **Scaffolds cannot drift.** Every scaffold the tooling ships — the LSP `SNIPPETS`
   *and* `vscode-bynk/snippets/bynk.json`, treated as two independent sets — is
   **lexed and parsed against the current grammar in CI** (placeholders substituted
   to a compilable skeleton first), the same posture as the docs track's
   snippet-verification harness and the existing `keywords_reference` drift test. A
   scaffold that no longer compiles fails the build. This is a per-set *compiles*
   assertion, **not** a set-equality/parity check across the two — the consumers keep
   distinct catalogues and insert-text dialects (see DECISION A). (This test would
   have caught the `test` snippet.)

Neither ADR needs to reserve a number range; both are small and enable every slice
below.

## Slice decomposition

Ordered by value-to-effort; each lands as its own proposal. Mostly independent,
with two acknowledged couplings: slices 2 and 3 share the **name-receiver
resolution** (capability-op hover in slice 2 reuses what slice 3's member
completion performs — land that resolution in whichever ships first), and slice 4's
"de-staled" guarantee **assumes slice 0's drift test exists** to enforce it. Neither
forces a strict order beyond 0-before-4; they just share machinery.

### Slice 0 — the guardrail + the first casualty

Land the two ADRs. Ship the mechanical checks: the **keyword/token coverage test**
(ADR-1) and the **scaffold-compiles test** (ADR-2), plus the proposal-template
tooling-delta line. Delete the stale `test` snippet from `SNIPPETS`; the compile
test now proves the remaining scaffolds parse. As part of standing up the coverage
test, **audit the current arrears the guardrail will demand of others**: confirm
whether the testing-track constructs already have semantic-token mappings and
signature-help coverage, and record the finding (see DECISION D) — it would be
incoherent for the gate to require of future slices what this track left unverified.
*(No user-visible feature beyond removing a broken scaffold; this is the
foundation.)* **Size is conditional on the audit:** if the testing-track constructs
are already tokenised, slice 0 is small — delete one snippet, add two tests, amend
the template. If the audit finds token/signature-help arrears, the coverage test
cannot go green until the legend is fixed, so slice 0 absorbs that fix and grows
accordingly (or the proposal splits the legend catch-up into a fast-follow that
lands before the coverage test is switched to enforcing). Whoever writes the slice-0
proposal should scope against the audit result first.

### Slice 1 — parameter & local hover

Wire the existing `locals_nav` resolution + `LocalBinding.ty` (and `expr_types` for
sub-expressions) into the `hover` handler, before the lexical fallback. Hovering a
`let` binding, a parameter, or `self` yields `let x: <ty>` / `param x: <ty>` /
`self: <Agent>`. Reuses shipped machinery; closes the single clearest regression.
*(Directly answers the "hover of parameters and variables" gap.)*

### Slice 2 — hover depth for declarations

Refresh `describe_*` to render what the language now means:

- **types** — record fields, sum variants, the refined `where` predicate, the
  opaque base (today all collapse to `type X = record`/`sum`/`Int`);
- **functions** — `requires`/`ensures` contracts and capability requirements
  (`given`/effects), not just params + return;
- **services / agents** — routes & protocols; the store-field list plus
  `invariant` / `transition` step invariants;
- **keywords** — surface the one-line `KEYWORDS` doc on hover (completion already
  uses it; hover does not).

### Slice 3 — completion depth

Fill the non-keyword contexts: record **field-name** completion on construction
(`Order { <cursor>`), `match`/`is` **pattern** completion, and the header/clause
positions — `on` (handler kinds), `from` (http/cron/queue), `by` (actors),
`exports`, `provides Cap = <cursor>`. Inside `requires`/`ensures`, offer the
parameters (and `result`). *(Answers "completion is partial".)*

### Slice 4 — scaffold refresh

Add the missing scaffolds, de-staled against the current grammar — the testing
track (`suite`/`case`/`property`/`expect`), `actor`, `invariant`/`transition`,
`requires`/`ensures`, refined/opaque `type`, and `uses`/`consumes`/`given`. This is
an **asymmetric** fill, not a symmetric add: the two catalogues start from different
places (`bynk.json` already carries `provides`/`agent`/`type record`/`type enum`/`fn`
that `SNIPPETS` lacks; `SNIPPETS` carries `adapter`/`on call`), so the slice adds
each construct to whichever set lacks it rather than duplicating a single list. The
slice-0 compile test guards both sets independently. Supersedes and corrects
[#307](https://github.com/accuser/bynk/issues/307), whose own suggested
`test "…" { assert … }` scaffold predates the testing track and is itself stale.

### Slice 5 — the UI surface (menus, keybindings, editor config)

Manifest-only. Add `contributes.menus` with `when`-scoping and `keybindings`
([#305](https://github.com/accuser/bynk/issues/305)): `commandPalette` gated on
`editorLangId == bynk || workspaceContains:bynk.toml`; `editor/context` and
`editor/title/run` run/debug buttons; `explorer/context` for New Context / Open
`bynk.toml`; Run/Debug Tests shortcuts. Fold in the `language-configuration.json`
enrichment ([#306](https://github.com/accuser/bynk/issues/306): `onEnterRules`,
`wordPattern`). *(Answers "menu integration".)* Pre-Marketplace polish
([#258](https://github.com/accuser/bynk/issues/258)).

### Slice 6 — codelens depth (optional tail)

Give the test-run lens a **per-case filter** so `▷ Run Test` runs the case under it,
not the whole project (`testCodeLens.ts` calls this out today). Add a
provider/implementation-count lens on capabilities — the flat "what refines/provides
T" query that [#259](https://github.com/accuser/bynk/issues/259) parked as icebox.

## Open decisions — settle dispositions

- **DECISION A — two scaffold catalogues, not one.** The LSP `SNIPPETS` and VS
  Code's `bynk.json` are largely disjoint (sharing only `context`/`capability`/
  `service`) and serve different consumers with different insert-text dialects.
  *Disposition:* keep them as **two independent sets**. Slice 0's test asserts that
  **each set compiles**, not that the sets are equal — there is deliberately no
  parity/set-equality requirement, since forcing symmetry would churn both consumers
  for no user benefit. Slice 4 fills each set's own gaps. Revisit only if the
  duplication starts causing real divergence in what the two *cover*.
- **DECISION B — hover for capability ops at call sites.** Should hovering `Clock.now`
  in an expression show the op signature? *Disposition:* yes, folded into slice 2 via
  the name-receiver resolution completion already performs; no new index.
- **DECISION C — how much of the checker's inferred type to show.** Full normalised
  `Ty` vs the surface `type_ref_str` form. *Disposition:* surface form, matching
  inlay hints and signature help, for one rendering across all three.
- **DECISION D — semantic tokens & signature help: arrears or already current?**
  ADR-1's checklist names four responses, but the catch-up slices only cover hover
  (1–2) and completion (3–4). Whether the testing-track constructs are *already*
  semantically tokenised and reachable by signature help is a **factual question
  slice 0 must answer**, since the coverage test needs the answer to be "yes" to
  pass. *Disposition (pending slice-0 audit):* if they are current, record it as the
  gate's baseline and no slice is owed; if not, the shortfall is folded into slice 0
  (token legend) or a short slice 2b (signature help), so this track closes the
  arrears it asks future slices to avoid. The gate must not demand of others what the
  track leaves unverified.

## Risks

- **Re-parse cost on hover.** `describe_symbol` re-parses the defining file per
  request. Slices 1–2 should prefer the retained analysis snapshot/tables over a
  fresh parse where the cursor's file is the analysed one (the cache already exists).
- **Guardrail friction.** The tooling-delta line must stay *lightweight* — a
  checklist, not a second proposal — or slices route around it. It names the
  responses to consider; it does not demand every response change.
- **Snippet test brittleness.** Scaffolds contain `${1:…}` tab stops; the drift test
  must strip placeholders to a compilable skeleton before parsing (a fixed
  substitution), not parse the raw snippet text.

## Issue map

| Slice | Closes / advances |
|---|---|
| 0 | tooling-delta gate; kills the stale `test` snippet |
| 1 | parameter/variable hover (the named gap) |
| 2 | hover currency for the testing track & refined types |
| 3 | [#307](https://github.com/accuser/bynk/issues/307) (completion half) |
| 4 | [#307](https://github.com/accuser/bynk/issues/307) (snippet half, de-staled) |
| 5 | [#305](https://github.com/accuser/bynk/issues/305), [#306](https://github.com/accuser/bynk/issues/306) |
| 6 | [#259](https://github.com/accuser/bynk/issues/259) |

Adjacent, **not** in scope (LSP depth of a different kind — keep as standalone
proposals): [#302](https://github.com/accuser/bynk/issues/302) (file-operation
rename awareness), [#303](https://github.com/accuser/bynk/issues/303) (extract
refactorings), [#304](https://github.com/accuser/bynk/issues/304) (call-hierarchy
callees), and [#397](https://github.com/accuser/bynk/issues/397) (the same hover/
completion surface for the in-browser LSP — a consumer of slices 1–3, not a driver).
