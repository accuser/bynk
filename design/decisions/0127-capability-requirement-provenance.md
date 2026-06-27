# 0127 — Capability requirements have generic provenance, surfaced as a materializable ghost `given`; and `by` is rejected on an agent handler

- **Status:** Accepted (capability-provenance increment; 2026-06-27).
- **Spec:** `reference/diagnostics.md` (`bynk.actor.by_on_agent`), `reference/agents.md` (the agent-handler `given` surface), `static-semantics.md` §5 (capability `given` checking).
- **Relates:** [ADR 0054](0054-structured-suggestions.md) (the structured suggestions / `given_insertion_edit` this reuses); [ADR 0056](0056-lsp-inlay-hints.md) (the inferred-type `HintSink` the requirement ledger mirrors); [ADR 0113](0113-cache-ttl-eviction.md) / [ADR 0121](0121-log-append-and-retention.md) (the store ops whose `given Clock` requirement is the motivating invisible case). The agent **capability-encapsulation boundary** (an agent owns its capabilities; a call across the boundary forwards nothing) is the **sibling correctness half**, promoted to the `agent-capability-encapsulation.md` feature track after its spike showed it pulls in a new bundle composition root — this ADR is the **discoverability + invariant** half, which is severable and ships first.

## Context

A storage `Cache`/`Log` op reads the clock for TTL eviction / timestamping, so the
agent handler that runs it must declare `given Clock` — but **nothing in the source
names `Clock`**. The requirement is real and derivable, yet invisible at the
declaration site: the only signal was a diagnostic at the *op* site
(`bynk.store.cache_needs_clock`) when the clause was already missing. There was no
way to *see*, at the handler's signature, what capabilities its body implies, and no
model of *why* a requirement arises.

The same problem generalises beyond `Clock`: any capability a body consumes — a
direct `Cap.op(...)` call, a store op, a builtin — is a requirement, and the editor
should be able to explain it and offer the clause. The hard part the design review
flagged: a hand-written reason like *"Cache TTL eviction reads the clock"* must be
**generatable for any capability**, including user-defined ones the compiler knows
nothing about.

Separately, the parser accepts a `by <Actor>` clause on **any** handler, including an
agent `on call` handler — where it is silently dropped (the agent emit path never
reads `by_clause`). An agent handler is reached across the agent boundary by the
factory, never from an ingress, so it has no actor; a `by` clause there is a category
error that parsed clean.

## Decisions

- **D1 — model a requirement as `{ capability, site, source }`; render the reason as
  a total function of `source`, with no per-capability text.** `RequirementSource` is
  a small closed enum:
  - `DirectCall { op }` → *"calls `Cap.op`"*. Mechanical and correct for **any**
    capability, user-defined included (`Payments.authorise` → *"calls
    `Payments.authorise`"*) — the call site *is* the explanation; no "why" is
    invented.
  - `StoreOp { kind, op }` → a fragment owned by the **storage feature**, keyed by
    `(StoreKind, op)` — the only code that knows *why* a store consumes a capability.
    The existing inline strings (`check_store_cache_op` / `check_store_log_op`) are
    promoted into one `store_reason(kind, op)` table.
  - `Builtin { feature }` → a fragment owned by the builtin's surface.

  **Decisive property:** adding a new capability requires **zero** new reason text;
  a fragment is authored only when a new capability-*consuming feature* is added (a
  store kind, a builtin) — a closed, compiler-internal set. A totality test pins the
  store table over the clock-consuming ops.

- **D2 — record every requirement, not only on absence.** `require_clock` (which
  spoke only when `Clock` was missing) generalises to `require_capability`, which
  **records every requirement** — covered or not — into a per-file ledger (the
  capability analogue of the inferred-type `HintSink`, ADR 0056) *and* still errors
  when the enclosing handler does not declare it. Direct capability calls record at
  both their covered dispatch site and their undeclared-capability error site. The
  ledger is the single producer the editor surfaces consume; it threads through the
  checker like `hints`/`locals` and drains into `ProjectDiagnostics`.

- **D3 — the "annotation" is a *materializable inlay hint*, not source syntax.** On a
  handler whose body has a requirement its `given` does not cover, the LSP renders a
  ghost clause at the declaration site — `… -> Effect[()]` `«given Clock»` — whose
  `text_edits` write the real `given Clock` via the same `given_insertion_edit` the
  undeclared-capability quick-fix uses (ADR 0054). Already-declared → no hint;
  deduplicated per handler and capability. A **source-level** `@requires(Clock)` is
  **rejected**: the requirement is derivable from `kind + @ttl`, so authoring it would
  restate an implied internal (the use-site-refinement rejection, design notes
  *Constrained refinement*).

- **D4 — reject `by` on an agent `on call` handler** (`bynk.actor.by_on_agent`). `by`
  is a service-edge clause establishing the actor from the inbound request; an agent
  handler has no actor. This turns the agent-boundary taxonomy's "actor auth never
  crosses the agent boundary" from an unenforced assumption into a checked invariant —
  a precondition the encapsulation track (the correctness half) relies on by
  construction. **Blast radius: zero** — a repo-wide sweep found no agent handler
  carrying a `by` clause; the rule is purely additive.

## Consequences

- A user-defined capability gets provenance — diagnostics, hover, and the ghost
  `given` — for **free**, with no new reason text. Only a new capability-consuming
  *feature* authors a fragment.
- The ledger is read-only infrastructure shared by the editor surfaces; the ghost
  `given` inlay hint ships here, while capability **hover** and a per-handler
  **CodeLens** reading the same ledger are a fast-follow.
- The version bump to **v0.99** is editorial (the new diagnostic + the inlay surface);
  no existing program changes behaviour, and the `by`-on-agent rule breaks no green
  test or shipped example.
- The agent capability **boundary** — the soundness fix for a capability-free handler
  calling a `given Clock` agent method (today emitting TypeScript `tsc` rejects) — is
  **not** in this increment. The spike showed it requires a new bundle composition
  root (neither target self-provisions agent capabilities today; the library bundle
  has no firstparty capability implementations), so it is settled in the
  `agent-capability-encapsulation.md` track and lands as its own increment.
