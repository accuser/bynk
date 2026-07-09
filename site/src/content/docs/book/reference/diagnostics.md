---
title: Diagnostic index
---
<!-- GENERATED FILE — do not edit by hand.
     Source: bynkc/src/diagnostics.rs (`render_markdown`).
     Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry -->

Every diagnostic code the compiler can emit, with a one-line summary of the cause, grouped by category. For step-by-step cause-and-fix guidance on the most common ones, see the [troubleshooting guides](/book/troubleshooting/).

There are **386** codes in total.

## Agents

| Code | Summary | Construct |
|---|---|---|
| `bynk.agent.construction_arity` | An agent was constructed with the wrong number of key arguments. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agent.handler_arity` | An agent handler was called with the wrong number of arguments. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agent.handler_not_found` | Called a handler the agent does not declare. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agent.key_mismatch` | An agent key argument has the wrong type. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agent.outside_context` | An `agent` was declared outside a context. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agent.return_not_effect` | An agent handler's return type is not an `Effect`. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.agents.bad_state_initialiser` | An agent `store` field initialiser is not a static value of the field's type. | [`store_field`](/book/reference/grammar/#rule-store_field) |
| `bynk.agents.non_zeroable_state_field` | An agent `store` field has no initialiser and no implicit zero value. | [`store_field`](/book/reference/grammar/#rule-store_field) |

## Boundaries

| Code | Summary | Construct |
|---|---|---|
| `bynk.boundary.structural_mismatch` | Data crossing a context boundary did not match the expected shape. |  |

## Capabilities

| Code | Summary | Construct |
|---|---|---|
| `bynk.capability.op_arity` | A capability operation was called with the wrong number of arguments. | [`capability_decl`](/book/reference/grammar/#rule-capability_decl) |
| `bynk.capability.outside_context` | A `capability` was declared outside a context. | [`capability_decl`](/book/reference/grammar/#rule-capability_decl) |
| `bynk.capability.unknown_operation` | Referenced an operation the capability does not declare. | [`capability_decl`](/book/reference/grammar/#rule-capability_decl) |

## Consumes

| Code | Summary | Construct |
|---|---|---|
| `bynk.consumes.alias_conflict` | Two `consumes` aliases collide. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.capability_name_clash` | Two flattened `consumes U { Cap }` capabilities collide, or one clashes with a local capability. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.in_commons` | `consumes` appears in a `commons` (it is only valid in a context). | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.name_conflict` | A `consumes` name collides with another name in scope. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.self_reference` | A context `consumes` itself. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.service_arity` | A consumed service was called with the wrong number of arguments. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.target_is_commons` | `consumes` targets a `commons` instead of a context. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.unknown_context` | `consumes` names a context that does not exist. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.consumes.unknown_service` | Called a service the consumed context does not declare. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |

## Contexts

| Code | Summary | Construct |
|---|---|---|
| `bynk.context.consumes_cycle` | Contexts form a `consumes` dependency cycle. |  |
| `bynk.context.external_construction` | A context-owned type was constructed from outside that context. |  |
| `bynk.context.external_provider` | A bodiless (external) provider was declared outside an `adapter`. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.context.opaque_inspection` | An opaquely-exported type was inspected from outside its context. |  |

## Contracts

| Code | Summary | Construct |
|---|---|---|
| `bynk.contract.duplicate_name` | A function declares two contract clauses (`requires`/`ensures`) with the same name. |  |
| `bynk.contract.impure_predicate` | A contract predicate uses an effectful or test-only construct; a contract clause must be pure. |  |
| `bynk.contract.not_bool` | A contract predicate does not have type `Bool`. |  |
| `bynk.contract.restated_by_test` | A `case`/`property` merely restates a contract clause already declared at the function; the test is redundant. |  |
| `bynk.contract.result_in_requires` | A precondition (`requires`) references `result`; the return value is only in scope inside an `ensures`. |  |

## Cron

| Code | Summary | Construct |
|---|---|---|
| `bynk.cron.bad_params` | A cron handler declares more than one parameter, or a non-`Int` one. | [`cron_handler`](/book/reference/grammar/#rule-cron_handler) |
| `bynk.cron.duplicate_schedule` | Two cron handlers declare the same schedule. | [`cron_handler`](/book/reference/grammar/#rule-cron_handler) |
| `bynk.cron.invalid_schedule` | A cron expression is not five whitespace-separated fields. | [`cron_handler`](/book/reference/grammar/#rule-cron_handler) |
| `bynk.cron.return_not_effect_result` | A cron handler does not return `Effect[Result[(), E]]`. | [`cron_handler`](/book/reference/grammar/#rule-cron_handler) |

## Effects

| Code | Summary | Construct |
|---|---|---|
| `bynk.effect.bind_in_pure_context` | An `<-` bind was used in a pure (non-effectful) context. | [`effect_let_stmt`](/book/reference/grammar/#rule-effect_let_stmt) |
| `bynk.effect.bind_on_non_effect` | An `<-` bind was applied to a non-`Effect` value. | [`effect_let_stmt`](/book/reference/grammar/#rule-effect_let_stmt) |
| `bynk.effect.capability_in_pure_context` | A capability was used in a pure context. |  |
| `bynk.effect.cross_context_in_pure_context` | A cross-context call was made in a pure context. |  |
| `bynk.effect.do_in_pure_context` | A `do` statement was used in a pure (non-effectful) context. | [`do_stmt`](/book/reference/grammar/#rule-do_stmt) |
| `bynk.effect.do_on_non_effect` | A `do` statement was applied to a non-`Effect` value. | [`do_stmt`](/book/reference/grammar/#rule-do_stmt) |
| `bynk.effect.do_requires_unit` | A `do` statement was applied to a valued `Effect[T]`; `do` performs a unit effect, so a real result would be dropped — use `let _ <- e` instead. | [`do_stmt`](/book/reference/grammar/#rule-do_stmt) |
| `bynk.effect.fn_value_in_pure_context` | An effectful function value was called in a pure context; like a capability call, it is legal only where the enclosing body is effectful. | [`call`](/book/reference/grammar/#rule-call) |

## Expectations

| Code | Summary | Construct |
|---|---|---|
| `bynk.expect.not_bool` | `expect` was given a non-`Bool` predicate. | [`expect_expr`](/book/reference/grammar/#rule-expect_expr) |
| `bynk.expect.outside_case` | `expect` was used outside a `case` body. | [`expect_expr`](/book/reference/grammar/#rule-expect_expr) |

## Exports

| Code | Summary | Construct |
|---|---|---|
| `bynk.exports.capability_not_provided` | An exported capability has no provider in its context. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.exports.conflicting_visibility` | A type is exported with conflicting visibilities. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.exports.duplicate_export` | The same name is exported more than once. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.exports.duplicate_in_clause` | A name appears twice in one `exports` clause. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.exports.undeclared_capability` | `exports capability` names a capability that is not declared. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.exports.undeclared_type` | `exports` names a type that is not declared. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |

## Given capabilities

| Code | Summary | Construct |
|---|---|---|
| `bynk.given.cross_context_unknown_capability` | `given B.Cap` names a capability the consumed context does not export. | [`given_clause`](/book/reference/grammar/#rule-given_clause) |
| `bynk.given.undeclared_capability` | A handler uses a capability it did not declare with `given`. | [`given_clause`](/book/reference/grammar/#rule-given_clause) |
| `bynk.given.unknown_capability` | `given` names a capability that does not exist. | [`given_clause`](/book/reference/grammar/#rule-given_clause) |
| `bynk.given.unused_capability` | A `given` capability is never used (warning). | [`given_clause`](/book/reference/grammar/#rule-given_clause) |

## HTTP

| Code | Summary | Construct |
|---|---|---|
| `bynk.http.body_on_get_or_delete` | A GET or DELETE handler declares a `body` parameter. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.cache_bad_max_age` | A `@cache` annotation's `maxAge` is missing or not a positive `Duration` literal. |  |
| `bynk.http.cache_bad_scope` | A `@cache` annotation's `scope` is not `public` or `private`. |  |
| `bynk.http.cache_duplicate` | A handler carries more than one `@cache` annotation. |  |
| `bynk.http.cache_on_non_get` | A `@cache` annotation is placed on a handler that is not `on http GET`. |  |
| `bynk.http.cache_unknown_arg` | A `@cache` annotation has an argument outside the closed set (`maxAge`/`scope`). |  |
| `bynk.http.cors_invalid_field` | A `cors` policy field (`headers`/`credentials`/`maxAge`) has the wrong value shape. |  |
| `bynk.http.cors_invalid_origins` | A `cors` policy's `origins` is missing, empty, or not a list of string literals. |  |
| `bynk.http.cors_not_http` | A `cors { }` policy appears on a service that is not `from http`. |  |
| `bynk.http.cors_unknown_field` | A `cors { }` policy declares a field outside the closed set. |  |
| `bynk.http.cors_wildcard_credentials` | A `cors` policy combines `credentials: true` with the wildcard origin `["*"]`. |  |
| `bynk.http.duplicate_route` | Two handlers share the same method and route. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.extra_param` | A handler parameter is neither a path parameter nor `body`. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.invalid_path` | An HTTP route path is malformed. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.limit_bad_max_body` | A `@limit` annotation's `maxBody` is missing or not a positive `Int` literal. |  |
| `bynk.http.limit_duplicate` | A handler carries more than one `@limit` annotation. |  |
| `bynk.http.limit_on_bodyless` | A `@limit` annotation is placed on a handler that takes no body (a GET or DELETE). |  |
| `bynk.http.limit_unknown_arg` | A `@limit` annotation has an argument outside the closed set (`maxBody`). |  |
| `bynk.http.limits_invalid_field` | A `limits` policy field (`maxBody`) has the wrong value shape. |  |
| `bynk.http.limits_not_http` | A `limits { }` policy appears on a service that is not `from http`. |  |
| `bynk.http.limits_unknown_field` | A `limits { }` policy declares a field outside the closed set. |  |
| `bynk.http.path_param_not_stringy` | A path parameter's type is not constructible from a string. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.reserved_prefix` | A route uses the reserved `/_bynk/` prefix. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.return_not_effect_http_result` | An HTTP handler does not return `Effect[HttpResult[T]]`. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.security_invalid_field` | A `security` policy field (`hsts`/`nosniff`) has the wrong value shape. |  |
| `bynk.http.security_not_http` | A `security { }` policy appears on a service that is not `from http`. |  |
| `bynk.http.security_unknown_field` | A `security { }` policy declares a field outside the closed set. |  |
| `bynk.http.unbound_path_param` | A `:name` route segment has no matching handler parameter. | [`http_handler`](/book/reference/grammar/#rule-http_handler) |
| `bynk.http.unknown_handler_annotation` | A handler carries an annotation outside the closed set (`@cache`/`@limit`). |  |

## Lexer

| Code | Summary | Construct |
|---|---|---|
| `bynk.lex.bad_escape` | An invalid escape sequence in a string literal. | [`string_literal`](/book/reference/grammar/#rule-string_literal) |
| `bynk.lex.float_literal_overflow` | A float literal does not fit a finite 64-bit float. | [`float_literal`](/book/reference/grammar/#rule-float_literal) |
| `bynk.lex.integer_overflow` | An integer literal is out of range. | [`number_literal`](/book/reference/grammar/#rule-number_literal) |
| `bynk.lex.unclosed_doc_block` | A documentation block is not closed. |  |
| `bynk.lex.unexpected_character` | An unexpected character in the source. |  |
| `bynk.lex.unterminated_interpolation` | An interpolation hole `\(…)` is not closed on its line. | [`string_literal`](/book/reference/grammar/#rule-string_literal) |
| `bynk.lex.unterminated_string` | A string literal is not terminated. | [`string_literal`](/book/reference/grammar/#rule-string_literal) |

## Observation

| Code | Summary | Construct |
|---|---|---|
| `bynk.observe.bad_count` | An observation call count is not a non-negative integer literal (`called once` / `called <n> times`). |  |
| `bynk.observe.impure_with` | A `with` predicate uses an effectful or test-only construct; it must be pure. |  |
| `bynk.observe.not_a_seam` | An observation targets a capability the unit under test does not consume. |  |
| `bynk.observe.outside_case` | An observation appears outside a `case` body. |  |
| `bynk.observe.trace_outside_test` | `trace(Cap.op)` appears outside a `case` body. |  |
| `bynk.observe.unknown_op` | An observation names an operation the capability does not declare. |  |
| `bynk.observe.with_not_bool` | A `with` predicate does not have type `Bool`. |  |

## Other

| Code | Summary | Construct |
|---|---|---|
| `bynk.actor.bearer_identity_not_string_constructible` | A `Bearer` actor's identity is not a string-constructible type. |  |
| `bynk.actor.bearer_missing_secret` | A `Bearer` actor does not name its signing secret. |  |
| `bynk.actor.binder_shadows_param` | A `by` actor binder collides with a handler parameter of the same name. |  |
| `bynk.actor.by_on_agent` | A `by` actor clause was placed on an agent `on call` handler, which has no actor. |  |
| `bynk.actor.duplicate_sum_scheme` | Two peers in a multi-actor sum share an authentication scheme. |  |
| `bynk.actor.identity_not_sealed` | An actor identity type is not a context-ownable (sealed) value type. |  |
| `bynk.actor.missing_by_on_http` | An HTTP handler lacks the required `by` actor clause. |  |
| `bynk.actor.oidc_identity_not_string_constructible` | An `Oidc` actor's identity is not a string-constructible type. |  |
| `bynk.actor.oidc_missing_audience` | An `Oidc` actor does not name its `audience`. |  |
| `bynk.actor.oidc_missing_issuer` | An `Oidc` actor does not name its `issuer`. |  |
| `bynk.actor.oidc_missing_jwks` | An `Oidc` actor does not name its `jwks` endpoint. |  |
| `bynk.actor.oidc_not_in_sum` | An `Oidc` actor appears as a member of a multi-actor sum. |  |
| `bynk.actor.outside_context` | An `actor` was declared outside a context (e.g. in a commons). |  |
| `bynk.actor.refinement_base_unsupported` | A refinement actor's base is not a `Bearer` actor (no claims to authorise against). |  |
| `bynk.actor.refinement_in_sum` | A refinement actor appears as a member of a multi-actor sum. |  |
| `bynk.actor.refinement_predicate_unsupported` | A refinement actor's `where` predicate is outside the closed claim-predicate set. |  |
| `bynk.actor.scheme_not_admissible` | An actor's scheme is not admissible on this handler's protocol. |  |
| `bynk.actor.signature_identity_unsupported` | A `Signature` actor declared an `identity`, which is not yet supported. |  |
| `bynk.actor.signature_missing_header` | A `Signature` actor does not name its signature header. |  |
| `bynk.actor.signature_missing_secret` | A `Signature` actor does not name its signing secret. |  |
| `bynk.actor.signature_requires_body` | A `Signature` handler does not take a `body` parameter. |  |
| `bynk.actor.signature_tolerance_without_timestamp` | A `Signature` actor set `tolerance` without a `timestamp` header. |  |
| `bynk.actor.sum_requires_binder` | A multi-actor sum `by` clause has no binder to match the resolved actor. |  |
| `bynk.actor.unknown_actor` | A handler's `by` clause names an actor that is not declared. |  |
| `bynk.actor.unknown_scheme` | An actor declares an authentication scheme that is not compiler-known. |  |
| `bynk.actor.unreachable_sum_arm` | A multi-actor sum has an arm unreachable after a catch-all (`None`) peer. |  |
| `bynk.adapter.consumes_context` | An `adapter` consumed a context; adapter dependencies are adapter-to-adapter. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.adapter.consumes_requires_selection` | An `adapter` used a whole-unit or aliased `consumes`; adapters must select capabilities with `consumes U { Cap, … }`. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.adapter.disallowed_item` | An `adapter` declared a `service`, `agent`, or other item it may not contain. | [`adapter_decl`](/book/reference/grammar/#rule-adapter_decl) |
| `bynk.adapter.duplicate_binding` | An `adapter` declared more than one `binding` clause. | [`binding_decl`](/book/reference/grammar/#rule-binding_decl) |
| `bynk.adapter.no_binding` | An `adapter` declares an external provider but no `binding` module to supply it. | [`adapter_decl`](/book/reference/grammar/#rule-adapter_decl) |
| `bynk.adapter.provider_has_body` | A provider inside an `adapter` has a Bynk body; adapter providers must be external. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.cell.invalid_target` | A `:=` write targets something that is not a `store Cell` field. |  |
| `bynk.cell.self_reference` | A `:=` right-hand side reads the cell being written (a read-modify-write); use `.update`. |  |
| `bynk.duration.literal_overflow` | A `Duration` literal (`<int>.<unit>`) exceeds the representable millisecond range. |  |
| `bynk.generics.no_bounds` | A type parameter carries a bound (`[A: …]`); bounded generics are not in v0.20a. | [`fn_decl`](/book/reference/grammar/#rule-fn_decl) |
| `bynk.generics.no_generic_types` | A `type` declaration carries a type-parameter list; generic type declarations are not in v0.20a (type parameters belong to functions). | [`type_decl`](/book/reference/grammar/#rule-type_decl) |
| `bynk.generics.type_arg_mismatch` | Inferred or explicit type arguments conflict, have the wrong arity, target a non-generic function, or a type parameter shadows a declared type. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.generics.uninferable_type_arg` | A generic function's type parameter could not be inferred from the arguments and was not given explicitly (`name[T](…)`); a bare generic function also cannot be passed as a value in v0.20a. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.held.branch_divergence` | Branches of a conditional leave a held value (e.g. `Connection[F]`) in inconsistent ownership states — one consumes or stores it, another leaves it owned (§2.9.5, real-time track slice 2). |  |
| `bynk.held.consume_on_borrow` | A consuming operation (`close`/`put`/`take`) is called on a *borrowed* held reference — borrows admit only non-consuming operations like `send` (§2.9.3, real-time track slice 2). |  |
| `bynk.held.leak` | A held value (`Connection[F]`) is still owned at scope exit — it must be disposed (stored, closed, or transferred) before the handler returns (§2.9.1, real-time track slice 2). |  |
| `bynk.held.unsupported_map_op` | A held `Map[K, Connection]` is given an `update`/`upsert` — a held resource cannot be transformed by a `(Connection) -> Connection` function; use `put`/`get`/`remove` (real-time track slice 3b-ii). |  |
| `bynk.held.unsupported_storage` | A held value (`Connection[F]`) is stored in a `Set`/`Log`/`Cache` — held values may only live in `Cell[Option[Connection]]` or `Map[K, Connection]` (§2.9.3, real-time track slice 2). |  |
| `bynk.held.use_after_consume` | A held value (`Connection[F]`) is used after a consuming operation (`close`/`put`/`take`) ended its lifetime (§2.9.2, real-time track slice 2). |  |
| `bynk.history.not_an_agent` | A `for all run: History[T]` names a `T` that is not an agent — only an agent has handlers to sequence and reachable states to observe (testing track slice 7, ADR 0155). |  |
| `bynk.history.not_generable` | A `for all run: History[Agent]` targets an agent with a handler parameter whose type cannot be generated (e.g. a `Matches` refinement), so its call-history cannot be driven (testing track slice 7, ADR 0155). |  |
| `bynk.history.outside_property` | `History[Agent]` appears outside a `property`'s `for all` binding — it is a test-only generator, not a value type (testing track slice 7, ADR 0155). |  |
| `bynk.history.restates_invariant` | A history property merely re-checks a guarantee a declared `invariant`/`transition` already enforces on every reached state (testing track slice 7, ADR 0155). |  |
| `bynk.index.bad_argument` | An `@indexed` argument is not a `by: <field>` label. |  |
| `bynk.index.missing` | A query filters a map by equality on a field that is not `@indexed` (a perf-hint warning). |  |
| `bynk.index.unkeyable_key` | An `@indexed(by: k)` field is not value-keyable. |  |
| `bynk.index.unknown_key` | An `@indexed(by: k)` field is not a field of the map's value type. |  |
| `bynk.index.unused` | A declared `@indexed(by: k)` is never used by an equality filter (a hygiene warning). |  |
| `bynk.invariant.cross_agent_reference` | An invariant predicate references another agent; invariants are per-agent. |  |
| `bynk.invariant.duplicate_name` | An agent declares two invariants with the same name. |  |
| `bynk.invariant.impure_predicate` | An invariant predicate uses an effectful or test-only construct. |  |
| `bynk.invariant.not_bool` | An invariant predicate does not have type `Bool`. |  |
| `bynk.lambda.unannotated_param` | A lambda parameter has no type annotation in a position where no function type is expected to infer it from. | [`lambda_expr`](/book/reference/grammar/#rule-lambda_expr) |
| `bynk.list.deprecated_function` | A `bynk.list` free function (`map`/`filter`/`find`/`any`/`all`) is deprecated in favour of the `List` method form (warning; auto-fixable). |  |
| `bynk.namespace.reserved` | A user unit is named `bynk` or `bynk.*`; the `bynk` root is reserved for the toolchain. |  |
| `bynk.provides.bad_sequence` | A `provides … returns each […]` sequence is malformed (e.g. empty). |  |
| `bynk.provides.not_a_seam` | A test `provides` overrides a capability the unit under test does not consume. |  |
| `bynk.provides.rhs_type` | A test `provides … returns <value>` right-hand side does not match the operation's return type. |  |
| `bynk.provides.unknown_op` | A test `provides` names an operation the capability does not declare. |  |
| `bynk.query.join_key_mismatch` | A `joinOn`/`leftJoin` left and right key function return different types. |  |
| `bynk.query.sum_needs_numeric` | A `sum`/`average` key function does not return a numeric type (`Int`, `Float`, or `Duration`). |  |
| `bynk.requires.unpinned_dependency` | An adapter `binding … requires { … }` entry has an unpinned version range. | [`binding_decl`](/book/reference/grammar/#rule-binding_decl) |
| `bynk.send.in_pure_context` | A `~>` send was used in a pure (non-effectful) context. | [`effect_send_stmt`](/book/reference/grammar/#rule-effect_send_stmt) |
| `bynk.send.non_effect` | A `~>` send was applied to a non-`Effect` value. | [`effect_send_stmt`](/book/reference/grammar/#rule-effect_send_stmt) |
| `bynk.send.requires_unit` | A `~>` send targets an operation whose reply is not `Effect[()]`. | [`effect_send_stmt`](/book/reference/grammar/#rule-effect_send_stmt) |
| `bynk.store.annotation_kind_mismatch` | A storage annotation is used on a kind it does not apply to (e.g. `@ttl` on a `Map`). |  |
| `bynk.store.annotation_unsupported` | A known storage annotation (`@ttl`/`@retain`/`@indexed`/`@bounded`) is used before the slice that supports it. |  |
| `bynk.store.cache_needs_clock` | A handler performs a `Cache` operation (TTL expiry reads the clock) without declaring `given Clock`. |  |
| `bynk.store.cache_ttl_required` | A `Cache` field is missing its required `@ttl(<duration>)` annotation (a keyed store with no expiry is a `Map`). |  |
| `bynk.store.kind_arity` | A storage kind was applied to the wrong number of type arguments (e.g. `Cell[A, B]`). |  |
| `bynk.store.kind_unsupported` | A known storage kind (`Queue`) is used before the slice that supports it. |  |
| `bynk.store.log_needs_clock` | A handler calls `Log.append` (which stamps the current time) without declaring `given Clock`. |  |
| `bynk.store.unknown_annotation` | A `store` field carries an annotation outside the closed `@indexed`/`@ttl`/`@retain`/`@bounded` set. |  |
| `bynk.store.unknown_kind` | A `store` field's type is not a known storage kind. |  |
| `bynk.store.unknown_op` | A storage-`Map`/`Set` operation is not a recognised entry/membership method. |  |
| `bynk.target.browser_bundle_only` | The `browser` platform builds only the in-process `Bundle` topology; `--target workers` is not a browser build. |  |
| `bynk.target.vendor_conflict` | One deployment unit's in-process closure uses platform-native capabilities from two mutually-exclusive platforms. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.target.vendor_required` | A deployment unit uses a platform-native capability but the build selects another `--platform`. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.tier.property_has_tier` | A `property` carries an `as <tier>` clause; tiers are a `case`-only affordance. |  |
| `bynk.tier.system_needs_wire` | An `as system` test stands up fewer than two contexts; the system tier wires across contexts. |  |
| `bynk.ws.message_frame_param` | A WebSocket `on message` handler does not have exactly one parameter of the service's inbound (`in:`) frame type — the decoded frame (real-time track slice 3b-iii). |  |
| `bynk.ws.open_given_unsupported` | A WebSocket `on open` handler declares `given` capabilities — unsupported at v1, since on Workers the handler runs inside the connection-hosting Durable Object, which has no composition root to supply them (real-time track slice 3b). |  |
| `bynk.ws.open_transfer_shape` | A WebSocket `on open` handler does not transfer its `connection` into exactly one agent, so the Workers upgrade has no single Durable Object to route to (real-time track slice 3b). |  |
| `bynk.ws.route_param_mismatch` | A WebSocket `on message`/`on close` route parameter does not match the `on open` parameter at the same position — route values are recovered positionally from the connection, so they must be a type-compatible prefix of the `on open` parameters (real-time track slice 3b-iii). |  |

## Parser

| Code | Summary | Construct |
|---|---|---|
| `bynk.parse.consumes_after_decls` | `consumes` appears after other declarations. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.parse.dangling_handler_annotation` | A handler-position annotation (e.g. `@cache`) is not followed by an `on` handler. |  |
| `bynk.parse.duplicate_cors` | A service declares more than one `cors { }` policy. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.parse.duplicate_limits` | A service declares more than one `limits { }` policy. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.parse.duplicate_security` | A service declares more than one `security { }` policy. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.parse.empty_agent` | An `agent` body is empty. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.parse.empty_capability` | A `capability` body is empty. | [`capability_decl`](/book/reference/grammar/#rule-capability_decl) |
| `bynk.parse.empty_interpolation` | An interpolation hole `\(…)` contains no expression. |  |
| `bynk.parse.empty_match` | A `match` has no arms. | [`match_expr`](/book/reference/grammar/#rule-match_expr) |
| `bynk.parse.empty_service` | A `service` body is empty. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.parse.expected_agent_key` | Expected a `key` declaration in an agent. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.parse.expected_agent_storage` | An agent declares no storage — it has no `store` fields. |  |
| `bynk.parse.expected_base_type` | Expected a base type. | [`base_type`](/book/reference/grammar/#rule-base_type) |
| `bynk.parse.expected_capability_op` | Expected a capability operation. | [`capability_op`](/book/reference/grammar/#rule-capability_op) |
| `bynk.parse.expected_expression` | Expected an expression. |  |
| `bynk.parse.expected_handler` | Expected a handler. | [`handler`](/book/reference/grammar/#rule-handler) |
| `bynk.parse.expected_item` | Expected a declaration. |  |
| `bynk.parse.expected_predicate` | Expected a refinement predicate. | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.parse.expected_provider_op` | Expected a provider operation. | [`provider_op`](/book/reference/grammar/#rule-provider_op) |
| `bynk.parse.expected_token` | Expected a specific token. |  |
| `bynk.parse.expected_type` | Expected a type. |  |
| `bynk.parse.expected_unit_header` | Expected a `commons` or `context` header. |  |
| `bynk.parse.expected_visibility` | Expected a visibility keyword. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.parse.exports_after_decls` | `exports` appears after other declarations. | [`exports_decl`](/book/reference/grammar/#rule-exports_decl) |
| `bynk.parse.extra_tokens` | Unexpected tokens after an otherwise complete construct. |  |
| `bynk.parse.generic_arg_count` | Wrong number of generic type arguments. | [`generic_type_ref`](/book/reference/grammar/#rule-generic_type_ref) |
| `bynk.parse.handler_in_agent` | A protocol handler (`on GET`/`schedule`/`message`) was declared in an agent. | [`handler`](/book/reference/grammar/#rule-handler) |
| `bynk.parse.invariant_after_handler` | An `invariant` was declared after a handler; invariants precede handlers. |  |
| `bynk.parse.malformed_float_literal` | A float literal is missing a digit on one side of the `.` (`1.`, `.5`). | [`float_literal`](/book/reference/grammar/#rule-float_literal) |
| `bynk.parse.non_associative` | A non-associative operator was chained (e.g. `a == b == c`). | [`binary_expr`](/book/reference/grammar/#rule-binary_expr) |
| `bynk.parse.orphan_doc_block` | A documentation block is not attached to a declaration (warning). |  |
| `bynk.parse.reserved_keyword` | A reserved keyword was used as an identifier. | [`identifier`](/book/reference/grammar/#rule-identifier) |
| `bynk.parse.self_outside_method` | `self` used outside a method or handler. | [`self_expr`](/book/reference/grammar/#rule-self_expr) |
| `bynk.parse.storage_after_phase` | Agent storage (`state` / `store`) is declared after the invariants or handlers. |  |
| `bynk.parse.transition_after_handler` | A `transition` is declared after an agent handler; step invariants precede the handlers. |  |
| `bynk.parse.unexpected_adapter` | An `adapter` appeared where it is not allowed. |  |
| `bynk.parse.unexpected_context` | A `context` appeared where it is not allowed. | [`context_decl`](/book/reference/grammar/#rule-context_decl) |
| `bynk.parse.unexpected_eof` | Unexpected end of input. |  |
| `bynk.parse.unexpected_suite` | A `suite` appeared where it is not allowed. | [`suite_decl`](/book/reference/grammar/#rule-suite_decl) |
| `bynk.parse.unknown_effect_method` | An unknown method on `Effect`. |  |
| `bynk.parse.unknown_handler_kind` | An unknown handler form (expected `call`, an HTTP method, `schedule`, or `message`). | [`handler`](/book/reference/grammar/#rule-handler) |
| `bynk.parse.unknown_predicate` | An unknown refinement predicate. | [`predicate_name`](/book/reference/grammar/#rule-predicate_name) |
| `bynk.parse.unknown_tier` | A `case`/`suite` `as <tier>` clause names something other than `unit`, `integration`, or `system`. |  |
| `bynk.parse.uses_after_decls` | `uses` appears after other declarations. | [`uses_decl`](/book/reference/grammar/#rule-uses_decl) |

## Project

| Code | Summary | Construct |
|---|---|---|
| `bynk.project.file_and_directory` | A unit exists as both a file and a directory. |  |
| `bynk.project.inconsistent_commons_name` | A source file's path does not match its declared name. |  |
| `bynk.project.kind_conflict` | A name is declared as both a commons and a context. |  |
| `bynk.project.no_root` | No project root could be determined. |  |
| `bynk.project.no_sources` | The project contains no source files. |  |
| `bynk.project.read_failed` | A source file could not be read. |  |

## Properties (generative tests)

| Code | Summary | Construct |
|---|---|---|
| `bynk.property.restates_refinement` | A `property` merely re-checks a refinement its type already guarantees. | [`for_all`](/book/reference/grammar/#rule-for_all) |
| `bynk.property.where_not_bool` | A `for all ... where` filter does not type to `Bool`. | [`for_all`](/book/reference/grammar/#rule-for_all) |

## Providers

| Code | Summary | Construct |
|---|---|---|
| `bynk.provider.dependency_cycle` | Providers form a capability dependency cycle through `given`. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.provider.extra_operation` | A `provides` block implements an operation not in the capability. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.provider.missing_operation` | A `provides` block is missing a capability operation. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.provider.outside_context` | `provides` was declared outside a context. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.provider.signature_mismatch` | A `provides` operation's signature does not match the capability. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.provider.unknown_capability` | `provides` names a capability that does not exist. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |

## Queue

| Code | Summary | Construct |
|---|---|---|
| `bynk.queue.bad_params` | An `on message` handler does not take exactly one `message` parameter. | [`queue_handler`](/book/reference/grammar/#rule-queue_handler) |
| `bynk.queue.duplicate_consumer` | Two `on message` handlers consume the same queue. | [`queue_handler`](/book/reference/grammar/#rule-queue_handler) |
| `bynk.queue.invalid_name` | A `from queue("…")` binding has an empty queue name. | [`queue_handler`](/book/reference/grammar/#rule-queue_handler) |
| `bynk.queue.return_not_queue_result` | An `on message` handler does not return `Effect[QueueResult]`. | [`handler`](/book/reference/grammar/#rule-handler) |

## Record spread

| Code | Summary | Construct |
|---|---|---|
| `bynk.record_spread.field_type_mismatch` | A record-spread override has the wrong type for the field. | [`record_spread`](/book/reference/grammar/#rule-record_spread) |
| `bynk.record_spread.non_record_base` | The base of a record spread is not a record. | [`record_spread`](/book/reference/grammar/#rule-record_spread) |
| `bynk.record_spread.type_mismatch` | A record spread's base is a different record type. | [`record_spread`](/book/reference/grammar/#rule-record_spread) |
| `bynk.record_spread.unknown_field` | A record spread overrides a field the record does not have. | [`record_spread`](/book/reference/grammar/#rule-record_spread) |

## Refinement

| Code | Summary | Construct |
|---|---|---|
| `bynk.refine.literal_violates` | A literal does not satisfy the refined type's predicate. | [`refined_type`](/book/reference/grammar/#rule-refined_type) |

## Resolution

| Code | Summary | Construct |
|---|---|---|
| `bynk.resolve.ambiguous_variant` | A variant name is ambiguous across several sum types. |  |
| `bynk.resolve.arity_mismatch` | A function was called with the wrong number of arguments. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.resolve.duplicate_actor` | Two actors share a name. |  |
| `bynk.resolve.duplicate_agent` | Two agents share a name. | [`agent_decl`](/book/reference/grammar/#rule-agent_decl) |
| `bynk.resolve.duplicate_capability` | Two capabilities share a name. | [`capability_decl`](/book/reference/grammar/#rule-capability_decl) |
| `bynk.resolve.duplicate_field` | A record declares a field twice. | [`record_type`](/book/reference/grammar/#rule-record_type) |
| `bynk.resolve.duplicate_field_init` | A record construction initialises a field twice. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.resolve.duplicate_fn` | Two functions share a name. | [`fn_decl`](/book/reference/grammar/#rule-fn_decl) |
| `bynk.resolve.duplicate_method` | Two methods share a name. | [`fn_decl`](/book/reference/grammar/#rule-fn_decl) |
| `bynk.resolve.duplicate_param` | A parameter name is repeated. | [`param`](/book/reference/grammar/#rule-param) |
| `bynk.resolve.duplicate_provider` | A capability is provided more than once. | [`provider_decl`](/book/reference/grammar/#rule-provider_decl) |
| `bynk.resolve.duplicate_service` | Two services share a name. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.resolve.duplicate_type` | Two types share a name. | [`type_decl`](/book/reference/grammar/#rule-type_decl) |
| `bynk.resolve.duplicate_variant` | A sum type declares a variant twice. | [`sum_type`](/book/reference/grammar/#rule-sum_type) |
| `bynk.resolve.fn_without_call` | A function was referenced without being called. |  |
| `bynk.resolve.let_shadows_fn` | A `let` binding shadows a function. | [`let_stmt`](/book/reference/grammar/#rule-let_stmt) |
| `bynk.resolve.let_shadows_type` | A `let` binding shadows a type. | [`let_stmt`](/book/reference/grammar/#rule-let_stmt) |
| `bynk.resolve.method_unknown_type` | A method is defined on an unknown type. |  |
| `bynk.resolve.missing_field` | A record construction omits a required field. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.resolve.name_conflict` | Two declarations share a name. |  |
| `bynk.resolve.not_a_record_type` | Record syntax was used on a non-record type. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.resolve.opaque_record_construction` | An opaque type was constructed with record syntax. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.resolve.param_as_function` | A value (such as a parameter) was called as a function. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.resolve.recursive_record_field` | A record directly contains a field of its own type. | [`record_type`](/book/reference/grammar/#rule-record_type) |
| `bynk.resolve.self_outside_method` | `self` referenced outside a method or handler. | [`self_expr`](/book/reference/grammar/#rule-self_expr) |
| `bynk.resolve.type_as_function` | A type name was called as if it were a function. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.resolve.type_in_expr` | A type name was used where a value is expected. |  |
| `bynk.resolve.unconsumed_context` | A context's service was called without a `consumes` declaration. | [`consumes_decl`](/book/reference/grammar/#rule-consumes_decl) |
| `bynk.resolve.unknown_field` | Accessed a field the record does not have. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.resolve.unknown_function` | Called a function that does not exist. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.resolve.unknown_name` | Referenced a name that is not in scope. |  |
| `bynk.resolve.unknown_static_member` | Referenced an unknown static member (e.g. `T.x`). | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.resolve.unknown_type` | Referenced a type that does not exist. |  |

## Services

| Code | Summary | Construct |
|---|---|---|
| `bynk.service.missing_from` | A `from`-less service has a handler other than `on call`. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.service.mixed_protocols` | A service mixes handler forms that do not match its `from <protocol>`. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.service.outside_context` | A `service` was declared outside a context. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.service.return_not_effect` | A service handler's return type is not an `Effect`. | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.service.unknown_protocol` | A `from <protocol>` names an unknown protocol (e.g. a transport like Kafka). | [`service_decl`](/book/reference/grammar/#rule-service_decl) |
| `bynk.service.websocket_header` | The `from WebSocket` header is malformed — it binds frame types as `WebSocket(in: <type>, out: <type>)` (real-time track slice 3). |  |
| `bynk.service.websocket_multiple` | A context holds more than one `from WebSocket` service — at v1 the Workers upgrade routes by the `Upgrade: websocket` header alone, so one WebSocket service per context (real-time track slice 3b). |  |
| `bynk.service.websocket_open_arity` | A `from WebSocket` service must hold exactly one `on open` handler (the edge upgrade), and at most one `on message` (inbound) and one `on close` (real-time track slice 3/3b-iii). |  |

## Suites and cases

| Code | Summary | Construct |
|---|---|---|
| `bynk.suite.duplicate_case_name` | Two `case`s share a description. | [`case`](/book/reference/grammar/#rule-case) |
| `bynk.suite.unknown_target` | A `suite` targets a unit that does not exist. | [`suite_decl`](/book/reference/grammar/#rule-suite_decl) |

## Transitions (step invariants)

| Code | Summary | Construct |
|---|---|---|
| `bynk.transition.cross_agent_reference` | A transition predicate references another agent; step invariants are per-agent. |  |
| `bynk.transition.duplicate_name` | An agent declares two transitions with the same name. |  |
| `bynk.transition.impure_predicate` | A transition predicate uses an effectful or test-only construct; a step invariant must be pure. |  |
| `bynk.transition.no_step_reference` | A transition references neither `old` nor `new`; it constrains one state, so it is an `invariant`, not a step. |  |
| `bynk.transition.not_bool` | A transition predicate does not have type `Bool`. |  |

## Type checking

| Code | Summary | Construct |
|---|---|---|
| `bynk.types.ambiguous_constructor` | `Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it. |  |
| `bynk.types.argument_mismatch` | A function argument has the wrong type. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.types.bytes_at_workers_boundary` | A bare `Bytes` appears in a `workers` wire signature — the erased cross-context boundary does not base64-encode it, so v1 diagnoses it rather than mis-encode. The typed paths (`bundle` calls, `store`/record fields) round-trip a `Bytes` fine (ADR 0142 D8). |  |
| `bynk.types.call_arity` | A function value was applied with the wrong number of arguments. | [`call`](/book/reference/grammar/#rule-call) |
| `bynk.types.cannot_infer_option_type_param` | The value type of `None` could not be inferred. | [`none_expr`](/book/reference/grammar/#rule-none_expr) |
| `bynk.types.cannot_infer_result_type_params` | The type parameters of a `Result` could not be inferred. |  |
| `bynk.types.constructor_arity` | A variant constructor got the wrong number of arguments. |  |
| `bynk.types.constructor_base_mismatch` | A `.of` constructor was given an argument of the wrong base type. |  |
| `bynk.types.duplicate_literal_arm` | A `match` has two arms for the same literal value. | [`match_arm`](/book/reference/grammar/#rule-match_arm) |
| `bynk.types.duplicate_variant_arm` | A `match` has two arms for the same variant. | [`match_arm`](/book/reference/grammar/#rule-match_arm) |
| `bynk.types.empty_refinement` | A refinement admits no values (contradictory predicates). | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.err_value_mismatch` | An `Err` payload has the wrong type. | [`err_expr`](/book/reference/grammar/#rule-err_expr) |
| `bynk.types.field_access_on_non_record` | Field access on a value that is not a record. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.types.field_refinement_not_base` | An inline field refinement requires a base or refined type. | [`record_field`](/book/reference/grammar/#rule-record_field) |
| `bynk.types.field_value_mismatch` | A record field was given a value of the wrong type. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.types.function_at_boundary` | A function type appeared in a serialisable or boundary position (a record field, sum payload, service/agent handler signature, capability operation signature, agent state field, or agent key); functions cannot serialise or cross a boundary. | [`function_type_ref`](/book/reference/grammar/#rule-function_type_ref) |
| `bynk.types.guard_not_bool` | A match-arm `if` guard is not a `Bool` expression. | [`match_arm`](/book/reference/grammar/#rule-match_arm) |
| `bynk.types.held_at_boundary` | A held value (`Connection[F]`) appears in a serialisable or boundary position — a held resource is built and disposed in place, never persisted or sent across a boundary (§2.9, real-time track slice 2). |  |
| `bynk.types.held_not_comparable` | A held value (`Connection[F]`) is compared with `==`/`!=` — held values have identity, not value-equality (§2.9.3, real-time track slice 2). |  |
| `bynk.types.if_branch_mismatch` | The branches of an `if` have different types. | [`if_expr`](/book/reference/grammar/#rule-if_expr) |
| `bynk.types.if_non_bool_cond` | An `if` condition is not a `Bool`. | [`if_expr`](/book/reference/grammar/#rule-if_expr) |
| `bynk.types.if_without_else_requires_unit` | An `if` with no `else` branch has a non-unit then-branch; the missing else defaults to `()`, so the branch must be `()` or `Effect[()]`. | [`if_expr`](/book/reference/grammar/#rule-if_expr) |
| `bynk.types.interpolation_non_scalar` | An interpolation hole holds a value with no string form. |  |
| `bynk.types.invalid_regex` | A `Matches` predicate contains an invalid regular expression. | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.inverted_range` | An `InRange` predicate has its bounds inverted. | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.is_base_mismatch` | An `is` refinement check is applied to a value of the wrong base type. | [`is_expr`](/book/reference/grammar/#rule-is_expr) |
| `bynk.types.is_literal_pattern` | A literal was used on the right of `is`; `is` tests type/refinement, not value equality (use `==`). | [`is_expr`](/book/reference/grammar/#rule-is_expr) |
| `bynk.types.is_non_sum` | `is` was applied to a value that is not a sum type. | [`is_expr`](/book/reference/grammar/#rule-is_expr) |
| `bynk.types.is_unknown_variant` | `is` names a variant the type does not have. | [`is_expr`](/book/reference/grammar/#rule-is_expr) |
| `bynk.types.json_uncodable` | A `Json.encode`/`Json.decode` target type cannot pass through the typed JSON codec (functions, effects, error builtins). | [`method_call`](/book/reference/grammar/#rule-method_call) |
| `bynk.types.key_not_orderable` | A `sortBy`/`min`/`max` key function does not return an orderable type (`Int`, `Float`, `String`, `Duration`, or `Instant`). |  |
| `bynk.types.lambda_mismatch` | A lambda's parameter count, parameter annotations, or body type do not match the expected function type. | [`lambda_expr`](/book/reference/grammar/#rule-lambda_expr) |
| `bynk.types.let_annotation_mismatch` | A `let` value does not match its type annotation. | [`let_stmt`](/book/reference/grammar/#rule-let_stmt) |
| `bynk.types.list_element_mismatch` | A list-literal element has a different type from the list's element type. | [`list_literal`](/book/reference/grammar/#rule-list_literal) |
| `bynk.types.match_arm_mismatch` | A `match` arm has a different type from the others. | [`match_arm`](/book/reference/grammar/#rule-match_arm) |
| `bynk.types.match_non_sum_discriminant` | `match` was applied to a value that is not a sum type. | [`match_expr`](/book/reference/grammar/#rule-match_expr) |
| `bynk.types.method_arity` | A method was called with the wrong number of arguments. | [`method_call`](/book/reference/grammar/#rule-method_call) |
| `bynk.types.method_not_found` | Called a method the type does not have. | [`method_call`](/book/reference/grammar/#rule-method_call) |
| `bynk.types.method_on_non_named_type` | A method was called on a built-in type that has no methods. | [`method_call`](/book/reference/grammar/#rule-method_call) |
| `bynk.types.mixed_pattern_bindings` | A pattern mixes named and positional bindings. | [`variant_pattern`](/book/reference/grammar/#rule-variant_pattern) |
| `bynk.types.negative_length` | A length predicate was given a negative value. | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.no_numeric_coercion` | `Int` and `Float` were mixed without an explicit conversion — in an operation or in refinement bounds. | [`binary_expr`](/book/reference/grammar/#rule-binary_expr), [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.non_exhaustive_match` | A `match` does not cover every variant. | [`match_expr`](/book/reference/grammar/#rule-match_expr) |
| `bynk.types.ok_value_mismatch` | An `Ok` payload has the wrong type. | [`ok_expr`](/book/reference/grammar/#rule-ok_expr) |
| `bynk.types.opaque_raw_outside` | `.raw` on an opaque type was used outside its defining commons. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.types.opaque_record_construction` | An opaque type was constructed with record syntax. | [`record_construction`](/book/reference/grammar/#rule-record_construction) |
| `bynk.types.opaque_unsafe_outside` | `.unsafe` on an opaque type was used outside its defining context. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.types.pattern_arity` | A pattern binds the wrong number of payload fields. | [`variant_pattern`](/book/reference/grammar/#rule-variant_pattern) |
| `bynk.types.pattern_type_mismatch` | A pattern's type does not match the matched value. | [`variant_pattern`](/book/reference/grammar/#rule-variant_pattern) |
| `bynk.types.predicate_base_mismatch` | A predicate does not apply to the type's base (e.g. a string predicate on an `Int`). | [`refinement`](/book/reference/grammar/#rule-refinement) |
| `bynk.types.query_at_boundary` | A `Query` type appears in a storable or boundary-crossing position — a query is built and executed in place, never persisted or sent (ADR 0115). |  |
| `bynk.types.question_error_mismatch` | `?` propagates an error type incompatible with the function's. | [`question_expr`](/book/reference/grammar/#rule-question_expr) |
| `bynk.types.question_on_non_result` | `?` was applied to a non-`Result` value. | [`question_expr`](/book/reference/grammar/#rule-question_expr) |
| `bynk.types.question_option_outside_http` | `?` lifts an `Option` only inside a handler returning `HttpResult` (`None` becomes `NotFound`); elsewhere use `.okOr(err)`. | [`question_expr`](/book/reference/grammar/#rule-question_expr) |
| `bynk.types.question_outside_result` | `?` used in a function that does not return a `Result`. | [`question_expr`](/book/reference/grammar/#rule-question_expr) |
| `bynk.types.return_mismatch` | A returned value does not match the declared return type. |  |
| `bynk.types.some_value_mismatch` | A `Some` payload has the wrong type. | [`some_expr`](/book/reference/grammar/#rule-some_expr) |
| `bynk.types.stream_at_boundary` | A `Stream` type appears in a storable or boundary-crossing position — a stream is a live value-over-time source, never persisted or sent across a boundary (real-time track slice 0). |  |
| `bynk.types.stream_not_comparable` | A `Stream` value is compared with `==`/`!=` — a stream is a live value-over-time source, not a comparable value (real-time track slice 0). |  |
| `bynk.types.type_mismatch` | Two types that were required to match did not. |  |
| `bynk.types.uninferable_element_type` | An empty `[]` (or `List.empty()` / `Map.empty()`) has no expected type to infer its element type from. | [`list_literal`](/book/reference/grammar/#rule-list_literal) |
| `bynk.types.unkeyable_distinct` | A `distinct`/`distinctBy` element or key is not value-keyable (`String`, `Int`, or a refined/opaque type over them). |  |
| `bynk.types.unkeyable_map_key` | A `Map` key type is not value-keyable (`String`, `Int`, or a refined/opaque type over them). | [`generic_type_ref`](/book/reference/grammar/#rule-generic_type_ref) |
| `bynk.types.unknown_field` | Referenced a field the record type does not declare. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.types.unknown_pattern_field` | A pattern names a field the variant does not have. | [`variant_pattern`](/book/reference/grammar/#rule-variant_pattern) |
| `bynk.types.unknown_static_member` | Referenced an unknown static member on a type. | [`field_access`](/book/reference/grammar/#rule-field_access) |
| `bynk.types.unknown_variant_in_pattern` | A pattern names a variant the sum type does not have. | [`variant_pattern`](/book/reference/grammar/#rule-variant_pattern) |
| `bynk.types.unreachable_arm` | A `match` arm is unreachable. | [`match_arm`](/book/reference/grammar/#rule-match_arm) |
| `bynk.types.variant_arity` | A variant constructor got the wrong number of payload values. |  |
| `bynk.types.variant_missing_payload` | A variant requiring a payload was used without one. |  |
| `bynk.types.variant_payload_mismatch` | A variant payload has the wrong type. |  |

## Uses

| Code | Summary | Construct |
|---|---|---|
| `bynk.uses.name_conflict` | A `uses` name collides with another name. | [`uses_decl`](/book/reference/grammar/#rule-uses_decl) |
| `bynk.uses.self_reference` | A commons `uses` itself. | [`uses_decl`](/book/reference/grammar/#rule-uses_decl) |
| `bynk.uses.target_is_context` | `uses` targets a context instead of a commons. | [`uses_decl`](/book/reference/grammar/#rule-uses_decl) |
| `bynk.uses.unknown_commons` | `uses` names a commons that does not exist. | [`uses_decl`](/book/reference/grammar/#rule-uses_decl) |

## Value fabrication

| Code | Summary | Construct |
|---|---|---|
| `bynk.val.agent_not_generable` | A `for all`/`Val` cannot generate an agent — fabricated agent states need not be reachable. | [`for_all`](/book/reference/grammar/#rule-for_all) |
| `bynk.val.arity` | `Val[T]` was given the wrong number of pin arguments. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.literal_violates` | A pinned `Val[T]` value violates the type's refinement. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.needs_pin` | A bare `Val[T]` cannot generate a value (e.g. a `Matches` string); pin one. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.outside_test` | `Val[T]` was used outside a test case body. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.pin_not_literal` | A `Val[T]` pin argument is not a compile-time literal. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.pin_unsupported` | A pin was given for a type kind that does not support pinning. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.unknown_type` | `Val[T]` names a type that does not resolve. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
| `bynk.val.unsupported_kind` | `Val[T]` cannot fabricate a value for this kind of type. | [`val_expr`](/book/reference/grammar/#rule-val_expr) |
