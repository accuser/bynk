# Karn Book — reorganisation proposal (concern-first)

- **Status:** Draft (proposal). Merging is approval to build. *Reader-facing
  structural change to `docs/`; no language-surface change.*
- **Relates to:** ADR [`0060-named-concern-modules`](decisions/0060-named-concern-modules.md)
  (the same "split by named single concern" move, applied to the book);
  complements [`karn-refactor-proposal-queue.md`](karn-refactor-proposal-queue.md)
  (compiler internal quality) and
  [`karn-tooling-proposal-queue.md`](karn-tooling-proposal-queue.md) (features).
  This is the third axis: documentation structure.
- **Scope:** `docs/src/**`, `docs/src/SUMMARY.md`, the authoring guide
  (`docs/src/contributing/documentation.md`), and the doc-version sites listed
  under *Workstream 0*. No `karnc`/grammar change beyond the generated-page test
  paths, which this proposal deliberately avoids touching (see *Tooling
  constraints*).

---

## 1. Context

The book is organised **mode-first** at the top level: five user-facing
sections by Diátaxis kind — Tutorials, How-to, Reference, Explanation, Spec —
plus two audience sections (Contributing, Tooling). The "one mode per page"
discipline and the four-voices guidance are well executed and worth preserving.
The **top-level division by mode is the friction.**

A reader with a single concern must hop across sections. For "understand and use
refined types" that is seven pages in five places: `tutorials/04-refined-types`,
`how-to/refined-types/*`, `how-to/pattern-matching/*`, `reference/types` +
`reference/refined-types`, `explanation/type-system-philosophy` +
`refined-literal-admission`, and `spec/type-system`. The concern is coherent; the
book scatters it.

The instinct to group by concern is **already present and fighting the layout**:
`how-to/index.md` re-groups its own pages into "Types & values / Services & state
/ Project & tooling", and `reference/index.md` into "Language / Project & output
/ Generated". The sections self-organise by concern internally; only the top
level resists.

This is also an identity argument. Karn is *architecture-first* — the shape of a
program is in the language. A book whose shape mirrors the language's concerns is
more in the spirit of the project than one shaped by a generic documentation
framework. ADR 0060 made the same call for the compiler source (split sprawling
files into named single-concern modules for legibility); this proposal applies it
to the book.

### Selected option

Of the three options considered — (A) keep mode-first, clean up only;
(B) fully concern-first; (C) hybrid — **C is adopted.**

The principle behind C: Diátaxis's four modes divide into two families.
*Journey* surfaces — tutorial, how-to, explanation — are read while learning,
doing, or trying to understand a concern, and read best **grouped by concern**.
*Lookup* surfaces — reference and the normative spec — are read while confirming
an exact rule, and read best as **complete, uniform catalogues**. C reorganises
the journey surfaces by concern and leaves the lookup surfaces whole. This also
happens to be the low-risk path through the doc tooling (see *Tooling
constraints*): the generated reference pages and the citable spec do not move.

Diátaxis is honoured throughout — "one mode per page" is unchanged, and in fact
becomes *more* load-bearing once modes sit side by side within a concern.

---

## 2. Decision — the target shape

Top-level navigation (`SUMMARY.md`):

1. **Introduction** — what Karn is, coming from TypeScript, install, and a
   rewritten *how this book is organised* (it currently describes the mode-first
   layout and must be re-authored to describe the concern-first one).
2. **Learn Karn** — the guided spine: the existing linear tutorials 01–06, kept
   as one guaranteed end-to-end path. Each tutorial cross-links into the relevant
   concern for depth.
3. **Guides** (the main body) — one section per **concern**, each co-locating its
   explanation page(s) and how-to(s), with a short landing page and "see also:
   reference / spec" links. Proposed concerns:
   - **The type system**
   - **Program structure** (commons, contexts, `uses`, `consumes`, `exports`)
   - **Effects & capabilities** (incl. providers and adapters)
   - **Agents & state**
   - **Entry points** (HTTP, cron, queue)
   - **Testing**
   - **Projects, build & deployment**
   - **Editor & tooling**
4. **Reference** — kept whole, dry, scannable, in its current location
   (`reference/`), including the four generated pages.
5. **Specification** — kept whole and normative, unchanged (`spec/`).
6. **About Karn** (background) — book-level explanations that belong to no single
   concern: *Why Karn exists*, *Karn compared to TypeScript*, *Versioning &
   roadmap*. See `[DECISION] background-home`.
7. **Troubleshooting** — see `[DECISION] troubleshooting-placement`.
8. **Contributing** — unchanged.
9. **Tooling reference** (`karn-fmt`, `karn-lsp`, `tree-sitter-karn`,
   `vscode-karn`) — see `[DECISION] tooling-ref-home`.

---

## 3. Page move map (journey surfaces only)

Reference, spec, and contributing pages **do not move**. The following relocate
from mode-first folders into concern folders under `guides/`. Paths are
indicative; final slugs settle during implementation.

| Current path | → Concern | Mode |
|---|---|---|
| `explanation/type-system-philosophy.md` | The type system | explanation |
| `explanation/refined-literal-admission.md` | The type system | explanation |
| `how-to/refined-types/define-and-validate.md` | The type system | how-to |
| `how-to/refined-types/literal-admission.md` | The type system | how-to |
| `how-to/types/define-types.md` | The type system | how-to |
| `how-to/types/result-and-optionals.md` | The type system | how-to |
| `how-to/pattern-matching/match.md` | The type system | how-to |
| `how-to/pattern-matching/narrow-with-is.md` | The type system | how-to |
| `explanation/how-a-karn-program-is-shaped.md` | Program structure | explanation |
| `how-to/types/consumes.md` | Program structure | how-to |
| `how-to/capabilities/compose-a-provider.md` | Effects & capabilities | how-to |
| `how-to/capabilities/share-across-contexts.md` | Effects & capabilities | how-to |
| `how-to/adapters/wrap-a-library.md` | Effects & capabilities | how-to |
| `explanation/the-agent-model.md` | Agents & state | explanation |
| `how-to/agents/stateful-agent.md` | Agents & state | how-to |
| `how-to/agents/state-machine.md` | Agents & state | how-to |
| `how-to/http/handle-request.md` | Entry points | how-to |
| `how-to/cron/handle-cron-trigger.md` | Entry points | how-to |
| `how-to/queue/handle-queue-message.md` | Entry points | how-to |
| `explanation/testing-philosophy.md` | Testing | explanation |
| `how-to/testing/write-tests.md` | Testing | how-to |
| `how-to/testing/integration.md` | Testing | how-to |
| `how-to/projects/layout.md` | Projects, build & deployment | how-to |
| `how-to/projects/cloudflare-workers.md` | Projects, build & deployment | how-to |
| `explanation/why-compile-to-typescript.md` | Projects, build & deployment | explanation |
| `how-to/tooling/format.md` | Editor & tooling | how-to |
| `how-to/tooling/editor-support.md` | Editor & tooling | how-to |

**Stay book-level / background:** `explanation/why-karn-exists.md`,
`explanation/karn-compared-to-typescript.md`,
`explanation/versioning-and-roadmap.md`.

**Stay put (lookup + audience):** everything under `reference/`, `spec/`,
`contributing/`, and the four `tooling/*` reference pages (pending
`[DECISION] tooling-ref-home`).

Gap noticed during the survey: **Effects & capabilities has how-tos but no
explanation page** — the closest conceptual material is folded into
`how-a-karn-program-is-shaped`. Worth a dedicated explanation page once the
concern exists (not blocking).

---

## 4. Open decisions (settle at review)

**[DECISION] background-home — where do book-level explanations live?**
*Rec:* a small top-level **"About Karn"** section holding *Why Karn exists*,
*Karn compared to TypeScript*, and *Versioning & roadmap*. These argue
positions about the whole language and don't belong to one concern. Alternative:
fold *Why Karn exists* / *compared to TypeScript* into the Introduction and keep
*Versioning & roadmap* near the changelog.

**[DECISION] troubleshooting-placement — one section or distributed?**
The 11 diagnostic pages are lookup-flavoured. *Rec:* **keep a single
Troubleshooting section** (and the diagnostic index in Reference), and add "If
this fails…" cross-links from each concern. Distributing them per concern raises
churn and splits a surface readers scan as a whole. Alternative: distribute, with
a redirect index.

**[DECISION] tutorials-per-concern — split the linear spine, or keep it?**
You framed concerns as "supported by their own tutorials". The existing
tutorials are a *single guaranteed end-to-end path* — splitting them per concern
loses the "works start to finish" property that makes a tutorial a tutorial.
*Rec:* **keep the linear "Learn Karn" spine** as the single guaranteed track, and
allow **concern-local mini-tutorials** later where a concern genuinely warrants
its own guided lesson. Treat per-concern tutorials as additive, not as a
fragmentation of the spine.

**[DECISION] tooling-ref-home — fold tooling reference into the concern?**
*Rec:* **keep `tooling/*` as a distinct audience section** (it serves tool users,
some of whom don't write Karn) but link it prominently from the *Editor &
tooling* concern. Alternative: fold the four pages into *Editor & tooling* and
drop the separate section.

**[DECISION] reference-internal-grouping — leave Reference flat or concern-group it?**
*Rec:* **leave Reference whole and as-is.** It already self-groups on its index
page, and the four generated pages have output paths hard-coded in `karnc` tests
(see below) — moving them is cost without reader benefit, since Reference is a
lookup catalogue.

---

## 5. Tooling constraints (what the migration must respect)

The doc system is heavily guarded; most guards *help* the migration, one couples
to file paths.

- **Doc-example gate** (`karnc/tests/doc_examples.rs`) globs `docs/src/**`, so it
  is **path-independent** — moving pages does not break it.
- **Link checking** (`mdbook-linkcheck`) fails the build on any broken internal
  link, so the reorg **cannot silently rot links**: rewrite `SUMMARY.md`, then
  chase linkcheck failures to fix every relative link.
- **Generated reference pages — the one hard coupling.**
  `reference/{diagnostics,keywords,cli,grammar}.md` have their paths hard-coded
  in Rust test sources (`diagnostics_registry.rs:134`, `keywords_reference.rs:60`,
  `cli_reference.rs`, `grammar_reference.rs`, and `grammar_coverage.rs:34` which
  also asserts `grammar.md`'s `{#rule-*}` anchor coverage). **Leaving Reference in
  place (per Option C) avoids editing compiler test sources entirely.** This is a
  concrete argument for C over a full concern-first split.
- **Glossary first-use linking** (`#term-*` anchors) and the **British-English
  lint** are page-content concerns, unaffected by structure.
- **No native redirects.** mdBook has no built-in redirect map for moved pages;
  external inbound links (and reader muscle-memory) will break. Mitigation: a
  `[output.html.redirect]` table in `book.toml` (mdBook supports a redirect map)
  mapping old paths to new — worth adding as part of the move.

---

## 6. Workstream 0 — version single-source (do now, independent of structure)

The book hard-codes its version in several hand-maintained places and they
**disagree**: the introduction banner, `tooling/index.md`, and
`versioning-and-roadmap.md` say **v0.20**; `tooling/karn-lsp.md` says **v0.25**;
`spec/index.md` says **v0.26**; the changelog runs to **v0.31.2** (the current
release). The bump script (`scripts/bump-version.sh`) updates only the Cargo/npm
manifests and lockfiles — **it does not touch `docs/`** — which is why the drift
exists.

This is orthogonal to the reorg and should be fixed first; the v0.30 and v0.31
releases since this was drafted have only widened the gap (none updated these
pages). Options:

- **Single source via a build variable.** Define the version once (e.g. an
  mdBook preprocessor substitution or a generated include) and reference it from
  the banner/spec/tooling pages, so there is one place to change.
- **Extend `bump-version.sh`** to also rewrite the doc version sites, and add a
  CI check that all doc version strings agree (mirroring the release workflow's
  `verify` job for the manifests).

*Rec:* the build-variable approach — it removes the strings entirely rather than
keeping them in sync. Either way, a CI equality check is cheap insurance.

The stale **"compiler written in Go"** claim is **not** in `docs/src` (only in
`design/karn-design-notes.md`, already flagged as drifted) — no book change
needed there.

---

## 7. Migration phases

Sequenced so the project can pause between phases (you can run these after the
code reorg lands):

0. **Version single-source + consistency sweep** (Workstream 0). Independent;
   do immediately (the drift is already six-way as of v0.31.2).
1. **Lock the concern taxonomy** — resolve the `[DECISION]`s above. (This
   proposal's merge.)
2. **Re-home journey pages** — move tutorials' cross-links, how-to, and
   explanation pages into `guides/<concern>/`; leave reference, spec, and
   contributing untouched. Add a redirect map in `book.toml`.
3. **Rewrite `SUMMARY.md`**, then iterate against `mdbook-linkcheck` until the
   build is green (this is how every stray relative link gets caught).
4. **Re-author the framing pages** — `introduction/how-these-docs-are-organised.md`
   (currently describes the mode-first layout) and the authoring guide
   `contributing/documentation.md` (its "Style / one mode per page / the four
   voices" guidance stays, but its description of the *layout* changes to
   concern-first).
5. **Add per-concern landing pages** and tighten the "Learn Karn" spine's
   cross-links into the concerns.

---

## 8. Risks

- **Link churn.** Largest mechanical risk; fully covered by linkcheck (a broken
  link fails the build) plus the redirect map for external inbound links.
- **Losing the guaranteed linear tutorial.** Mitigated by keeping the "Learn
  Karn" spine intact (`[DECISION] tutorials-per-concern`).
- **Reference fragmentation / generated-page coupling.** Avoided by design —
  Reference and Spec stay whole and in place.
- **"One mode per page" erosion.** Co-locating modes within a concern makes it
  easier to blur them on a page. The four-voices section of the authoring guide
  becomes more important, not less — keep and foreground it.
- **Partial-migration limbo.** The phases are designed to leave the book
  buildable and coherent at each boundary, so a pause between phases is safe.

---

## 9. Definition of done

- `SUMMARY.md` reflects the concern-first shape; `mdbook build docs` is green
  (highlighting, grammar includes, linkcheck, British-English lint all pass).
- Every moved page keeps a single Diátaxis mode; concern landing pages link out
  to Reference and Spec rather than duplicating them.
- `book.toml` carries redirects for moved paths.
- `introduction/how-these-docs-are-organised.md` and
  `contributing/documentation.md` describe the new structure.
- Doc version strings are single-sourced (Workstream 0) and agree; a CI check
  guards them.
- No `karnc` test path edits were required (Reference/Spec unmoved) — confirms
  the migration stayed within Option C's risk envelope.
