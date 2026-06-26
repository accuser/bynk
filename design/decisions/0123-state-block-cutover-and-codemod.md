# 0123 — The parity slice: `state { }` / `commit` are removed in one cutover; a `bynk migrate` codemod does the mechanical half and flags the semantic half; invariant predicates stay bounded single-element reads (whole-collection quantifiers deferred)

- **Status:** Accepted (storage track, parity slice; 2026-06-26)
- **Track:** `design/tracks/storage.md` (slice 1's deferred parity slice — the `state { }` → `store` hard cutover). One of the two non-kind items left before the track retires (the other is rehydration Q6/Q7).
- **Realises:** [ADR 0108](0108-state-record-to-store-fields.md) (`store` **replaces** `state { }`; D1 the single surface, D2 the hard cutover + codemod shape, D4 "agent state" survives as a concept, D5 the invariant restatement). This ADR settles the two things 0108 deferred *to the parity slice*: the codemod's semantic-rewrite mechanics (D2) and the whole-collection-quantifier predicate question (D5, "the one genuinely open question").
- **Relates:** ADR 0109 (handler-atomic commit — the implicit commit that *retires* the `commit` keyword); ADR 0107 (agent invariants — restated, not reopened); the `Cell` slice (the `store`-as-state-record machinery the migration targets); the round-trip-tested `bynk-fmt` (the codemod's host).

## Context

Since the `Cell` slice (v0.82), `store` fields and the legacy `state { }` block
have **coexisted** — a transitional dual surface ADR 0108 D3 explicitly called
*not* a committed design, to end at this parity slice. With all five storage
kinds shipped, the coexistence has outlived its purpose: it is now two ways to say
the same thing (the dialectal duplication §2 forbids), and every `state`-model
fixture is dead weight the codegen still carries.

ADR 0108 settled the *direction* (cut over, don't deprecate; a `bynk-fmt` codemod
migrates). It left two things to this slice: **how** the codemod handles the
write-form rewrite it called "semantic, not a reflow" (D2), and the **whole-
collection-quantifier** invariant case it called "the one genuinely open question"
(D5). This ADR settles both and pins the cutover's scope.

The blast radius is small, as 0108 D2 anticipated: ~6 in-repo fixtures use
`state { }`, ~2 use `commit { }` — plus any `bynk new` template and book examples.

## Decisions

**D1 — One cutover removes the `state { }` block, the `commit { }` statement, the
`commit` keyword, and `self.state` reads.** After this slice an agent's storage is
**only** `store` fields (read by bare name / kind ops, written by `:=` / kind ops,
committed implicitly at handler end, ADR 0109). The grammar, AST, checker, and
emitter drop: the `state { … }` declaration, the `commit { … }` spread statement,
the `commit` reserved keyword, and `self.state.<f>` field access. A leftover
`commit` or `state` is a **hard error** (a migration artefact to chase down, ADR
0108 D2), not a silent no-op. "Agent state" survives as the *concept* — the
committed aggregate, the invariant subject, the white-box test read (ADR 0108 D4).

**D2 — A `bynk migrate` codemod does the mechanical rewrite automatically and
*flags* the semantic one.** The migration is a **one-shot driver subcommand**
(`bynk migrate`, distinct from the idempotent recurring `bynk fmt`) backed by the
`bynk-fmt` AST-rewrite machinery (ADR 0108 D2's "`bynk-fmt` codemod"). It splits
exactly along 0108 D2's mechanical/semantic line:

- **Mechanical (automatic):** `state { f: T, … }` → one `store f: Cell[T]` per
  field (carrying the field's refinement and any initialiser); every `self.state.f`
  read → the bare name `f`. The formatter can do this losslessly.
- **Semantic (best-effort + review marker):** `commit { ...self.state, f: v }` →
  per-field `f := v` (or `.update` when the new value reads the old). The codemod
  rewrites the cases it can prove (a literal field-set whose RHS does not reference
  `self.state.f`), and for anything it cannot prove confidently it emits the
  rewrite it believes correct **plus a `-- TODO(bynk migrate): review this commit
  rewrite` marker** and lists it in the migration report. It never silently drops
  a `commit`. The report is the human-review checklist.

`bynk migrate` prints a per-file summary (fields migrated, commits rewritten,
commits flagged) and is **not** part of the normal build — it is run once.

**D3 — The grammar removal and the in-repo migration land in the same change.**
The cutover and the corpus migration are atomic: the same PR removes the `state`/
`commit` surface and migrates every in-repo `state`-model artefact (fixtures via
`bynk migrate` + hand-finishing the flagged commits, re-blessed; the `bynk new`
template; book/spec examples), so the corpus never sits in a non-compiling state.
The `state`-model **negative** fixtures that asserted old behaviour are repurposed
to assert the new hard error (D1).

**D4 — Invariant predicates stay bounded single-element reads; whole-collection
quantifiers are deferred.** This finalises ADR 0108 D5's restatement of ADR 0107:

- A predicate read is a **pure read of the staged/proposed write-set**, evaluated
  at handler end before the atomic flush (ADR 0109) — the direct analogue of the
  old `commitState` gate. A bare `status` (`Cell` deref) and a **keyed** `map.get(k)`
  / `set.contains(x)` (key/element fixed at evaluation) are admissible: each is O(1)
  and reduces to a pure staged read. `Cache` reads stay out (TTL-evictable between
  handlers).
- **Whole-collection quantifiers** — the `connections.keys.all(u => members.contains(u))`
  shape — **remain deferred**, not admitted. Settled *as* a deferral, with the
  reason: over `store` collections such a predicate is an **O(n) scan re-run at
  every commit**, a real and open cost/semantics question (it is pure but unbounded,
  unlike every other admitted read). The parity slice does **not** need it — it
  changes no predicate that the flat-record model accepted — so predicates remain
  **bounded reads only**, and the quantifier rides a later, dedicated decision.

**D5 — Scope.** This slice ships D1 (the removal), D2 (`bynk migrate`), D3 (the
in-repo migration), and D4's finalised predicate surface. It does **not** ship the
whole-collection quantifier (D4), nor does it touch the rehydration questions
(Q6/Q7) — those are the track's other remaining item. With this slice landed,
`store` is the single agent-storage surface (ADR 0108 D1 realised).

## Consequences

- **`store` is canonical; `state { }` is gone.** The dual surface ends; the
  codegen sheds the `state`-record path; new and existing code speak one storage
  language.
- **`bynk migrate` is a one-time tool**, not a build step — its semantic half is
  best-effort with explicit review markers, honest about what a formatter cannot
  infer (D2). It can be retired once the ecosystem has migrated (pre-1.0, this is
  cheap).
- **The `commit` keyword leaves the reserved set** — a small keyword-registry /
  TextMate / docs change rides the cutover.
- **The invariant surface is now fully pinned** (D4) — ADR 0109 D3 and
  `storage.md` §4 already echo the bounded-read canonical statement; this removes
  the "until settled" caveat for everything except the explicitly-deferred
  quantifier.
- **Rejected alternatives.** (a) A deprecation window keeping `state { }` alive
  across releases — rejected by ADR 0108 D2 (pre-1.0, tiny corpus, a hard cutover
  is cheaper than two state models). (b) A fully-automatic codemod for the write
  rewrite — rejected: diffing which fields a `commit` spread actually changes
  across every path is exactly what a formatter cannot infer reliably (0108 D2);
  best-effort-plus-marker is honest. (c) Admitting whole-collection quantifiers
  now — rejected: an unbounded per-commit scan is a distinct cost/semantics
  decision the parity slice does not require (D4).
