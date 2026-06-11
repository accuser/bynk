# §5 Static semantics

A program that parses ([§3](lexical-grammar.md), [§4](syntactic-grammar.md)) is
not yet known to be well-formed. This chapter states the **well-formedness
rules**: the conditions a program MUST satisfy beyond parsing, each tied to the
`karn.*` diagnostic a conforming implementation emits when the rule is violated
([§1.3](scope.md)). A program is well-formed exactly when it provokes no such
diagnostic.

> [!NOTE]
> Lexical and grammatical errors — the `karn.lex.*` and `karn.parse.*` codes —
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
(`karn.resolve.unknown_name`, `karn.resolve.unknown_type`,
`karn.resolve.unknown_function`, `karn.resolve.unknown_field`). A name used where
a value is expected MUST denote a value, not a type (`karn.resolve.type_in_expr`,
`karn.resolve.type_as_function`); a function MUST be called, not referenced bare
(`karn.resolve.fn_without_call`).

Within a scope, names MUST be unique: duplicate types, functions, methods,
services, capabilities, providers, agents, record fields, variants, and
parameters are each rejected (the `karn.resolve.duplicate_*` codes). A `let`
binding MUST NOT shadow a function or a type (`karn.resolve.let_shadows_fn`,
`karn.resolve.let_shadows_type`).

A bare reference to a **named function** is a value only where a function
type is expected (v0.20a); elsewhere it MUST be called
(`karn.resolve.fn_without_call`). A call on an in-scope **value** is legal
only when the value's type is a function type
(`karn.resolve.param_as_function` otherwise) — both judgments are made by the
checker, with the type information they require; a *type name* is never
callable (`karn.resolve.type_as_function`). Call resolution prefers declared
functions, then variant constructors, then agents, then in-scope values —
scope-first call resolution would change the meaning of existing programs, so
the pre-existing ident/call precedence asymmetry is preserved deliberately.

A `commons` is imported with `uses`, which MUST name an existing `commons`, not a
context, and MUST NOT be self-referential or introduce a colliding name
(`karn.uses.unknown_commons`, `karn.uses.target_is_context`,
`karn.uses.self_reference`, `karn.uses.name_conflict`). The visibility of types
across context boundaries is governed by `exports` and `consumes`
([§5.8](#58-boundaries--cross-context)).

## §5.2 Well-typedness

Every expression MUST have the type its position requires. A function or method
argument MUST match the parameter type (`karn.types.argument_mismatch`), and a
call MUST supply the right number of arguments (`karn.resolve.arity_mismatch`,
`karn.types.method_arity`). A returned value MUST match the declared return type
(`karn.types.return_mismatch`); a `let` value MUST match any annotation
(`karn.types.let_annotation_mismatch`); a record field MUST be given a value of
its type (`karn.types.field_value_mismatch`), and every required field MUST be
supplied (`karn.resolve.missing_field`).

An `if` condition MUST be a `Bool` and both branches MUST share a type
(`karn.types.if_non_bool_cond`, `karn.types.if_branch_mismatch`). The payloads of
`Ok`, `Err`, `Some`, and the like MUST match the expected component type (the
`karn.types.*_value_mismatch` codes). Where a constructor is ambiguous between
`Result` and `HttpResult`, it MUST be qualified (`karn.types.ambiguous_constructor`).

**Lambdas** (v0.20a). Against an expected function type, a lambda's
parameters take the expected types (an annotation MUST agree), its body is
checked against the expected return — a pure body auto-lifts into an
effectful expectation — and arity MUST match (`karn.types.lambda_mismatch`).
In a position with no expected function type, every parameter MUST be
annotated (`karn.lambda.unannotated_param`) and the lambda's type is read off
its body: a body that performs an effect operation (an `<-` bind, a
capability call, a call returning `Effect`) makes the lambda **effectful**,
wrapping its result in `Effect` — effectfulness is judged by the *presence*
of effect operations, never by a pre-declared result type, which is what
dissolves the apparent circularity. A nested lambda's effects are its own. A
`commit` MUST NOT appear inside a lambda (the existing
`karn.commit.outside_agent`).

**Value application** (v0.20a). Applying a function-typed value checks
arguments against the function type's parameters
(`karn.types.argument_mismatch`, `karn.types.call_arity`).

**Generic instantiation** (v0.20a). A generic function's type arguments are
inferred from its arguments by argument-directed unification: non-lambda
arguments first, left to right; lambda arguments after, against the
substituted expectations — a lambda whose expected *parameter* types remain
undetermined is rejected unless fully annotated, and an expected *return*
variable is captured from the lambda's actual type. Conflicting inferences
MUST agree exactly (`karn.generics.type_arg_mismatch`); a type parameter
neither inferable nor given explicitly (`name[T](…)`) is rejected
(`karn.generics.uninferable_type_arg`), as is a bare generic function passed
as a value. There is no inference between lambdas and none from the call's
own expected type. Generic *type* declarations and parameter *bounds* are
rejected (`karn.generics.no_generic_types`, `karn.generics.no_bounds`); a
type parameter MUST NOT shadow a declared type. Within a generic function's
body its type parameters are rigid: equal only to themselves. The checker
maintains the invariant that a type-variable-bearing expected type imposes
no constraint on expression checking.

{{#grammar-semantics if_expr}}

## §5.3 Refinement & admission

A refinement's predicates MUST apply to the type's base — a string predicate on
an `Int` is rejected (`karn.types.predicate_base_mismatch`) — and MUST be
internally consistent: an `InRange` MUST NOT be inverted
(`karn.types.inverted_range`), a length MUST NOT be negative
(`karn.types.negative_length`), a `Matches` regex MUST be valid
(`karn.types.invalid_regex`), and the predicates together MUST admit at least one
value (`karn.types.empty_refinement`).

A **literal** written where a refined type is expected is admitted at compile
time ([§6.4](type-system.md#64-admission--construction)) in these positions:
return (block tail), a `let` with a type annotation, an `Ok`/`Some`/`Err`
payload, and a refined-typed call argument. The literal MUST satisfy the
predicate, or it is rejected (`karn.refine.literal_violates`); an admitted
literal MUST be a compile-time literal, not an expression or identifier. **Opaque
types are excluded** from admission and MUST be constructed through `.of`,
`.unsafe`, or `.raw`, never record syntax (`karn.resolve.opaque_record_construction`,
`karn.types.opaque_record_construction`); `.raw` MUST be used only within the
defining `commons` (`karn.types.opaque_raw_outside`) and `.unsafe` only within
the defining context (`karn.types.opaque_unsafe_outside`).

{{#grammar-semantics refined_type}}

## §5.4 Agents & state

An `agent` MUST be declared inside a context (`karn.agent.outside_context`) and
MUST NOT declare `on http`, `on cron`, or `on queue` handlers (the
`karn.parse.*_in_agent` codes). Each agent handler's return type MUST be an
`Effect` (`karn.agent.return_not_effect`).

Every `state` field MUST have a defined initial value: either an **explicit
initialiser** — a compile-time constant of the field's type, not referencing
`self`, parameters, or capabilities (`karn.agents.bad_state_initialiser`) — or an
**implicit zero** (`Int` → `0`, `Bool` → `false`, `String` → `""`, `Option[T]` →
`None`, a record of zeroable fields). A field with neither is rejected
(`karn.agents.non_zeroable_state_field`).

A `commit` MUST occur only in an agent handler (`karn.commit.outside_agent`), its
value MUST match the agent's state type (`karn.commit.wrong_state_type`), and at
most one `commit` may be reachable on any execution path
(`karn.commit.two_reachable_commits`). Constructing or calling an agent MUST use
the right key arity and type and a declared handler (`karn.agent.construction_arity`,
`karn.agent.key_mismatch`, `karn.agent.handler_arity`, `karn.agent.handler_not_found`).

{{#grammar-semantics state_decl}}

## §5.5 Effects, capabilities & providers

Karn separates **pure** from **effectful** code. An `<-` bind MUST occur in an
effectful position and MUST be applied to an `Effect`
(`karn.effect.bind_in_pure_context`, `karn.effect.bind_on_non_effect`); a
capability call or a cross-context call MUST NOT occur in a pure context
(`karn.effect.capability_in_pure_context`, `karn.effect.cross_context_in_pure_context`).

A capability MUST be declared inside a context or an adapter
(`karn.capability.outside_context`); a bodied provider MUST implement exactly its
capability's operations — no missing, no extra, signatures matching
(`karn.provider.missing_operation`, `karn.provider.extra_operation`,
`karn.provider.signature_mismatch`) — and every provider MUST name an existing
capability (`karn.provider.unknown_capability`). A handler or provider MUST
declare every capability it uses with `given`, and `given` MUST name a real
capability; a call to an undeclared capability is rejected and an unused one
warned (`karn.given.unknown_capability`, `karn.given.undeclared_capability`,
`karn.given.unused_capability`). Providers MUST NOT form a dependency cycle
through `given` (`karn.provider.dependency_cycle`).

Calling an **effectful function value** — one whose type's return is
`Effect[_]` — is an effect operation: it MUST occur in an effectful context
(`karn.effect.fn_value_in_pure_context`), exactly as a capability call must.
`Effect[T]` remains non-storable in pure contexts; this feature opens no back
door (the eager-`Promise` translation makes an un-bound effectful call
observable, so the confinement is load-bearing).

**Provider placement follows the unit kind.** A provider in a *context* MUST
have a Karn body (`karn.context.external_provider`); a provider in an *adapter*
MUST be external — bodiless, its implementation supplied by the binding
(`karn.adapter.provider_has_body`); a provider anywhere else is rejected
(`karn.provider.outside_context`). An **external** provider's `given` resolves
exactly as a bodied provider's does — each bare name MUST be a local capability
or one flattened from a `consumes` selection
([§5.8](#58-boundaries--cross-context)) — and external providers participate in
the same dependency-cycle check.

{{#grammar-semantics given_clause}}

## §5.6 Pattern matching

A `match` MUST be **exhaustive** — every variant of the scrutinised sum,
`Result`, or `Option` covered (`karn.types.non_exhaustive_match`) — and its
scrutinee MUST be a sum type (`karn.types.match_non_sum_discriminant`). Its arms
MUST share a result type (`karn.types.match_arm_mismatch`), MUST NOT repeat a
variant (`karn.types.duplicate_variant_arm`), and MUST NOT be unreachable
(`karn.types.unreachable_arm`).

A pattern MUST name a real variant (`karn.types.unknown_variant_in_pattern`) and
real payload fields (`karn.types.unknown_pattern_field`), bind the right number
of fields (`karn.types.pattern_arity`), and MUST NOT mix named and positional
bindings (`karn.types.mixed_pattern_bindings`). An `is` check MUST be applied to a
value of the matching base or sum (`karn.types.is_base_mismatch`,
`karn.types.is_non_sum`, `karn.types.is_unknown_variant`).

{{#grammar-semantics match_expr}}

## §5.7 Handlers

A `service` MUST be declared inside a context (`karn.service.outside_context`) and
every service handler MUST return an `Effect` (`karn.service.return_not_effect`).

An `on http` handler MUST return `Effect[HttpResult[T]]`
(`karn.http.return_not_effect_http_result`); its route MUST be well-formed and
unique, MUST NOT use the reserved `/_karn/` prefix
(`karn.http.invalid_path`, `karn.http.duplicate_route`, `karn.http.reserved_prefix`),
and each `:name` segment MUST bind to a string-constructible parameter
(`karn.http.unbound_path_param`, `karn.http.path_param_not_stringy`,
`karn.http.extra_param`); `GET` and `DELETE` MUST NOT take a `body`
(`karn.http.body_on_get_or_delete`). An `on cron` handler MUST take at most one
`Int` parameter, a valid five-field schedule, and return `Effect[Result[(), E]]`
(the `karn.cron.*` codes); an `on queue` handler MUST take exactly one `message`
parameter, a non-empty queue name, and the same return shape (the `karn.queue.*`
codes).

{{#grammar-semantics http_handler}}

## §5.8 Boundaries & cross-context

`consumes` MUST appear only in a context or an adapter (`karn.consumes.in_commons`),
MUST name an existing context or adapter — not a `commons`
(`karn.consumes.unknown_context`, `karn.consumes.target_is_commons`) — and not the
consumer itself (`karn.consumes.self_reference`), and MUST NOT produce colliding
names or aliases (the `karn.consumes.*` codes). Calling another context's service
requires a `consumes` declaration (`karn.resolve.unconsumed_context`), and units
MUST NOT form a `consumes` cycle (`karn.context.consumes_cycle`).

A **capability selection** (`consumes b { Cap, … }`) flattens each named
capability into the consumer's local namespace under its bare name, so it reads
as `given Cap` / `Cap.op(…)`. Each selected name MUST be a capability the target
**exports** (`karn.given.cross_context_unknown_capability`), and a flattened bare
name MUST NOT collide with a locally declared capability or with a name
flattened from another unit (`karn.consumes.capability_name_clash`) — a clash is
resolved by the qualified `given b.Cap` form or an alias.

An **adapter's** `consumes` is further restricted: it MUST use the
capability-selection form — an adapter has no services to call, so the
whole-unit and aliased forms are rejected
(`karn.adapter.consumes_requires_selection`) — and it MUST target an adapter,
never a context (`karn.adapter.consumes_context`).

`exports` MUST name declared types or capabilities, MUST NOT export a name twice
or with conflicting visibility, and an exported capability MUST have a provider
(the `karn.exports.*` codes). A value crossing a boundary MUST be structurally
compatible with the receiving side ([§6.5](type-system.md#65-type-compatibility--boundaries),
`karn.boundary.structural_mismatch`); a context-owned type MUST NOT be constructed
or an opaque export inspected from outside (`karn.context.external_construction`,
`karn.context.opaque_inspection`).

**Adapters are the host boundary.** An adapter MUST NOT declare a `service` or an
`agent` (`karn.adapter.disallowed_item`); it MAY declare at most one `binding`
clause (`karn.adapter.duplicate_binding`), and MUST declare one if it declares
any external provider (`karn.adapter.no_binding`). A binding's `requires` ranges
MUST be pinned: a range MUST name at least one version digit — `*`, `x`,
`latest`, and digit-free ranges are rejected
(`karn.requires.unpinned_dependency`). The `karn` namespace is **reserved for the
toolchain**: no user unit's name may have `karn` as its first segment
(`karn.namespace.reserved`); the toolchain's own first-party adapters — the
ambient `karn` surface and the `karn.<platform>` platform adapters
([§7.3.6](emission.md#736-adapters)) — live inside that reserved prefix and are
injected when a unit consumes them.

**The platform lock** (v0.19). A capability of a **platform adapter**
(`karn.cloudflare`) is *platform-native*: its binding runs only on that
platform. A **deployment unit** — each context under `--target workers`; the
whole program under `bundle`, where co-location shares the lock — is locked to
the union of native platforms its **in-process closure** reaches: the providers
its composition would instantiate, walked through `given` and flattening edges.
A service `consumes` edge between contexts is RPC under `workers` and does
**not** propagate the lock. The selected `--platform` MUST be the locked
platform (`karn.target.vendor_required`), and one deployment unit MUST NOT span
two mutually-exclusive native platforms (`karn.target.vendor_conflict`). The
`karn` surface and library adapters impose no lock.

An integration test MUST wire at least two distinct, declared contexts, MUST NOT
duplicate a participant or suite name, and MUST wire every consumed dependency
(the `karn.integration.*` codes).

{{#grammar-semantics consumes_decl}}

{{#grammar-semantics adapter_decl}}

{{#grammar-semantics binding_decl}}

## §5.9 Testing constructs

An `assert` MUST occur only in a test case body and MUST be given a `Bool`
(`karn.assert.outside_test`, `karn.assert.non_bool`). A `test` block MUST target
an existing unit and MUST NOT duplicate a case description
(`karn.test.unknown_target`, `karn.test.duplicate_case_name`).

`Mock[T]` MUST occur only in a test body (`karn.mock.outside_test`), name a
resolvable type (`karn.mock.unknown_type`), and receive pins that are
compile-time literals of the right arity satisfying the type
(`karn.mock.pin_not_literal`, `karn.mock.arity`, `karn.mock.literal_violates`); a
type that cannot be fabricated MUST be pinned (`karn.mock.needs_pin`,
`karn.mock.unsupported_kind`). A `mocks` block MUST name an in-scope capability,
match its signature, and MUST NOT be used in an integration test or a commons
test (`karn.mock.unknown_target`, `karn.mock.signature_mismatch`,
`karn.integration.mock_in_integration`, `karn.mock.in_commons_test`).

{{#grammar-semantics mock_expr}}

## §5.10 Collections

*(v0.20b)* `List[T]` and `Map[K, V]` are built-in generic types
([§6.2](type-system.md#62-built-in-generic-types)); this section is their
static semantics.

**Construction.** A list literal `[a, b, c]`
([§4 list_literal](../reference/grammar.md#rule-list_literal)) types each
element against the **expected element type** when one is supplied — so
refined literals admit ([§5.3](#53-refinement--admission)) — and a mismatched
element is `karn.types.list_element_mismatch`. With no expected type, the
first element fixes the element type. An **empty `[]` MUST have an expected
type** (`karn.types.uninferable_element_type`); the qualified statics
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
| `Map[K, V]` | `length()` | `Int` |
| `Map[K, V]` | `keys()` | `List[K]` |
| `Map[K, V]` | `get(k: K)` | `Option[V]` |
| `Map[K, V]` | `insert(k: K, v: V)` | `Map[K, V]` |

A method outside the kernel is `karn.types.method_not_found`; a wrong arity
is `karn.types.method_arity`. **`foldEff` is an effect operation**: it runs
its effectful step function sequentially, and calling it in a pure context
is `karn.effect.fn_value_in_pure_context`, exactly the function-value
confinement of [§5.5](#55-effects-capabilities--providers).

**Keys.** A `Map` key type MUST be value-keyable — `String`, `Int`, or a
refined/opaque type over them; anything else is
`karn.types.unkeyable_map_key`, checked at every written `Map[K, V]`
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
`karn.types.function_at_boundary`. The wire forms are
[§7.3.7](emission.md#737-collections).

**The combinator stdlib.** Everything derivable from the kernel is ordinary
Karn in the first-party `karn.list` / `karn.map` commons
([§8.4](compilation-model.md#84-build-pipeline--conformance-to-typescript)),
imported with `uses karn.list` like any commons: `map`, `filter`, `find`,
`any`, `all`, `reverse`, `traverse` (sequential); `values`, `contains`,
`getOr`. There is no `Map.fromList` — Karn has no pair type to spell its
argument with; maps build via `Map.empty()` + `insert` folds.

```karn
context jobs

uses karn.list

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
