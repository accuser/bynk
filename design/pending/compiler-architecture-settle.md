---
level: patch
changelog: Settle the compiler-architecture track — the refactor acceptance gate, the emit-ABI posture, and the lowering substrate
---

## ADR: refactor-acceptance-gate-per-tier
title: The refactor acceptance gate is per-tier, not byte-identical goldens alone
summary: Amends ADR 0059 property 1 — the golden gate is insufficient where the moved code is not pure helpers

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

## ADR: emit-abi-not-published-before-1-0
title: The emit ABI is not published before 1.0, and its eventual shape is lockstep with a fail-closed check
summary: Vendored bindings stay vendored; when published, exact-version lockstep plus a refused-on-mismatch runtime check

**Context.** [[0086]] made the first-party sources real files under `bynk-check/src/firstparty/`,
embedded by `include_str!`, and was explicit about what they are: "The bindings and runtime are part
of the compiler's **emit ABI** — coupled to emit shapes (`Result`/`Option` tag layout, `JsonError`,
`Uuid.of`, `FetchError`). Vendoring … makes version skew impossible by construction." It deferred the
question: "publishing is deferred to a future ADR, gated on runtime-ABI stability (≈1.0)."

That gate is now in sight, and it collides with a commitment already made. `bynk-1.0-definition.md`
states that stability does **not** freeze "**The emitted TypeScript.** The compile target is an
implementation detail, not part of the frozen contract; the codegen may improve within a 1.x release
as long as documented behaviour holds."

Both cannot hold once a third party authors a capability binding. A binding is hand-written
TypeScript that constructs `Ok(…)`, reads `.tag === "Err"` and calls `Uuid.of` — and that the Bynk
compiler never type-checks against a changed emit shape. A 1.x codegen improvement would then break
third-party capabilities silently, at runtime.

The pressure toward publishing is real and is the collaboration story: a capability adapter written
outside this repository needs something to import.

**Decision.** The first-party bindings and the runtime are **not published before 1.0**. They stay
vendored, and [[0086]]'s "version skew impossible by construction" property is retained.

**And the eventual shape is recorded now**, so that publishing later is a decision rather than a
drift: when they are published, the dependency is **exact-version lockstep with the compiler** — not
a semver range — plus a **fail-closed runtime-version check at load**, refusing a mismatch rather
than tolerating it. This is [[0200]]'s contract-hash pattern applied to the ABI: a skew becomes a
legible, named error at the boundary instead of a miscompile discovered in production.

**Consequences.** Publishing on semver-stable terms would freeze the emit ABI at 1.0 by implication.
That is a *language stability* decision wearing a packaging decision's clothes, and it would have to
be argued in the 1.0 record rather than arrived at by shipping one third-party adapter. Recording the
lockstep shape closes that path without closing the option.

Third-party capability adapters therefore remain in-repo or vendored until the future ADR
[[0086]] promised. The tier taxonomy that separates a substrate-free capability from one requiring a
runtime ABI is described in `design/bynk-greenfield-compiler.md` Part 14; the extension point it
names (E7, transaction participation) is the first thing that would *require* a published ABI, and it
is not scheduled.

The cost is stated plainly: the collaboration story for capabilities is deferred with the
publication. The mitigation is that the first-party surface is already ordinary vendored `.bynk`
source (`adapter bynk { … }`, eight capabilities, bodiless providers), so an outside contributor can
read and copy the pattern even where they cannot yet depend on it.

---

## ADR: the-lowering-substrate
title: The emitter lowers to TypeScript text on purpose; here is the cost and the trigger to stop
summary: The substrate decision that was never recorded, made explicit, with its amendment and its replacement triggers

**Context.** The compiler goes from typed AST to TypeScript text with no intermediate
representation. This is the single most consequential implementation decision in the project, and
**no decision record argues for it.** The claim lives in crate rustdoc, and it is not accurate: there
*is* an intermediate representation, and it is the pair `(String, &mut Vec<String>)` —
`lower_expr(e, stmts, cx) -> String` returns an expression's text and appends any statements that
must run before it to a vector the caller supplies. Twenty-seven signatures in
`bynk-emit/src/emitter/lower.rs` carry that sink.

[[0059]] §4 is the mechanism by which this went unrecorded: "**No new per-increment ADRs.** Refactors
are not language-defining, so they do not each earn a decision record." That is correct for a file
split and wrong for a representation. The decision-record culture was scoped by *topic* — the
language — rather than by *consequence* — irreversibility. So the costliest implementation decision
was never written down, never argued, and never re-examined when its cost curve crossed.

The 2026-07-27 pipeline review is the accounting. From one property — that nothing in the type forces
a caller to consume what it was given — follow: statements dropped at two ternary sites; statements
spliced into an expression position; `lower_match_as_iife` wrapping hoisted discriminant statements
in a fresh synchronous arrow, so `let x = match risky()? { … }` early-returns into the wrapper and
the match silently evaluates to the `Err` object ("a miscompile of ordinary code with no diagnostic
and no assertion"); and `lower_bin_op` lowering both operands into one vector, defeating the
short-circuit property `bynk-type-system.md` says "developers can rely on".

**Decision.** Three parts.

**D1 — The substrate is recorded as deliberate.** Lowering directly to TypeScript text was the right
choice for a language being discovered alongside its compiler. Text is cheap to change; an IR node is
a five-place edit. The project reached this design surface in roughly a year at least partly because
its emitter imposed no representational tax on a moving language. This ADR records that as a choice,
not an accident.

**D2 — The substrate is amended, not replaced.** `lower_expr` returns `Lowered { pre: Vec<String>,
expr: String }` rather than taking a sink. Every caller must then say what it does with `pre`; the
two ternary sites become compile errors until they bail to the IIFE form or hoist into the enclosing
block, and `lower_and_with_is` cannot flatten statements into a string because `Lowered` is not a
`String`. Async-ness is decided by a flag computed during lowering rather than by asking whether the
generated text contains `"await "`.

This is a *signature* change, not a representation change. It does not adopt an intermediate
representation and does not commit the project to one.

**D3 — The triggers for replacing the substrate are recorded.** A typed IR — resolved names, types
attached by construction, an explicit statement/expression distinction — is the end state described
in `design/bynk-greenfield-compiler.md` Part 6. It is not scheduled. It becomes warranted when one of
the following is observed:

- a defect class recurs in the lowering after being patched once at a different site (this has
  already happened once, at `maybe_async_iife`, and was not recognised as a signal);
- a documented language-level semantic property — short-circuit, evaluation order, atomicity — is
  found violated by the emitter rather than by the checker;
- the emitter's in-file test-line ratio does not rise in the two releases after a crate-local test
  seam exists;
- a second consumer of the emitted artefact appears (a second target, a debugger, a course).

**Consequences.** [[0059]] §4 stands for file splits and is narrowed: a decision about the
compiler's **representation** earns a record before code, on the same terms as a decision about the
language. The trigger for a record is irreversibility, not topic.

D2's amendment closes four defects in one change and is mechanical across roughly ninety functions.
Its completion criterion is that no function in `bynk-emit` takes `stmts: &mut Vec<String>` — a
mechanical probe, not a judgement.

D3's third trigger is phrased as a hypothesis test rather than a threshold. The stated blocker on
emitter testing is the missing seam; if density still does not move once the seam exists, the blocker
was structural, and that is the evidence a substrate replacement would need. An arbitrary percentage
would not have supplied it.

Nothing in this ADR authorises building the IR. It records what the substrate is, amends it where the
amendment is cheap and the defects are live, and states what would have to be true before the
question is reopened.
