# Bynk — Adoption Sequencing & the Tooling Freeze

*Decision record for [#540](https://github.com/accuser/bynk/issues/540) §7(2).
Sibling to [`bynk-positioning.md`](bynk-positioning.md): that record settled *what
Bynk is* (a production service-tier language); this one settles *what to build
next given that*. A strategy record, not a language-defining call — it lives here,
not in [`decisions/`](decisions/README.md). Per the tracking issue, these are
decisions, not designs: the track docs the decision authorises are downstream.*

---

## The problem this settles

The forward roadmap ([`bynk-status-and-roadmap.md`](bynk-status-and-roadmap.md)
§6) orders the next work as **language** tracks: an Events track, then
sagas/compensation, the query algebra and rich storage kinds, agent invariants,
and held connections. That is the deferred **v1 coordination layer** — the
aspirational language vision the design notes have always scheduled.

Every one of those adds *expressiveness*. Not one of them makes Bynk *adoptable*.
A team evaluating Bynk today hits three walls before language expressiveness is
ever the deciding factor:

1. **You cannot ship.** `doctor → new → dev` runs a project locally; there is no
   `deploy`. Taking a project live is back to the manual ritual `dev` retired for
   local — create KV namespaces by hand, paste ids into a regenerated
   `wrangler.toml`, apply DO migrations, set secrets, `wrangler deploy` each
   context in order (see [`tracks/deploy.md`](tracks/deploy.md) §1).
2. **You cannot evolve stored state safely.** The moment a deployed agent owns
   real state, the next breaking change to that state's shape is unshippable
   without a migration story. Bynk has schema-versioning *in the design notes*
   and nothing shipped.
3. **You cannot share or depend on code.** There is no packaging identity, no
   dependency model, no registry — no ecosystem posture. A language you cannot
   publish to or consume from is a language you use alone.

Until these three exist, more language surface is polishing a language nobody can
put in production. That is the review's finding, and it is correct.

## The decision

**Sequence the three adoption blockers — deploy → migrations → ecosystem
posture, in that order — ahead of the v1 coordination-layer language tracks. And
freeze tooling depth.**

The language-vision tracks are not cancelled; they are *reordered behind* the
blockers. They remain the language's future. They are simply not what moves
adoption now, and adoption is the bottleneck.

### Why this order

The order is not arbitrary — each blocker *creates the need* for the next:

- **Deploy first.** Nothing happens in production without shipping, so this gates
  the other two (you cannot need a migration for state you never deployed, nor an
  ecosystem for code you cannot run). It is also the **furthest along**: the track
  doc is written and settling, the spine issue is open
  ([#558](https://github.com/accuser/bynk/issues/558)), the provisioning-state
  model is worked out. It is closest to ready and unblocks the most.
- **Migrations second.** Deploy *manufactures* this need: the first deployed
  stateful agent turns its next breaking state change into an unshippable event
  without migrations. This is Bynk-level **state-schema evolution**, and it is
  distinct from the platform DO-migration application that deploy's slice 1
  already handles ([`tracks/deploy.md`](tracks/deploy.md) §4.2) — that wires up
  Cloudflare's own migration mechanism; this is evolving the *Bynk* state schema a
  regenerated codebase reads. Deploy makes stateful agents *shippable*; migrations
  make them *evolvable*.
- **Ecosystem posture third.** It matters once there are several real projects to
  share between — a slower-burning need than "can I ship at all". It also shares
  deploy's identity model: deploy already has to assume the packaging naming model
  ([`tracks/deploy.md`](tracks/deploy.md) Q8 — `org.package.context`, so a rename
  does not orphan provisioned state), and that is the same identity the ecosystem
  work builds out. Sequencing it last lets deploy's naming and the packaging
  identity ADR land in the right order rather than fighting.

### Freeze tooling depth

Editor tooling and playground depth are **already far past what adoption
justifies**. The LSP shipped the full A/B-tier arc — diagnostics, the binding
index, completion, signature help, inlay + semantic tokens, codeLens, call
hierarchy, implementation navigation, folding/selection
([`bynk-tooling-roadmap.md`](bynk-tooling-roadmap.md) §1). That is a modern
language server for a language you cannot yet deploy. The imbalance is the point.

**Freeze means:** no net-new tooling *depth* until the adoption blockers ship.
Concretely —

- **Frozen:** the remaining LSP backlog beyond currency — locals-rename +
  generic-parameter indexing, type-definition navigation, test-run CodeLens,
  `inlayHint/resolve`, semantic-tokens delta, incremental recompute
  ([`bynk-tooling-roadmap.md`](bynk-tooling-roadmap.md) §7.1–7.2) — and any new
  advanced editor or playground feature. These are real, but they are polish on a
  language whose adoption is gated elsewhere.
- **Not frozen — this is currency, not depth:** the **keep-tooling-current
  standing rule** stays in force. Each language increment still enumerates and
  pays its tooling delta (LSP/fmt/tree-sitter), and the **tree-sitter catch-up**
  to the current surface (`from <protocol>`/`on http`, `assert`-expr,
  `test`/`mocks`, `HttpResult`, actors — [`bynk-status-and-roadmap.md`](bynk-status-and-roadmap.md)
  §5) is hygiene that keeps the *existing* surface honest, not new depth. Freezing
  currency would let the tooling rot; that is not the intent.

The distinction is: **currency keeps what exists true; depth adds new capability.**
Depth is frozen; currency is not.

## Where each blocker stands today

| Blocker | State today | The move |
|---|---|---|
| **1. Deploy** | Track doc thorough and settling; spine [#558](https://github.com/accuser/bynk/issues/558); **no slice authorised** ([`tracks/README.md`](tracks/README.md)). | Promote from *settling* to **slicing**: authorise slice 0 — the provisioning-state ADR + KV-only single-context MVP ([`tracks/deploy.md`](tracks/deploy.md) §8). This is the top priority. |
| **2. Migrations** | **No track.** State-schema versioning lives only in the design notes as a deferred v1 concept. | Open a **state-migrations track** (spine issue + settling doc, per [ADR 0167](decisions/0167-feature-tracks-run-github-native.md)). Sequenced to begin as deploy's stateful slices land, since deploy creates the need. |
| **3. Ecosystem posture** | The **`packaging.md` track is referenced but unwritten** ([`tracks/deploy.md`](tracks/deploy.md) Q8; [`tracks/documentation.md`](tracks/documentation.md)). | Write the **packaging track**: the `org.package.context` identity model, a dependency/manifest model, and a registry posture. Deploy's naming must assume this identity (Q8) so the cutover does not orphan provisioned state. |

## Interlock with the 1.0 definition (§7(4), not resolved here)

The review's separate §7(4) call defines **1.0 = Foundations-layer stability +
deploy + state migrations**. Two of the three blockers here are *literally the 1.0
bar*; ecosystem posture is adoption-critical but 1.0-optional (a language can hit
1.0 before its registry exists). So this sequence **is** the road to 1.0: ship
deploy and migrations and Bynk is at the 1.0 line; the ecosystem work carries past
it. That interlock is why the order puts the two 1.0-gating blockers first.

## What this defers, and what it does not touch

- **Deferred (not cancelled):** the v1 coordination-layer language tracks —
  Events, sagas/compensation, the query algebra and rich storage kinds, agent
  invariants, held connections. They stay the language vision and move behind the
  blockers. The one carve-out: if a blocker genuinely required new language
  surface it would ride with it — but none does. Deploy is driver + provisioning,
  migrations is state-schema + platform, packaging is identity + tooling; all
  three are buildable **without** touching the language the coordination layer
  would extend.
- **Not touched:** the shipped language and its `tsc --strict` quality gate; the
  keep-current tooling rule (above); the per-increment ADR/spec discipline.

## Out of scope here

This record settles only §7(2) — the blocker sequence and the tooling freeze.
The tracking issue's other §7 calls (the 1.0 definition itself, monthly milestone
cadence, README/about drift guards, the honest comparison page, the
two-production-deployment 1.0 bar) remain open on [#540](https://github.com/accuser/bynk/issues/540)
as separate decisions.
