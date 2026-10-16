---
level: minor
probes: diagnostic_coverage
changelog: `let <pattern> = value else { … }` (and `<-` for an effect's result) binds a pattern's names for the rest of the block, and yields the `else` block, or the arms of `else match { … }`, when the value does not match, so a chain of fallible steps reads top to bottom with each failure handled beside its step
---

## ADR: let-else-is-a-block-scoped-match
title: `let <pattern> … else …` is the `match` it stands for, scoped to its block
summary: `let P <- e else E` followed by the rest of the block means `match` on `e` with `P => { rest }` and `_ => E`; the parser builds that `match`, so it adds no control flow, and the `else` has the enclosing block's type

**Context.** A handler that runs several fallible steps, each of which must undo
earlier work or choose a response when it fails, could only be written as a
staircase of `match … { Err => …, Ok => { … } }`. The `PUT /orders/:id` handler in
`bynk-lang/shop` reached eleven levels of braces for four steps. Every flatter
spelling was refused: agent and cross-context calls do not resolve outside a
handler, a cross-context call in a combinator lambda is
`bynk.effect.cross_context_in_pure_context`, there is no `return`, and `?` on a
`Result` does not propagate out of an `HttpResult` handler. Handler atomicity
(ADR 0109) is per agent, so a service handler that touches several agents and
contexts has to compensate by hand, and the staircase is the shape every such
handler takes. Proposed and accepted in #2010.

**Decision.**

- **A — The `else` exits the enclosing block.** `let P <- e else E`, followed by
  the statements `S…` and tail `t` of its block, means exactly
  `let v <- e` then `match v { P => { S… t }, _ => E }`, with `v` a name the
  program cannot write. `let P = e else E` is the same with `=`. `E` therefore
  has the type of the rest of the block, not of the function's return. This is
  the staircase authors already write, so it adds no control flow, works the
  same in pure and effectful bodies and in nested blocks, and inherits every
  existing check and the emitter's lowering, including ADR 0437's statement-form
  route (a `?` in `E` returns from the function). A function-level early exit,
  as in Rust's or Swift's `let … else`, was rejected: it adds non-local control
  flow, Bynk has no `Never` type to check that an `else` diverges, and the two
  readings differ inside a nested block. In a handler body, where the problem
  is, they agree.
- **B — `else match { arms }` binds what did not match.** A plain `else` block
  cannot name the value; `else match` takes `match` arms over the values `P`
  leaves, and they follow the pattern's arm unchanged. Arms written directly
  after `else` (`else { Err(why) => … }`) were rejected as ambiguous with a block
  whose tail is a lambda.
- **C — A pattern must open as a pattern.** After `let`, a literal, `(`,
  `Ok`/`Err`/`Some`/`None`, or a name followed by `(` or `.` starts a pattern; a
  bare name, capitalised or not, stays a binding. No existing program changes
  meaning. A nullary variant is written qualified (`Status.Done`) or in
  parentheses (`(Done)`), and the formatter keeps those parentheses. A pattern
  `let` with no `else` is `bynk.parse.let_pattern_without_else`; an `else` after
  a plain binding is `bynk.parse.let_else_on_binding`. Irrefutable destructuring
  (`let { a, b } = r`) is not part of this decision.
- **D — No annotation on a pattern `let`.** `let Ok(x): T = …` does not say
  whether `T` is the value's type or `x`'s, so it is
  `bynk.parse.let_pattern_annotation`.
- **E — The parser builds the `match`.** The proposal placed the desugaring in a
  lowering step. The checker, emitter, formatter and editor all walk the AST
  directly, so the parser builds the hidden binding and the `match` itself,
  keeping every source span, and marks the binding with `LetStmt::let_else`
  (`LetElse { form, else_span }`). The formatter prints the sugar back from that
  marker. The checker recognises a desugared `match` by its scrutinee, a
  `__letElse<offset>` name no user identifier can collide with (identifiers
  start with a letter), and words its diagnostics in the author's terms. A new
  `Statement` variant was rejected: every exhaustive statement walk would need a
  case that only re-derives the same `match`.
- **F — Existing type codes, reworded.** The proposal named new type codes.
  Each case is already one the `match` check reports, so the existing code is
  kept with let-else wording: the `else`'s type (`bynk.types.match_arm_mismatch`,
  "this `else` produces `T`, but the rest of the block after the `let` produces
  `U`"), an `else` that can never run (`bynk.types.unreachable_arm`), a residual
  `else match` arm (`bynk.types.unreachable_arm` or
  `bynk.types.duplicate_variant_arm`), and residual coverage
  (`bynk.types.non_exhaustive_match`). The new codes are the three parse codes
  in C and D, since every pattern `let` needs its `else` and no type is needed
  to know that.
- **G — One nesting level per step.** The parser folds a block's let-else
  statements iteratively, but each nests the rest of its block one level, as
  the `match` it stands for does. Each step is charged against the parser's
  nesting budget, so an over-long chain is `bynk.parse.nesting_too_deep` rather
  than a stack overflow in a later pass.
- **H — Editor surfaces.** The hidden binder gets no inlay hint, no locals entry
  and no extent (its spans are empty, at the `let`). The pattern's names get
  what a `let` binding gets: inferred-type inlay hints, a locals entry scoped to
  the rest of the block (so hover, go-to-definition and variable semantic
  tokens), and `else` hover documents the form. At an arm start inside
  `else match`, completion offers the value's variants minus one the pattern
  matches outright. Signature help is unchanged.

**Consequences.** The rewritten `shop` handler drops from eleven brace levels to
five and its tests pass unchanged. The emitted TypeScript is the staircase's
(statement-form `switch`/`if` chains), so no existing fixture's output moved.
tree-sitter gains `let_else_stmt`, `let_else_match` and the three `let_pattern`
rules, and `if_expr` becomes `prec.right` so `let P = if c { … } else …` keeps
the `if`'s `else`, as the compiler does. A nesting lint, the request that
started this, now has a flat form to recommend; it remains tooling depth,
which the adoption-sequencing decision freezes.
