# 0152 — observation is auto-recorded at the capability seam: a thin `expect Cap.op called …` sugar and a `trace(Cap.op)` escape hatch, recorded ambiently in the test build only

- **Status:** Accepted (v0.117; 2026-07-02)
- **Provenance:** the v0.117 increment — the testing track's sixth slice, the *interaction* rung. It is the load-bearing record for the observation surface: how a test asserts *that a capability was called*, with what arguments, how many times, and in what order.
- **Realises:** the testing track's subject ladder — `value → domain → call → snapshot → step → history` — at the *interaction* level, and the track's "observation, not spies" pillar. Because a capability is injected at a known seam, the runtime records its calls for free; a pure-observation `case` supplies nothing.
- **Relates:** the one predicate surface (ADR 0144 — a `with <pred>` is the invariant predicate over the call's arguments, no matcher library); the contract guard's build-profile discipline (ADR 0150 — recording is emitted under `bynkc test` only and stripped from the deploy build); the generation record (ADR 0149 — the synthetic call-record type is registered the way a fabricated agent-state record is); the step rung (ADR 0151 — `trace` is a contextual test-only builtin the same way `old`/`new` are contextual bindings).

## Context

A test needs to assert not only what a unit *returns*, but how it *interacts* with its
collaborators: that an oversized order writes nothing, that a rejection is logged exactly
once, that the ledger is read before it is written. The classic answer is a spy/mock library
— a second vocabulary (`expect(x).toHaveBeenCalledWith(…)`) bolted beside the value
assertions, with its own matchers, its own setup, and its own ways to drift from the code.

Bynk already injects every capability at a known seam: a capability call lowers to
`<deps>.<Cap>.op(args)`, and a test substitutes at that same seam (`mocks` today, `provides`
after the tier slice). That seam is the leverage. Three questions had to settle. First, the
surface: a matcher library, or the one predicate surface reused? Second, the mechanism: does
a test *arrange* recording (spies it installs), or is recording *ambient*? Third, the shape
of the escape hatch for facts the sugar does not cover — a new in-test iteration construct,
or the existing `List` surface?

## Decisions

**D1 — observation is a thin sugar on `expect` over a `Cap.op` subject.** Inside a `case`,
the subject of an observation is a **`Cap.op` reference** — the capability and one of its
operations, *named, not called* (no argument list). The sugar forms are: `expect Cap.op
called` (at least one call); `expect Cap.op never called` (zero); `expect Cap.op called once`
/ `called <n> times` (an exact count, `<n>` a non-negative integer literal); `expect Cap.op
called … with <pred>` (at least one call whose arguments satisfy the predicate — composes
with a count, `called once with …` = exactly one call and it matches); and `expect A.op
before B.op` (an ordering claim). No matcher library; the vocabulary is fixed and small.

**D2 — the `with` predicate is the invariant predicate over the operation's parameters
(cites ADR 0144).** `with <pred>` is the one predicate surface with the operation's
parameters in scope by their **declared names** (`Logger.log(msg: String)` → `msg` in
scope), so `with msg is "…"` / `with amount > 1000` read directly and multi-argument
operations need no positional indices. It is pure `Bool` — no effects, capabilities,
`expect`, or `Val`. A non-`Bool` `with` is `bynk.observe.with_not_bool`; an impure one
`bynk.observe.impure_with`. The rejected alternative — a single `call` record threaded as
`with call.msg == …` — is noisier for no gain.

**D3 — recording is ambient at the seam, in the test build only (cites ADR 0150).** In the
test build, the runner wraps every observable capability operation on the case's `deps`
object with a **recording proxy**: each call appends its arguments and a monotonic order
index to a per-operation log, then delegates to whatever stands behind the seam — a `mocks`
double today, a `provides` stub or a real collaborator later. So observation needs **no
setup**: a pure-observation `case` declares no `mocks`/`provides`. Overriding a *return* is a
separate concern, needed only when the test depends on the value. Recording is emitted only
under the `bynkc test` profile (the same build-profile switch the contract guard uses); the
deploy build calls the seam directly, so observation adds no production cost or behaviour.
The sugar and `trace` read the **same** log — two views of one recorded list; they can never
disagree.

**D4 — the escape hatch reuses `List`; no test-only iteration is added.** For anything the
sugar does not cover, `trace(Cap.op)` binds the recorded calls as an ordinary value:
`List[<CallRecord>]` in call order. It is asserted with the surface that already exists —
`length()`, `all`/`any`, indexing — inside a single `expect` (`expect calls.all((c) =>
c.msg.length() > 0)`). A dedicated in-test `for all … in` is **rejected**, not deferred: it
would be a second iteration construct competing with `List.all`/`List.any`, and `for all`
already means *generative* binding. This is the deliberate reuse that lets the observation
vocabulary stay fixed while remaining fully expressive.

**D5 — the call-record type is synthetic, per operation, test-only (cites ADR 0149).**
`trace(Cap.op)` yields `List[<CallRecord>]` where `<CallRecord>` is a record whose fields are
the operation's parameters at their declared types (`{ msg: String }` for `Logger.log`), so
`c.msg` type-checks like any record access. It is registered into the test-body type table
the way a fabricated agent-state record is; it is a value type (field access, `is`, methods),
produced only by `trace`, and never exists in the deploy build.

**D6 — the sugar words are contextual; `trace` is a test-only builtin.** `called`, `never`,
`once`, `times`, `with`, `before` are parsed only in the observation-clause position after a
`Cap.op` subject in an `expect`; everywhere else they stay ordinary identifiers (as
`old`/`new`/`result` are). Reserving them would break existing identifiers for no gain.
`trace` is a test-only builtin: `trace` outside a `case` is `bynk.observe.trace_outside_test`.

**D7 — observation targets capability seams; placement and subject are checked.** The subject
must be a capability the unit under test `consumes`/`given`s. Observing a non-capability is
`bynk.observe.not_a_seam` (mirroring the provider seam check); a `Cap.op` naming no operation
of the capability is `bynk.observe.unknown_op`; an observation outside a `case` body is
`bynk.observe.outside_case`. `before` is **first-before-first**: `A.op before B.op` holds iff
both occurred and the *first* `A.op` precedes the *first* `B.op` — a simple, predictable rule;
richer temporal orderings are a `trace`-plus-`List` concern, not sugar.

## Consequences

- A test asserts interaction in the *same* `expect` vocabulary it asserts values in — one
  predicate surface, no spy library, no second setup.
- A pure-observation case needs no doubles: recording is ambient, so `expect Store.put never
  called` stands alone. A double is written only to control a *return*.
- Recording is provably absent from the deploy build (the recording proxy and the synthetic
  call-record types are emitted only under `bynkc test`), so a module with no observation
  emits byte-for-byte unchanged and production carries no observation cost.
- The sugar's vocabulary is fixed; expressiveness grows through `trace(Cap.op)` + the value
  surface, never by adding matchers.
- **Re-openable:** observing agent handlers, or calls across the real serialise → JSON →
  deserialise boundary (the `system` tier); the tier dial that renames `mocks` → `provides`
  (observation records at whatever seam the tier establishes, unchanged); a universal-emission
  guarantee ("every payment audits, on every path"), which is a cross-cutting policy, not a
  per-case observation; and history / handler-sequence properties — each a named future, none
  blocking v1.
