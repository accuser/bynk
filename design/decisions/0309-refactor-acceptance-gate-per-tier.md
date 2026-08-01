# 0309 — The refactor acceptance gate is per-tier, not byte-identical goldens alone

- **Status:** Accepted (v0.246.1)

**Context.** [[0059]] property 1 states: "**Behaviour-preserving.** The Bynk language and the
compiler's observable output are unchanged. The acceptance gate is the existing golden fixtures
passing **byte-identical and unedited**."

That gate did its job for the crate-decomposition track, and for a specific reason: the code those
slices moved was pure helpers. `project.rs` and `checker.rs` split cleanly because their movers were
string and path functions with value-in/value-out signatures, so a golden that stayed byte-identical
really did establish behaviour preservation.

It does not hold for the emitter. `emitter.rs` and `lower.rs` have almost no pure helpers — their
units are `fn(&mut String, &Ast, &mut LowerCtx)` — so a restructuring lands with whole-file goldens,
one crate up, driven from disk, as its only net. When a golden breaks, the diff is an entire emitted
TypeScript file and the failing test lives in `bynkc` rather than in the crate being changed.

Two shipped precedents show the gate passing while the change was wrong. [[0198]]: "**No fixture
asserts an attributed path.** `expected_error.txt` lists *category strings only*. The e2e suite has
**331 negative fixtures** and not one of them can observe which file a diagnostic was blamed on — so
the identity could be wrong for every split project and every test would still pass. It did, and they
did." And its verdict on exactly this question: "'the gate is green' is the weakest possible evidence
here." [[0201]] records the second: converting the keyed sinks by grep, "`build_file_decl_index` is
the one that proves the point, because converting it looked right and was not … the failure is a
**hang**, not an assertion."

**Decision.** The acceptance gate for a behaviour-preserving refactor is stated per tier, and
escalates with the structural depth of the change:

| Tier | Gate |
|---|---|
| Enablers | the tier's own artefacts exist and are exercised by at least one fixture each |
| Paydown | byte-identical goldens, **plus** a crate-local fixture per behaviour change. A slice that changes a diagnostic must add an `expected_diagnostics.txt` assertion (`code<TAB>path:line:col`) |
| Structural | crate-local fixtures over the in-memory `sources` seam, **plus** a named regression fixture per closed defect, **plus** byte-identical goldens |
| Layering | as Structural, plus the tier's mechanical completion probe reading zero |

A tier's completion criterion is that **the old path is deleted and its probe reads zero** — not that
the new path exists. A slice that leaves both paths reachable is not done.

**Consequences.** ADR 0059's property 1 is amended, not replaced: behaviour preservation is still the
standing property, and byte-identical goldens are still part of every gate above the enabler tier.
What changes is that goldens are no longer the *whole* gate.

The gate has a prerequisite: crate-local fixtures require an in-memory source seam
(`CompileOptions.sources`), which is why the enabler tier cannot be preceded by anything.

The escalation is deliberate: the paydown tier is cheap to gate because its changes are local, and
the layering tier is expensive to gate because a moved diagnostic is exactly the class [[0198]] shows
the fixture suite cannot see.

---
