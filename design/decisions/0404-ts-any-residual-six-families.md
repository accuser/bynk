# 0404 — ts_any's non-collection-kernel residual is fully argued across six families; the Durable Object stub is not excluded

- **Status:** Accepted (v0.289.25)

**Context.** ADR 0399 (#1423) named a 23-site "honest majority" (bucket 3) and a 1-site DO-stub
question (bucket 4) as needing individual, file:line-grounded arguments before R7.1 could
honestly read "eliminated in full, named residual" — authorising this writeup as a follow-on,
never done. Arc E has since landed, moving `ts_any`'s own per-file distribution (serialisation.rs
2 → 1, `project/tests_emit.rs` 11 → 12; net total unchanged at 30) — this pass re-verifies
against current `main`, not ADR 0399's own now-stale table. `#1460` grounds the remaining
4-site collection-kernel group (`lower.rs`'s `distinctBy`/`joinOn`/`leftJoin`/`groupBy`)
separately; this ADR covers the other 26.

**Decision.** All 26 sites collapse into six families, not 26 independent cases — most already
carrying an explicit in-place reason; three (`gen_descriptor_entry`'s own `rng`/`v`/`v` arrow
params) had a comment for *what* the shape is but not *why* `any` specifically, argued fresh
here:

1. **Cross-context event/dispatch qualification (5 — `emitter/workers.rs:1068,1070,1893`,
   `project.rs:3298,3300`).** Each already documents a specific, previously attempted and failed
   fix (`workers.rs:1068`'s own comment: two distinct qualification attempts, each broke a
   different real `tsc --strict` fixture). Genuinely blocked on tracing exactly where a
   cross-context event's synthetic type is exported from and building a real qualification
   scheme for it — real, separate design work, not a same-line change. The closest thing to a
   tractable candidate among all 26, but not tractable *today*.
2. **`emitter/lower.rs`'s two non-kernel P7.2-deferred casts (2 — lines 2143, 2172).** Each
   blocked on a distinct, named plumbing gap: `2143`'s brand cast needs `workers.rs`'s
   `qualified_type_ref` reachable from this module (it isn't); `2172`'s identity cast needs the
   addressed actor's own identity type threaded from `bynk-check` (not plumbed here today).
3. **`emitter.rs:4259` (`unchecked_construct_test`).** Test scaffolding imports the branded
   type's name only as an `any`-typed value binding, never as a type — there is no real type to
   cast to at this call site. Fixing it needs restructuring what the test scaffold imports, not a
   same-line text change.
4. **`emitter/workers.rs:866`, the Durable Object stub — corrected, not excluded.** ADR 0399
   tentatively recommended treating this as an ambient third-party type, the same footing as
   `verbatim_sites`' `adapter_bindings`/`runtime.ts` exclusions. Direct read finds this
   characterisation wrong: it is a **Bynk-authored** `TsDecl::TypeAlias` fallback (not copied
   foreign text) — emitted only when no real `DurableObjectNamespace` import is in scope — and it
   already carries its own P7.2-deferred reason (`workers.rs:851-858`): a real, differently-shaped
   imported `DurableObjectNamespace` (from `./runtime`) can appear in the same file as this local
   fallback, sharing a name but not a shape; reconciling them needs a real import/alias or a
   rename, not a guessed structural type. Governed by R7.1 like every other site in this list —
   **not excluded from `ts_any`'s scope.**
5. **`emitter/serialisation.rs:1867` (`UncheckedReason::Effect`).** Already argued in its own doc
   comment (`:1856-1864`): narrowing to `unknown` would compile but silently drop the fact that
   this arm is still genuinely open — worse than a visible `any`. Not part of the 2-site "closes
   via Arc E" bucket ADR 0399 named (only one of that pair actually closed, via a real narrowing;
   this is the other, and it was never claimed to close).
6. **Property-test driver/replay machinery (16 — `project/tests_emit.rs`'s 12 +
   `emitter/emit.rs`'s 4) — one family, not two.** Both build machinery whose real type varies
   per-binding or per-handler across an unbounded, user-declared property-test surface:
   `tests_emit.rs`'s `env_{ns}`/`rootEnv` (needs cross-referencing each participant's own
   generated `Env` type), `deps.surface` mock access (needs cross-referencing separately-built
   mock shapes), `__vals`/`__where`/`__body` (heterogeneous bindings, some `bigint`-represented),
   and `target_ns`/`deps` in the history driver (the callee's own `deps: any` param is itself
   deferred, `emit.rs`'s own driver-signature narrowing) all say so explicitly in place.
   `gen_descriptor_entry`'s own `rng: any`/`v: any`/`v: any` (lines 4183/4217/4231) had no
   individual reason recorded — argued fresh here: `rng` is a property-testing-library-supplied
   generator function whose real type is generic across every binding's own declared type, and
   `v`'s shrink/show candidate value is likewise per-binding; both are the same "arbitrary shape
   across an unbounded test-property surface" reasoning `emit.rs`'s own four sites already state,
   not a distinct gap. `emitter.rs`'s own four sites (`step_ty`'s `call`, `deps`, `__inst` cast,
   `__call`) are already argued in place: a real type needs the union/intersection of every
   handler this agent's history driver targets, not a same-line change.

No site among the 26 turned out newly tractable on this pass. Family 1 (cross-context
dispatch) is the closest candidate — every site already names the missing piece (a real
cross-context type-export/qualification scheme) rather than "never attempted" — but designing
that scheme is real, separate work belonging to its own future proposal, not this settling pass.

**Consequences.** `ts_any`'s argued floor for these 26 sites is **26** — none excluded, none
closed. Combined with `#1460`'s own outcome (the 4-site collection-kernel group), the track's
overall `ts_any` floor lands between 26 (if `#1460` narrows all four) and 30 (if it finds the
element type genuinely out of reach too) — not 29/28 as ADR 0399's own provisional DO-stub-
exclusion language implied. `design/tracks/the-typescript-tree.md`'s Floor-correction section
(§6, Part 2) is corrected to this six-family breakdown, replacing ADR 0399's now-stale per-file
table and retracting its DO-stub exclusion recommendation.
