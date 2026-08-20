# 0387 — `TsType::Any` is eliminated in full within this phase; a 2–3-site residual is named and deferred to R7.7's runtime-typing work

- **Status:** Accepted (v0.249.37)

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
