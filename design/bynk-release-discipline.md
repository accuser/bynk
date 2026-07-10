# Bynk — Release Discipline: Milestone Cadence & Doc-Truth Guards

*Decision record for [#540](https://github.com/accuser/bynk/issues/540) §7(4), the
two bullets left after the [1.0 definition](bynk-1.0-definition.md): **monthly
milestone cadence** and **README/about drift guards**. Fourth of the strategy
records. A strategy record, not a language-defining call. With this, §7(4) is
closed.*

---

## Why these two go together

The [1.0 definition](bynk-1.0-definition.md) makes a **stability promise**. A
promise is only worth what the discipline behind it can *keep* and *prove*:

- **Keep it** — batch the daily breaking increments into a form an outside
  upgrader can actually follow, with the migration written down (the *cadence*).
- **Prove it** — never let the docs claim something the compiler does not do,
  because *for a spec-first project, doc truth is the brand* (the *drift guards*).

Both are the release-discipline layer under the 1.0 definition: the machinery that
turns "Foundations is stable" from an intention into something a user can rely on
and verify.

## Part 1 — Monthly milestone cadence

### The bind

Bynk ships **daily breaking increments**, each cutting its own version
(`v0.142`, `v0.143`, …) with themed changelog sections and a per-increment ADR +
spec delta. That granularity is right for *building* the language and fine while
the only consumer is this repo's own corpus. But it is unusable as an **upgrade
path** for anyone outside: no one can be asked to read thirty daily changelog
entries and thread thirty separate migrations to move up a month.

### The decision

**Batch the daily increments into named monthly milestones.** A milestone is a
**rollup** over that month's increment-versions — the per-increment version, ADR,
and spec discipline underneath is unchanged; the milestone sits on top of it and
carries the two things an upgrader needs:

1. **A cumulative migration note** — the *net* delta to move from the previous
   milestone to this one, written as one coherent upgrade, not the day-by-day
   play-by-play. If a surface was added and then refined across three increments,
   the milestone note describes the surface as it lands, once.
2. **Codemods, post-1.0** — the automation for the mechanical part of a breaking
   change. [ADR 0123](decisions/0123-state-block-cutover-and-codemod.md) is the
   per-cutover template: it settled that *pre-1.0*, with the corpus the only Bynk
   source in existence, a shipped codemod would "parse a retired surface forever
   for no caller," so migration is a one-time in-repo hand rewrite. *Post-1.0*,
   when external code exists, that calculus flips: a Foundations-affecting change
   (rare, by the 1.0 definition) is expected to ship a codemod, and the milestone
   bundles it.

### Why monthly, and what it changes

Monthly is the grain that keeps the migration note **accurate** (a month's changes
are still small enough to describe honestly) while being **coarse enough** that an
outside upgrader reads *milestones*, not increments. It changes nothing about how
increments are built, versioned, or reviewed — the ADR-per-call and
spec-updated-in-place discipline stands. It adds one artefact: the monthly
milestone rollup with its cumulative migration note (and, post-1.0, codemods).

This is exactly the cadence the [1.0 definition](bynk-1.0-definition.md) leans on
— "named milestones with cumulative migration notes" — and it is what keeps the
1.0 stability promise *legible*: a 1.x user upgrades milestone to milestone, reads
one migration note, and (for anything mechanical) runs one codemod.

## Part 2 — Doc-truth drift guards

### The principle

For a spec-first project the documentation **is** the contract: the normative spec
defines the language, and the README and about pages are the first promise a
prospective user reads. A doc that lies — a front-page example that no longer
compiles, a feature blurb for a surface that was retired — is not a cosmetic bug,
it is a **broken promise on the most visible surface there is**. So doc truth is
guarded by CI like any other invariant.

### What is already guarded

The drift-guard pattern is well established, and the specific README/about failures
the review named are **already closed in code**:

- **`doc_examples`** compiles every `` ```bynk `` block in the Book, the Developer
  Documentation, the landing page, **and the root `README.md`** — the last added
  after the front-page showcase "had drifted through three syntax revisions before
  this gate covered it" (`bynkc/tests/doc_examples.rs`). The about pages live under
  `book/` and are covered by the same gate.
- **`doc_diagnostics`** checks that quoted diagnostic output matches the compiler;
  **`doc_version`** that version references do not drift; **`grammar_reference`**
  that the EBNF matches the tree-sitter grammar; **`decisions_index`** that the ADR
  index is complete by construction; **`legend_drift`** that the LSP legend and the
  VS Code extension agree; plus the sidebar drift guard in CI.

So the review's concrete complaints — a front-page example that did not compile,
and a README advertising a retired testing surface — have both been addressed. The
open work is the **policy**, not a missing example gate.

### The standing rule (the decision)

1. **Every high-visibility surface's compilable claims are CI-gated.** README,
   landing page, and about pages are in the `doc_examples` gate and stay in; any
   new front-facing surface that ships a `` ```bynk `` example joins the gate as it
   appears. A code fence on a front-door page is a promise, and promises are
   compiled.
2. **The per-increment doc-delta discipline extends to README + about prose.** An
   increment that retires, renames, or changes a user-facing surface updates the
   README and about pages **in the same PR**, the way it already updates the spec
   and the tooling — the drift-guard proposal-template line grows a
   "front-door prose" entry. This is what would have caught the "retired testing
   surface" blurb at the point it went stale.
3. **Prose that cannot be compiled is held by discipline, and converted to
   checkable references where feasible.** A natural-language "does the README lie"
   checker is not worth building. Instead: any prose claim that maps to a
   *checkable fact* — a diagnostic code, a CLI flag, an ADR status, a spec section
   — is preferred as a **guarded reference** (a link or an included snippet the
   existing guards already police) over free-floating prose. Un-mechanizable
   claims fall to the doc-delta review discipline of rule 2.

The line this draws: we **compile every example**, make the **doc-delta a required
part of every increment** for the front-door surfaces, and **prefer checkable
references over unguarded claims** — that is the extension of the drift-guard
pattern the review asked for, without pretending to mechanize prose.

## Interlocks

- **With the 1.0 definition (§7(4)).** The cadence keeps the stability promise
  *legible* (milestone-to-milestone upgrades with one migration note); the drift
  guards keep the doc-truth-is-brand promise *honest*. Together they are the
  discipline that makes 1.0 credible.
- **With the sequencing decision (§7(2)).** The tooling freeze there and the
  doc-delta discipline here are the same instinct — spend effort on truth and
  adoption, not on surface the project cannot yet back.
- **With the §7(5) validation bar.** The cumulative migration note per milestone is
  exactly what the two proposed external deployments would be "carried through" to
  prove the promise holds. §7(5) stays open on the tracking issue.

## Out of scope here

This closes §7(4) (the 1.0 definition, the milestone cadence, and the drift
guards). The remaining §7 calls — the honest **comparison page** (§7(3)) and the
**two-production-deployment validation bar** (§7(5)) — stay open on
[#540](https://github.com/accuser/bynk/issues/540).
