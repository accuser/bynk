---
level: patch
changelog: "P6.34: closed by investigation, no code change needed (P6.9/P6.24b precedent). The completion plan's own P6.34 row proposed precomputing EmitProjectCtx::actors into a per-handler ActorSeamIr, describing it as a hidden dependency blocking emitter/workers.rs, emitter/workers_entry.rs, and emitter/emit.rs. Traced directly: only emit.rs reads this field (workers.rs/workers_entry.rs reach actor data through the unrelated bynk_check::symbols::UnitTable::actors, already counted independently) -- the \"blocking three files\" framing did not hold. Even for emit.rs, lower_actor_seam_ir's own signature requires the raw actor declarations to do its own by_clause-binder resolution; precomputing would relocate, not remove, the AST dependency (project.rs is already counted for unrelated reasons, so this would not move ast_importers even if built), while introducing a new indexing mechanism into a security-sensitive, fail-closed identity-verification path (Bearer/Oidc/Signature/Caller seam resolution) for zero probe benefit. Not pursued. ast_importers unaffected (7)."
---

## ADR: actor-seam-investigation

title: `EmitProjectCtx::actors` precomputation investigated and declined — relocates, not removes, the AST dependency, at real risk to a security-sensitive path

summary: Phase E of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.34) — closed by investigation rather than shipped, per the P6.9/P6.24b precedent for a slice whose own premise doesn't survive contact with the tree

**Context.** This row proposed converting `EmitProjectCtx::actors: HashMap<String, ActorDecl>`
(`project.rs:3478`; the plan's own row named it `ModuleCtx::actors`, a naming slip — `ModuleCtx` itself
carries no `actors` field) into a precomputed per-handler `ActorSeamIr`, describing it as "a hidden
multi-file dependency... threaded to `workers_entry.rs` and read by `emit.rs`, blocking three files at
once."

**Traced directly: the dependency graph is smaller than described.** `EmitProjectCtx::actors` is
populated once, by cloning `info.table.actors` (`project.rs:1250`), and read at exactly three sites,
all in `emitter/emit.rs`: `lower_actor_seam_ir(handler, &ctx.actors)` (per-handler Bearer/Oidc/Sum/
Caller seam resolution, called once per handler during body emission) and
`any_service_binds_caller`/`caller_binder_for` (a project-wide "does any service bind a `Caller` actor"
check). `emitter/workers.rs` and `emitter/workers_entry.rs` never touch this field — both reach actor
data through an entirely separate route, `bynk_check::symbols::UnitTable::actors`, threaded via their
own `table: &UnitTable` parameter (the same parameter `emit_worker_compose`/`emit_worker_entry` already
receive), and that surface is already counted independently in each file's own `ast_importers`
contribution (both already import `ActorDecl` in their own P6.26 explicit lists). The "blocking three
files" framing does not hold.

**Even scoped to `emit.rs` alone, precomputing relocates the dependency rather than removing it, for
no probe benefit.** `lower_actor_seam_ir` (defined in `ir/lower.rs`, an excluded file) requires
`&HashMap<String, ActorDecl>` as its own parameter — matching a handler's `by_clause` binder name
against declared actors is inherently a raw-declaration lookup, the lowering pass's own necessary
input, the identical shape P6.33's Decision 1 already established isn't this track's fixable target
(the excluded pass needing raw AST to do its own job is not the defect R6.13 tracks). Moving the call
from `emit.rs` (per-handler, at use time) to `project.rs` (at `EmitProjectCtx`-build time, keyed by
some stable `HandlerKey` — this row's own flagged "needs its own ADR on the handler-key shape") would
not remove `ActorDecl` from the picture: `project.rs` is already counted for other, unrelated reasons
(P6.35's own remaining rows, `own_contract_hashes`), so `ast_importers` would not move even if this
were built.

**Decision: not pursued.** Weighed against that zero probe benefit, `lower_actor_seam_ir` resolves
Bearer/Oidc/Signature/Caller identity verification — a fail-closed, security-sensitive seam, per the
handler's own standing comments in `emit.rs`. Introducing a new per-handler indexing/lookup mechanism
here (the `HandlerKey` this row's own ADR question would need to settle) carries a real correctness
risk this track's own scope does not require taking on: a wrong key mapping would silently apply one
handler's own verification seam to another. `emit.rs`'s own `ActorDecl` import remains, alongside its
own larger, genuinely open declaration-read surface (this plan's own Phase E/F rows) — this finding
does not change `emit.rs`'s eventual floor either way, and is orthogonal to that remaining work.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — no code change. This is the P6.9/P6.24b
shape of closure: a slice's own premise investigated directly against the tree and found not to hold,
recorded so a future reader does not re-attempt the same estimate.
