# §5 Static semantics

A program that parses ([§3](lexical-grammar.md), [§4](syntactic-grammar.md)) is
not yet known to be well-formed. This chapter states the **well-formedness
rules**: the conditions a program MUST satisfy beyond parsing, each tied to the
`bynk.*` diagnostic a conforming implementation emits when the rule is violated
([§1.3](scope.md)). A program is well-formed exactly when it provokes no such
diagnostic.

> [!NOTE]
> Lexical and grammatical errors — the `bynk.lex.*` and `bynk.parse.*` codes —
> are *syntactic*: they report a text that does not match the grammar, and are
> governed by §3 and §4. This chapter covers only **post-syntactic**
> well-formedness. This note is informative.

The rules are organised by theme. Each theme states its load-bearing rules and
cites the governing codes; the **exhaustive** code-by-code catalogue is the
[diagnostic index](../reference/diagnostics.md) (and §9). Where a theme maps to a
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

An `if` condition MUST be a `Bool` and both branches MUST share a type
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
dissolves the apparent circularity. A nested lambda's effects are its own. A
`commit` MUST NOT appear inside a lambda (the existing
`bynk.commit.outside_agent`).

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
(boundaries are guarded: [§7.2](emission.md#72-targets)).

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

## §5.3 Refinement & admission

A refinement's predicates MUST apply to the type's base — a string predicate on
an `Int` is rejected (`bynk.types.predicate_base_mismatch`) — and MUST be
internally consistent: an `InRange` MUST NOT be inverted
(`bynk.types.inverted_range`), a length MUST NOT be negative
(`bynk.types.negative_length`), a `Matches` regex MUST be valid
(`bynk.types.invalid_regex`), and the predicates together MUST admit at least one
value (`bynk.types.empty_refinement` — on `Float`, `Positive` excludes the
lower endpoint `0.0`, so `InRange(-1.0, 0.0) and Positive` is empty).

`InRange` bounds MUST match the numeric base (v0.21): integer bounds on
`Int`, float bounds on `Float`. A bound of the other numeric type, or a
mixed pair, is `bynk.types.no_numeric_coercion`.

A **literal** written where a refined type is expected is admitted at compile
time ([§6.4](type-system.md#64-admission--construction)) in these positions:
return (block tail), a `let` with a type annotation, an `Ok`/`Some`/`Err`
payload, and a refined-typed call argument. The literal MUST satisfy the
predicate, or it is rejected (`bynk.refine.literal_violates`); an admitted
literal MUST be a compile-time literal, not an expression or identifier. **Opaque
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

Every `state` field MUST have a defined initial value: either an **explicit
initialiser** — a compile-time constant of the field's type, not referencing
`self`, parameters, or capabilities (`bynk.agents.bad_state_initialiser`) — or an
**implicit zero** (`Int` → `0`, `Bool` → `false`, `String` → `""`, `Option[T]` →
`None`, a record of zeroable fields). A field with neither is rejected
(`bynk.agents.non_zeroable_state_field`).

A `commit` MUST occur only in an agent handler (`bynk.commit.outside_agent`), its
value MUST match the agent's state type (`bynk.commit.wrong_state_type`), and at
most one `commit` may be reachable on any execution path
(`bynk.commit.two_reachable_commits`). Constructing or calling an agent MUST use
the right key arity and type and a declared handler (`bynk.agent.construction_arity`,
`bynk.agent.key_mismatch`, `bynk.agent.handler_arity`, `bynk.agent.handler_not_found`).

### §5.4.1 Invariants (v0.80)

An **invariant** is a universally-quantified property that MUST hold of every
committed agent state (`design/bynk-design-notes.md` §14; ADR 0107). Its predicate
references the agent's state fields by bare name and is a *pure, agent-local
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
evaluated against the value passed to `commit`, before the state is persisted. A
violation is a **fault** (`InvariantViolation`), not an outcome — see §7 and the
emission model. "Revert" is the **non-persistence of the offending commit**, not
whole-handler rollback (ADR 0107 D6): effects already performed by the handler,
and any earlier `commit`, stand.

{{#grammar-semantics state_decl}}

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
MUST share a result type (`bynk.types.match_arm_mismatch`), MUST NOT repeat a
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
(`bynk.http.body_on_get_or_delete`). An cron handler MUST take at most one
`Int` parameter, a valid five-field schedule, and return `Effect[Result[(), E]]`
(the `bynk.cron.*` codes); an `on message` handler MUST take exactly one `message`
parameter, a non-empty queue name, and the same return shape (the `bynk.queue.*`
codes).

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
only on `from http` handlers. A declared `identity = T` MUST be a context-ownable
value type, so the verified identity is sealed — minted only inside the owning
context (`bynk.actor.identity_not_sealed`).

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
MUST be admissible on the handler's protocol — HTTP admits `None`, `Bearer`, and
`Signature`; the internal protocols (call/cron/queue) admit `Internal`
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
compatible with the receiving side ([§6.5](type-system.md#65-type-compatibility--boundaries),
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
([§7.3.6](emission.md#736-adapters)) — live inside that reserved prefix and are
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

An integration test MUST wire at least two distinct, declared contexts, MUST NOT
duplicate a participant or suite name, and MUST wire every consumed dependency
(the `bynk.integration.*` codes).

{{#grammar-semantics consumes_decl}}

{{#grammar-semantics adapter_decl}}

{{#grammar-semantics binding_decl}}

## §5.9 Testing constructs

An `assert` MUST occur only in a test case body and MUST be given a `Bool`
(`bynk.assert.outside_test`, `bynk.assert.non_bool`). A `test` block MUST target
an existing unit and MUST NOT duplicate a case description
(`bynk.test.unknown_target`, `bynk.test.duplicate_case_name`).

`Mock[T]` MUST occur only in a test body (`bynk.mock.outside_test`), name a
resolvable type (`bynk.mock.unknown_type`), and receive pins that are
compile-time literals of the right arity satisfying the type
(`bynk.mock.pin_not_literal`, `bynk.mock.arity`, `bynk.mock.literal_violates`); a
type that cannot be fabricated MUST be pinned (`bynk.mock.needs_pin`,
`bynk.mock.unsupported_kind`). A `mocks` block MUST name an in-scope capability,
match its signature, and MUST NOT be used in an integration test or a commons
test (`bynk.mock.unknown_target`, `bynk.mock.signature_mismatch`,
`bynk.integration.mock_in_integration`, `bynk.mock.in_commons_test`).

{{#grammar-semantics mock_expr}}

## §5.10 Collections

*(v0.20b)* `List[T]` and `Map[K, V]` are built-in generic types
([§6.2](type-system.md#62-built-in-generic-types)); this section is their
static semantics.

**Construction.** A list literal `[a, b, c]`
([§4 list_literal](../reference/grammar.md#rule-list_literal)) types each
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
| `List[T]` | `map(f: T -> U)` | `List[U]` |
| `List[T]` | `filter(p: T -> Bool)` | `List[T]` |
| `List[T]` | `flatMap(f: T -> List[U])` | `List[U]` |
| `List[T]` | `sortBy(key: T -> K)` | `List[T]` |
| `List[T]` | `take(n: Int)` / `skip(n: Int)` | `List[T]` |
| `List[T]` | `distinct()` | `List[T]` |
| `List[T]` | `distinctBy(key: T -> K)` | `List[T]` |
| `List[T]` | `count()` | `Int` |
| `List[T]` | `any(p: T -> Bool)` / `all(p: T -> Bool)` | `Bool` |
| `List[T]` | `first()` | `Option[T]` |
| `List[T]` | `firstOrElse(default: T)` | `T` |
| `List[T]` | `sum(key: T -> K)` | `K` |
| `List[T]` | `min(key: T -> K)` / `max(key: T -> K)` | `Option[K]` |
| `List[T]` | `average(key: T -> K)` | `Option[Float]` |
| `Map[K, V]` | `length()` | `Int` |
| `Map[K, V]` | `keys()` | `List[K]` |
| `Map[K, V]` | `get(k: K)` | `Option[V]` |
| `Map[K, V]` | `insert(k: K, v: V)` | `Map[K, V]` |

A method outside the kernel is `bynk.types.method_not_found`; a wrong arity
is `bynk.types.method_arity`. **`foldEff` is an effect operation**: it runs
its effectful step function sequentially, and calling it in a pure context
is `bynk.effect.fn_value_in_pure_context`, exactly the function-value
confinement of [§5.5](#55-effects-capabilities--providers).

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
[§7.3.7](emission.md#737-collections).

**The combinator stdlib.** Everything derivable from the kernel is ordinary
Bynk in the first-party `bynk.list` / `bynk.map` commons
([§8.4](compilation-model.md#84-build-pipeline--conformance-to-typescript)),
imported with `uses bynk.list` like any commons: `map`, `filter`, `find`,
`any`, `all`, `reverse`, `traverse` (sequential); `values`, `contains`,
`getOr`. There is no `Map.fromList` — Bynk has no pair type to spell its
argument with; maps build via `Map.empty()` + `insert` folds.

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
