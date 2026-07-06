# Durable-state migration — schema identity, a declared `migrate` transform, and the exit a live agent has none of today

- **Status:** Draft (settling). Direction not yet merged; no slice authorised.
- **Realises:** the retired **[storage track](README.md#retired-tracks)** (shipped
  v0.82–v0.97; its doc removed at retirement per the track lifecycle) named
  "**Deferred follow-ons**" — *"a versioned-schema migration
  capability, per-field default-on-read, a soft recovery handler"* — and the same
  three named by [ADR 0124](../decisions/0124-rehydration-validation-and-migration.md)
  **D5**. It sharpens the [design notes](../bynk-design-notes.md) §7 promise that
  *"storage rehydration validates refined fields on agent start"* into the missing
  second half — what happens **after** the fault, when the orphaned data is good and
  the schema simply moved. It aligns, deliberately, with the **Events track**'s
  schema-evolution philosophy (design notes §7 lines 243–278: additive-through-
  defaults automatic, breaking-by-convention loud) rather than inventing a parallel
  one — porting `@schema(N)` / `via schema(...)` from event envelopes to agent state.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on **all three** axes: **multi-increment** (schema identity → the
  `migrate` transform → default-on-read → a soft recovery handler → agent enumeration
  — five slices, the connective design of which a delete-on-merge proposal cannot
  hold); **surface not yet settled** (whether schema identity is author-declared
  `@schema(N)` or compiler-derived; the `migrate` surface shape; lazy-vs-eager
  execution; the enumeration model); and a **data-safety boundary** — the *safety*
  half of ADR 0076's "security/safety boundary" axis, read as
  [`deploy.md`](deploy.md) reads it for operational safety: this is the first track
  whose failure mode is **irreversible loss of a user's durable data**, not a defect
  in generated code. (The ADR's trigger is *two or more* axes; the first two alone
  already qualify.) The review names it *"the gap that historically
  kills durable-state platforms."*
- **Front-loaded ADRs (named, not numbered):** the **schema-identity model** (what
  is persisted, how it is computed, and why it lives with the state); the
  **migration-execution model** (lazy-on-load per instance, the rollback posture —
  Q8 — and how a declared transform composes with the rehydration fault it does
  *not* replace); and the
  **ADR 0124 supersession** call (which of D3/D5 this reverses, and what it keeps).
  Each is created and numbered by the slice that lands it (§8) — this doc
  deliberately does not pre-allocate numbers, since concurrent tracks would collide.

## 1. Motivation

The storage track shipped excellent *local* semantics and, in its last slice
(v0.97, ADR 0124), a load-time refinement gate: an agent's persisted state is
validated against the **current** type definition when it rehydrates, and a
mismatch **faults** (`RehydrationViolation`) rather than reaching application code.
That gate is real and correct. What it lacks is an **exit**.

Three failure shapes follow, and all three brick production systems:

1. **Tightening a refinement faults live agents on load with no way out.** When a
   refinement tightens across a deploy (`MinLength(6)` → `MinLength(8)`), already-
   persisted, *previously-valid* data now fails the current predicate. ADR 0124 is
   explicit that this is *"schema evolution that orphaned good data"* (its Context)
   yet, per D3, at load *"indistinguishable … from corruption,"* so it faults. The
   ADR's remedy —
   *"stage an explicit migration"* — is **advice without a mechanism**: there is no
   migration hook, no stored schema version to branch on, and (because agents are
   per-key Durable Objects with agent-local queries) **no way to enumerate the
   instances** to migrate them.

2. **Renames are silent data loss, not a fault.** The emitted load merges
   zero-then-stored — `const __merged = { ...__zeroOf<Agent>State(), ...stored };`
   ([`bynk-emit/src/emitter/emit.rs:2315`](../../bynk-emit/src/emitter/emit.rs), ADR
   0124 D4) — so a **renamed** `store` field presents as *additive*: the new name is
   absent from `stored` and zeroes in cleanly, while the old field **rides along
   orphaned** inside the spread. A refined new field even passes the rehydration
   gate (a valid zero). The one case the loud-fault philosophy most needs to catch —
   real data quietly abandoned — presents as a clean deploy.

3. **The only versioned-schema machinery designed lives in the unshipped Events
   track.** `@schema(N)`, deserialisation defaults, and `via schema(...)` version-
   aware dispatch are specified for **events** (design notes §7 lines 243–278) and
   for events only. Shipped **agent state** has none of it. ADR 0124 D5 names the
   port — *"a `via schema(...)` / `@schema(N)` analogue that runs a declared
   transform on load"* — as the deferred capability. This track is that port.

The load path today (`loadState`, [`emit.rs:2303`](../../bynk-emit/src/emitter/emit.rs)):

```ts
const stored = await this.state.storage.get<LinkState>("state");
if (stored === undefined) return __zeroOfLinkState();      // fresh: valid by construction
const __merged = { ...__zeroOfLinkState(), ...stored };    // D4: additive fields default
__rehydrateLinkState(__merged);                            // D1/D2: fault on mismatch
return __merged;
```

The fault (`rehydrationViolation`, [`runtime.ts:219`](../../bynk-emit/src/emitter/runtime.ts))
is the whole exit surface. There is no branch for "the schema changed and here is
how to move the data across."

## 2. Scope and non-goals

**In scope.**

- **Persisted schema identity per agent class** (§3) — a `{ version, fingerprint }`
  the load path can read *before* validating, so schema *evolution* is distinguished
  from schema *corruption* rather than collapsed into one fault.
- **A declared `migrate` transform run at rehydration** — the exit ADR 0124 D3 left
  as advice. A pure transform from the decoded old record to the current one, run in
  the Durable Object's serialised load before any handler.
- **Rename / orphan detection as a *loud* fault** — turn the silent `...stored` merge
  (motivation #2) into a caught failure that names the orphaned field, closing the
  one hole in the loud-fault philosophy.
- **(Late, optional) an agent-enumeration / bulk-migration verb** — the platform
  affordance for migrations that must *complete* before a dependent cutover, rather
  than lazily as instances wake.

**Non-goals (and why).**

- **Automatic coercion or silent reset-to-zero of orphaned data.** ADR 0124 D3
  rejected both — they invent or hide data. This track keeps that: a breaking change
  *without* a declared `migrate` still **faults**. The track adds an exit; it does
  not soften the default.
- **A general cross-instance query surface.** Enumeration here serves migration only.
  Agent-local queries stay the model (design notes §11); a global "query all agents"
  is a different feature with its own justification.
- **Changing the Events track.** Events already have their evolution story; this
  track *borrows its vocabulary* for agent state and leaves event envelopes alone.
- **Multi-key / per-entry DO storage.** The retired storage track's remaining
  deferrals ([README](README.md#retired-tracks)) — "per-entry DO storage keys" and
  "refined non-textual-key rehydration validation" — are storage-shape follow-ons,
  not migration; they stay unowned until a storage-shape effort claims them, out of
  this arc.

## 3. The core problem: schema identity and the enumeration bind

This is the decision the whole track turns on — the durable-state analogue of what
the provisioning-state model is to [`deploy.md`](deploy.md). Two coupled binds.

### 3.1 The compiler cannot see the stored data, so identity must be persisted

The rehydration gate validates against the type *as compiled in this deploy* (ADR
0124 D1) — deliberately, since that is what makes the compile-time guarantee hold at
runtime. But it means the load path has **no idea what shape the stored bytes were
written against**. It cannot tell "this record predates the `MinLength(8)` tightening
and needs migrating" from "this record is genuinely corrupt." Both simply fail the
current predicate.

The answer is to **persist schema identity alongside the state** — a small meta
record the compiler stamps and the load path reads *first*:

```
"__schema" ⟶ { version: Int, fingerprint: String }
```

- The **version** is what a `migrate` transform branches on (`migrate from 1`,
  `from 2..`) — orderable, so a chain N→…→current can be selected.
- The **fingerprint** is a **canonical, order-independent structural hash** of
  `<Agent>State` (field names + kinds + refinements, sorted — *not* declaration
  order, or a formatting churn would spuriously bump it). It catches the author error
  the version alone cannot: *shape changed but the version did not* — the exact check
  the Events track already makes (design notes §7: *"the compiler verifies the
  declared version against what the schema would otherwise warrant"*).

Whether the version is **author-declared** (`@schema(N)`, pinned like events),
**compiler-derived** (bumped when the fingerprint changes), or **both** is Q1 — the
front-loaded, hard-to-reverse call. *Leaning: both — a derived fingerprint for
change-detection, an author-facing version for migration selection, the compiler
reconciling the two.*

### 3.2 Per-key Durable Objects cannot be enumerated, so v1 migrates lazily

An agent is a **per-key DO**; the runtime reaches one only through a stub
`namespace.get(id)` ([`runtime.ts`](../../bynk-emit/src/emitter/runtime.ts)) — there
is **no list-all-keys API**, in the platform or in the emitted code. This is exactly
why "stage a migration" has been advice: there was no verb to *reach* the population.

The bind dissolves for the common case once migration is **lazy-on-load per
instance**: each DO, when it next wakes and finds `stored.version < current`, runs
the declared transform on *itself*, inside its own serialised load, before any
handler runs — then re-validates and commits the upgraded record. No enumeration is
needed; instances migrate themselves as they are touched, exactly as the Events track
upgrades old wire records on read (design notes §7 *Replay*). The cost is that a
never-touched instance stays on the old schema until it wakes — acceptable, because
nothing reads it until it does.

Enumeration is therefore needed **only** for the eager case: a migration that must
*complete across all instances before* a dependent change (a downstream reader that
assumes the new shape). That is a genuinely harder capability — it needs a **key
registry** the runtime does not maintain today — and it is deferred to a late,
optional slice (§8 slice 4, Q6), not the spine of the track.

## 4. Internal architecture

### 4.1 The migration-aware load path

`loadState` (§1) gains a branch **between** the merge and the gate:

```
read stored + "__schema" meta
  ├─ fresh key              → zero (valid by construction)          [unchanged]
  ├─ version == current     → { ...zero, ...stored }, then gate     [today's path]
  ├─ version <  current     → run chained migrate transforms,
  │                            then the gate on the result,
  │                            then persist the upgraded record + bumped meta
  └─ version >  current      → fault (a rolled-back deploy meets newer data — Q8)
```

The rehydration gate (ADR 0124 D1/D2) is **retained unchanged** as the final check
*after* migration — a `migrate` that produces an invalid record still faults, so the
type guarantee is never weakened. When **no** `migrate` covers the gap and the
fingerprint mismatches, the load faults as today — with a report built from the
mismatch. Detection and naming are separate steps: the **fingerprint** decides *that*
the shape moved — never a "looks like a rename" guess (Q5) — and the report then
diffs the stored record's keys against the current shape, **naming any orphaned
field path** it finds (motivation #2 closed). A mismatch with **no** orphaned key —
a refinement-only tightening, `MinLength(6)` → `MinLength(8)` — has no field to
name; there the fault reports the schema move alongside the field the gate rejects.

### 4.2 Deltas by layer (what each slice touches)

- **Grammar / AST (`bynk-syntax`).** A `migrate` block on `AgentDecl`
  ([`ast.rs:819`](../../bynk-syntax/src/ast.rs)) — arms keyed on a source version /
  range, each a pure transform expression old-record → new-record — and an optional
  `@schema(N)` annotation (the `Annotation` machinery already exists,
  [`ast.rs:882`](../../bynk-syntax/src/ast.rs)). Parser in
  `bynk-syntax/src/parser/declarations.rs`.
- **Checker (`bynk-emit/src/project/validate.rs` agent loop ~2445; `bynk-check`
  scope).** Verify the transform is **pure** (no capabilities — the same rule the
  Events track applies to default expressions), that it produces a value of the
  current `<Agent>State` record, that arms are **monotone and cover** the reachable
  version range, and that a declared `@schema(N)` agrees with the derived fingerprint.
- **Emitter (`bynk-emit`).** Compute the compile-time fingerprint and version; emit
  the `"__schema"` persistence; rewrite `loadState`
  ([`emit.rs:2303`](../../bynk-emit/src/emitter/emit.rs)) to the §4.1 branch; emit the
  chained transform. New runtime helpers beside `RehydrationViolation`
  ([`runtime.ts:212`](../../bynk-emit/src/emitter/runtime.ts)). The Cloudflare
  `[[migrations]]` block ([`wrangler.rs`](../../bynk-emit/src/emitter/wrangler.rs)) is
  **untouched** — that is DO *class* registration, not state schema, and conflating
  the two would be a category error.
- **Runtime.** The migrate execution, the fingerprint compare, and the schema-meta
  read/write — all inside the DO's single-threaded load (§6, atomicity).

## 5. Tooling delta (the standing rule)

Per [ADR 0156](../decisions/0156-editor-surface-tracks-language.md), each slice that
adds surface names all four; the headline for this track:

- **Hover:** on `migrate` (its source-version range and target) and on `@schema(N)`
  (the derived-vs-declared version, and the fingerprint it pins).
- **Completion:** the `migrate` keyword in agent bodies, and schema-version literals /
  ranges after `from`.
- **Semantic tokens:** the `migrate` contextual keyword and `@schema` annotation.
- **Signature help:** unchanged, because `migrate` introduces no call surface — a
  transform body, not an invocable. Stated explicitly, per the rule that silence is
  an oversight.

Slice 0 (schema identity, no author surface) is emitter/runtime-only and states all
four as *unchanged, because it adds no syntax*.

## 6. Security & safety model

The axis that makes this a track, not a verb: its failure mode is **irreversible loss
of a user's durable data**.

- **Silent data loss is the primary threat.** A rename that orphans a field
  (motivation #2) destroys data on the *next* deploy, invisibly. *Mitigation:* the
  fingerprint makes the shape change detectable, and the load **faults** on an
  uncovered orphan rather than merging past it — turning the silent case loud, which
  is the whole loud-fault philosophy applied to the one place it currently leaks.
- **A buggy `migrate` transform mutates persisted state.** A transform is *write*
  access to durable data. *Mitigation:* transforms are **pure** (compiler-verified,
  no capabilities), so they cannot reach outward or depend on ambient state; and the
  **rehydration gate runs on the transform's output** (§4.1), so a transform that
  produces an invalid record faults instead of persisting corruption.
- **Migration atomicity.** A half-applied migration (transform ran, upgraded record
  not yet committed, then a crash) must never leave a partially-migrated record read
  as current. *Mitigation:* the migrate-then-revalidate-then-commit sequence runs
  inside the DO's **single-threaded, serialised load** before any handler — the same
  atomicity the commit boundary already relies on (ADR 0109); either the upgraded
  record (with bumped meta) is committed whole, or the next load re-runs from the
  original bytes.
- **Evolution vs corruption must not be conflated in either direction.** Reading real
  corruption as "just an old version" would run a transform over garbage; reading old
  data as corruption is today's brick. *Mitigation:* version selects *whether* a
  transform applies, fingerprint guards *that the current shape is what the code
  expects* — corruption still faults, evolution migrates, and the two are decided by
  data the compiler stamped, not a guess.
- **The default stays loud.** A breaking change with **no** declared `migrate` faults
  exactly as it does today (ADR 0124 D2/D3 retained). The track never makes a breaking
  change *quiet*; it makes it *survivable when the author declares how*.

## 7. Open questions (settle before slicing)

- **Q1 — schema identity. [the front-loaded decision]** Fingerprint hash **and**
  integer version, or one alone; where it is persisted (a dedicated `"__schema"` key
  vs a reserved field on the state record); the **canonical order-independent** shape
  hash (so field reordering / formatting never spuriously bumps it, but a
  name/kind/refinement change always does); and whether the version is author-declared
  `@schema(N)`, compiler-derived, or both. *Leaning: both, persisted as
  `{ version, fingerprint }`; compiler reconciles a declared `@schema(N)` against the
  derived fingerprint, as Events does. Settle in slice 0 — this is the load-bearing
  ADR every later slice inherits.*
- **Q2 — the migration surface.** A `migrate from N { … }` transform block vs a
  wholesale port of Events' `via schema(N)` dispatch vs per-field `default-on-read`.
  *Leaning: a pure `migrate` transform block, terminology aligned with `@schema(N)`;
  `default-on-read` is a lighter, complementary tool (slice 2), not the primary
  surface.*
- **Q3 — execution model.** Lazy-on-load per instance vs an eager platform sweep.
  *Leaning: lazy-on-load for v1 (§3.2); enumeration/eager is a late slice.*
- **Q4 — migration chains.** Direct N→current transforms vs chained
  N→N+1→…→current composition. *Leaning: chained — the author writes each step once,
  the compiler composes; a single N→current jump duplicates logic and rots.*
- **Q5 — rename / orphan detection.** The review's *"unknown stored field + new
  zeroed field ⇒ diagnostic"* heuristic vs a fingerprint-anchored fault. *Leaning:
  anchor to the fingerprint — it subsumes rename detection into the identity
  mechanism and avoids a fragile "looks like a rename" guess that both false-positives
  (a genuine add next to a genuine remove) and false-negatives (a rename with no new
  field). Naming is unaffected: the orphaned field comes from the fault report's
  stored-vs-current keyset diff after the fingerprint has fired (§4.1) — a property
  of the report, not the trigger, so no heuristic ever decides whether to fault.*
- **Q6 — the enumeration verb.** Defer entirely vs a minimal `bynk migrate <Agent>`
  backed by an emitted key registry. *Leaning: defer (§3.2); document the residual
  limitation — a migration that must complete before a dependent cutover still needs
  convention until this lands. If built, the registry is the load-bearing sub-ADR,
  since maintaining a live key index has its own write-amplification and consistency
  cost.*
- **Q7 — relationship to ADR 0124.** Supersede D3/D5's *"no migration hook … in v1"*
  for the transform + fingerprint; **keep** D2 (`RehydrationViolation` as the
  no-migration fallback) and D4 (additive zero-merge); leave the **soft recovery
  handler** (D5's third deferral) to slice 3. The superseding ADR must state, in ADR
  0124's own terms, that its rejected-alternative **(c)** (*"a v1 schema-migration
  hook / stored schema version … over-engineering for a pre-1.0, single-deploy
  reality"*) is **reversed on the 1.0 / real-data premise** the review raises — not
  overturned silently.
- **Q8 — rollback across a migration wave.** Lazy migration bumps the persisted
  meta on every instance that wakes (§3.2), so rolling a deploy back makes exactly
  the recently-active population meet `version > current` and fault on every load
  (§4.1) until the deploy rolls forward again — a loud, bounded outage, not data
  loss: the newer-schema bytes are intact and the fault clears on roll-forward.
  Faulting is deliberate — old code must not silently read newer-schema data, and
  running a reverse transform the author never declared would invent one. The open
  call is the operational posture: document a migration-triggering deploy as
  **roll-forward-only**, delay the meta bump behind a pin/window, or admit an
  author-declared down-migration. *Leaning: keep the fault and document
  roll-forward-only for v1 — a down transform doubles the migration surface for a
  rare case, and a bump-delay window reintroduces mixed-version reads. Settle with
  the migration-execution ADR in slice 1.*

## 8. Slice decomposition (ordered)

Each slice is an ordinary [increment proposal](../proposals/README.md) — a GitHub
issue opened from the increment-proposal template and labelled `proposal` — citing
this doc and its ADRs; the accepted issue authorises the build. Slice 0 front-loads
the identity ADR; later slices build on it.

- **Slice 0 — schema identity + rename/orphan fault.** Persist
  `{ version, fingerprint }`; teach `loadState` to read it; turn today's silent
  `...stored` merge into a **caught fault** when the shape moved with no migration —
  anchored to the fingerprint, **naming any orphaned field** from the
  stored-vs-current keyset diff (§4.1). **No new author surface** — this is the
  immediate P0 win (renames stop being silent) and the load-bearing
  **schema-identity ADR** (Q1) every later slice inherits. Emitter/runtime-only;
  tooling unchanged.
- **Slice 1 — the `migrate` transform + lazy-on-load execution.** The language
  surface (grammar / AST / checker / emitter, §4.2): a pure, chained `migrate`
  transform run in the DO's serialised load (§3.2, §6). Lands the
  **migration-execution ADR** (including the Q8 rollback posture) and the
  **ADR 0124 supersession** (Q7). The spine of the track — after it, a breaking
  change *has an exit*.
- **Slice 2 — per-field default-on-read** (ADR 0124 D5's second deferral). A lighter
  narrowing tool: an absent or newly-invalid *single* field reads as a declared
  default instead of demanding a whole `migrate`. Complements slice 1 for the
  common trivial narrowing.
- **Slice 3 — soft / recoverable rehydration handler** (ADR 0124 D5's third
  deferral). A typed recovery seam for an agent that wants to *handle* corruption —
  quarantine, repair, re-key — rather than fault. The security-bearing slice (it
  changes what a fault *can* do), so it carries a threat model and a
  `/security-review` gate per ADR 0076 §4.
- **Slice 4 — agent enumeration + bulk `migrate` verb** (Q6). The hardest and most
  optional: a key registry plus a driver verb, for eager / pre-cutover migration.
  Only pursued if the lazy model (§3.2) proves insufficient for a real need.

## 9. Risks

- **The schema-identity model is wrong and every downstream slice inherits it.** A
  fingerprint that churns on formatting, or a version that cannot select a chain,
  breaks migration selection at the root. *Mitigation:* front-load the identity ADR
  (slice 0, Q1); make the fingerprint canonical and order-independent by construction.
- **A `migrate` transform corrupts durable data.** *Mitigation:* pure transforms,
  compiler-verified; the rehydration gate re-run on the output; atomic commit in the
  DO's serialised load (§6).
- **A rollback after a lazy migration wave faults the live population.** Every
  instance that woke since the deploy carries the bumped meta, so a rollback turns
  each of them into a `version > current` fault (§4.1) until the deploy rolls
  forward. *Mitigation:* Q8 — the fault is loud and recoverable (nothing is lost or
  rewritten), and the v1 posture is an explicitly documented **roll-forward-only**
  rule for migration-triggering deploys rather than a silent read of newer-schema
  data.
- **Reversing an accepted ADR quietly erodes trust in the decision record.**
  *Mitigation:* an explicit superseding ADR that names ADR 0124 (c) and states the
  premise that changed (1.0 / real persisted data), keeping D2/D4; the loud fault is
  retained as the fallback, so the reversal *adds* an exit rather than *softening* the
  guarantee (Q7).
- **Enumeration scope-creep.** A key registry is a standing runtime cost and a
  consistency problem of its own. *Mitigation:* lazy-first (§3.2); enumeration
  deferred to slice 4 and pursued only against a real eager-migration need.
- **Fingerprint drift across compiler versions.** A compiler change that alters the
  hash for unchanged source would fault every live agent. *Mitigation:* define the
  fingerprint over the *language-level* shape (names, kinds, refinement predicates),
  never over emitter internals or compiler version, and cover it with a stability test.

## 10. Relationship to the north star

The storage track deliberately stopped at the rehydration fault (ADR 0124 D5),
correct for *"a pre-1.0, single-deploy reality with no external persisted corpus."*
This track picks up at the premise the review changes: **real data, real deploys,
real 1.0.** It finishes the durable-state story by turning *"stage an explicit
migration"* from advice into a **mechanism** — schema identity the load path can read,
a declared transform it can run, and (later) a verb that can reach the population —
while changing nothing about the philosophy that made the storage track sound. A
breaking change with no declared `migrate` still faults, loudly — naming the orphaned
field when there is one to name (§4.1). The track's one genuinely new idea is
**persisted schema identity** — the durable-state counterpart to what a stored schema
version is for any serious data platform, and the thing agents have never had — and
its one genuinely new *responsibility* is stewarding a user's durable data safely
across the shape changes that a living system always, eventually, makes.
