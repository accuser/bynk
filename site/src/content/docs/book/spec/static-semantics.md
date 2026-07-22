---
title: "§5 Static semantics"
---
A program that parses ([§3](/book/spec/lexical-grammar/), [§4](/book/spec/syntactic-grammar/)) is
not yet known to be well-formed. This chapter states the **well-formedness
rules**: the conditions a program MUST satisfy beyond parsing, each tied to the
`bynk.*` diagnostic a conforming implementation emits when the rule is violated
([§1.3](/book/spec/scope/)). A program is well-formed exactly when it provokes no such
diagnostic.

> [!NOTE]
> Lexical and grammatical errors — the `bynk.lex.*` and `bynk.parse.*` codes —
> are *syntactic*: they report a text that does not match the grammar, and are
> governed by §3 and §4. This chapter covers only **post-syntactic**
> well-formedness. This note is informative.

The rules are organised by theme. Each theme states its load-bearing rules and
cites the governing codes; the **exhaustive** code-by-code catalogue is the
[diagnostic index](/book/reference/diagnostics/) (and §9). Where a theme maps to a
single construct, its full set of governing diagnostics is surfaced inline with
`{{#grammar-semantics}}`.

## §5.1 Name resolution & visibility

Every referenced name MUST resolve to a declaration in scope
(`bynk.resolve.unknown_name`, `bynk.resolve.unknown_type`,
`bynk.resolve.unknown_function`, `bynk.resolve.unknown_field`). A name used where
a value is expected MUST denote a value, not a type (`bynk.resolve.type_in_expr`,
`bynk.resolve.type_as_function`); a function MUST be called, not referenced bare
(`bynk.resolve.fn_without_call`).

Within a scope, names MUST be unique: duplicate types, functions, methods,
services, capabilities, providers, agents, record fields, variants, and
parameters are each rejected (the `bynk.resolve.duplicate_*` codes). A `let`
binding MUST NOT shadow a function or a type (`bynk.resolve.let_shadows_fn`,
`bynk.resolve.let_shadows_type`).

A bare reference to a **named function** is a value only where a function
type is expected (v0.20a); elsewhere it MUST be called
(`bynk.resolve.fn_without_call`). A call on an in-scope **value** is legal
only when the value's type is a function type
(`bynk.resolve.param_as_function` otherwise) — both judgments are made by the
checker, with the type information they require; a *type name* is never
callable (`bynk.resolve.type_as_function`). Call resolution prefers declared
functions, then variant constructors, then agents, then in-scope values —
scope-first call resolution would change the meaning of existing programs, so
the pre-existing ident/call precedence asymmetry is preserved deliberately.

A `commons` is imported with `uses`, which MUST name an existing `commons`, not a
context, and MUST NOT be self-referential or introduce a colliding name
(`bynk.uses.unknown_commons`, `bynk.uses.target_is_context`,
`bynk.uses.self_reference`, `bynk.uses.name_conflict`). The visibility of types
across context boundaries is governed by `exports` and `consumes`
([§5.8](#58-boundaries--cross-context)).

## §5.2 Well-typedness

Every expression MUST have the type its position requires. A function or method
argument MUST match the parameter type (`bynk.types.argument_mismatch`), and a
call MUST supply the right number of arguments (`bynk.resolve.arity_mismatch`,
`bynk.types.method_arity`). A returned value MUST match the declared return type
(`bynk.types.return_mismatch`); a `let` value MUST match any annotation
(`bynk.types.let_annotation_mismatch`); a record field MUST be given a value of
its type (`bynk.types.field_value_mismatch`), and every required field MUST be
supplied (`bynk.resolve.missing_field`).

An `if` condition MUST be a `Bool` and both branches MUST **join** to a common
type — their least upper bound, so a refined type and its base (or two refined
types over one base) agree at the base, but unrelated types do not
(`bynk.types.if_non_bool_cond`, `bynk.types.if_branch_mismatch`). The payloads of
`Ok`, `Err`, `Some`, and the like MUST match the expected component type (the
`bynk.types.*_value_mismatch` codes). Where a constructor is ambiguous between
`Result` and `HttpResult`, it MUST be qualified (`bynk.types.ambiguous_constructor`).

**Lambdas** (v0.20a). Against an expected function type, a lambda's
parameters take the expected types (an annotation MUST agree), its body is
checked against the expected return — a pure body auto-lifts into an
effectful expectation — and arity MUST match (`bynk.types.lambda_mismatch`).
In a position with no expected function type, every parameter MUST be
annotated (`bynk.lambda.unannotated_param`) and the lambda's type is read off
its body: a body that performs an effect operation (an `<-` bind, a
capability call, a call returning `Effect`) makes the lambda **effectful**,
wrapping its result in `Effect` — effectfulness is judged by the *presence*
of effect operations, never by a pre-declared result type, which is what
dissolves the apparent circularity. A nested lambda's effects are its own.

**Value application** (v0.20a). Applying a function-typed value checks
arguments against the function type's parameters
(`bynk.types.argument_mismatch`, `bynk.types.call_arity`).

**Numeric operators** (v0.21). The arithmetic operators `+ - * /` are
defined on `Int` operands (yielding `Int`) and on `Float` operands
(yielding `Float`). They MUST NOT mix the two: an `Int` and a `Float`
operand in the same operation is `bynk.types.no_numeric_coercion` — there
is **no implicit numeric coercion** in either direction. The same rule
applies to the comparison operators `< <= > >=` (defined on `Int`,
`Float`, and `String`, same-typed) and to `==`/`!=`. Refined numeric
types widen to their base in operator positions, as before.

**`Float` equality** (v0.21). `==`/`!=` on `Float` follow the host's IEEE
754 semantics, and both classic surprises apply: `0.1 + 0.2 != 0.3`
(decimal fractions are not exact doubles), and a `NaN` produced by
arithmetic is **unequal to itself**. Exact `Float` equality is rarely the
test a program needs — compare with an explicit tolerance, or work in
`Int` units. Division by zero and overflow in `Float` arithmetic follow
the host (`Infinity`/`NaN`); no Bynk-level guard applies **in arithmetic**
(boundaries are guarded: [§7.2](/book/spec/emission/#72-targets)).

**The numeric kernel** (v0.21, extended v0.22a). Conversion between the
numeric types is explicit, via built-in value methods on the bare base
types: `i.toFloat() -> Float` (total) on `Int`; `f.round()`, `f.floor()`,
`f.ceil()`, `f.truncate()` (each `-> Int`, named and lossy) on `Float` —
there is deliberately no ambiguous `toInt`. v0.22a (ADR 0048) adds, on
**both** numeric types, `x.abs()`, `a.min(b)`, `a.max(b)`, and
`x.clamp(lo, hi)` (arguments take the receiver's type — mixing is the
no-coercion error), and on `Float` only, `f.isNaN()` and `f.isFinite()`
(`-> Bool`). v0.42 (ADR 0074) adds `x.toString() -> String` on **both** types
(total — the render direction `Int.parse` lacks); for `Float` the result is the
**host's number→string** (ECMAScript `Number::toString` — shortest round-trip),
pinned to the platform the same way ADR 0046 pins the string kernel. Wrong arity
is `bynk.types.method_arity`; an unknown method on a numeric receiver is
`bynk.types.method_not_found`.

**The numeric parse statics** (v0.22a). `Int.parse(s) -> Option[Int]` and
`Float.parse(s) -> Option[Float]` — statics, per 0041's rule (ways to
*obtain* a value). Parsing is **full-string**: leading/trailing garbage is
`None` (not `parseFloat`'s prefix laxity); the empty or whitespace-only
string is `None`; a value outside the safe-integer range (`Int`) or
non-finite (`Float`) is `None`. `parse` is the only static on the numeric
types (`bynk.resolve.unknown_static_member`).

**The string kernel** (v0.22a, ADR 0046). `String` is opaque — no direct
character access — so its operations are built-in value methods:
`s.length() -> Int`, `s.split(sep) -> List[String]`, `s.trim()`,
`s.toUpper()`, `s.toLower()`, `s.concat(t)`, `s.contains(sub)`,
`s.startsWith(sub)`, `s.endsWith(sub)` (`-> Bool`),
`s.replace(a, b)`, `s.slice(lo, hi)`, `s.indexOf(sub) -> Option[Int]`,
and `s.chars() -> List[String]`. **Semantics are UTF-16 code units**,
normatively, with two pinned exceptions: `replace` replaces **every**
occurrence (not TS's first-only string form), and `chars()` splits by
**code points** (so `s.length() != s.chars().length()` when `s` contains
astral characters). `slice` clamps negative indices to `0` — there is no
wrap-around. `indexOf` returns `None` for a missing substring, never a
sentinel `-1`.

**Refined receivers inherit the base kernel** (v0.143, ADR 0168). A method
call whose receiver is a **refined** type resolves against that type's declared
instance methods first; on a miss, it resolves against its **base type's
read-only kernel** — the numeric, string, `Duration`, `Instant`, and `Bytes`
kernels above. The result is **base-typed**: `n.toUpper()` on a `Name = String
where …` has type `String`, never `Name` — the same widening a refined value
already undergoes in arithmetic (D2/D3) and comparison. A declared method always
takes precedence over the inherited kernel, so it can never be shadowed. `Bool`
has no kernel, so a `Bool`-based refinement inherits nothing. An **opaque** type
does **not** widen and therefore inherits nothing — a kernel call on an opaque
receiver stays `bynk.types.method_not_found`. Refined arguments to an inherited
method widen to the base like any other argument.

**String interpolation** (v0.43, ADR 0075). An interpolated string
`"… \(e) …"` has type `String`. Each hole expression `e` must have type
`String`, `Int`, `Float`, `Bool`, or a **refinement** of one of those (which
widens to its base for display) — these are the types with a well-defined
string form (`Int`/`Float` via the ADR 0074 `toString` contract, `Bool` as
`true`/`false`). Any other hole type — `record`, `sum`, `Option`, `Result`,
`List`, an opaque type (whose base is hidden — `.raw` it first), … — is a
static error (`bynk.types.interpolation_non_scalar`): map the value to a
`String` first. The conversion is implicit only here, in a display context; it
does **not** generalise to arithmetic or comparison (ADR 0046 is unchanged —
`+` stays numeric, `concat` stays a method).

**The `Option`/`Result` kernel** (v0.22a, ADR 0048). The combinators are
built-in value methods on the compiler-known generic receivers — *not*
free functions, which would collide by bare name on `uses` import
(`bynk.resolve.duplicate_fn`). On `Option[T]`: `o.map(f)`,
`o.andThen(f)` (the function MUST return an `Option`), `o.getOrElse(x)`,
`o.isSome()`, `o.okOr(e) -> Result[T, E]`. On `Result[T, E]`: `r.map(f)`,
`r.andThen(f)` (the function MUST return a `Result` with the receiver's
error type), `r.mapErr(f)`, `r.getOrElse(x)`, `r.isOk()`. The function
argument's parameters type contextually from the receiver and its return
is read from the actual (the v0.20a pass-2 rule) — so a lambda body that
itself needs an expected type (a bare `Ok`/`Err`/`None`/`[]`) annotates a
`let`, exactly as with lambdas passed to generic calls.

```bynk
commons checkout {
  fn parseQty(s: String) -> Int {
    Int.parse(s.trim()).map((n) => n.clamp(1, 99)).getOrElse(1)
  }

  fn label(name: Option[String]) -> Result[String, String] {
    name.map((n) => n.toUpper()).okOr("missing name")
  }
}
```

**The `Effect[Result[T, E]]` kernel** (v0.152, ADR 0176 — design doc §2.8.3).
`Effect[Result[T, E]]` is the universal shape of cross-context calls, and the
volume of that composition earns four compiler-synthesised combinators directly
on the receiver — so success/error reshaping and effectful recovery need no
intervening `<-` peel and `match`:

| Method | Result |
|---|---|
| `e.mapOk(f: T -> U)` | `Effect[Result[U, E]]` |
| `e.mapErr(f: E -> F)` | `Effect[Result[T, F]]` |
| `e.flatMapOk(f: T -> Effect[Result[U, E]])` | `Effect[Result[U, E]]` |
| `e.flatMapErr(f: E -> Effect[Result[T, F]])` | `Effect[Result[T, F]]` |

`mapOk`/`mapErr` map the two sides of the success/error split; `flatMapOk`
chains a further effectful-fallible step on success (keeping the single error
type `E`, exactly as `?` does); `flatMapErr` attempts an effectful recovery on
error (its recovery MUST produce the receiver's success type `T`). The naming is
verb-first, matching `map`/`mapErr` on `Result`. A method outside the four is
`bynk.types.method_not_found`; a `flatMapOk`/`flatMapErr` argument that does not
return `Effect[Result[…]]`, or whose error/success side does not line up, is
`bynk.types.argument_mismatch` (no new diagnostic). Only an `Effect` wrapping a
`Result` carries these — any other `Effect[_]` has no kernel methods. Unlike the
eager `List.forEach`/`traverseTry` iterators, these **produce** an `Effect`
rather than running one, so they are **not** effectful-context-confined: a pure
helper may reshape an `Effect[Result[…]]` it was handed and return it. `.map`
and `.flatMap` on such a receiver remain Effect's own (operating on the whole
`Result`); the four named methods remove the "which `.map`?" ambiguity. Other
`Effect`-of-X shapes (`Effect[Option[T]]`, `Effect[List[T]]`) have no synthesised
methods — write `e.map((r) => …)` explicitly.

**Declared error embeddings and `?` conversion** (v0.154, ADR 0178). A sum type
MAY declare error embeddings — `type OrderError = … embeds PaymentError as
Payment, ScheduleError as Fulfilment` — where each `embeds E as V` names a
variant `V` of the same sum that MUST have **exactly one payload field, of type
`E`** (`bynk.types.embeds_unknown_variant` if `V` is not a variant;
`bynk.types.embeds_variant_shape` if its shape does not match). A given source
type MAY be embedded by at most one variant, so the conversion is unambiguous
(`bynk.types.embeds_ambiguous`). The `?` operator then uses the embedding: in a
function returning `Result[_, F]` (or `Effect[Result[_, F]]`), applying `?` to a
`Result[T, E]` propagates as before when `E` matches `F`, and otherwise — when
`F` declares `embeds E as V` — **auto-wraps** the `Err(e)` into `Err(F.V(e))`
instead of requiring a manual `.mapErr`. The conversion is **one level**: `E`
must match a declared embedding of `F` directly. When neither the types match
nor an embedding applies, it is `bynk.types.question_error_mismatch`.

**The typed JSON codec** (v0.22b, ADR 0045). `Json.encode(v) -> String` and
`Json.decode[T](s) -> Result[T, JsonError]` are compiler-backed statics on
the built-in `Json` module: `encode` dispatches to the generated
`serialise_<T>` for the value's checked type; `decode` to `JSON.parse` +
`deserialise_<T>`. The **domain of `T`** (and of `encode`'s argument) is any
boundary-legal shape — base types, named types, and the built-in containers
over them; functions, effects, `HttpResult`, the error builtins, and type
variables are `bynk.types.json_uncodable`. `decode`'s target is given
explicitly (`Json.decode[Order](s)`, any boundary-legal type-ref including
`Json.decode[List[Order]]`) or inferred from an expected
`Result[T, JsonError]`; with neither, `bynk.generics.uninferable_type_arg`.
`encode` is `-> String` but **throws on a value containing a non-finite
`Float`** — the 0040 contract violation, documented rather than `Result`-ified
(the program itself created that state). A user-declared type named `Json`
shadows the built-in module.

**`JsonError`** (v0.22b, ADR 0047). The decode error is a compiler-known
record — `kind`, `path`, `message`, all `String` — putting a boundary
failure in the program's hands for the first time (the `ValidationError`
precedent). `kind` is `"Malformed"` for unparseable input, else the
boundary kind (`"StructuralMismatch"`, `"RefinementViolation"`); `path` is
the tracked field path (`$.items[2].qty`); decode failures are runtime
values, never compile diagnostics.

```bynk
commons store {
  type Item = {
    sku: String,
    price: Float,
    qty: Int,
  }

  fn snapshot(i: Item) -> String {
    Json.encode(i)
  }

  fn restore(s: String) -> Result[Item, JsonError] {
    Json.decode[Item](s)
  }

  fn restoreError(s: String) -> String {
    match Json.decode[Item](s) {
      Ok(i) => i.sku
      Err(e) => e.kind.concat(" at ").concat(e.path)
    }
  }
}
```

**Generic instantiation** (v0.20a). A generic function's type arguments are
inferred from its arguments by argument-directed unification: non-lambda
arguments first, left to right; lambda arguments after, against the
substituted expectations — a lambda whose expected *parameter* types remain
undetermined is rejected unless fully annotated, and an expected *return*
variable is captured from the lambda's actual type. Conflicting inferences
MUST agree exactly (`bynk.generics.type_arg_mismatch`); a type parameter
neither inferable nor given explicitly (`name[T](…)`) is rejected
(`bynk.generics.uninferable_type_arg`), as is a bare generic function passed
as a value. There is no inference between lambdas and none from the call's
own expected type. Generic *type* declarations and parameter *bounds* are
rejected (`bynk.generics.no_generic_types`, `bynk.generics.no_bounds`); a
type parameter MUST NOT shadow a declared type. Within a generic function's
body its type parameters are rigid: equal only to themselves. The checker
maintains the invariant that a type-variable-bearing expected type imposes
no constraint on expression checking.

{{#grammar-semantics if_expr}}

### §5.2a Numeric literals — digit separators (v0.142) {#numeric-literals}

> [!NOTE]
> This is a *lexical* rule (governed normatively by §3), surfaced here because it
> concerns how a numeric value is written.

An `Int` or `Float` literal MAY carry an underscore **`_`** as a **digit
separator** between digit groups — `1_048_576`, `1_000.5`, `26_214_400`. A
separator MUST fall between two digits: a leading (`_1`), trailing (`1_`), or
doubled (`1__2`) separator, or one adjacent to the decimal point or exponent
marker, is a lexical error. The separators are **purely visual** — they are
stripped before the value is parsed, so `1_000` and `1000` denote the same value.
As with `Float` literals, the **as-written lexeme is preserved**, so `bynkc fmt`
keeps the author's grouping verbatim.

## §5.3 Refinement & admission

A refinement's predicates MUST apply to the type's base — a string predicate on
an `Int` is rejected (`bynk.types.predicate_base_mismatch`) — and MUST be
internally consistent: an `InRange` MUST NOT be inverted
(`bynk.types.inverted_range`), a length MUST NOT be negative
(`bynk.types.negative_length`), a `Matches` regex MUST be valid
(`bynk.types.invalid_regex`) and MUST NOT nest unbounded quantifiers — a repeated
group that itself contains `*`, `+`, or `{n,}`, such as `(a+)+`, is rejected
(`bynk.types.catastrophic_regex`) because the emitted boundary check runs under a
backtracking `RegExp` where that shape is exponential on crafted input — and the
predicates together MUST admit at least one value (`bynk.types.empty_refinement`
— on `Float`, `Positive` excludes the lower endpoint `0.0`, so
`InRange(-1.0, 0.0) && Positive` is empty).

`InRange` bounds MUST match the numeric base (v0.21): integer bounds on
`Int`, float bounds on `Float`. A bound of the other numeric type, or a
mixed pair, is `bynk.types.no_numeric_coercion`.

A **literal** written where a refined type is expected is admitted at compile
time ([§6.4](/book/spec/type-system/#64-admission--construction)) in these positions:
return (block tail), a `let` with a type annotation, an `Ok`/`Some`/`Err`
payload, and a refined-typed call argument. The literal MUST satisfy the
predicate, or it is rejected (`bynk.refine.literal_violates`); an admitted
literal MUST be a compile-time literal, not an expression or identifier. A refined
type is thus constructed at run time through `.of` alone and has **no** unchecked
`.unsafe` — the unchecked escape hatch is opaque-only (`Age.unsafe(x)` is rejected
as `bynk.resolve.unknown_static_member`). **Opaque
types are excluded** from admission and MUST be constructed through `.of`,
`.unsafe`, or `.raw`, never record syntax (`bynk.resolve.opaque_record_construction`,
`bynk.types.opaque_record_construction`); `.raw` MUST be used only within the
defining `commons` (`bynk.types.opaque_raw_outside`) and `.unsafe` only within
the defining context (`bynk.types.opaque_unsafe_outside`).

{{#grammar-semantics refined_type}}

## §5.4 Agents & state

An `agent` MUST be declared inside a context (`bynk.agent.outside_context`) and
MUST NOT declare `from http`, `from cron`, or `on message` handlers (the
`bynk.parse.*_in_agent` codes). Each agent handler's return type MUST be an
`Effect` (`bynk.agent.return_not_effect`).

Every `store` field MUST have a defined initial value: either an **explicit
initialiser** — a compile-time constant of the field's (element) type, not
referencing `self`, parameters, or capabilities (`bynk.agents.bad_state_initialiser`)
— or an **implicit zero** (`Int` → `0`, `Bool` → `false`, `String` → `""`,
`Option[T]` → `None`, a record of zeroable fields). A field with neither is
rejected (`bynk.agents.non_zeroable_state_field`).

A `:=` store write MUST target a `store Cell` field (`bynk.cell.invalid_target`),
and its right-hand side MUST NOT read the cell being written
(`bynk.cell.self_reference`) — a read-modify-write reads the old value into a
`let` first. A handler's writes are staged and committed atomically when it
returns (ADR 0109). Constructing or calling an agent MUST use the right key arity
and type and a declared handler (`bynk.agent.construction_arity`,
`bynk.agent.key_mismatch`, `bynk.agent.handler_arity`, `bynk.agent.handler_not_found`).

### §5.4.1 Invariants (v0.80)

An **invariant** is a universally-quantified property that MUST hold of every
committed agent state (`design/bynk-design-notes.md` §14; ADR 0107). Its predicate
references the agent's `store` fields by bare name and is a *pure, agent-local
`Bool` expression*:

- the predicate MUST have type `Bool` (`bynk.invariant.not_bool`);
- it MUST be pure — no capabilities, no effects, no test-only constructs
  (`bynk.invariant.impure_predicate`);
- it MUST NOT reference another agent (`bynk.invariant.cross_agent_reference`) —
  invariants constrain a single agent's reachable states; a property that spans
  agents belongs in a saga or scenario;
- invariant names MUST be distinct within an agent (`bynk.invariant.duplicate_name`).

The predicate language is ordinary expressions plus `implies` (logical
implication, `P implies Q` ≡ `!P || Q`) and `is` (pattern-matching as a `Bool`
expression). Invariants are **runtime-checked at the commit boundary**: each is
evaluated against the state staged by the handler's `store` writes, before it is
persisted. A violation is a **fault** (`InvariantViolation`), not an outcome — see
§7 and the emission model. "Revert" is the **non-persistence of the staged
state**, not whole-handler rollback (ADR 0107 D6): effects already performed by
the handler stand.

#### §5.4.1-i Step invariants — transitions (v0.116) {#step-invariants}

A **transition** is the invariant predicate widened to a *step*: a claim about the
move from the last committed state (`old`) to the state a commit would persist
(`new`) — ADR 0151. It sits in the same phase as invariants and reads state fields
through two contextual bindings, `old` and `new`, each the agent's synthetic state
record (so `old.status` / `new.balance` are ordinary record accesses); `old`/`new`
are ordinary identifiers outside a `transition`. The predicate is a *pure,
agent-local `Bool` expression* in the same predicate language:

- the predicate MUST have type `Bool` (`bynk.transition.not_bool`);
- it MUST be pure (`bynk.transition.impure_predicate`);
- it MUST NOT reference another agent (`bynk.transition.cross_agent_reference`);
- transition names MUST be distinct within an agent
  (`bynk.transition.duplicate_name`);
- it MUST reference `old` or `new` (`bynk.transition.no_step_reference`) — a
  predicate over neither is a snapshot claim, which is an `invariant`.

Placement is structural: a `transition` is an agent-body declaration only, so there
is no "transition on a non-agent" rule to state. Transitions are **runtime-checked
at the commit boundary**, alongside the snapshot invariants, but only **from the
second commit onward**: the genesis commit (an agent's first) has no `old` and is
skipped (its state is still constrained by the snapshot invariants). A violation is
the same **fault** (`InvariantViolation`), with the same non-persistence semantics.
A transition is **not** attacked by the generative runner — a valid agent state is
not necessarily reachable (ADR 0149), so behavioural generation over steps is a
handler-sequence concern (a later increment), not state fabrication.

### §5.4.1a Function contracts (v0.115)

A **contract** is the invariant predicate attached to a pure function (ADR 0150).
A `fn` carries any number of named `requires` (preconditions) and `ensures`
(postconditions) between its return type and body; each predicate is a pure `Bool`
expression in the same predicate language as an invariant (`implies`, `is`,
operators, pure methods):

- a `requires` predicate is typed with the parameters in scope; a `ensures`
  predicate is typed with the parameters **and** `result` in scope, where `result`
  has the declared return type (the awaited element `T` for an `Effect[T]` return);
- `result` MUST NOT appear in a `requires` (`bynk.contract.result_in_requires`) —
  the return value is not bound on entry;
- each predicate MUST have type `Bool` (`bynk.contract.not_bool`);
- each MUST be pure — no capabilities, effects, or test-only constructs
  (`bynk.contract.impure_predicate`);
- clause names MUST be distinct across a function's `requires` and `ensures`
  (`bynk.contract.duplicate_name`).

A contract is checked at two points, and stripped from the deploy build (the
runtime library and emission model describe the guard and the runner attack). A
`case`/`property` whose predicate is syntactically / α-equivalent to a declared
clause over the same bound arguments is flagged `bynk.contract.restated_by_test`
(conservative — under-flagging is acceptable, over-flagging is not).

### §5.4.2 Held-resource linearity (v0.102)

A **held resource** is a value of the closed `Held` kind — currently the single
type `Connection[F]` (§6.2). A held value is **runtime-produced** (no surface
constructor) and governed by an **ownership discipline** (linearity): at every
point a held binding is in one of three states — **owned**, **borrowed**, or
**consumed** — and a static *linearity pass* tracks each binding through them and
enforces (ADR 0130):

- **Disposal before scope exit.** An owned held binding MUST be **disposed** —
  stored, closed, or transferred to a handler that takes ownership — before its
  scope exits; one still owned at the end is `bynk.held.leak`.
- **No use after consume.** A *consuming* operation (`c.close()`, a `put`/`take`
  into storage, or transfer) ends the binding's lifetime; any later use is
  `bynk.held.use_after_consume`. A *non-consuming* operation (`c.send(f)`) leaves
  the binding owned.
- **Consistent disposal across branches.** Every branch of an `if`/`match` MUST
  leave a held binding in the same ownership state (`bynk.held.branch_divergence`).
- **Borrows are non-consuming.** A borrowed held binding — e.g. the closure
  parameter of a `forEach`/`parTraverse` over a `Map[K, Connection]` — admits only
  non-consuming operations; a consuming op on a borrow is
  `bynk.held.consume_on_borrow`.

A held value is **non-boundary and not value-comparable** (§6.5,
`bynk.types.held_at_boundary` / `bynk.types.held_not_comparable`). It may be stored
**only** in `Cell[Option[Connection]]` or `Map[K, Connection]` — a `put` consumes
the value and a `remove` removes-and-closes it — and a `Set`/`Log`/`Cache` rejects
it (`bynk.held.unsupported_storage`); a held `Map` rejects the transforming
`update`/`upsert` ops (`bynk.held.unsupported_map_op`). On an abnormal exit a
connection owned within a handler is implicitly closed by the runtime, and a stored
one rolls back with agent state (ADR 0130 Q5). Held resources are produced by the
`from websocket` protocol ([§5.7](/book/spec/static-semantics/#57-handlers)).

### §5.4.3 Rehydration validation (v0.97)

An agent's persisted state is **validated when it is loaded** (ADR 0124). When
stored state exists, each value position — a `Cell`'s `T`, a `Map`/`Cache`'s `V`, a
`Log`'s `T`, and textual `Set` elements / `Map` keys — is run through the **same
boundary deserialiser** the HTTP/queue seams use, against the **current** type
definition. A failure is an internal **fault**, `RehydrationViolation` — the
load-time twin of `InvariantViolation` (§5.4.1) — **not** a caller-facing `400`:
the supplier of stored state is trusted past-self, not an untrusted caller. A
refinement that **tightens** across a deploy therefore faults on load (orphaned
data is indistinguishable from corruption); breaking migrations remain
by-convention (no coercion, no silent drop).

**Additive evolution is automatic:** loading merges `{ ...zero(), ...stored }`, so
a `store` field added in a later deploy takes its zero/initialiser
([§5.4](/book/spec/static-semantics/#54-agents--state)) instead of reading as absent.

## §5.5 Effects, capabilities & providers

Bynk separates **pure** from **effectful** code. An `<-` bind MUST occur in an
effectful position and MUST be applied to an `Effect`
(`bynk.effect.bind_in_pure_context`, `bynk.effect.bind_on_non_effect`); a
capability call or a cross-context call MUST NOT occur in a pure context
(`bynk.effect.capability_in_pure_context`, `bynk.effect.cross_context_in_pure_context`).

An **asynchronous send** (`~>`, §4.8.5) MUST likewise occur in an effectful
position (`bynk.send.in_pure_context`) and MUST be applied to an `Effect`
(`bynk.send.non_effect`). Because a send does not await its reply and binds
nothing, its reply MUST be `Effect[()]` — the **error gate**: a send whose
operation returns a non-unit `Effect[T]` is rejected (`bynk.send.requires_unit`),
since the value or error `T` would be silently discarded. "No value" and "no
need to wait" are independent: to *await* a unit-returning effect (a durable
write that must join the commit) keep the `<-` bind; to await and discard a
**valued** reply, write `let _ <- e`. A send is a statement, never an expression.

A **`do` statement** (`do e`, §4.8.6) performs a unit effect without a binder: it
MUST occur in an effectful position (`bynk.effect.do_in_pure_context`) and MUST
be applied to an `Effect[()]` (`bynk.effect.do_on_non_effect`,
`bynk.effect.do_requires_unit`). Unlike a send, the effect *is* awaited and joins
the enclosing computation — `do e` is exactly `let _ <- e` when the awaited value
is unit. The unit gate is the send's error gate transplanted: a **valued** reply
(`Effect[T]`, `T ≠ ()`) is rejected so that discarding a real value stays the
explicit `let _ <- e`.

**Implicit unit tail (§4.8.1).** A block with no trailing expression has value
`()`: the tail auto-lift (an `Effect[T]` context lifts a tail of type `T`) then
lifts it to `Effect[()]` where the enclosing return type is `Effect[()]`, so an
effectful unit body may simply *end* rather than close with `Effect.pure(())`.
Against a **valued** expected type the synthesised `()` is an ordinary type
mismatch, reported where any wrong-typed tail would be.

**Else-less `if` (§4.6.3).** An `if` with no `else` defaults the missing branch
to `()`. It is legal only when the then-branch is unit (`()` or `Effect[()]`);
otherwise the missing value has no default and it is rejected
(`bynk.types.if_without_else_requires_unit`). A value-producing `if` still
requires an explicit `else`, and the "both branches agree" rule (§5) is
unchanged — the synthesised `()` branch simply matches the unit then-branch.

A capability MUST be declared inside a context or an adapter
(`bynk.capability.outside_context`); a bodied provider MUST implement exactly its
capability's operations — no missing, no extra, signatures matching
(`bynk.provider.missing_operation`, `bynk.provider.extra_operation`,
`bynk.provider.signature_mismatch`) — and every provider MUST name an existing
capability (`bynk.provider.unknown_capability`). A handler or provider MUST
declare every capability it uses with `given`, and `given` MUST name a real
capability; a call to an undeclared capability is rejected and an unused one
warned (`bynk.given.unknown_capability`, `bynk.given.undeclared_capability`,
`bynk.given.unused_capability`). Providers MUST NOT form a dependency cycle
through `given` (`bynk.provider.dependency_cycle`).

Calling an **effectful function value** — one whose type's return is
`Effect[_]` — is an effect operation: it MUST occur in an effectful context
(`bynk.effect.fn_value_in_pure_context`), exactly as a capability call must.
`Effect[T]` remains non-storable in pure contexts; this feature opens no back
door (the eager-`Promise` translation makes an un-bound effectful call
observable, so the confinement is load-bearing).

**Provider placement follows the unit kind.** A provider in a *context* MUST
have a Bynk body (`bynk.context.external_provider`); a provider in an *adapter*
MUST be external — bodiless, its implementation supplied by the binding
(`bynk.adapter.provider_has_body`); a provider anywhere else is rejected
(`bynk.provider.outside_context`). An **external** provider's `given` resolves
exactly as a bodied provider's does — each bare name MUST be a local capability
or one flattened from a `consumes` selection
([§5.8](#58-boundaries--cross-context)) — and external providers participate in
the same dependency-cycle check.

{{#grammar-semantics given_clause}}

## §5.6 Pattern matching

A `match` MUST be **exhaustive** — every variant of the scrutinised sum,
`Result`, or `Option` covered (`bynk.types.non_exhaustive_match`) — and its
scrutinee MUST be a sum type (`bynk.types.match_non_sum_discriminant`). Its arms
MUST **join** to a common result type — their least upper bound, so a refined
type and its base agree at the base (`bynk.types.match_arm_mismatch`), MUST NOT
repeat a
variant (`bynk.types.duplicate_variant_arm`), and MUST NOT be unreachable
(`bynk.types.unreachable_arm`).

A pattern MUST name a real variant (`bynk.types.unknown_variant_in_pattern`) and
real payload fields (`bynk.types.unknown_pattern_field`), bind the right number
of fields (`bynk.types.pattern_arity`), and MUST NOT mix named and positional
bindings (`bynk.types.mixed_pattern_bindings`). An `is` check MUST be applied to a
value of the matching base or sum (`bynk.types.is_base_mismatch`,
`bynk.types.is_non_sum`, `bynk.types.is_unknown_variant`).

{{#grammar-semantics match_expr}}

## §5.7 Handlers

A `service` MUST be declared inside a context (`bynk.service.outside_context`) and
every service handler MUST return an `Effect` (`bynk.service.return_not_effect`).

An HTTP handler MUST return `Effect[HttpResult[T]]`
(`bynk.http.return_not_effect_http_result`); its route MUST be well-formed and
unique, MUST NOT use the reserved `/_bynk/` prefix
(`bynk.http.invalid_path`, `bynk.http.duplicate_route`, `bynk.http.reserved_prefix`),
and each `:name` segment MUST bind to a string-constructible parameter
(`bynk.http.unbound_path_param`, `bynk.http.path_param_not_stringy`,
`bynk.http.extra_param`); `GET` and `DELETE` MUST NOT take a `body`
(`bynk.http.body_on_get_or_delete`). Constructing a `Raw` variant (v0.111) MUST
supply exactly two arguments — a `Bytes` body then a `String` content-type
(`bynk.types.variant_arity`, `bynk.types.argument_mismatch`) — and yields
`HttpResult[()]`, the JSON body parameter `T` being unused, as for `Streaming`
and the redirect variants. An cron handler MUST take at most one
`Int` parameter, a valid five-field schedule, and return `Effect[Result[(), E]]`
(the `bynk.cron.*` codes); an `on message` handler MUST take exactly one `message`
parameter, a non-empty queue name, and the same return shape (the `bynk.queue.*`
codes).

### §5.7.1 CORS policy (v0.131) {#cors}

A `from http` service MAY declare a single **`cors { }`** policy in header position
(before the handlers). It is legal only on a `from http` service
(`bynk.http.cors_not_http`) and at most once per service
(`bynk.parse.duplicate_cors`). The grammar accepts any `name: value` field; the
checker enforces the closed field set — an unknown field is
`bynk.http.cors_unknown_field`. The fields:

- **`origins`** — REQUIRED; a non-empty list of string literals
  (`bynk.http.cors_invalid_origins`). The allowlist of origins, or the single
  wildcard `["*"]`.
- **`headers`** — OPTIONAL; a list of string literals
  (`bynk.http.cors_invalid_field`). Overrides the default `Access-Control-Allow-Headers`.
- **`credentials`** — OPTIONAL; a boolean literal (`bynk.http.cors_invalid_field`).
- **`maxAge`** — OPTIONAL; a `Duration` literal (`bynk.http.cors_invalid_field`).

`credentials: true` combined with a wildcard origin (`["*"]`) is rejected
(`bynk.http.cors_wildcard_credentials`) — the Fetch standard forbids that pair.
`Access-Control-Allow-Methods` is **not** a field: it is derived from the service's
declared route methods (plus `HEAD` where `GET` is declared, plus `OPTIONS`) — the
same derivation that drives the always-on `Allow` header. A service with no
`cors { }` policy emits no `Access-Control-*` headers (and a cross-origin preflight
carries no CORS grant, so the browser blocks the read); it still answers the
always-on method contract — a plain `OPTIONS` is a `204 + Allow`, a wrong method a
`405 + Allow`, and `HEAD` mirrors `GET`. See [§7 emission](/book/spec/emission/)
for the synthesised preflight, the method-contract fall-through, and header
stamping, whose ordering (the preflight and the `405`/`OPTIONS` answers precede the
handler authentication seam) is normative.

### §5.7.2 Handler caching annotation (v0.140) {#cache}

A `GET` handler MAY carry a single **`@cache`** annotation in handler position
(immediately before `on`). It is the first handler-position annotation; the grammar
admits any `@name(args)` before a handler, so a dangling annotation with no
following `on` is a parse error (`bynk.parse.dangling_handler_annotation`) and any
name outside the closed set is `bynk.http.unknown_handler_annotation`. `@cache` is
legal **only on an `on http GET` handler** — on any other method, protocol, or an
agent handler it is `bynk.http.cache_on_non_get` — and at most once per handler
(`bynk.http.cache_duplicate`). Its arguments:

- **`maxAge`** — REQUIRED; a positive `Duration` literal
  (`bynk.http.cache_bad_max_age`). The freshness window, lowered to
  `Cache-Control: max-age` in whole seconds.
- **`scope`** — OPTIONAL; the bare identifier `public` or `private`
  (`bynk.http.cache_bad_scope`), defaulting to `private`.

Any other argument is `bynk.http.cache_unknown_arg`. The annotation governs only
the opt-in **freshness** half; the conditional **`ETag`/`304`** half is automatic
for every eligible `GET` (a handler whose success representation is `Ok`) and
carries no author surface. Because eligibility is a value-level property — the
returned variant, not the `Effect[HttpResult[T]]` return type — the runtime
attaches the validator only to an `Ok` response; a `@cache` on a `GET` that returns
only `Streaming`/`Raw` is a harmless no-op for the stream (a static diagnostic for
that case is a named follow-on). See [§7.4.3](/book/spec/runtime-library/#743-httpresult)
for the runtime lowering and the normative composition order.

### §5.7.3 Security-headers policy (v0.141) {#security}

A `from http` service MAY declare a single **`security { }`** policy in header
position (beside `cors { }`). It is legal only on a `from http` service
(`bynk.http.security_not_http`) and at most once per service
(`bynk.parse.duplicate_security`). The grammar accepts any `name: value` field; the
checker enforces the closed field set — an unknown field is
`bynk.http.security_unknown_field`. The fields:

- **`nosniff`** — OPTIONAL; a boolean literal (`bynk.http.security_invalid_field`).
  Whether `X-Content-Type-Options: nosniff` is stamped. Defaults to **`true`**.
- **`hsts`** — OPTIONAL; a **positive** `Duration` literal
  (`bynk.http.security_invalid_field`). Opts in to `Strict-Transport-Security`,
  lowered to `max-age` in whole seconds. Absent ⇒ no HSTS.

Unlike `cors`, the safe header is **on by default**: a `from http` service with no
`security { }` policy (or one that omits `nosniff`) still stamps
`X-Content-Type-Options: nosniff` on every response — the compiler synthesises a
default policy (`nosniff: true`, no HSTS) for **every** `from http` service. Only an
explicit `nosniff: false` suppresses it. The headers are stamped uniformly across
every response family and the synthesised preflight, `405`/`OPTIONS`, and `304`,
composing with the CORS headers (the two sets are disjoint, so the stamping order is
not observable). `Content-Security-Policy` and `X-Frame-Options` are **not** emitted
— they constrain markup, which the Bynk HTTP surface does not serve. See
[§7.4.3](/book/spec/runtime-library/#743-httpresult) for the runtime lowering.

### §5.7.4 Request body-size limits (v0.142) {#body-limits}

A `from http` service MAY declare a single **`limits { }`** section in header
position (beside `cors { }` and `security { }`), and a body-taking handler MAY
carry a single **`@limit`** annotation that overrides it. Both bound the size of a
request body, in bytes.

The **`limits { }`** section is legal only on a `from http` service
(`bynk.http.limits_not_http`) and at most once per service
(`bynk.parse.duplicate_limits`). The grammar accepts any `name: value` field; the
checker enforces the closed field set:

- **`maxBody`** — the only field; a positive `Int` byte count
  (`bynk.http.limits_invalid_field`). Any other field is
  `bynk.http.limits_unknown_field`.

The **`@limit`** annotation is a handler-position annotation (the `@cache`
position, ADR 0111). It is legal **only on an `on http POST`/`PUT`/`PATCH`
handler** — a body-taking method — and on a `GET`/`DELETE` (or any bodyless
method) it is `bynk.http.limit_on_bodyless`; at most once per handler
(`bynk.http.limit_duplicate`). Its arguments:

- **`maxBody`** — the only argument; a positive `Int` byte count
  (`bynk.http.limit_bad_max_body`). Any other argument is
  `bynk.http.limit_unknown_arg`.

**Effective cap and precedence.** For a body-taking route the effective cap is the
route `@limit` `maxBody` if present, otherwise the service `limits { maxBody }` if
present, otherwise **none**. A route with no effective cap reads the body
unchanged, so this surface is **opt-in** (the CORS posture, not the security
default-on posture): a service declaring neither `limits { }` nor any `@limit` is
byte-for-byte unchanged.

**Enforcement.** A capped route rejects a request whose `Content-Length` exceeds
the effective cap with a synthesised **`413 PayloadTooLarge`** (`{ kind:
"PayloadTooLarge", details: … }`), produced **before the body is read** and
**before the `by`/Bearer auth seam** — the boundary posture of the method-semantics
`405` ([§5.7](#57-handlers)). It reuses the existing `413` status; the closed
`HttpResult` registry is **unchanged**. The `413` is stamped with the CORS and
security headers so a cross-origin caller can read it. Because it keys on
`Content-Length` — which may be absent (chunked) or spoofed — this is a fast-reject,
not a hard guarantee; it pairs with the platform request cap, and a streamed-read
cap is a named follow-on. `maxBody` is an `Int` byte count in this version; a byte
`Size` literal is a named follow-on. See
[§7.4.3](/book/spec/runtime-library/#743-httpresult) for the runtime lowering and
its position in the dispatch order.

A **`from websocket(in: I, out: O)`** service (v0.103) binds the inbound frame
type `I` and the server-sent frame type `O` on its header, and declares the
connection-lifecycle handlers:

- **`on open`** — exactly one per service. The upgrade is authenticated **at the
  edge**: like a `from http` handler it MUST name its actor with `by` (there is no
  anonymous upgrade), and the boundary admits `None`/`Bearer` actors and rejects
  `Signature` (a browser cannot set an `Authorization` header). The handler
  receives a fresh, **owned** `connection: Connection[O]` that it MUST dispose of
  under the held-resource linearity discipline
  ([§5.4.2](/book/spec/static-semantics/#542-held-resource-linearity-v0102)) — the canonical
  disposal being transfer into an agent. On the Workers target that transfer MUST
  resolve to exactly one routable agent (`bynk.ws.open_transfer_shape`).
- **`on message`** (v0.106) — an inbound frame arrived; its parameters are the
  route params plus the decoded frame of type `I`.
- **`on close`** (v0.106) — the connection ended; the stored connection is
  disposed (typically via the owning agent).

Inbound frames are dispatched to the agent that owns the connection, so the
service holds **exactly one** `on open` and broadcasts by iterating a held
`Map[K, Connection]` ([§5.4.2](/book/spec/static-semantics/#542-held-resource-linearity-v0102)).

{{#grammar-semantics http_handler}}

## §5.7a Actors & the `by` clause (v0.45)

An `actor` MUST be declared inside a context (`bynk.actor.outside_context`). Its
`auth` scheme MUST be compiler-known (`bynk.actor.unknown_scheme`) — `None`,
`Internal`, `Bearer` (v0.47), and `Signature` (v0.51) are supported. A `Bearer`
actor MUST name its signing secret (`auth = Bearer(secret = "<ENV>")`,
`bynk.actor.bearer_missing_secret`) and MUST declare a string-constructible
`identity` — a refined or opaque `String`, minted from the JWT `sub` claim
(`bynk.actor.bearer_identity_not_string_constructible`); `Bearer` is admissible
only on `from http` handlers. A `Signature` actor (HMAC over the request body)
MUST name its secret (`bynk.actor.signature_missing_secret`) and its signature
`header` (`bynk.actor.signature_missing_header`); a `tolerance` requires a
`timestamp` header (`bynk.actor.signature_tolerance_without_timestamp`); a
`Signature` actor takes **no** `identity` — the signature attests authenticity,
not a principal (`bynk.actor.signature_identity_unsupported`) — and is admissible
only on `from http` handlers. An **`Oidc`** actor (v0.151) verifies an
asymmetrically-signed (RS256/ES256) JWT against a provider's published JWKS. It
MUST name its `issuer` (`bynk.actor.oidc_missing_issuer`), its `audience`
(`bynk.actor.oidc_missing_audience`), and its `jwks` endpoint URL
(`bynk.actor.oidc_missing_jwks`) — public trust parameters, **no secret** — and
MUST declare a string-constructible `identity`, minted from the verified `sub`
claim (`bynk.actor.oidc_identity_not_string_constructible`); `Oidc` is admissible
only on `from http` handlers and, this slice, only as a **single** actor — never a
sum member (`bynk.actor.oidc_not_in_sum`) and not a refinement base. A declared
`identity = T` MUST be a context-ownable
value type, so the verified identity is sealed — minted only inside the owning
context (`bynk.actor.identity_not_sealed`).

> **Who, not whose.** An actor contract answers *who* is at the boundary — it
> authenticates a party and seals its identity. It deliberately does **not**
> answer *whose* a given object is: object-level authorisation (may *this* user
> read *this* record?) is domain logic and lives in the handler body, by design.
> The `where`-clause authorisation invariants below narrow *who* (a claim the
> party carries), never *whose*.

The **refinement form** `actor Admin = User where <predicate>` (v0.53) declares
an **authorisation invariant**: an `Admin` is a `User` who additionally satisfies
the predicate. Its base MUST be a declared `Bearer` actor — only `Bearer` carries
claims to authorise against (`bynk.actor.refinement_base_unsupported`) — and its
`where` predicate MUST be in the closed claim-predicate set: `hasClaim("name")`
(the claim is present and truthy) and `claimEquals("name", "value")` (string
equality), composed with `&&`, `||`, `!` (`bynk.actor.refinement_predicate_unsupported`).
A refinement actor is a handler's sole `by` contract, never a sum member
(`bynk.actor.refinement_in_sum`, §5.7a.1). By refinement elimination an `Admin`
is usable wherever its base `User` is: a `by a: Admin` binder yields the base
`User` identity. The invariant is discharged at the boundary (§7.3.4a): the scheme
is verified (failure → 401), then the predicate is checked against the verified
claims (failure → **403**, distinct from 401), then the identity is minted and the
body runs.

A handler consumes an actor on its `by (<binder>:)? <Actor>` clause. The named
actor MUST resolve to a declared actor or a prelude actor (`Visitor`,
`Scheduler`, `Producer`, `Caller`) (`bynk.actor.unknown_actor`), and its scheme
MUST be admissible on the handler's protocol — HTTP admits `None`, `Bearer`,
`Signature`, and `Oidc`; the internal protocols (call/cron/queue) admit `Internal`
(`bynk.actor.scheme_not_admissible`). A `Signature` handler MUST take a `body`
parameter — the signature is computed over the request body, so a bodyless signed
request is meaningless (`bynk.actor.signature_requires_body`). A handler that
omits `by` inherits its protocol's default actor; an **HTTP handler has no safe
default and MUST declare `by`** (`bynk.actor.missing_by_on_http`).

The `Caller` prelude actor (the `on call` default) yields a **live `CallerId`**
(v0.54): a cross-context `on call … by c: Caller (…)` handler binds `c.identity`
to the **calling context's qualified name**, established at the boundary over the
internal Service Binding before the body runs. The `Internal` scheme trusts the
channel — verification is static / channel-based, no crypto — but a call that
does not identify its caller is rejected fail-closed (the internal analogue of a
401). A binder-less `on call` captures nothing and is unaffected.
The **binder is optional** (v0.50): with `by <binder>: <Actor>` the verified
identity binds to `<binder>` and is read as `<binder>.identity` — a sealed value,
minted at the boundary before the body runs and never re-checked downstream; with
the binder-less `by <Actor>` the contract is still declared and verified
fail-closed, but no identity is captured (anonymous / verify-and-discard). `_`
MUST NOT be used as the binder (omit it instead). A named binder MUST NOT collide
with a handler parameter (`bynk.actor.binder_shadows_param`).

### §5.7a.1 Multi-actor sum dispatch (v0.52)

A `by` clause MAY name an **ordered sum of peer actors**
(`by who: A | B | …`): distinct parties distinguished by **scheme**, resolved
**first-wins**. The boundary tries each peer's scheme in declared order and binds
the first that verifies; the body `match`es on the resolved actor, each arm
yielding that actor's identity directly (`User(u)` ⇒ `u` is the `User` identity;
a unit-identity peer such as `Visitor` or a `Signature` webhook binds nothing).
A sum is well-formed when:

- it **binds the resolved actor** — a sum MUST have a binder, since the body
  learns *which* peer verified by matching it (`bynk.actor.sum_requires_binder`);
- its members are **peer base actors** — a refinement actor (`actor A = B where
  …`) MUST NOT be a member (every `A` is a `B`, so the arm is dead,
  `bynk.actor.refinement_in_sum`); narrowing belongs *inside* the resolved arm;
- **no two members share a scheme** — peers are distinguished by scheme, so a
  second same-scheme member is unreachable (`bynk.actor.duplicate_sum_scheme`);
- a **`None`-scheme (catch-all) member is last** — it accepts every caller, so any
  member after it is unreachable (`bynk.actor.unreachable_sum_arm`);
- **every member is admissible** on the handler's protocol (in practice a sum is
  HTTP-only — the only protocol with more than one admissible non-internal
  scheme) (`bynk.actor.scheme_not_admissible`);
- the body `match` is **exhaustive** over the members (the ordinary
  sum-exhaustiveness rule, `bynk.types.non_exhaustive_match`).

The reachability checks are **decidable and scheme-level**; the compiler does not
reason about predicate-level disjointness. Total verification failure (no member
verifies) is **fail-closed → 401**; a sum's members carry no invariants, so there
is no 403 path. Verification is side-effect-free and idempotent: first-wins
short-circuits, so the set and order of verifications attempted is observable, and
audit/logging belongs *after* resolution.

## §5.8 Boundaries & cross-context

`consumes` MUST appear only in a context or an adapter (`bynk.consumes.in_commons`),
MUST name an existing context or adapter — not a `commons`
(`bynk.consumes.unknown_context`, `bynk.consumes.target_is_commons`) — and not the
consumer itself (`bynk.consumes.self_reference`), and MUST NOT produce colliding
names or aliases (the `bynk.consumes.*` codes). Calling another context's service
requires a `consumes` declaration (`bynk.resolve.unconsumed_context`), and units
MUST NOT form a `consumes` cycle (`bynk.context.consumes_cycle`).

A **capability selection** (`consumes b { Cap, … }`) flattens each named
capability into the consumer's local namespace under its bare name, so it reads
as `given Cap` / `Cap.op(…)`. Each selected name MUST be a capability the target
**exports** (`bynk.given.cross_context_unknown_capability`), and a flattened bare
name MUST NOT collide with a locally declared capability or with a name
flattened from another unit (`bynk.consumes.capability_name_clash`) — a clash is
resolved by the qualified `given b.Cap` form or an alias.

An **adapter's** `consumes` is further restricted: it MUST use the
capability-selection form — an adapter has no services to call, so the
whole-unit and aliased forms are rejected
(`bynk.adapter.consumes_requires_selection`) — and it MUST target an adapter,
never a context (`bynk.adapter.consumes_context`).

`exports` MUST name declared types or capabilities, MUST NOT export a name twice
or with conflicting visibility, and an exported capability MUST have a provider
(the `bynk.exports.*` codes). A value crossing a boundary MUST be structurally
compatible with the receiving side ([§6.5](/book/spec/type-system/#65-type-compatibility--boundaries),
`bynk.boundary.structural_mismatch`); a context-owned type MUST NOT be constructed
or an opaque export inspected from outside (`bynk.context.external_construction`,
`bynk.context.opaque_inspection`).

**Adapters are the host boundary.** An adapter MUST NOT declare a `service` or an
`agent` (`bynk.adapter.disallowed_item`); it MAY declare at most one `binding`
clause (`bynk.adapter.duplicate_binding`), and MUST declare one if it declares
any external provider (`bynk.adapter.no_binding`). A binding's `requires` ranges
MUST be pinned: a range MUST name at least one version digit — `*`, `x`,
`latest`, and digit-free ranges are rejected
(`bynk.requires.unpinned_dependency`). The `bynk` namespace is **reserved for the
toolchain**: no user unit's name may have `bynk` as its first segment
(`bynk.namespace.reserved`); the toolchain's own first-party adapters — the
ambient `bynk` surface and the `bynk.<platform>` platform adapters
([§7.3.6](/book/spec/emission/#736-adapters)) — live inside that reserved prefix and are
injected when a unit consumes them.

**The platform lock** (v0.19). A capability of a **platform adapter**
(`bynk.cloudflare`) is *platform-native*: its binding runs only on that
platform. A **deployment unit** — each context under `--target workers`; the
whole program under `bundle`, where co-location shares the lock — is locked to
the union of native platforms its **in-process closure** reaches: the providers
its composition would instantiate, walked through `given` and flattening edges.
A service `consumes` edge between contexts is RPC under `workers` and does
**not** propagate the lock. The selected `--platform` MUST be the locked
platform (`bynk.target.vendor_required`), and one deployment unit MUST NOT span
two mutually-exclusive native platforms (`bynk.target.vendor_conflict`). The
`bynk` surface and library adapters impose no lock. New operations on an
already-native capability (v0.23: `Kv.putTtl`, `Kv.list`) inherit the lock
unchanged — no per-operation rules exist.

A `system`-tier test ([§5.9c](#59c-tiers-v0118)) derives its wired participant set from
the unit under test's transitive `consumes` closure; the inferred set MUST span at
least two contexts (`bynk.tier.system_needs_wire`). There is no participant list,
so the set cannot drift from the dependency graph and every consumed dependency is
wired by construction.

{{#grammar-semantics consumes_decl}}

{{#grammar-semantics adapter_decl}}

{{#grammar-semantics binding_decl}}

## §5.9 Testing constructs

An `expect` MUST occur only in a `case` body and MUST be given a `Bool` predicate
(`bynk.expect.outside_case`, `bynk.expect.not_bool`) — the same invariant
predicate surface as `invariant`/`ensures` (one predicate surface, ADR 0144). A
`suite` MUST target an existing unit and MUST NOT duplicate a case description
(`bynk.suite.unknown_target`, `bynk.suite.duplicate_case_name`).

`Val[T]` (v0.114, retiring `Mock[T]`) MUST occur only in a test body
(`bynk.val.outside_test`), name a resolvable type (`bynk.val.unknown_type`), and
receive pins that are compile-time literals of the right arity satisfying the
type (`bynk.val.pin_not_literal`, `bynk.val.arity`, `bynk.val.literal_violates`);
a type that cannot be fabricated MUST be pinned (`bynk.val.needs_pin`,
`bynk.val.pin_unsupported`, `bynk.val.unsupported_kind`).

{{#grammar-semantics val_expr}}

### §5.9c Tiers (v0.118)

*(ADR 0153)* A `case` runs at one of three **tiers** — `unit` | `integration` |
`system` — declared by an optional `as <tier>` clause on the `case` header. `unit`
is the default and is elided. `as` also sits on the `suite` header as an inherited
default; a case's effective tier is `case.tier ?? suite.tier ?? unit`, so a case
always overrides the suite default. A tier is metadata on the header, **not** an
executable statement: promotion changes substitution, not assertion, and the case
body is identical across tiers.

- **Tiers are `case`-only.** A `property` generates and does not promote, so a
  suite-level `as` binds its `case` members only; a tier attached to a `property`
  header (or a `property` that would inherit one) is `bynk.tier.property_has_tier`.
- **Participants are inferred (DECISION K).** For `integration` / `system` the
  real/wired participant set is the unit under test's transitive `consumes` closure
  — there is no `wires` clause. `system` is the cross-context, wired tier and its
  inferred set MUST span at least two contexts (`bynk.tier.system_needs_wire`);
  `integration` is real collaborators **within one context, no wire**, and carries
  no ≥ 2-context rule.
- **The agent-state lifecycle is fixed across tiers (DECISION D7).** The unit under
  test is always a real in-memory instance, keyed normally, fresh per case; only
  the realness of its collaborators and whether sends cross a serialisation
  boundary change with the tier. Snapshot and step invariants are checked at the
  commit boundary and therefore fire at every tier.
- **At `unit`, an un-overridden seam keeps its real provider (DECISION D8).** Full
  auto-stubbing (a synthesised return per collaborator) is a named follow-on, not
  part of this increment; `unit` and `integration` differ in the default provision
  discipline the author follows, and `system` is the tier that differs
  mechanically (it wires participants across the real boundary).

The tier names `unit` / `integration` / `system` are **contextual** — parsed only
in the `as`-clause position after a case/suite header; elsewhere they remain
ordinary identifiers.

### §5.9d Test-double provision — `stub` (v0.118)

*(ADR 0154; keyword `stub` since #548, formerly a pun on `provides`)* A
`stub Cap.method(<args>) returns <value> | fails` clause
overrides one capability seam's provision under test. `as <tier>` sets the
*default* provision of every seam; a `stub` clause overrides one. It names a
consumed seam of the unit under test (`consumes` declares, `given` requires,
`stub` substitutes under test), scoped to a test.

- **Seam resolution and legality (DECISION M).** `stub` is **capability-only**:
  its target MUST be a capability the unit under test consumes / has in scope via
  `given` (`bynk.stub.not_a_seam`), and `method` MUST be one of that
  capability's declared operations (`bynk.stub.unknown_op`).
- **Scope and precedence.** A `stub` MAY appear at suite scope (applies to every
  case) and at case scope (overrides for one case); precedence is case `stub` >
  suite `stub` > the tier default.
- **Argument patterns (DECISION D3).** Each parameter takes a pattern from the one
  predicate surface — `_` (any) or a literal / pure value the recorded argument must
  equal, plus `is` narrowing. Multiple clauses for one method form an ordered match
  list, tried top to bottom, **first match wins**, so a specific clause MUST precede
  a fallback.
- **RHS typing (DECISION D2).** The right-hand side is a *value* or the fault atom
  `fails`, never a computed body. A `returns <value>` whose type disagrees with the
  operation's declared return type is `bynk.stub.rhs_type`.
- **Sequenced provision (DECISION V).** `returns each [<outcome>, …]` supplies one
  outcome per call, in order; each outcome is a value, `fails`, or `ok(v)`. On
  **exhaustion the last outcome repeats** (steady state). A malformed sequence
  (e.g. empty) is `bynk.stub.bad_sequence`.

Bare observation ([§5.9b](#59b-observation)) needs no `stub` — calls are
recorded at the seam regardless; a `stub` is written only when a case depends on
a collaborator's *return*.

### §5.9a Generative properties

*(v0.114)* A `property` is a generative test: its body is a single `for all`
binder over inhabitants the runner produces. Each binding `x: T` binds `x` to a
generated inhabitant of `T`; an optional `where <pred>` filters generated tuples
before the body runs; the body is one or more `expect`s (the one predicate
surface). The `where` filter MUST type to `Bool` (`bynk.property.where_not_bool`),
as MUST each `expect` (`bynk.expect.not_bool`).

**Generation (DECISION P): a type is its own inhabitant space.** The generator
for `T` produces only values satisfying `T`'s refinements, so a generated subject
is valid by construction. A `T` used in `for all` (or `Val`) MUST be
refinement-generable: a `String where Matches(re)` has no refinement-driven
generator and MUST be pinned instead (`bynk.val.needs_pin`); an **agent** MUST
NOT be generated (`bynk.val.agent_not_generable`) — a fabricated agent state need
not be reachable, so behavioural agent generation is handler-*sequence*
generation, deferred to the history rung.

**Restating a refinement is redundant.** A property whose predicate merely
re-checks a refinement the bound variable's type already guarantees is flagged
(`bynk.property.restates_refinement`). The check is **conservative** — it fires
only when the predicate is syntactically the refinement over the bound variable
(under-flagging is acceptable; over-flagging is not).

{{#grammar-semantics property_decl}}

{{#grammar-semantics for_all}}

### §5.9b Observation

*(v0.117)* An **observation** asserts over a unit's *interaction* with a
capability, inside a `case` — ADR 0152. Its subject is a `Cap.op` reference (a
capability and one of its operations, named not called). Well-formedness:

- the observation MUST occur inside a `case` body (`bynk.observe.outside_case`);
- `Cap` MUST be a capability the unit under test consumes / has in scope via
  `given` (`bynk.observe.not_a_seam`); `op` MUST be one of its declared operations
  (`bynk.observe.unknown_op`);
- a `with <pred>` predicate is the one predicate surface with the operation's
  parameters in scope by their declared names; it MUST type to `Bool`
  (`bynk.observe.with_not_bool`) and MUST be pure (`bynk.observe.impure_with`);
- a call count MUST be a non-negative integer literal
  (`bynk.observe.bad_count`) — `called once` desugars to a count of one.

`trace(Cap.op)` is a **test-only builtin** yielding `List[<CallRecord>]`, where
`<CallRecord>` is a synthetic record of the operation's parameters at their declared
types (`{ msg: String }` for `Logger.log`), registered into the test-body type table
the way a fabricated agent-state record is. It types as an ordinary `List`
(field access on its elements, `length()`, `all`/`any`, indexing); `trace` outside a
`case` is `bynk.observe.trace_outside_test`. Recording is *ambient at the seam and
test-build-only* — no source declares it, and the deploy build carries none of it
(see [emission](/book/spec/emission/)).

## §5.10 Collections

*(v0.20b)* `List[T]` and `Map[K, V]` are built-in generic types
([§6.2](/book/spec/type-system/#62-built-in-generic-types)); this section is their
static semantics.

**Construction.** A list literal `[a, b, c]`
([§4 list_literal](/book/reference/grammar/#rule-list_literal)) types each
element against the **expected element type** when one is supplied — so
refined literals admit ([§5.3](#53-refinement--admission)) — and a mismatched
element is `bynk.types.list_element_mismatch`. With no expected type, the
first element fixes the element type. An **empty `[]` MUST have an expected
type** (`bynk.types.uninferable_element_type`); the qualified statics
`List.empty()` and `Map.empty()` obey exactly the same rule — an expected
type is their only source of type arguments. `insert` and `prepend`
propagate an expected collection type down their receiver chain, so
`let m: Map[String, Int] = Map.empty().insert("a", 1)` infers.

**The kernel.** The built-in operations are compiler-known special forms,
dispatched on the receiver's checked type before declared-method lookup;
they may be generic in their accumulator without declared generic methods
existing (ADR 0037). The whole kernel:

| Receiver | Operation | Type |
|---|---|---|
| `List[T]` | `length()` | `Int` |
| `List[T]` | `get(i: Int)` | `Option[T]` |
| `List[T]` | `prepend(x: T)` | `List[T]` |
| `List[T]` | `fold(init: A, f: (A, T) -> A)` | `A` |
| `List[T]` | `foldEff(init: A, f: (A, T) -> Effect[A])` | `Effect[A]` |
| `List[T]` | `forEach(f: T -> Effect[()])` | `Effect[()]` |
| `List[T]` | `parTraverse(f: T -> Effect[()])` | `Effect[()]` |
| `List[T]` | `traverseAll(f: T -> Effect[Result[U, E]])` | `Effect[List[Result[U, E]]]` |
| `List[T]` | `parTraverseAll(f: T -> Effect[Result[U, E]])` | `Effect[List[Result[U, E]]]` |
| `List[T]` | `traverseTry(f: T -> Effect[Result[U, E]])` | `Effect[Result[List[U], E]]` |
| `List[T]` | `parTraverseTry(f: T -> Effect[Result[U, E]])` | `Effect[Result[List[U], E]]` |
| `List[T]` | `map(f: T -> U)` | `List[U]` |
| `List[T]` | `filter(p: T -> Bool)` | `List[T]` |
| `List[T]` | `flatMap(f: T -> List[U])` | `List[U]` |
| `List[T]` | `sortBy(key: T -> K)` | `List[T]` |
| `List[T]` | `take(n: Int)` / `skip(n: Int)` | `List[T]` |
| `List[T]` | `distinct()` | `List[T]` |
| `List[T]` | `distinctBy(key: T -> K)` | `List[T]` |
| `List[T]` | `joinOn(other: List[U], left: T -> K, right: U -> K, into: (T, U) -> V)` | `List[V]` |
| `List[T]` | `leftJoin(other: List[U], left: T -> K, right: U -> K, into: (T, Option[U]) -> V)` | `List[V]` |
| `List[T]` | `join(other: List[U], on: (T, U) -> Bool, into: (T, U) -> V)` | `List[V]` |
| `List[T]` | `groupBy(key: T -> K, into: (K, List[T]) -> V)` | `List[V]` |
| `List[T]` | `count()` | `Int` |
| `List[T]` | `any(p: T -> Bool)` / `all(p: T -> Bool)` | `Bool` |
| `List[T]` | `first()` | `Option[T]` |
| `List[T]` | `firstOrElse(default: T)` | `T` |
| `List[T]` | `sum(key: T -> K)` | `K` |
| `List[T]` | `min(key: T -> K)` / `max(key: T -> K)` | `Option[K]` |
| `List[T]` | `average(key: T -> K)` | `Option[Float]` |
| `Map[K, V]` | `length()` | `Int` |
| `Map[K, V]` | `keys()` | `List[K]` |
| `Map[K, V]` | `values()` | `List[V]` |
| `Map[K, V]` | `get(k: K)` | `Option[V]` |
| `Map[K, V]` | `insert(k: K, v: V)` | `Map[K, V]` |

A method outside the kernel is `bynk.types.method_not_found`; a wrong arity
is `bynk.types.method_arity`. **`foldEff`, `forEach`, `parTraverse`,
`traverseAll`, `parTraverseAll`, `traverseTry`, and `parTraverseTry` are effect
operations**: each runs its
effectful function value, so calling one in a pure context is
`bynk.effect.fn_value_in_pure_context`, exactly the function-value confinement of
[§5.5](#55-effects-capabilities--providers). `forEach(f: T -> Effect[()])`
(v0.146, [ADR 0170](https://github.com/accuser/bynk/blob/main/design/decisions/0170-do-statement-implicit-unit-and-list-foreach.md))
is the `Query.forEach` terminal over an eager list — the effect-per-element loop,
**sequential** (each element awaited in order). `parTraverse(f: T -> Effect[()])`
(v0.147, [ADR 0171](https://github.com/accuser/bynk/blob/main/design/decisions/0171-list-partraverse.md))
is its **concurrent** sibling — the `Query.parTraverse` fan-out over an eager
list, issuing every element's effect at once and awaiting them together, so a
slow element does not head-of-line-block the rest; the order in which side
effects interleave is unspecified. Unlike the sequential `forEach` (which
short-circuits on the first failure and never issues the rest), a rejecting
element does **not** cancel siblings already issued — every element's effect is
in flight before the first failure surfaces, and each runs to completion.

The **collect-all** pair `traverseAll` / `parTraverseAll` (v0.148,
[ADR 0172](https://github.com/accuser/bynk/blob/main/design/decisions/0172-list-collect-all-iterators.md))
take a **`Result`-returning** function `f: T -> Effect[Result[U, E]]` and return
`Effect[List[Result[U, E]]]` — every element's outcome, `Ok` or `Err`, gathered
into the result list in input order. Because a `Result` `Err` is a **value**, not
a fault, neither short-circuits: `traverseAll` awaits each element in turn,
`parTraverseAll` issues all at once and collects them together (its interleaving
order, like `parTraverse`, unspecified). The function *must* return
`Effect[Result[U, E]]`; a non-`Result` effect is `bynk.types.argument_mismatch`.
They are the fault-gathering counterpart to `traverse` (the short-circuiting
sequential collect); the collecting **short-circuit** `parTraverse` overload and
`traverse`'s `Result` overload remain a later slice. Like `forEach`/`parTraverse`,
both also apply over a lazy `Query[T]` and a lifted `store Map[K, V]` (v0.149,
[ADR 0173](https://github.com/accuser/bynk/blob/main/design/decisions/0173-map-values-and-broadcast-collect-all.md)),
so the fan-out reaches a held `Map[K, Connection]` — each connection is
**borrowed** into the closure (`send` allowed, `close`/transfer rejected as
`bynk.held.consume_on_borrow`) and the map keeps ownership.

The **short-circuit** pair `traverseTry` / `parTraverseTry` (v0.150,
[ADR 0174](https://github.com/accuser/bynk/blob/main/design/decisions/0174-short-circuit-collect-iterators.md))
take the same `f: T -> Effect[Result[U, E]]` but **stop at the first `Err`**,
returning `Effect[Result[List[U], E]]` — `Ok` of the collected values, or the
first `Err` encountered. `traverseTry` awaits each element in order and bails on
the first `Err` (later elements never run); `parTraverseTry` issues all at once,
awaits them, then returns the first `Err` in input order (in-flight calls are not
cancelled). They are the fault-**propagating** counterpart to the fault-gathering
`traverseAll`/`parTraverseAll`, and likewise apply over the `Query`/`Map`
broadcast.

*(v0.88, [ADR 0116](https://github.com/accuser/bynk/blob/main/design/decisions/0116-query-vocabulary-and-ordering.md))*
The builder/terminal rows above are the **eager in-memory half** of the query
algebra ([design notes §11](https://github.com/accuser/bynk/blob/main/design/bynk-design-notes.md)) —
the same combinator names a lazy storage `Query[T]` will carry. **Ordering keys**
(`sortBy`/`min`/`max`) are drawn from the closed orderable base set — `Int`,
`Float`, `String`, `Duration`, `Instant` (refined types widening; an opaque key
is **not** orderable) — else `bynk.types.key_not_orderable`. **Numeric keys**
(`sum`/`average`) are `Int`/`Float`/`Duration` (not `Instant` — instants are not
summable), else `bynk.query.sum_needs_numeric`;
`average` of a `Duration` is a `Duration` (integer-rounded millis), otherwise a
`Float`. **`distinct`/`distinctBy`** need a value-keyable element/key (the
`Map`-key rule, incl. opaque), else `bynk.types.unkeyable_distinct`. **Empty
aggregates are total**: `first`/`min`/`max`/`average` return `Option` (`None` on
empty); `sum` returns the zero, `count` returns `0`. The aggregate terminals take
a **projection** `T -> K`, uniform with the storage half where a record field is
the common key.

*(v0.94, [ADR 0120](https://github.com/accuser/bynk/blob/main/design/decisions/0120-join-group-combiner-form.md))*
**Joins & grouping take a combiner** — bynk has no pair type, so the join row
exists only inside `into`, which names the result. `joinOn`/`leftJoin` are
equi-joins hashing on a **value-keyable** key (the `Map`-key rule); the left and
right key functions MUST return the **same** key type, else
`bynk.query.join_key_mismatch`. `join` is a general-predicate (nested-loop) join.
`groupBy` partitions by a value-keyable key in **first-seen** key order, projecting
each `(K, List[T])` group through `into`. The `other` side is a `List`/`Query` of
the **matching shape** (a `List` joins a `List`, a `Query` joins a `Query` — a bare
`store Map` used as a value lifts to a `Query` over its values). The same names
carry to a storage `Query` (the builders return `Query[V]`, still lazy). Arguments
are **positional** in v1 (labelled call arguments are a named deferral). The
cross-shape `Map × Log` join lands with the storage `Log` slice.

*(v0.92, [ADR 0115](https://github.com/accuser/bynk/blob/main/design/decisions/0115-query-model-lazy-eager-dispatch.md)/[0119](https://github.com/accuser/bynk/blob/main/design/decisions/0119-durable-object-query-lowering.md))*
The same combinator names form a **lazy** query over a `store` `Map[K, V]` field —
dispatched by **receiver provenance** (ADR 0110, generalised from op-set to
evaluation strategy). A chain rooted in a store map is lazy: a builder lifts the
map's **values** into a `Query[V]` (`reservations.filter(r => …)`) and chains
build further `Query`s; a **terminal** executes it and is **`Effect`-typed**
(`.collect() -> Effect[List[V]]`, awaited with `<-`), folding into the storage
capability the `store` fields carry — no new `given`. **`Query[T]` is a
first-class, by-reference, non-storable and non-boundary type** (like
`Effect`/`Fn`): nameable in a pure helper's return, passable as an argument, but
rejected in any storable or boundary position (`bynk.types.query_at_boundary`).
`flatMap`'s function returns a `Query` over storage (`T -> Query[U]`). Joins and
`groupBy` arrive with a later slice. A query is **agent-local** (it reaches only
the owning agent's storage) and reads **staged** state (read-your-writes); it
lowers to a scan over the in-memory `Record` of the wholesale-persisted map, or
to an **index lookup** when an `@indexed` field routes it (below).

*(v0.93, [ADR 0118](https://github.com/accuser/bynk/blob/main/design/decisions/0118-indexed-indexing-model.md))*
A `store Map[K, V]` field may carry `@indexed(by: f, …)` to maintain a **secondary
index** on one or more of its value type's fields. Each `by:` target MUST name a
**value-keyable field** of `V` (the `Map`-key rule — `String`/`Int`, incl.
refined/opaque over them); a non-`by:` argument is `bynk.index.bad_argument`, a
field the value type lacks is `bynk.index.unknown_key`, and a non-keyable field
is `bynk.index.unkeyable_key`. The runtime maintains a sibling posting-list
`Record` per indexed field **inside the same atomic commit** (ADR 0109) as the
map it indexes — re-indexed on every `put`/`update`/`upsert`/`remove`
(last-write-wins). An **equality `filter` directly on the map**
(`reservations.filter(r => r.f == v)`, with `v` not mentioning the row) **routes**
to a posting-list lookup instead of a scan; any other predicate (a comparison, a
compound condition, a filter deeper in a chain) still scans. **Index hygiene is
build-time *warnings*** (non-failing, ADR 0117): an equality filter on a
non-indexed keyable field is `bynk.index.missing` (add the index), and a declared
index no equality filter routes through is `bynk.index.unused` (it costs
maintenance on every write). Under the wholesale-`Record` representation the index
is a **CPU** optimisation (the map loads whole regardless); the I/O scaling awaits
per-entry storage keys. The most-selective tie-break and a `bynk.index.ambiguous`
note arrive with compound-predicate routing (a later slice).

**Keys.** A `Map` key type MUST be value-keyable — `String`, `Int`, or a
refined/opaque type over them; anything else is
`bynk.types.unkeyable_map_key`, checked at every written `Map[K, V]`
reference. A type parameter is admitted in key position: it can only be
instantiated through a concrete reference elsewhere, which is checked.

**Order.** A `List` is ordered by construction. A `Map` is
**insertion-ordered**, normatively: `keys()` enumerates in insertion order,
and `insert` on an existing key updates in place, keeping its position.

**Boundaries.** Collections serialise: a handler may take or return a
`List` or `Map`, and both may appear in record fields, sum payloads, agent
state, and capability signatures. The function-type confinement of
[§5.8](#58-boundaries--cross-context) **looks through** collections — a
`List[Int -> Int]` in a boundary position is still
`bynk.types.function_at_boundary`. The wire forms are
[§7.3.7](/book/spec/emission/#737-collections).

**The combinator stdlib.** Everything derivable from the kernel is ordinary
Bynk in the first-party `bynk.list` / `bynk.map` commons
([§8.4](/book/spec/compilation-model/#84-build-pipeline--conformance-to-typescript)),
imported with `uses bynk.list` like any commons: `map`, `filter`, `find`,
`any`, `all`, `reverse`, `traverse` (sequential); `values`, `contains`,
`getOr`. There is no `Map.fromList` — Bynk has no pair type to spell its
argument with; maps build via `Map.empty()` + `insert` folds.

> **Deprecated (v0.91, ADR 0116 D6).** The free functions whose `List` method
> forms exist — `map`, `filter`, `find`, `any`, `all` — emit a non-failing
> `bynk.list.deprecated_function` warning at each call site (the build still
> succeeds; [§9](/book/spec/diagnostics/)), with a machine-applicable fix to the method
> form. They still work; `reverse` and `traverse` keep their free-function form.

```bynk
context jobs

uses bynk.list

capability Clock {
	fn now() -> Effect[Int]
}

provides Clock = FixedClock {
	fn now() -> Effect[Int] {
		42
	}
}

service stamps {
	on call(names: List[String]) -> Effect[Result[List[Int], ()]]
			given Clock {
		let stamped <- traverse(names, (name) => Clock.now())
		Ok(stamped)
	}
}
```
