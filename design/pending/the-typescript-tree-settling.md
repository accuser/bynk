---
level: patch
changelog: Settle phase 7 of the compiler trajectory (`design/tracks/the-typescript-tree.md`) — `bynk-ts` is carved as a crate up front, the migration uses a statement-level `Verbatim` escape hatch with a companion textual lint (golden fixtures alone can't see inside it), `TsType::Any` is eliminated in full with one small named residual deferred to the runtime-typing rule, and R8 splits into five rules closed by the conversion itself, two needing their own slices, and twelve already closed
---

## ADR: typescript-tree-crate-carve-timing
title: `bynk-ts` is carved as a crate in the first slice, not built in-module and carved later
summary: Both real prior-art precedents in this codebase (`bynk-strip`, `bynk-render`) carved up front; phase 6's in-module `ir.rs` choice doesn't transfer because it lacked a second consumer, a materially different condition

**Context.** Phase 6 built its IR (`ir.rs`/`ir/lower.rs`) inside `bynk-emit` and deferred the `bynk-ir`/
`bynk-lower` crate split explicitly (ADR 0332), for lack of a second consumer. Whether `bynk-ts` should
follow the same in-module-first pattern, or be carved immediately, was open at this track's own opening.
Two real precedents exist in this codebase for carving a crate prospectively under R10.3: `bynk-strip`
(commit `868fda94`, #385 — created new, in the same PR as its only consumer, to keep `oxc` out of
`bynk-emit` and the LSP) and `bynk-render` (commit `b56f22de`, #251, `crate-decomposition` track slice 6
— created new, in the same PR that moved seven existing renderer functions out of `bynkc`). Both were
carved up front; neither was built in-module first. `bynk-render`'s own module doc states its load-bearing
invariant directly — `cargo tree -p bynk-render` is `bynk-syntax` + `ariadne` only, enforced structurally
by the crate graph. That is the same shape `bynk-ts` needs: R7.3's invariant ("the printer... is the only
code in the compiler that writes a character") is a boundary a `pub(crate)` module cannot enforce on
itself. The July review's finding #42 — 33 of 38 world-reachable `bynk_emit::emitter` items are `pub` only
to reach a sibling module — is direct, contemporaneous evidence that "enforce the boundary by convention,
carve the crate later" does not reliably happen in this codebase once code is already crate-internal.
Phase 6's own choice is not a counter-precedent: ADR 0332's stated reason was the absence of *any* second
consumer, not a preference for deferring boundary enforcement.

**Decision.** `bynk-ts` is carved as a new workspace crate in the first Arc B slice (P7.5), before any
conversion work begins. It depends on nothing but `bynk-syntax` (for `Span`); `bynk-emit` depends on it.
No circular-dependency risk exists in this shape.

**Consequences.** The crate boundary — not a `pub(crate)` convention — is what enforces R7.3/R7.4 from the
first slice that constructs a `TsProgram`. Carving `bynk-ts` immediately also manufactures the second IR
consumer ADR 0332 was waiting for, so phase 6's own deferred `bynk-ir`/`bynk-lower` split (P7.10, this
track's own §6) can happen inside this phase once Arc B lands, rather than needing a further, unscheduled
trigger later.

## ADR: typescript-tree-verbatim-hatch
title: The migration escape hatch is a statement-level `Verbatim` node with a closed origin enum and a companion textual lint
summary: Neither of this trajectory's own prior migration techniques (phase 3's parallel-data duality, phase 6's bare AST/IR coexistence) transfers, because phase 7 is the first phase where the *writer*, not an input representation, must become singular — and golden fixtures alone can't verify what a `Verbatim` block hides

**Context.** Converting ~1,540 TypeScript-producing sites cannot happen in one slice without violating
trajectory §2 ("a phase that half-lands leaves two paths reachable... the failure mode every regret in
this corpus shares"), but converting all of them atomically has no precedent at this scale either — phase
6 needed 59 slices against a smaller surface. This track's own research recovered two prior migration
techniques from this trajectory and found neither transfers directly. Phase 3's "parallel-data" technique
(`identity-and-totality.md`) kept old and new *representations* of the same fact live simultaneously,
safe because every consumer action was the same kind (read a map) regardless of which table backed it —
no behaviour depended on which representation was live at a given moment. Phase 6's IR migration used no
bridge type at all: AST-reading and IR-reading call sites simply coexisted, tracked by a per-file import
count, safe because the *output-producing mechanism* (`writeln!`) never varied regardless of which input
model fed it. Phase 7 differs structurally from both: R7.2–R7.4 exist specifically to make the *writer*
itself singular, so a bare "some sites still call `writeln!` directly" approach — phase 6's own approach
— would be a literal rule violation during this phase, not a benign coexistence. Separately, this track's
research also surfaced a risk the initial framing didn't carry: a byte-golden fixture, this migration's
only cheap correctness check across ~1,540 sites, cannot see *inside* an opaque escape-hatch node. ADR
0198 names the general shape of this failure directly — a defect survived 331 negative fixtures for 60
increments because the fixture format asserted category strings, never the actual attributed value,
"the weakest possible evidence" in that ADR's own words. A `Verbatim` block hiding `enum`/`: any`/
`namespace` would pass every golden fixture unchanged while defeating R7.1's "cannot be typed" claim.

**Decision.** The hatch is `TsStmt::Verbatim { origin: VerbatimOrigin, text: String }`: a sealed
constructor, statement granularity only (not expression-level, which would compose invisibly inside nodes
the tree claims cannot express banned constructs), tagged by a closed `VerbatimOrigin` enum with one
variant per named residue family, so the ratchet is a compile-time construct rather than a grep. The
printer still owns the buffer, indentation and offset arithmetic for a `Verbatim` block from the slice
that introduces it, so R7.3/R7.4 hold throughout the migration, not only once it completes. **A companion
textual lint over `Verbatim` content — forbidding `enum`, `namespace`, decorators, constructor parameter
properties, `: any`/`as any` by pattern match on the wrapped text — ships in the same slice as the hatch
itself (P7.5) and runs in CI alongside the golden fixtures, not as a follow-up.**

**Consequences.** Every Arc C conversion slice has a genuine, individually safe stopping point: before
`Artefacts` (P7.6) lands, stopping anywhere is safe by construction; after it, an unconverted site routes
through `Verbatim`, which the printer already owns per R7.3/R7.4, and the textual lint catches a banned
construct hiding inside one immediately rather than only at full conversion. `verbatim_origins` becomes
the second completion probe (§5 of the track doc), retiring at a named, argued floor rather than 0,
mirroring `ast_importers`'s own re-settled floor from phase 6.

## ADR: typescript-tree-any-elimination-scope
title: `TsType::Any` is eliminated in full within this phase; a 2–3-site residual is named and deferred to R7.7's runtime-typing work
summary: A site-by-site classification of every `as any` and bare `: any` occurrence found no site needs an IR extension — the earlier "48 sites, may re-open phase 6" framing was too pessimistic

**Context.** R7.1 forbids `TsType::Any` outright. The track's own opening draft treated this as a live
risk of re-opening phase 6's settled IR shape, citing `emitter/workers.rs:577`'s documented policy of
keeping `: any` on several parameter wrappers deliberately. A full classification of every occurrence —
not a sample — found the real picture smaller and more tractable. The raw `as any` count is 42, not the
48 first measured; only ~24 are real TypeScript-emission sites, the rest being Rust comments, one
unrelated English phrase, hand-written runtime `.ts` files (R7.7's own territory, not R7.1's tree), and a
test-fixture string. Separately, a grep scoped to `as any` alone was found to under-count R7.1's real
surface: 19 bare `: any` type annotations exist independently (e.g. `workers_entry.rs:771`,
`serialisation.rs:1026`, several sites in `emit.rs`'s history driver), confirmed by an independent
spot-check during this settling pass. Of all real sites, classification found roughly 20 narrowable with
zero IR work — most to `unknown` (the honest, safe target where no declared type exists, e.g. event
payloads and undeclared queue message bodies — R7.1 forbids `Any`, not `unknown`), several to a
locally-derivable structural or marker type, and a handful (dynamic handler dispatch via
`(this as any)[methodName]`, 3 sites) to a generated index-signature type built from data the IR already
carries (the resolved handler set) — new emission code using existing IR data, not a new IR field. A
small residual, 2–3 sites (`serialisation.rs:802,1026`), casts runtime-owned error types
(`ValidationError`/`JsonError`/`HttpResult`/`QueueResult`) that have no exported TypeScript type today —
this genuinely needs new runtime typing, not a checker/IR change, and falls under R7.7 rather than R7.1's
tree work. `workers.rs:577`'s policy comment is a parameter-provenance argument (params mix
codec-produced and route/query-string values) that phase 6's IR did not change and this decision does not
overturn — those wrappers gain a real structural type in place of `any`, not a removal of the wrapper.

**Decision.** `TsType::Any` is eliminated in full within this phase. The 2–3-site runtime-error-type
residual is named explicitly and deferred to R7.7's own runtime-typing work (Arc C's `serialisation.rs`
conversion slice), not treated as open-ended, not treated as grounds to reopen phase 6's `IrItem`/`TyId`
shape.

**Consequences.** Arc A's P7.2 narrows roughly 20 sites ahead of the tree's existence — a plain text
change requiring no `bynk-ts` infrastructure, a real correctness win (closing finding #18's `tsc --strict`
disarming) available immediately rather than gated on Arc B. P7.8 (the tree's own `TsType` enum,
containing no `Any` variant) is correspondingly lower-risk than the draft assumed, since most sites are
pre-narrowed by the time it lands. The `ts_any` probe (P7.0) must scan for `as any` **and** bare `: any`,
per this decision's own measurement correction, or it will under-report R7.1's real remaining surface.

## ADR: typescript-tree-r8-scope
title: R8.1–R8.22 splits three ways — twelve already closed, five closing as a byproduct of this track's own conversion, two needing named separate slices, one shared with phase 8
summary: A rule-by-rule audit against the current tree, not the review's stale findings, replaces an unscoped "R8 residue, TBD" placeholder with a precise accounting

**Context.** The trajectory names both R7.1–R7.8 and R8.1–R8.22 as phase 7's reference rules, but R8 is
chiefly emission-*semantics* — much of which plausibly moved once phase 6's `IrItem`/`Callee`/
`CommitShape` existed as real types, rather than waiting for this phase's tree/printer split. Treating all
21 rules (R8.2–R8.22) as open by inheritance risked this track claiming, and being measured against, work
it doesn't actually need to do. A rule-by-rule audit against the current tree — not the July review's
stale findings — found a three-way split, not the draft's assumed closed/open binary: **twelve rules read
CLOSED** (R8.5, R8.7, R8.9, R8.11, R8.12, R8.13, R8.15, R8.17, R8.18, R8.19, R8.21, R8.22), each with
direct file:line evidence in the current tree; **five read PARTIAL** — behaviourally correct but
structurally sourced from the AST or ad-hoc collection rather than the IR (R8.3's `is_opaque` branches at
five emission call sites instead of reading a pre-decided shape; R8.6's `CommitShape` exists exactly as
specified in `ir.rs` but has zero consumers, with emission still re-deriving the same distinction
independently; R8.8's invariant/transition ordering is exact but iterates the raw `AgentDecl`; R8.10's key
mangling is a single pure function but lacks the rule's own required stated inverse; R8.16's per-consumer
surface generation is already correct but built from an untyped `HashMap`, not a `ProjectGraph`) — each of
these is naturally finished by this track's own tree/printer conversion, needing no separate construction;
**two read genuinely OPEN**, separately scoped (R8.2's brand prefix is computed at emission rather than
read from a recorded brand; R8.14's codec collector still walks raw AST, with its own doc comment
recording that an IR-based conversion was investigated and declined at P6.56 — worth revisiting once a
tree exists to collect over instead). R8.20 was found to be the identical defect to R7.6, already in this
track's scope, not separate work. R8.16's PARTIAL finding splits down the middle with phase 8, which
already owns a typed `ProjectGraph` by name per phase 4's own retirement note.

**Decision.** This track's R8 scope is: the two OPEN rules (R8.2, R8.14) as named Arc D slices; the five
PARTIAL rules close automatically as Arc B/C's own conversion lands, verified rather than separately
built; the twelve CLOSED rules get a single verify-only pass at retirement, with R8.12 flagged explicitly
as self-superseding the moment R7.1's `Any` elimination lands (closed today under its own current text,
but the rule's meaning changes once `Any` is gone — a point worth a reviewer's explicit attention, not a
silent transition that could read as regression). R8.16's data-model half stays phase 8's.

**Consequences.** This is the most load-bearing of this settling pass's four decisions in terms of scope
control — it prevents the track from silently absorbing work several of these rules' own evidence shows
is either already done or genuinely belongs to a different phase. P7.17 (§6 of the track doc) is the
explicit slice this verify-only pass and the R8.12 self-supersession note both ride on.
