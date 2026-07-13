---
title: "§4 Syntactic grammar"
---
This chapter defines Bynk's phrase structure: how tokens ([§3](/book/spec/lexical-grammar/))
combine into declarations, types, expressions, patterns, and statements. Each
production is generated from the grammar ([§2.1](/book/spec/conventions/)) and embedded by
name.

A production states what **parses**. Every constraint beyond parsing — name
resolution, typing, exhaustiveness, refinement admission, the effect discipline,
and all other well-formedness — is a static-semantics rule, specified normatively
in **§5** and not repeated here. Where a construct carries such constraints, this
chapter forward-references §5 rather than restating them.

The chapters mirror the construct groupings of the friendly
[grammar reference](/book/reference/grammar/); the productions are shared, the
register here is the normative definition.

## §4.1 Top-level & modules

A source file is a `commons`, a `context`, an `adapter`, or test declarations.

### §4.1.1 source_file

{{#grammar source_file}}

A whole file: one or more top-level declarations, or a single fragment used by
editor tooling.

### §4.1.2 item_fragment

{{#grammar _item_fragment}}

A tooling entry point: a single body item parsed in isolation. Not written by
hand.

### §4.1.3 expr_fragment

{{#grammar _expr_fragment}}

A tooling entry point: statements and/or an expression parsed in isolation. Not
written by hand.

### §4.1.4 commons_decl

{{#grammar commons_decl}}

A `commons` module. The body braces are optional at file scope; with no braces
the body items run to the end of the file.

### §4.1.5 context_decl

{{#grammar context_decl}}

A `context`. As with `commons`, the body braces are optional at file scope.
Well-formedness: §5.

### §4.1.6 adapter_decl

{{#grammar adapter_decl}}

An `adapter` — the host boundary: a capability contract co-located with a named
TypeScript binding. As with `commons`, the body braces are optional at file
scope. An adapter's providers are **external** (bodiless,
[§4.3.8](#438-provider_decl)) and it may not declare services or agents; those
placement rules, the binding requirement, and the reserved `bynk` namespace are
well-formedness: §5.

### §4.1.7 suite_decl {#416-test_decl}

{{#grammar suite_decl}}

A `suite` block naming the `commons` or `context` it targets. A suite header may
carry an optional `as <tier>` clause (§4.9) that sets the default tier its cases
inherit. Well-formedness: §5.

### §4.1.8 commons_body_item

{{#grammar _commons_body_item}}

The declaration forms admitted in a `commons` body.

### §4.1.9 context_body_item

{{#grammar _context_body_item}}

The declaration forms admitted in a `context` body, including `consumes` and
`exports`.

### §4.1.10 adapter_body_item

{{#grammar _adapter_body_item}}

The declaration forms admitted in an `adapter` body: the `binding` clause,
capability and type declarations, pure helpers and `uses`, `consumes`,
`exports`, and providers. The grammar is deliberately permissive — `service` and
`agent` parse here so the placement error can be precise; their rejection is
well-formedness: §5.

### §4.1.11 test_body_item

{{#grammar _test_body_item}}

The declaration forms admitted in a `suite` body: `uses`, `stub` clauses, and
`case` / `property` declarations.

### §4.1.15 qualified_name

{{#grammar qualified_name}}

A dotted sequence of identifiers, e.g. `shop.orders`. A dotted name is a single
**flat** identifier, not a hierarchy: `bynk` and `bynk.time` are independent
names that merely share a leading segment.

### §4.1.16 uses_decl

{{#grammar uses_decl}}

`uses` followed by a qualified name. Well-formedness: §5.

### §4.1.17 consumes_decl

{{#grammar consumes_decl}}

`consumes` a unit, in one of three forms: the **whole unit** (`consumes b`), the
whole unit under an **alias** (`consumes b as Alias`), or a **capability
selection** (`consumes b { Cap, … }`), which flattens the named capabilities into
the consumer's local capability namespace under their bare names. The target may
be a context or an adapter; which forms each consumer kind admits, and the
flattening and clash rules, are well-formedness: §5.

### §4.1.18 exports_decl

{{#grammar exports_decl}}

`exports`, one of `opaque` / `transparent` / `capability`, and a brace-delimited
identifier list. Well-formedness: §5.

### §4.1.19 binding_decl

{{#grammar binding_decl}}

An adapter's `binding` clause: the TypeScript module supplying its external
provider classes, as a string-literal path resolved relative to the adapter's
source file, with an optional `requires { … }` map of npm dependencies.
Well-formedness: §5.

### §4.1.20 binding_requirement

{{#grammar binding_requirement}}

One `"package": "range"` entry in a binding's `requires` map. Ranges MUST be
pinned; well-formedness: §5.

## §4.2 Types & refinements

Type declarations and the type references that appear in signatures.

### §4.2.1 type_decl

{{#grammar type_decl}}

`type`, a name, an optional `[A, B]` **type-parameter list** (v0.157 — only a
record body may be generic; a parameter is an unconstrained, bound-free name
scoped to the declaration), `=`, and a type body. Well-formedness: §5; the type
system: §6.

### §4.2.2 type_body

{{#grammar _type_body}}

The right-hand side of a `type`: one of the five type forms.

### §4.2.3 opaque_type

{{#grammar opaque_type}}

`opaque`, a base type, and an optional `where` refinement.

### §4.2.4 refined_type

{{#grammar refined_type}}

A base type with an optional `where` refinement. Well-formedness: §5;
admission: §6.

### §4.2.5 record_type

{{#grammar record_type}}

A brace-delimited, comma-separated list of record fields, with an optional
trailing comma.

### §4.2.6 record_field

{{#grammar record_field}}

A field name, `:`, a type, an optional inline `where` refinement, and an optional
`=` default expression. Well-formedness: §5.

### §4.2.7 sum_type

{{#grammar sum_type}}

One or more `|`-prefixed variants, followed by an optional trailing
`embeds <type> as <Variant>, …` clause (v0.154) declaring **error embeddings**
— mappings a value of `<type>` auto-wraps into the named single-payload
variant, which the `?` operator uses for automatic error conversion
([§5.2](/book/spec/static-semantics/)). `embeds` and `as` are contextual
keywords here. Well-formedness: §5.

### §4.2.8 sum_variant

{{#grammar sum_variant}}

A `|`, a constant name, and an optional parenthesised payload.

### §4.2.9 variant_payload_field

{{#grammar variant_payload_field}}

A named field in a sum-variant payload: an identifier, `:`, and a type.

### §4.2.10 enum_type

{{#grammar enum_type}}

`enum` and a brace-delimited list of constant names — a sum type whose variants
all carry no payload.

### §4.2.11 refinement

{{#grammar refinement}}

One or more predicates joined by `and`. Well-formedness: §5.

### §4.2.12 refinement_pred

{{#grammar _refinement_pred}}

A single predicate: a predicate call or a bare predicate name.

### §4.2.13 pred_call

{{#grammar pred_call}}

A predicate name applied to parenthesised arguments, e.g. `InRange(1, 100)`.

### §4.2.14 predicate_name

{{#grammar predicate_name}}

The set of built-in refinement predicates. Well-formedness: §5.

### §4.2.15 pred_arg

{{#grammar _pred_arg}}

An argument to a predicate: a number or string literal.

### §4.2.16 base_type

{{#grammar base_type}}

The primitive types `Int`, `String`, and `Bool`. Well-formedness: §5.

### §4.2.17 type_ref

{{#grammar _type_ref}}

A type as it appears in a signature: a function type, a base type, the unit
type, the validation-error type, a generic application, or a named type.

### §4.2.17a function_type_ref

{{#grammar function_type_ref}}

A function type (v0.20a): `Int -> Int`, `(Int, String) -> Bool`, `() -> Int`.
The arrow is **right-associative** — `A -> B -> C` is `A -> (B -> C)` — and a
parenthesised list before `->` is a parameter list (a single parenthesised
type without an arrow is a grouping; the empty `()` without an arrow stays
the unit type). A function type is **effectful** exactly when its return type
is `Effect[_]` — the structural rule of §6. Function types are confined to
non-boundary positions; well-formedness: §5.

### §4.2.18 unit_type

{{#grammar unit_type}}

The unit type `()`.

### §4.2.19 validation_error_type

{{#grammar validation_error_type}}

`ValidationError`, the error type produced when refined-type validation fails.

### §4.2.20 generic_type_ref

{{#grammar generic_type_ref}}

A generic constructor — `Result`, `Option`, `Effect`, `HttpResult`, or
(v0.20b) `List`, `Map` — applied to bracketed type arguments.
Well-formedness: §5 (`Map` keys are value-keyable,
[§5.10](/book/spec/static-semantics/#510-collections)); the type system: §6.

### §4.2.20a applied_type_ref

{{#grammar applied_type_ref}}

An application of a **user-declared generic type** (v0.157) to bracketed type
arguments — `Paginated[User]`, `Keyed[String, Int]`. The head is a user type
name declared `type Name[T, …] = { … }` (a record). A generic type must be
applied to exactly its declared number of arguments, and a generic record is a
**non-boundary** value. Well-formedness: §5; the type system: §6.

## §4.3 Functions, capabilities & providers

Pure functions and methods, capability interfaces, and the providers that
implement them.

### §4.3.1 fn_decl

{{#grammar fn_decl}}

`fn`, a function name or a `Type.method` name, an optional `[A, B]`
**type-parameter list** (v0.20a — free functions only; a type parameter is an
unconstrained, bound-free name scoped to the signature and body), a parameter
list, `->`, a return type, any number of **contract clauses** (v0.115), and a
block body. Well-formedness: §5.

### §4.3.1a requires_clause / ensures_clause (v0.115)

{{#grammar requires_clause}}

{{#grammar ensures_clause}}

A function **contract** (testing track slice 3): named `requires` (preconditions)
and `ensures` (postconditions) between the return type and the body, each a pure
`Bool` predicate expression — the one predicate surface (§4.6, ADR 0144). A
`requires` predicate is over the parameters; an `ensures` predicate is over the
parameters and `result`, the contextual binding for the return value (an ordinary
identifier elsewhere). Well-formedness — purity, `Bool` type, `result` scope,
distinct names: §5 (ADR 0150).

### §4.3.2 method_name

{{#grammar method_name}}

A `Type.method` name, defining a method on a named type.

### §4.3.3 params

{{#grammar _params}}

A parameter list: an optional `self` receiver followed by named parameters, with
an optional trailing comma.

### §4.3.4 self_param

{{#grammar self_param}}

The `self` receiver of a method or handler.

### §4.3.5 param

{{#grammar param}}

One parameter: an identifier, `:`, and a type. Well-formedness: §5.

### §4.3.6 capability_decl

{{#grammar capability_decl}}

`capability`, a name, and a brace-delimited list of operation signatures.
Well-formedness: §5.

### §4.3.7 capability_op

{{#grammar capability_op}}

One operation in a capability: `fn`, a name, parameters, `->`, and a return type
— no body. Well-formedness: §5.

### §4.3.8 provider_decl

{{#grammar provider_decl}}

`provides`, the capability name, `=`, an implementation name, an optional `given`
clause, and an **optional** brace-delimited list of operation implementations.
The presence of the brace block distinguishes the two provider kinds: with a
block the provider is implemented **in Bynk** (context-only); with no block it is
**external** — its implementation is the named class exported by the enclosing
adapter's binding module ([§4.1.19](#4119-binding_decl)). The absence of the
block, not an empty one, is the signal. Placement and wiring rules:
well-formedness, §5.

### §4.3.9 provider_op

{{#grammar provider_op}}

One operation implementation: a capability operation signature with a block body.
Well-formedness: §5.

### §4.3.10 given_clause

{{#grammar given_clause}}

`given` and a comma-separated list of the capabilities a handler or provider may
use. Well-formedness: §5.

## §4.4 Services & handlers

A `service` groups the handlers that respond to calls and external triggers.

### §4.4.1 service_decl

{{#grammar service_decl}}

`service`, a name, an optional `from <protocol>` header clause, and a
brace-delimited list of handlers. One protocol per service. Well-formedness: §5.

### §4.4.2 service_protocol

{{#grammar service_protocol}}

The `from <protocol>` clause: `from http`, `from cron`, `from queue("name")`
(v0.44), or `from WebSocket(in: I, out: O)` (v0.103). Absent ⇒ the
contract-mediated default, which admits only `on call`. Well-formedness: §5.

### §4.4.2a handler

{{#grammar handler}}

A handler: a call, HTTP, cron, or queue entry point, matching the service's
protocol. Well-formedness: §5.

### §4.4.3 call_handler

{{#grammar call_handler}}

`on call`, an optional name, parameters, `->`, a return type, an optional `given`
clause, and a block body.

### §4.4.4 http_handler

{{#grammar http_handler}}

`on <Method>("route")` — an HTTP method-builder (the verb collapses verb+route
into one config expression in the handler-config slot), then parameters, `->`, a
return type, an optional `given` clause, and a block body. Valid only in a
`from http` service. Well-formedness: §5.

### §4.4.5 http_method

{{#grammar http_method}}

The HTTP verbs a route may handle. Well-formedness: §5.

### §4.4.6 cron_handler

{{#grammar cron_handler}}

`on schedule("expr")`, parameters, `->`, a return type, an optional `given`
clause, and a block body. Valid only in a `from cron` service. Well-formedness: §5.

### §4.4.7 queue_handler

{{#grammar queue_handler}}

`on message(message)` — the bound queue lives on the service's `from
queue("name")` header. Parameters, `->` `Effect[QueueResult]`, an optional
`given` clause, and a block body. Well-formedness: §5.

### §4.4.7a WebSocket handlers (v0.103)

A `from WebSocket(in: I, out: O)` service declares the connection-lifecycle
handlers `on open`, `on message`, and `on close`. Each is a handler head — `on`,
the lifecycle keyword, parameters, `->` `Effect[()]`, a required
[`by_clause`](#448-by_clause) naming the actor, an optional `given` clause, and a
block body — and is valid only in a `from WebSocket` service:

- **`on open`** — the upgrade handshake; the body sees an owned `connection`
  binding of type `Connection[O]`.
- **`on message`** — parameters end with the decoded inbound frame of type `I`.
- **`on close`** — the connection ended.

Well-formedness — exactly one `on open`, edge authentication, held-resource
disposal: §5. *(The rendered grammar productions for these handler heads land with
the tree-sitter grammar; see [Reference — grammar](/book/reference/grammar/).)*

### §4.4.8 by_clause (v0.45) {#448-by_clause}

{{#grammar by_clause}}

`by (<binder>:)? <Actor> ("|" <Actor>)*` — the actor(s) a handler consumes,
positioned after the return type, alongside `given` (v0.155 relocated it from
before the parameter list, where `by <Actor> (…)` read as a call)
(`on schedule("…") () -> … by s: Scheduler`). The **binder is optional** (v0.50):
`by <name>: <Actor>` captures the verified identity (read as `<name>.identity`);
`by <Actor>` declares-and-verifies the contract without capturing it (anonymous
or verify-and-discard). Omitting `by` entirely inherits the protocol's default
actor; on a `from http` handler `by` is required (the binder still optional).

A `|`-separated list of actors (v0.52) is an **ordered sum of peer actors**
(`by who: User | Visitor`): the boundary tries each peer's scheme in declared
order and binds the first that verifies; the body `match`es on the resolved
actor (the binder is **required** for a sum). Well-formedness: §5.

An `actor` is a nominal *boundary contract* — a closed, compiler-known
authentication scheme plus an optional sealed identity — consumed by a handler's
`by` clause (§4.4.8). Actors are context-only.

### §4.4.9 actor_decl (v0.45)

{{#grammar actor_decl}}

`actor <Name> { auth = <Scheme> }`, optionally `, identity = <Type>`. The
refinement form `actor <Name> = <Base> where <predicate>` (v0.53) declares an
authorisation invariant over a `Bearer` base; the predicate is a closed set of
claim predicates (`hasClaim`, `claimEquals`). Well-formedness: §5.

### §4.4.10 scheme

{{#grammar scheme}}

The closed authentication-scheme set. `None`, `Internal`, `Bearer`,
`Signature` (v0.51), and `Oidc` (v0.151) are supported. The authenticated schemes
carry a keyed-args config — `Bearer(secret = "<ENV>")`, `Signature(secret =
"<ENV>", header = "<Header>", (timestamp = "<Header>", tolerance = <seconds>)?)`,
and `Oidc(issuer = "<url>", audience = "<aud>", jwks = "<url>")` — parsed by the
`scheme_config` production (string- or integer-valued args; the checker validates
which keys each scheme admits). Unlike `Bearer`/`Signature`, an `Oidc` config
names **no secret**: its trust root is the provider's public JWKS. Well-formedness: §5.

## §4.5 Agents

An `agent` is a keyed, stateful entity whose state lives in `store` fields that
handlers read by name and write with `:=`.

### §4.5.1 agent_decl

{{#grammar agent_decl}}

`agent`, a name, and a body holding a key declaration, `store` fields, zero or
more invariants and transitions, and handlers — in that fixed order.
Well-formedness: §5.

### §4.5.2 key_decl

{{#grammar key_decl}}

`key`, an identifier, `:`, and a type — the agent's identity.

### §4.5.2a store_field

{{#grammar store_field}}

`store`, a name, `:`, a [`store_kind`](#452b-store_kind) over its type parameters,
zero or more [`store_annotation`](#452c-store_annotation)s, and an optional `=`
initialiser — a persistent field of the agent. Well-formedness — required initial
value, kind validity: §5 (ADR 0108).

### §4.5.2b store_kind

{{#grammar store_kind}}

The closed catalogue of storage kinds: `Cell`, `Map`, `Set`, `Cache`, `Log`. The
catalogue is **closed** — there is no `Queue` storage kind (ADR 0122).

### §4.5.2c store_annotation

{{#grammar store_annotation}}

A `@name(args)` annotation between the kind and the initialiser — `@ttl` (on
`Cache`), `@retain` (on `Log`), `@indexed` (on `Map`), `@bounded`. Arguments are
compile-time literals. Well-formedness — kind match, known name: §5 (ADR 0111).

### §4.5.3 invariant_decl (v0.80)

{{#grammar invariant_decl}}

`invariant`, a name, `:`, and a predicate expression — a universally-quantified
property that must hold of every committed state. Invariants form a phase between
the `store` fields and the handlers; one after a handler is a parse error
(`bynk.parse.invariant_after_handler`). The predicate references the agent's
`store` fields by bare name. Well-formedness — purity, `Bool` type,
agent-locality: §5 (ADR 0107).

### §4.5.4 transition_decl (v0.116)

{{#grammar transition_decl}}

`transition`, a name, `:`, and a predicate expression — a step invariant over the
agent's `old`/`new` state pair. Transitions sit in the same phase as invariants
(between the `store` fields and the handlers); one after a handler is a parse error
(`bynk.parse.transition_after_handler`). The predicate reads state fields through
the contextual `old` and `new` bindings (`old.status`, `new.balance`), which are
ordinary identifiers outside a `transition`. Well-formedness — purity, `Bool` type,
agent-locality, a genuine step reference: §5 (ADR 0151).

## §4.6 Expressions

Bynk is expression-oriented: a block's value is its final expression. Operator
precedence is fixed by the `binary_expr` production ([§4.6.7](#467-binary_expr)).

### §4.6.1 expression

{{#grammar _expression}}

Any expression: control flow, a refinement check, an operator expression, or a
primary.

### §4.6.2 primary

{{#grammar _primary}}

The atomic and postfix expressions: literals, names, calls, field and method
access, constructors, and parenthesised expressions.

### §4.6.3 if_expr

{{#grammar if_expr}}

`if`, a condition, a block, and an **optional** `else` arm (either a further
`if` or a block). A missing `else` defaults to `()` (v0.146, ADR 0170) and is
legal only when the then-branch is unit (`()` / `Effect[()]`); a value-producing
`if` still requires its `else`. Well-formedness: §5.

### §4.6.4 match_expr

{{#grammar match_expr}}

`match`, a scrutinee, and a brace-delimited list of match arms. Well-formedness —
including exhaustiveness: §5.

### §4.6.5 is_expr

{{#grammar is_expr}}

An expression, `is`, and a pattern. Well-formedness — including the narrowing it
introduces: §5.

### §4.6.6 binary_expr {#467-binary_expr}

{{#grammar binary_expr}}

The binary operators, listed from lowest precedence (`implies`, then `||`) to
highest (`*`, `/`); the production order is the precedence order. `implies`
(v0.80) is logical implication, right-associative, `P implies Q` ≡ `!P || Q`.
Well-formedness: §5.

### §4.6.7 unary_expr

{{#grammar unary_expr}}

Logical negation `!` and numeric negation `-`, prefixed to an expression.

### §4.6.8 method_call

{{#grammar method_call}}

A receiver, `.`, a method name, and parenthesised arguments. Well-formedness: §5.

v0.22a: the numeric base-type keywords `Int` and `Float` are admitted in
**static-receiver position** — `Int.parse(s)` / `Float.parse(s)` — but only
when immediately followed by `.`; a bare `Int` in expression position remains
a parse error. (`List.empty()` needs no such rule: `List` is lexically an
ordinary identifier.)

v0.22b: a method call accepts **explicit type arguments** —
`Json.decode[Order](s)` — under the same same-line-`[` rule as `call` type
application (0039): a `[` opening a new line is a list literal. In v0.22b
only the `Json.decode` static consumes them; type arguments on any other
method are `bynk.generics.type_arg_mismatch` (generic *user* methods remain
deferred). The bare `name[T]` value form stays reserved.

### §4.6.9 field_access

{{#grammar field_access}}

A receiver, `.`, and a field name. Well-formedness: §5.

### §4.6.9a lambda_expr

{{#grammar lambda_expr}}

A lambda (v0.20a): `(o) => o.paid`, `(acc, t) => acc + t`, `() => 0`, or with
a block body `(o) => { … }`. Always parenthesised; `=>` is the **value**
arrow, shared with `match` arms — `->` stays the type arrow. Well-formedness
(contextual parameter typing, the unannotated rule, bottom-up effectfulness):
§5.

### §4.6.9b lambda_param

{{#grammar lambda_param}}

One lambda parameter with an optional type annotation — optional because an
expected function type supplies it; required in unconstrained positions (§5).

### §4.6.10 call

{{#grammar call}}

A name, optional bracketed **type arguments** (v0.20a, `name[T](…)` — the
explicit-instantiation form; a bare `name[T]` without an argument list is a
reserved parse error), and parenthesised arguments — a function call, a
variant construction, an agent instantiation, or (v0.20a) the **application
of a function-typed value** in scope. Well-formedness: §5.

### §4.6.11 record_construction

{{#grammar record_construction}}

A type name and a brace-delimited list of field initialisers. Well-formedness: §5.

### §4.6.12 field_init

{{#grammar field_init}}

One field of a record construction: `name: value`, or the shorthand `name`.

### §4.6.13 record_spread

{{#grammar record_spread}}

A `...` spread of an existing record, optionally overriding fields, with or
without a leading type name. Well-formedness: §5.

### §4.6.14 question_expr

{{#grammar question_expr}}

An expression followed by `?`. Well-formedness: §5.

### §4.6.15 ok_expr

{{#grammar ok_expr}}

`Ok(…)` — the success constructor of `Result` or `HttpResult`. Well-formedness:
§5.

### §4.6.16 err_expr

{{#grammar err_expr}}

`Err(…)` — the failure constructor of `Result`. Well-formedness: §5.

### §4.6.17 some_expr

{{#grammar some_expr}}

`Some(…)` — the present constructor of `Option`. Well-formedness: §5.

### §4.6.18 none_expr

{{#grammar none_expr}}

`None` — the absent constructor of `Option`.

### §4.6.19 effect_pure_expr

{{#grammar effect_pure_expr}}

`Effect.pure(…)` — lifts a pure value into an `Effect`.

### §4.6.20 val_expr

{{#grammar val_expr}}

`Val[T]` with an optional pin argument (v0.114, retiring `Mock[T]`).
Well-formedness — including that it is valid only in test bodies: §5.

### §4.6.21 val_arg

{{#grammar val_arg}}

The pin to a `Val[T]`: positional arguments or a brace-delimited record of field
pins.

### §4.6.21a list_literal

{{#grammar list_literal}}

*(v0.20b)* `[a, b, c]`, with an optional trailing comma — a **leading** `[`
in expression position. It does not collide with explicit type application
(`name[T](…)`, [§4.6.10](#4610-call)): that `[` is a *postfix* form on a
callee identifier and MUST sit on the **same line** as it — a `[` opening a
new line starts a list literal. There is no `Map` literal (`{ }` is records
and blocks) and no indexing form (`get(i)` returns `Option[T]`).
Well-formedness — including empty-literal element-type inference:
[§5.10](/book/spec/static-semantics/#510-collections).

### §4.6.22 paren_expr

{{#grammar paren_expr}}

A parenthesised expression, for grouping.

### §4.6.23 self_expr

{{#grammar self_expr}}

`self` — the receiver inside a method or agent handler. Well-formedness: §5.

## §4.7 Patterns & matching

The patterns used in `match` arms and `is` checks.

### §4.7.1 match_arm

{{#grammar match_arm}}

A pattern, an optional `if` guard (an arbitrary `Bool` expression over the
pattern's bindings), `=>`, a result expression, and an optional trailing comma —
arm separators are optional. Well-formedness: §5.

### §4.7.2 pattern

{{#grammar _pattern}}

A pattern: a wildcard or a variant pattern.

### §4.7.3 variant_pattern

{{#grammar variant_pattern}}

A constant name, optionally qualified, with an optional parenthesised list of
bindings. Well-formedness: §5.

### §4.7.4 wildcard_pattern

{{#grammar wildcard_pattern}}

`_` — matches anything and binds nothing.

### §4.7.5 pattern_binding

{{#grammar _pattern_binding}}

A binding within a variant pattern: a named binding, or a full sub-pattern
matched against the payload field (one recursion point). A lowercase-led
identifier binds the field, an uppercase-led one discriminates a nested nullary
variant (`Err(PollClosed)`), `_` ignores it, and a nested variant recurses
(`Some(Ok(x))`). Well-formedness: §5.

### §4.7.6 named_binding

{{#grammar named_binding}}

Binds a payload field by name, matching it against a sub-pattern: `field: name`,
or `field: _` to ignore it.

## §4.8 Statements

A block is a sequence of statements ending in an optional value expression.

### §4.8.1 block

{{#grammar block}}

A brace-delimited sequence of statements with an optional trailing expression,
which is the block's value. A block with **no** trailing expression has the
implicit value `()` (unit) — the parser synthesises a `()` tail (v0.146, ADR
0170); an empty block `{}` is therefore the unit value.

### §4.8.2 statement

{{#grammar _statement}}

A statement: a `let`, an effectful `let`, an asynchronous send (`~>`), a `do`
effect statement, a `:=` store write, or an assertion.

### §4.8.3 let_stmt

{{#grammar let_stmt}}

`let`, a binding name, an optional type annotation, `=`, and an expression.
Well-formedness: §5.

### §4.8.4 effect_let_stmt

{{#grammar effect_let_stmt}}

`let`, a binding name, an optional type annotation, `<-`, and an effect
expression. Well-formedness: §5.

### §4.8.5 effect_send_stmt

{{#grammar effect_send_stmt}}

`~>` and an effect expression — an **asynchronous send**. Unlike an
`effect_let_stmt` it carries **no binder**: the reply is not awaited and nothing
is bound. Well-formedness — including the requirement that the reply be
`Effect[()]` (the error gate): §5.

### §4.8.6 do_stmt (v0.146)

{{#grammar do_stmt}}

`do` and an effect expression — a **unit effect statement** (ADR 0170). Like an
`effect_send_stmt` it carries **no binder**; unlike it, the effect *is* awaited
and joins the enclosing computation. The binder-free spelling of `let _ <- e`
when the awaited value is unit. Well-formedness — including the requirement that
the operand be `Effect[()]` (a valued reply keeps the explicit `let _ <- e`): §5.

### §4.8.7 assign_stmt (v0.81)

{{#grammar assign_stmt}}

An identifier, `:=`, and an expression — a `Cell` store write. Well-formedness —
including that the target is a `store Cell` field and the right-hand side does not
read it: §5 (ADR 0108).

### §4.8.8 expect_expr

{{#grammar expect_expr}}

`expect` and a `Bool` predicate, or — inside a `case` — an `observation_expr`.
Well-formedness: §5.

### §4.8.9 observation_expr (v0.117)

{{#grammar observation_expr}}

An observation over a capability seam: a `Cap.op` subject (the capability and one
of its operations, *named, not called*) followed by a matcher — `called`
(optionally `once` / `<n> times`, optionally `with <predicate>`), `never called`,
or `before Cap.op`. The matcher words `called`, `never`, `once`, `times`, `with`,
`before` are contextual: outside an observation clause they are ordinary
identifiers. A `with` predicate reads the operation's parameters by their declared
names. Well-formedness — seam resolution, `with` scope and purity: §5.

### §4.8.9 trace_expr (v0.117)

{{#grammar trace_expr}}

`trace(Cap.op)` — a test-only builtin yielding the recorded calls of a capability
operation as a `List` of per-operation call records, in call order. `trace` is an
ordinary identifier outside a `case`. Well-formedness: §5.

### §4.8.10 binding_name

{{#grammar _binding_name}}

The name bound by a `let`: an identifier, or `_` to discard the value.

## §4.9 Testing constructs

Cases, tiers, and provider overrides. See also the top-level `suite_decl`
([§4.1.7](#416-test_decl)).

### §4.9.1 case

{{#grammar case}}

`case`, a description string, an optional `as <tier>` clause (`unit` |
`integration` | `system`; `unit` is the default and elided), and a block body
whose leading items may be case-scoped `stub` clauses. Well-formedness: §5.

### §4.9.2 stub_clause

{{#grammar stub_clause}}

A per-seam provider override: `stub`, a capability, `.`, a method, a
parenthesised argument-pattern list (`_` or a value per parameter), and a
right-hand side — `returns <value>`, `returns each [<outcome>, …]`, or `fails`.
Legal at suite and case scope. Well-formedness: §5.
