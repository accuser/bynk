# 0124 — Rehydration validation: the load-time refinement gate fails as an **internal fault** (`RehydrationViolation`), the twin of the invariant gate — not a caller-facing boundary error; a refinement that **tightens** across a deploy faults on load, breaking migrations are **by convention** (no v1 migration hook), and additive evolution is automatic via zeroable defaults

- **Status:** Accepted (storage track, rehydration slice — the track's last open question, Q6/Q7; 2026-06-26).
- **Provenance:** the storage track (Q6/Q7 — the second of the two non-kind items left before the track retires; the parity slice, [ADR 0123](0123-state-block-cutover-and-codemod.md), was the first). This ADR **settles** the policy; the **rehydration slice** builds the gate (D5).
- **Realises:** the design notes' standing promise that *"storage rehydration validates refined fields on agent start, catching schema corruption before it reaches application code"* (§ "trust boundaries" / § "refined storage") — committed but **unbuilt**: `loadState` is `stored ?? zero()` today, a raw cast with no validation. This ADR decides the two things that promise left open — the failure *shape* (Q6) and the migration *policy* (Q7).
- **Relates:** [ADR 0107](0107-agent-invariant-predicate-surface.md) / [ADR 0109](0109-handler-atomic-commit.md) (the **invariant gate** at commit — this is its load-time twin); [ADR 0047](0047-jsonerror-compiler-known.md)-era `BoundaryError` (the trust-boundary validator this **reuses**, but disposes of differently); the events-track schema-evolution philosophy (design notes § "event versioning is the existing refinement mechanism extended"), which this ADR **aligns with** for breaking changes; [ADR 0108](0108-state-record-to-store-fields.md)/[ADR 0110](0110-storage-map-vs-value-map.md) (the `store`-fields-are-the-state-record model the gate validates).

## Context

A refined type carries its predicate into runtime validation at **every trust
boundary** (design notes § 15 / § "refined storage"): an HTTP body, a URL param, a
queue message, an event payload — **and rehydration from durable storage**. The
notes are explicit that loading an agent's state validates its refined fields
"against the current type definition, catching schema corruption or migration
mismatches before application code runs." That guarantee is **committed but not
implemented**: the emitted `loadState` is

```ts
const stored = await this.state.storage.get<LinkState>("state");
return stored ?? __zeroOfLinkState();   // raw cast — no validation
```

— a TypeScript cast that trusts the bytes. So the rehydration slice has two
genuinely open questions the notes deferred (track §3 Q6/Q7):

- **Q6 — the failure mode.** When a loaded refined field *fails* its predicate,
  what happens? The runtime already has two shapes for "a value didn't validate":
  a **`BoundaryError`** (`RefinementViolation`/`StructuralMismatch`), which at the
  HTTP seam becomes a **structured 400 the caller sees**; and an
  **`InvariantViolation`**, an **internal fault** (500-class, logged) the caller
  cannot pattern-match. Which is rehydration failure?
- **Q7 — the migration policy.** Beyond the error *shape*: when a refinement
  **tightens across a deploy** (`MinLength(6)` → `MinLength(8)`), already-persisted,
  *previously-valid* data now fails on load. This is not corruption — it is schema
  evolution that orphaned good data. What is the policy?

These are the storage track's last decision; with them settled the track's design
is complete (only the slice's implementation remains).

## Decisions

**D1 — Rehydration validates loaded state against the *current* type definition,
automatically, reusing the boundary validator.** When `loadState` finds stored
state, it validates that state's **shape** and **refined fields** before any
handler body runs — using the **same validator** the HTTP/queue/event boundaries
use (the refined type's own predicate; design notes § "the constraint is the
validator"). A **fresh key** (no stored state) takes its zero/initialiser, which
is valid by construction, so the gate runs **only when stored state exists**.
Validation is **automatic** (no opt-in, matching write-time validation), and its
cost is proportional to the **refined**-field count — state of only base types
needs a structural check at most, and an agent with no refined fields needs none.
There is no separate stored schema: the gate validates against the type as it is
*compiled in this deploy* (this is what makes the type's compile-time guarantee
hold at runtime).

**D2 — A rehydration-validation failure is an *internal fault*, not a caller-facing
structured outcome (Q6).** The gate **reuses** the boundary validator's *detection*
(a `StructuralMismatch` or `RefinementViolation` with field path + `ValidationError`
detail) but **disposes of it as a fault** — a dedicated **`RehydrationViolation`**,
the **load-time twin of `InvariantViolation`**: 500-class, logged with the **agent
type and field path** (never the key or the offending value — the ADR 0107 logging
discipline), surfacing the same way any internal fault does. It is **not** turned
into a `BoundaryError`-style 400. The reason is **who supplied the data**:

- At an HTTP/queue/event boundary the supplier **is the untrusted caller**, the
  contract is "validate caller input," and there is a request-scoped response slot
  — so a refinement failure is *the caller's* and becomes a structured 400/dead-letter.
- At **rehydration** the supplier is **trusted past-self / the platform's own
  storage**, the agent is *initialising before any handler runs*, and there is **no
  caller contract slot** for "my own durable state is corrupt." The handler's
  caller did not supply the data and cannot remediate it.

This is the exact symmetry with the invariant gate: invariants check the
**proposed** state at the **commit** boundary (write-time, ADR 0109); rehydration
checks the **loaded** state at the **load** boundary (read-time). Both are "the
agent's own state violates its declared shape," and **errors-as-values reserves
typed outcomes for expected, recoverable cases** — corrupt self-state is neither,
so it faults rather than forcing every agent caller to handle an unhandleable
`Result` arm.

**D3 — A refinement that *tightens* across a deploy faults on load; breaking
migrations are by convention, not a v1 language feature (Q7).** Because the gate
validates against the **current** definition (D1), a tightening that orphans
previously-valid data is **indistinguishable, at load, from corruption** — the
data genuinely no longer satisfies the type — and so it **faults** (D2). This is
deliberate over the two silent alternatives:

- **Silent coercion** (re-narrow / clamp the value) would invent data the user
  never wrote;
- **Silent drop / reset-to-zero** would hide data loss behind a "fresh" agent.

Both betray the type's guarantee quietly; a **loud fault forces a deliberate
migration**. So **breaking** schema changes — tightening a refinement, narrowing a
type, renaming a field — are handled **by convention**, exactly as the events track
handles breaking envelope changes (design notes § "breaking changes … by
convention rather than by language feature"): **widen-don't-narrow**, or stage an
explicit migration (deploy a transform, or introduce a new field/agent and migrate)
before the tightening lands. **No automatic coercion, no silent drop, and no
migration hook ship in v1.**

**D4 — Additive evolution is automatic via zeroable defaults; the load *merges*
zero-then-stored.** A `store` field **added** in a later deploy and absent from
persisted state takes its **zero / initialiser** — the additive, non-breaking case
the events track also makes automatic (via field defaults). This requires the
emitted load to **merge** rather than return the stored record wholesale:

```ts
return { ...__zeroOfLinkState(), ...(stored ?? {}) };   // new fields get their default
```

Today's `stored ?? __zeroOfLinkState()` returns the stored record as-is, so a
field added after that record was written would read **`undefined`** — a latent
bug. The zero-then-stored merge is the rehydration slice's load-path change, and it
makes "add a zeroable `store` field" a safe, no-migration deploy.

**D5 — Scope: settle-only; the rehydration slice builds it; named deferrals.** This
ADR settles the policy (D1–D4). The **rehydration slice** implements it: emit the
load-time validation gate (D1/D2), the `RehydrationViolation` fault, and the
zero-merge load (D4) — validated on the generated code (a tampered/orphaned stored
record faults on load; an additive field defaults). Explicitly **deferred**, to be
settled *with* the relevant track when the need is real, and aligned with the
events story when it is:

- a **versioned-schema migration capability** (a `via schema(...)` / `@schema(N)`
  analogue that runs a declared transform on load) — the supported path for
  *breaking* changes beyond convention;
- **per-field default-on-read** for narrowing (read an absent/invalid field as a
  declared default instead of faulting);
- a **soft / recoverable rehydration handler** — a typed recovery seam for an agent
  that wants to *handle* corruption (quarantine, repair, re-key) rather than fault.

## Consequences

- **The committed guarantee becomes real and honest.** "Refined storage is
  validated on rehydration" stops being a raw cast; corruption and orphaned data
  are caught *before* application code, as a fault that is loud in the logs (agent
  type + field), not a silent bad read.
- **Rehydration is the read-time twin of the invariant gate.** The two agent-state
  gates now mirror: invariants validate the proposed write before the commit
  (ADR 0109), rehydration validates the loaded state before the first read. Same
  posture (internal fault, ADR 0107 logging), opposite ends of the lifecycle.
- **Additive `store` evolution is a safe deploy; breaking evolution is a
  deliberate, visible act.** Adding a zeroable field needs no migration (D4);
  tightening a refinement *will* fault until the data is migrated (D3) — which is
  the point: the cost of a breaking change is surfaced, not hidden.
- **A latent load bug is fixed.** The `?? `-replaces-wholesale load (D4) would have
  read post-write-added fields as `undefined`; the merge closes it.
- **Cost is bounded and proportional.** The gate is one validation pass per cold
  load (not per handler), over refined fields only; base-type state pays nothing.
- **Rejected alternatives.** (a) **A caller-facing `BoundaryError`/HTTP 400 for
  rehydration failure** — rejected: the supplier is trusted past-self, not the
  untrusted caller; there is no caller contract slot for "my own state is corrupt,"
  and a structured outcome would force every agent caller to pattern-match a
  condition it cannot remediate (the trust-boundary 400 is right *only* because the
  HTTP caller *is* the supplier). (b) **Silent coercion or reset-to-zero of
  orphaned/invalid data** — rejected: both hide data loss and quietly break the
  type's guarantee; a fault is the honest failure (D3). (c) **A v1 schema-migration
  hook / stored schema version** — rejected as over-engineering for a pre-1.0,
  single-deploy reality with no external persisted corpus; the events track will set
  the migration-capability precedent, and this ADR aligns Q7 with its
  additive-automatic / breaking-by-convention split rather than inventing a parallel
  one. (d) **Validating against a stored *old* schema** — rejected: no schema is
  persisted, and validating against the **current** definition is precisely what
  upholds the compile-time guarantee at runtime (D1).
