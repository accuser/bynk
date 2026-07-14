# 0170 — The `do` statement, implicit unit, and `List.forEach`

- **Status:** Accepted (v0.146)
- **Provenance:** proposed in #542, resolving the design-review finding #542
  (Bynk Language Design Review 2026-07-05, §8 Language P1 #3, refs §3.2,
  §6.2(4), §6.2(5)).
- **Realises:** an effectful line no longer needs a discard binder (`do e`
  replaces `let _ <- e` when the reply is unit), an `Effect[()]` block may close
  with no tail instead of `Effect.pure(())`, an `if` guarding a unit effect
  drops its `else Effect.pure(())`, and a `List` may run an effect per element
  (`List.forEach`) instead of a unit-accumulator `foldEff`.
- **Relates:** ADR 0031 (`Effect` is non-storable; effectful calls are confined
  to effectful contexts — the rule `do`/`forEach` obey), ADR 0106 (the `~>`
  send, the sibling binder-free statement and the model for the unit gate),
  ADR 0116 (the eager `List` combinator vocabulary `forEach` joins), ADR 0115
  (`Query.forEach`, the terminal `List.forEach` mirrors), ADR 0156 (the editor
  surface tracks the language).

## Context

Effects are a headline feature, but the everyday spelling of "run an effect and
move on" carried avoidable ceremony that the review counted across the examples:

- **A discard binder on nearly every effectful line.** An effect used only for
  its side effect — `Logger.info(…)`, a durable write — had to be written
  `let _ <- e`, binding a `_` no one reads. The review found this ~30× across
  the examples.
- **`Effect.pure(())` to close a unit body.** A handler returning `Effect[()]`
  could not simply *end* after its last effect: the block needs a tail, and the
  tail had to be `Effect.pure(())` (the `sessions` agent alone closed four unit
  handlers this way).
- **`else Effect.pure(())` for a conditional effect.** A one-armed conditional
  effect — "log this *if* verbose" — had to spell out an else that produces the
  empty effect, because `else` was mandatory and both branches had to agree.
- **No `List.forEach`.** `List` had `foldEff` but no `forEach`, so running an
  effect over a list degenerated to a unit-accumulator fold
  `foldEff((), (acc: (), c) => …)`; `uptime-monitor` could not loop over its
  targets at all and copy-pasted the fetch/decode/store block per target.

None of this is new capability — the discard binder, the `()` tail, the empty
else, and the fold all already *compile*. The increment is **additive sugar**
plus one missing terminal: it removes ceremony a reader has to decode, without
changing what a program can express.

## The surface

Four author-facing shapes, all additive:

- **`do e`** — an effect-performing expression statement. Runs `e` (which must be
  `Effect[()]`) and discards its unit result. Sugar for `let _ <- e` when the
  awaited value is unit.
- **Implicit unit tail** — a block may close with no tail expression; the parser
  synthesises a `()` tail. Against an `Effect[()]` context the tail-position
  auto-lift wraps it, so an effectful handler may just *end*.
- **Else-less `if`** — `if c { e }` with no `else` defaults the missing branch to
  `()`. Legal only when the then-branch is unit (`()` / `Effect[()]`); a valued
  `if` still requires an explicit `else`.
- **`List.forEach(f: T -> Effect[()]) -> Effect[()]`** — run an effectful step
  for each element, in order, discarding the results.

## Decisions

**A — A keyword `do` statement, not a bare expression statement.** The review
offered either "a `do e` statement" or "admit a bare expression statement when
the type is `Effect[()]`". A bare expression statement is parse-ambiguous with
the block's tail: Bynk blocks have no statement terminator, so in `{ f() g() }`
the parser cannot know `f()` is a statement (not the tail) without types it does
not yet have. A leading keyword removes the ambiguity and makes the intent
greppable — `do e` is unmistakably "perform this effect and continue". It mirrors
the existing `~>` send (ADR 0106): both are binder-free effect statements led by
a token, both gate on `Effect[()]`. `do` becomes a reserved keyword — a
theoretical break for a source using `do` as an identifier, negligible pre-1.0
and consistent with every other statement head being a keyword.

**B — `do e` requires `Effect[()]`; a valued reply keeps `let _ <- e`.** The
`do` statement discards *nothing of value* — its result is already `()`. A valued
effect (`Effect[Option[UserId]]`) whose result is thrown away must stay the
explicit `let _ <- e` (`bynk.effect.do_requires_unit`), preserving the §5.5
discard rule: throwing away a real value is visible in the source, never
implicit. This is exactly the `~>` error gate (`bynk.send.requires_unit`)
transplanted to `do`.

**C — Implicit unit is a synthesised tail, gated by ordinary typing, not a new
"block must be unit" rule.** A block with no explicit tail gets a real
`ExprKind::UnitLit` tail (marked `implicit_tail` on the `Block`), generalising
the existing rule that let a test body close after a trailing `expect`. Against
`Effect[()]` the tail auto-lifts (ADR's tail-lift, unchanged); against a valued
return type the synthesised `()` is an ordinary type mismatch — no bespoke
diagnostic, no parser knowledge of types. The `implicit_tail` flag exists only so
the formatter can *omit* the synthetic `()`: Bynk has no statement terminator, so
a printed `()` on the next line would re-attach to the last statement on re-parse
(`x` `()` → `x()`). The parser re-derives the tail, so omitting it is loss-free.

**D — Else-less `if` synthesises a `{ () }` else, gated to unit; `if` stays an
expression everywhere else.** A missing `else` produces a synthesised unit
else-branch (the same `implicit_tail` unit block), so `ExprKind::If` keeps its
two mandatory branches and the emitter/linearity/resolver walks are untouched.
The checker recognises the synthetic branch structurally (`Block::is_synth_unit`)
and requires the then-branch to be unit — a valued else-less `if` is rejected
with `bynk.types.if_without_else_requires_unit`, not silently defaulted. So the
sugar is *only* for conditional effects; a value-producing `if` still owes its
`else`, and the "both branches agree" rule is undisturbed.

**E — `List.forEach` mirrors `Query.forEach`, sequential.** The signature and
semantics are the `Query.forEach` terminal (ADR 0115) over an eager list:
`forEach(f: T -> Effect[()]) -> Effect[()]`, awaiting each element's effect in
order. Like `foldEff` (ADR 0116) it runs an effectful function value and is
therefore confined to effectful contexts (`bynk.effect.fn_value_in_pure_context`,
ADR 0031). It emits **inline** — an `async` `for…await` IIFE, the list analogue
of the query form — so no runtime import is added and a file that never calls it
emits byte-identically. The parallel form is deferred: `parTraverse` already
exists for the fan-out case, and a `List.forEach` that awaits in order is the
sequential default a durable write path wants.

## Consequences

- `sessions` drops eight lines of `Effect.pure(())` / `let _ <-` ceremony;
  `uptime-monitor`'s per-target copy-paste collapses to a `targets.forEach(…)`
  loop over a list literal.
- New diagnostics: `bynk.effect.do_in_pure_context`,
  `bynk.effect.do_on_non_effect`, `bynk.effect.do_requires_unit`,
  `bynk.types.if_without_else_requires_unit`.
- `do` is a reserved keyword (breaking in principle; no example used it as an
  identifier).
- An empty block `{}` is now legal (it was a parse error) and means the unit
  value `()` — a no-op unit body.
- No emission churn for existing programs: the synthesised unit tail and unit
  else lower through the paths `()` / `Effect.pure(())` already used, and
  `List.forEach` is net-new inline emission. `foldEff`'s lowering is unchanged.

## Tooling (ADR 0156)

- **Hover:** unchanged — `do` introduces no hoverable binding; the effect
  operand hovers as before. `List.forEach` hovers via the same kernel-method
  registry as `foldEff`.
- **Completion:** `List.forEach` is offered from the `LIST_METHODS` registry
  (`methods_for`), like every other list kernel; `do` as a statement-leading
  keyword is a follow-on (the same posture as `let`/`~>`, which are not
  completed as statement heads today).
- **Semantic tokens:** `do` is a keyword token, highlighted via the tree-sitter
  `highlights.scm` keyword set (regenerated `parser.c` + `grammar.json`); the
  else-less `if` and implicit tail add no new token kinds.
- **Signature help:** `List.forEach` gains signature help via its
  `LIST_METHODS` entry (`forEach(f: T -> Effect[()]) -> Effect[()]`); `do`
  introduces no invocation.
- **Formatter:** renders `do e`, omits the synthesised unit tail and the
  synthesised `else`, and round-trips the else-less / tail-less forms
  idempotently.

## Alternatives considered

- **A bare expression statement (A).** Rejected for the tail-ambiguity above; a
  keyword is unambiguous and greppable. Kept as the documented fallback if a
  future terminator ever makes bare statements unambiguous.
- **A "block must end in unit" diagnostic for the implicit tail (C).** Rejected —
  the ordinary return-type/tail mismatch already reports a valued block that
  forgot its tail, with a better message than a bespoke rule, and keeps the
  parser type-free.
- **An `Option` else / an else-less `If` AST node (D).** Rejected — it churns
  every `ExprKind::If` match across checker, emitter, linearity, resolver, and
  formatter. A synthesised unit else recognised structurally keeps the node
  two-armed and the walks untouched.
- **A parallel `List.forEach` (E).** Rejected as the default — `parTraverse`
  covers fan-out; sequential await is what an ordered effect loop wants.
