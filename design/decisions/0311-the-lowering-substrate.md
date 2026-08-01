# 0311 — The emitter lowers to TypeScript text on purpose; here is the cost and the trigger to stop

- **Status:** Accepted (v0.246.1)

**Context.** The compiler goes from typed AST to TypeScript text with no intermediate
representation. This is the single most consequential implementation decision in the project, and
**no decision record argues for it.** The claim lives in crate rustdoc, and it is not accurate: there
*is* an intermediate representation, and it is the pair `(String, &mut Vec<String>)` —
`lower_expr(e, stmts, cx) -> String` returns an expression's text and appends any statements that
must run before it to a vector the caller supplies. Twenty-nine signatures in
`bynk-emit/src/emitter/lower.rs` carry that sink (32 workspace-wide: 29 in `lower.rs`, 2 in
`emitter.rs`, 1 in `emit.rs`).

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
