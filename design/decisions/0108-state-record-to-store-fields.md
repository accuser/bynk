# 0108 — Agent state is `store` fields, not a `state { }` record: `store` replaces `state`, with a partly automated migration

- **Status:** Accepted (storage track, slice 1; 2026-06-25)
- **Provenance:** the storage track.
- **Realises:** `design/bynk-design-notes.md` §10 ("Storage Types") — the `store`-field
  model where each piece of agent state is an access-pattern slot of a declared
  storage kind; and §2 ("One canonical way to do each thing").
- **Relates / amends:** supersedes the surface of ADR 0003 (inline state-field
  initialisers) and ADR 0004 (closed static-initialiser set) — both carry forward
  unchanged onto `store` fields; restates the field-reference and commit model of
  ADR 0107 (agent invariants), which was written against the `state { }` +
  explicit-`commit` implementation. Depends on **ADR 0109 — handler-atomic
  commit** (storage track) — `store`'s write semantics require it.

## Context

The design notes have always described agent state as a set of `store` fields,
each of a storage kind (`Cell`, `Map`, `Set`, `Log`, `Queue`, `Cache`), written
through kind-specific operations and committed atomically at handler end (§10).
The compiler diverged early to a pragmatic stand-in: an agent is `key id: T`,
then a single immutable `state { …fields… }` record, read via `self.state` and
written by spreading a new record into an explicit `commit { ...s, … }` statement
(ADRs 0003/0004; the divergence is recorded in ADR 0107's context).

The storage track introduces the real `store` model. That forces a disposition
question this ADR settles **up front**, because every later slice, every example,
and every doc page depends on it: do `state` and `store` coexist, or does one
replace the other — and if replaced, by deprecation or removal?

This is the load-bearing, hard-to-reverse call ADR 0076 says a track must make in
its settling phase. It is deliberately scoped to the **declaration surface and
its migration**; the *write semantics* it implies (staged writes, one atomic
commit at handler end, replacing today's eager per-statement `commitState` —
ADR 0107 D6) are ADR 0109's front-loaded decision, named in the track and not
re-litigated here.

## Decisions

**D1 — `store` is the single agent-storage surface; `state { }` is removed, not
retained.** Two declaration forms for the same concept — an immutable record with
`commit`-spread *and* `store` fields with `:=`/`.update` — is precisely the
dialectal duplication §2 forbids. They are not complementary the way `Cell` and
`Map` are; they are two answers to one question. The end state is a single
surface: `store`. The precedent is §10's own removal of the superseded
`map := map.insert(k, v)` pattern "in favour of direct methods" — Bynk cuts
superseded surface rather than carrying it.

**D2 — Removal is a hard cutover with a *partly* automated migration, not a long
deprecation window.** Bynk is pre-1.0 (v0.80) with a small, in-repo example
corpus and a round-trip-tested formatter, so a hard cutover beats maintaining two
state models across many slices. The migration has two halves with different
costs. The **declaration rewrite is mechanical** — decompose `state { … }` into
one `store` field per record field and lower `self.state.f` reads to bare names —
and a `bynk-fmt` codemod does it. The **write-form rewrite is semantic, not a
reflow**: turning `commit { ...s, f: v }` into per-field `f := v` / `.update`
requires diffing which fields a commit actually changes across every path, which
the formatter cannot infer reliably; that half is best-effort with human review.
The `commit` keyword itself is retired by the cutover — handler-end commit is
implicit under ADR 0109 — so a leftover `commit` is a migration artefact to chase
down, not a silent no-op.

**D3 — Coexistence during the track is transitional, not a committed design.**
`store` lands kind-by-kind across slices, so `state { }` cannot be deleted until
`store` reaches parity (through the `Cell` + storage-`Map` slices). Until then the
compiler accepts both, but this is an implementation reality of an incremental
rollout, not an endorsement of two surfaces. The `state { }` block is removed at
the slice that reaches parity; new code targets `store` from its first slice.

**D4 — "Agent state" survives as a concept; only the `state { }` *block* and its
access surface go.** The words "agent state" / "committed state" remain the name
for the aggregate of an agent's `store` fields — the unit the atomic commit acts
on and that invariants range over. What is removed is specifically the
`state { … }` declaration block, the `self.state` read path, and the
`commit { ...s, … }` spread write. White-box test reads of agent fields
(`agent.field`, §14) continue against `store` fields.

**D5 — ADR 0107 (invariants) is restated, not reopened — but the restatement must
be precise on three points the flat-record model left implicit.** Invariant
predicates already reference state by **bare name** (0107 D2) and `implies` is
unchanged; under `store` those bare names resolve to `store` fields. The amendment
that accompanies the parity slice settles:

- **A `Cell` read in a predicate is a pure read of the staged/proposed value, not
  a live storage op.** 0107 D1 forbids effects in predicates, and a bare `status`
  is sugar over an `Effect`-typed `Cell` read — so without this, D1 and the
  `Cell`-deref sugar appear to contradict. The amendment states that in predicate
  position the read is of the proposed write-set, evaluated purely.
- **The referenceable surface is a bounded *single-element* read, regardless of
  kind — a `Cell` deref, or a keyed `map.get(k)` / `set.contains(x)` for a
  key/element fixed at evaluation — and nothing whole-collection.** (This bullet is
  the **canonical** statement of the predicate surface; ADR 0109 D3 and
  the storage track echo it.) Each qualifying read is O(1) and reduces, like a
  `Cell` deref, to a pure read of the staged write-set, so the keyed Map/Set case
  is admissible (a wholesale "no `Map`" exclusion would otherwise forbid an
  obviously bounded `map.get(k)`). Two things stay out: `Cache` reads (TTL-
  evictable, so a predicate over them could fail *between* handlers, not at the
  commit gate); and **whole-collection quantifiers** — the design notes'
  `connections.keys.all(u => members.contains(u))` shape, pure as a value-method
  under 0107 D1 but an effectful, unbounded scan once `connections`/`members` are
  `store` fields. That quantifier case is the one genuinely open question; it
  rides this amendment, and until it is settled predicates are bounded reads only.
- **Invariants are checked against the staged write-set at handler end, before the
  atomic flush** — the direct analogue of today's `commitState` gate now that
  explicit `commit` is gone. This is the load-bearing link between ADR 0109 and
  invariant checking.

## Consequences

- **Migration is an increment deliverable.** The parity slice ships the
  `bynk-fmt` codemod (declaration half, automatic), applies the best-effort
  write-form rewrite with human review over `examples/` and the fixtures, retires
  the `commit` keyword, and removes the `state`/`commit`-spread grammar in the same
  PR — no stale dual surface left.
- **ADRs 0003/0004 carry forward.** `store f: Kind[T] = expr` keeps the inline
  initialiser (0003) and the closed static-initialiser set (0004) verbatim; only
  the enclosing block changes.
- **Docs and grammar churn.** `state { }` / `self.state` / `commit`-spread are
  removed from the spec, the book, and the tree-sitter / TextMate grammars at the
  parity slice; the worked examples are re-expressed in `store` form. Per
  `proposals/README.md`, leaving them stale is an incomplete increment.
- **Dependency stated, not hidden.** D1's write semantics are only honest if a
  handler is atomic; today it is not (eager `commit`, ADR 0107 D6). ADR 0109
  (handler-atomic commit) must land with or before the `Cell` slice. This ADR does
  not assume atomicity it has not yet secured — it names the dependency.

## Alternatives considered

- **Coexist permanently (both surfaces supported).** Rejected (D1): two idioms
  for one concept is the exact cost §2 exists to avoid; it doubles the surface
  every learner and reviewer must hold.
- **Deprecate `state` over a multi-version window.** Rejected (D2): a deprecation
  surface earns its keep when external users have un-migratable code; at pre-1.0
  with an in-repo corpus and a codemod, a hard cutover is cheaper and cleaner.
- **Keep `state { }` as sugar that desugars to `store Cell` fields.** Rejected: it
  re-introduces the second surface under a different name, and the desugaring hides
  the per-field access-pattern choice (`Cell` vs `Map` vs `Log`) that is the whole
  point of the storage-kind model.
