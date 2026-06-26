# 0123 — The parity slice: `state { }` / `commit` are fully removed in one cutover; the in-repo corpus (the only Bynk source that exists) is migrated by hand — no shipped codemod; invariant predicates stay bounded single-element reads (whole-collection quantifiers deferred)

- **Status:** Accepted (storage track, parity slice; 2026-06-26). **Amended
  2026-06-26 (D2):** the shipped `bynk migrate` codemod is dropped. The only Bynk
  source in existence is this repo's corpus, so there is no external code to
  migrate; the codemod would parse `state{}` forever for no one. The migration is
  a **one-time, in-repo hand rewrite**, which lets D1 do the *full* removal
  (parser/AST included) it always intended. This **supersedes ADR 0108 D2's
  "`bynk-fmt` codemod"** for the same reason.
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
committed implicitly at handler end, ADR 0109). The cutover removes the surface
**fully — parser, AST, checker, and emitter** all drop the `state { … }`
declaration, the `commit { … }` spread statement, the `commit` reserved keyword,
and `self.state.<f>` field access. (The full removal is possible precisely because
there is no shipped codemod that would need to keep parsing `state{}` — see D2.) A
leftover `commit` or `state` is a **parse/hard error**, not a silent no-op. "Agent
state" survives as the *concept* — the committed aggregate, the invariant subject,
the white-box test read (ADR 0108 D4).

**D2 — No shipped codemod; the in-repo corpus is migrated by hand, once.**
[Amended] The **only Bynk source that exists is this repository's corpus** — a
handful of `state{}`/`commit{}` fixtures, the `bynk new` template, and book/spec
examples. There is no external code, so a `bynk migrate` subcommand would parse
the legacy surface forever for no caller; building one is over-engineering, and
keeping the parser able to read `state{}` would block D1's full removal. So the
migration is a **one-time, in-repo hand rewrite**, applied in the cutover PR
itself:

- **Declaration:** `state { f: T, … }` → one `store f: Cell[T]` per field
  (carrying the refinement / initialiser); `self.state.f` reads → the bare name.
- **Write form:** `commit { ...self.state, f: v }` → per-field `f := v` (or
  `.update` when the new value reads the old), applied by hand with review — the
  "semantic, not a reflow" diff ADR 0108 D2 flagged is trivial across ~2 fixtures.

This **supersedes ADR 0108 D2's "`bynk-fmt` codemod"**: that machinery existed to
migrate user code that does not exist. The round-trip-tested formatter still
guarantees the *migrated* `store` source is canonical; nothing automated rewrites
the legacy surface.

**D3 — The removal and the in-repo migration land in the same change.** The
cutover and the corpus migration are atomic — the same PR removes the
`state`/`commit` surface and migrates every in-repo `state`-model artefact (the
fixtures, re-blessed for the `store`-agent emission; the `bynk new` template;
book/spec examples) — so the corpus never sits in a non-compiling state. The
`state`-model **negative** fixtures that asserted old behaviour are repurposed to
assert the new error (D1).

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

**D5 — Scope.** This slice ships D1 (the full removal), D2 (the in-repo hand
migration), D3 (atomic with the removal), and D4's finalised predicate surface. It
does **not** ship the whole-collection quantifier (D4), nor does it touch the
rehydration questions (Q6/Q7) — those are the track's other remaining item. With
this slice landed, `store` is the single agent-storage surface (ADR 0108 D1
realised).

## Consequences

- **`store` is canonical; `state { }` is gone — fully.** The dual surface ends;
  the parser/AST/codegen shed the `state`-record path entirely (no legacy parser
  to maintain); new and existing code speak one storage language.
- **No migration tooling ships.** The corpus is migrated by hand in the cutover
  PR (D2); the only Bynk code that exists is in this repo, so a codemod has no
  caller. This trades a tool nobody would run for a small one-time edit.
- **The `commit` keyword leaves the reserved set** — a small keyword-registry /
  TextMate / docs change rides the cutover.
- **The invariant surface is now fully pinned** (D4) — ADR 0109 D3 and
  `storage.md` §4 already echo the bounded-read canonical statement; this removes
  the "until settled" caveat for everything except the explicitly-deferred
  quantifier.
- **Rejected alternatives.** (a) A deprecation window keeping `state { }` alive
  across releases — rejected by ADR 0108 D2 (pre-1.0, tiny corpus, a hard cutover
  is cheaper than two state models). (b) **Shipping a `bynk migrate` codemod** (the
  original D2) — rejected on the realisation that the only Bynk source is in-repo,
  so the codemod would parse the legacy surface forever for no caller *and* would
  block the full parser/AST removal; a one-time hand migration is simpler and
  leaves no legacy parser behind. (c) Admitting whole-collection quantifiers now —
  rejected: an unbounded per-commit scan is a distinct cost/semantics decision the
  parity slice does not require (D4).
