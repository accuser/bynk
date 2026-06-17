# Diagnostic index

<!-- GENERATED FILE — do not edit by hand.
     Source: karnc/src/diagnostics.rs (`render_markdown`).
     Regenerate with: KARN_BLESS=1 cargo test -p karnc --test diagnostics_registry -->

Every diagnostic code the compiler can emit, with a one-line summary of the cause, grouped by category. For step-by-step cause-and-fix guidance on the most common ones, see the [troubleshooting guides](../troubleshooting/index.md).

There are **277** codes in total.

## Agents

| Code | Summary | Construct |
|---|---|---|
| `karn.agent.construction_arity` | An agent was constructed with the wrong number of key arguments. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agent.handler_arity` | An agent handler was called with the wrong number of arguments. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agent.handler_not_found` | Called a handler the agent does not declare. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agent.key_mismatch` | An agent key argument has the wrong type. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agent.outside_context` | An `agent` was declared outside a context. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agent.return_not_effect` | An agent handler's return type is not an `Effect`. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.agents.bad_state_initialiser` | An agent state-field initialiser is not a static value of the field's type. | [`state_decl`](grammar.md#rule-state_decl) |
| `karn.agents.non_zeroable_state_field` | An agent state field has no initialiser and no implicit zero value. | [`state_decl`](grammar.md#rule-state_decl) |

## Assertions

| Code | Summary | Construct |
|---|---|---|
| `karn.assert.non_bool` | `assert` was given a non-`Bool` expression. | [`assert_expr`](grammar.md#rule-assert_expr) |
| `karn.assert.outside_test` | `assert` was used outside a test case body. | [`assert_expr`](grammar.md#rule-assert_expr) |

## Boundaries

| Code | Summary | Construct |
|---|---|---|
| `karn.boundary.structural_mismatch` | Data crossing a context boundary did not match the expected shape. |  |

## Capabilities

| Code | Summary | Construct |
|---|---|---|
| `karn.capability.op_arity` | A capability operation was called with the wrong number of arguments. | [`capability_decl`](grammar.md#rule-capability_decl) |
| `karn.capability.outside_context` | A `capability` was declared outside a context. | [`capability_decl`](grammar.md#rule-capability_decl) |
| `karn.capability.unknown_operation` | Referenced an operation the capability does not declare. | [`capability_decl`](grammar.md#rule-capability_decl) |

## Commit

| Code | Summary | Construct |
|---|---|---|
| `karn.commit.outside_agent` | `commit` was used outside an agent handler. | [`commit_stmt`](grammar.md#rule-commit_stmt) |
| `karn.commit.two_reachable_commits` | Two `commit` statements are reachable on the same execution path. | [`commit_stmt`](grammar.md#rule-commit_stmt) |
| `karn.commit.wrong_state_type` | A `commit` value does not match the agent's state type. | [`commit_stmt`](grammar.md#rule-commit_stmt) |

## Consumes

| Code | Summary | Construct |
|---|---|---|
| `karn.consumes.alias_conflict` | Two `consumes` aliases collide. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.capability_name_clash` | Two flattened `consumes U { Cap }` capabilities collide, or one clashes with a local capability. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.in_commons` | `consumes` appears in a `commons` (it is only valid in a context). | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.name_conflict` | A `consumes` name collides with another name in scope. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.self_reference` | A context `consumes` itself. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.service_arity` | A consumed service was called with the wrong number of arguments. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.target_is_commons` | `consumes` targets a `commons` instead of a context. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.unknown_context` | `consumes` names a context that does not exist. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.consumes.unknown_service` | Called a service the consumed context does not declare. | [`consumes_decl`](grammar.md#rule-consumes_decl) |

## Contexts

| Code | Summary | Construct |
|---|---|---|
| `karn.context.consumes_cycle` | Contexts form a `consumes` dependency cycle. |  |
| `karn.context.external_construction` | A context-owned type was constructed from outside that context. |  |
| `karn.context.external_provider` | A bodiless (external) provider was declared outside an `adapter`. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.context.opaque_inspection` | An opaquely-exported type was inspected from outside its context. |  |

## Cron

| Code | Summary | Construct |
|---|---|---|
| `karn.cron.bad_params` | A cron handler declares more than one parameter, or a non-`Int` one. | [`cron_handler`](grammar.md#rule-cron_handler) |
| `karn.cron.duplicate_schedule` | Two cron handlers declare the same schedule. | [`cron_handler`](grammar.md#rule-cron_handler) |
| `karn.cron.invalid_schedule` | A cron expression is not five whitespace-separated fields. | [`cron_handler`](grammar.md#rule-cron_handler) |
| `karn.cron.return_not_effect_result` | A cron handler does not return `Effect[Result[(), E]]`. | [`cron_handler`](grammar.md#rule-cron_handler) |

## Effects

| Code | Summary | Construct |
|---|---|---|
| `karn.effect.bind_in_pure_context` | An `<-` bind was used in a pure (non-effectful) context. | [`effect_let_stmt`](grammar.md#rule-effect_let_stmt) |
| `karn.effect.bind_on_non_effect` | An `<-` bind was applied to a non-`Effect` value. | [`effect_let_stmt`](grammar.md#rule-effect_let_stmt) |
| `karn.effect.capability_in_pure_context` | A capability was used in a pure context. |  |
| `karn.effect.cross_context_in_pure_context` | A cross-context call was made in a pure context. |  |
| `karn.effect.fn_value_in_pure_context` | An effectful function value was called in a pure context; like a capability call, it is legal only where the enclosing body is effectful. | [`call`](grammar.md#rule-call) |

## Exports

| Code | Summary | Construct |
|---|---|---|
| `karn.exports.capability_not_provided` | An exported capability has no provider in its context. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.exports.conflicting_visibility` | A type is exported with conflicting visibilities. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.exports.duplicate_export` | The same name is exported more than once. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.exports.duplicate_in_clause` | A name appears twice in one `exports` clause. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.exports.undeclared_capability` | `exports capability` names a capability that is not declared. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.exports.undeclared_type` | `exports` names a type that is not declared. | [`exports_decl`](grammar.md#rule-exports_decl) |

## Given capabilities

| Code | Summary | Construct |
|---|---|---|
| `karn.given.cross_context_unknown_capability` | `given B.Cap` names a capability the consumed context does not export. | [`given_clause`](grammar.md#rule-given_clause) |
| `karn.given.undeclared_capability` | A handler uses a capability it did not declare with `given`. | [`given_clause`](grammar.md#rule-given_clause) |
| `karn.given.unknown_capability` | `given` names a capability that does not exist. | [`given_clause`](grammar.md#rule-given_clause) |
| `karn.given.unused_capability` | A `given` capability is never used (warning). | [`given_clause`](grammar.md#rule-given_clause) |

## HTTP

| Code | Summary | Construct |
|---|---|---|
| `karn.http.body_on_get_or_delete` | A GET or DELETE handler declares a `body` parameter. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.duplicate_route` | Two handlers share the same method and route. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.extra_param` | A handler parameter is neither a path parameter nor `body`. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.invalid_path` | An HTTP route path is malformed. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.path_param_not_stringy` | A path parameter's type is not constructible from a string. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.reserved_prefix` | A route uses the reserved `/_karn/` prefix. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.return_not_effect_http_result` | An HTTP handler does not return `Effect[HttpResult[T]]`. | [`http_handler`](grammar.md#rule-http_handler) |
| `karn.http.unbound_path_param` | A `:name` route segment has no matching handler parameter. | [`http_handler`](grammar.md#rule-http_handler) |

## Lexer

| Code | Summary | Construct |
|---|---|---|
| `karn.lex.bad_escape` | An invalid escape sequence in a string literal. | [`string_literal`](grammar.md#rule-string_literal) |
| `karn.lex.float_literal_overflow` | A float literal does not fit a finite 64-bit float. | [`float_literal`](grammar.md#rule-float_literal) |
| `karn.lex.integer_overflow` | An integer literal is out of range. | [`number_literal`](grammar.md#rule-number_literal) |
| `karn.lex.unclosed_doc_block` | A documentation block is not closed. |  |
| `karn.lex.unexpected_character` | An unexpected character in the source. |  |
| `karn.lex.unterminated_interpolation` | An interpolation hole `\(…)` is not closed on its line. | [`string_literal`](grammar.md#rule-string_literal) |
| `karn.lex.unterminated_string` | A string literal is not terminated. | [`string_literal`](grammar.md#rule-string_literal) |

## Mock and mocks

| Code | Summary | Construct |
|---|---|---|
| `karn.mock.arity` | `Mock[T]` was given the wrong number of pin arguments. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.duplicate_target` | A `mocks` target is declared more than once. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.mock.in_commons_test` | `mocks` used in a commons test, where there is no dependency to inject. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.mock.literal_violates` | A pinned `Mock[T]` value violates the type's refinement. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.needs_pin` | A bare `Mock[T]` cannot generate a value (e.g. a `Matches` string); pin one. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.outside_test` | `Mock[T]` was used outside a test case body. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.pin_not_literal` | A `Mock[T]` pin argument is not a compile-time literal. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.pin_unsupported` | A pin was given for a type kind that does not support pinning. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.signature_mismatch` | A `mocks` implementation's signature does not match the capability. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.mock.unknown_target` | `mocks` names a capability that is not in scope. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.mock.unknown_type` | `Mock[T]` names a type that does not resolve. | [`mock_expr`](grammar.md#rule-mock_expr) |
| `karn.mock.unsupported_kind` | `Mock[T]` cannot fabricate a value for this kind of type. | [`mock_expr`](grammar.md#rule-mock_expr) |

## Other

| Code | Summary | Construct |
|---|---|---|
| `karn.actor.bearer_identity_not_string_constructible` | A `Bearer` actor's identity is not a string-constructible type. |  |
| `karn.actor.bearer_missing_secret` | A `Bearer` actor does not name its signing secret. |  |
| `karn.actor.binder_shadows_param` | A `by` actor binder collides with a handler parameter of the same name. |  |
| `karn.actor.duplicate_sum_scheme` | Two peers in a multi-actor sum share an authentication scheme. |  |
| `karn.actor.identity_not_sealed` | An actor identity type is not a context-ownable (sealed) value type. |  |
| `karn.actor.missing_by_on_http` | An HTTP handler lacks the required `by` actor clause. |  |
| `karn.actor.outside_context` | An `actor` was declared outside a context (e.g. in a commons). |  |
| `karn.actor.refinement_base_unsupported` | A refinement actor's base is not a `Bearer` actor (no claims to authorise against). |  |
| `karn.actor.refinement_in_sum` | A refinement actor appears as a member of a multi-actor sum. |  |
| `karn.actor.refinement_predicate_unsupported` | A refinement actor's `where` predicate is outside the closed claim-predicate set. |  |
| `karn.actor.scheme_not_admissible` | An actor's scheme is not admissible on this handler's protocol. |  |
| `karn.actor.signature_identity_unsupported` | A `Signature` actor declared an `identity`, which is not yet supported. |  |
| `karn.actor.signature_missing_header` | A `Signature` actor does not name its signature header. |  |
| `karn.actor.signature_missing_secret` | A `Signature` actor does not name its signing secret. |  |
| `karn.actor.signature_requires_body` | A `Signature` handler does not take a `body` parameter. |  |
| `karn.actor.signature_tolerance_without_timestamp` | A `Signature` actor set `tolerance` without a `timestamp` header. |  |
| `karn.actor.sum_requires_binder` | A multi-actor sum `by` clause has no binder to match the resolved actor. |  |
| `karn.actor.unknown_actor` | A handler's `by` clause names an actor that is not declared. |  |
| `karn.actor.unknown_scheme` | An actor declares an authentication scheme that is not compiler-known. |  |
| `karn.actor.unreachable_sum_arm` | A multi-actor sum has an arm unreachable after a catch-all (`None`) peer. |  |
| `karn.adapter.consumes_context` | An `adapter` consumed a context; adapter dependencies are adapter-to-adapter. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.adapter.consumes_requires_selection` | An `adapter` used a whole-unit or aliased `consumes`; adapters must select capabilities with `consumes U { Cap, … }`. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.adapter.disallowed_item` | An `adapter` declared a `service`, `agent`, or other item it may not contain. | [`adapter_decl`](grammar.md#rule-adapter_decl) |
| `karn.adapter.duplicate_binding` | An `adapter` declared more than one `binding` clause. | [`binding_decl`](grammar.md#rule-binding_decl) |
| `karn.adapter.no_binding` | An `adapter` declares an external provider but no `binding` module to supply it. | [`adapter_decl`](grammar.md#rule-adapter_decl) |
| `karn.adapter.provider_has_body` | A provider inside an `adapter` has a Karn body; adapter providers must be external. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.generics.no_bounds` | A type parameter carries a bound (`[A: …]`); bounded generics are not in v0.20a. | [`fn_decl`](grammar.md#rule-fn_decl) |
| `karn.generics.no_generic_types` | A `type` declaration carries a type-parameter list; generic type declarations are not in v0.20a (type parameters belong to functions). | [`type_decl`](grammar.md#rule-type_decl) |
| `karn.generics.type_arg_mismatch` | Inferred or explicit type arguments conflict, have the wrong arity, target a non-generic function, or a type parameter shadows a declared type. | [`call`](grammar.md#rule-call) |
| `karn.generics.uninferable_type_arg` | A generic function's type parameter could not be inferred from the arguments and was not given explicitly (`name[T](…)`); a bare generic function also cannot be passed as a value in v0.20a. | [`call`](grammar.md#rule-call) |
| `karn.integration.duplicate_participant` | A context is listed more than once in a `wires` clause. | [`wires_decl`](grammar.md#rule-wires_decl) |
| `karn.integration.duplicate_suite` | Two integration tests share the same suite name. | [`integration_decl`](grammar.md#rule-integration_decl) |
| `karn.integration.mock_in_integration` | `mocks` is not allowed in an integration test. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.integration.too_few_participants` | An integration test wires fewer than two contexts. | [`wires_decl`](grammar.md#rule-wires_decl) |
| `karn.integration.unknown_participant` | A `wires` clause names something that is not a declared context. | [`wires_decl`](grammar.md#rule-wires_decl) |
| `karn.integration.unwired_dependency` | A participant consumes a context that is not wired into the integration test. | [`integration_decl`](grammar.md#rule-integration_decl) |
| `karn.lambda.unannotated_param` | A lambda parameter has no type annotation in a position where no function type is expected to infer it from. | [`lambda_expr`](grammar.md#rule-lambda_expr) |
| `karn.namespace.reserved` | A user unit is named `karn` or `karn.*`; the `karn` root is reserved for the toolchain. |  |
| `karn.requires.unpinned_dependency` | An adapter `binding … requires { … }` entry has an unpinned version range. | [`binding_decl`](grammar.md#rule-binding_decl) |
| `karn.target.vendor_conflict` | One deployment unit's in-process closure uses platform-native capabilities from two mutually-exclusive platforms. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.target.vendor_required` | A deployment unit uses a platform-native capability but the build selects another `--platform`. | [`consumes_decl`](grammar.md#rule-consumes_decl) |

## Parser

| Code | Summary | Construct |
|---|---|---|
| `karn.parse.consumes_after_decls` | `consumes` appears after other declarations. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.parse.empty_agent` | An `agent` body is empty. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.parse.empty_capability` | A `capability` body is empty. | [`capability_decl`](grammar.md#rule-capability_decl) |
| `karn.parse.empty_interpolation` | An interpolation hole `\(…)` contains no expression. |  |
| `karn.parse.empty_match` | A `match` has no arms. | [`match_expr`](grammar.md#rule-match_expr) |
| `karn.parse.empty_mock_body` | A `mocks` body is empty. | [`mocks_decl`](grammar.md#rule-mocks_decl) |
| `karn.parse.empty_service` | A `service` body is empty. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.parse.expected_agent_key` | Expected a `key` declaration in an agent. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.parse.expected_base_type` | Expected a base type. | [`base_type`](grammar.md#rule-base_type) |
| `karn.parse.expected_capability_op` | Expected a capability operation. | [`capability_op`](grammar.md#rule-capability_op) |
| `karn.parse.expected_expression` | Expected an expression. |  |
| `karn.parse.expected_handler` | Expected a handler. | [`handler`](grammar.md#rule-handler) |
| `karn.parse.expected_item` | Expected a declaration. |  |
| `karn.parse.expected_predicate` | Expected a refinement predicate. | [`refinement`](grammar.md#rule-refinement) |
| `karn.parse.expected_provider_op` | Expected a provider operation. | [`provider_op`](grammar.md#rule-provider_op) |
| `karn.parse.expected_token` | Expected a specific token. |  |
| `karn.parse.expected_type` | Expected a type. |  |
| `karn.parse.expected_unit_header` | Expected a `commons` or `context` header. |  |
| `karn.parse.expected_visibility` | Expected a visibility keyword. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.parse.exports_after_decls` | `exports` appears after other declarations. | [`exports_decl`](grammar.md#rule-exports_decl) |
| `karn.parse.extra_tokens` | Unexpected tokens after an otherwise complete construct. |  |
| `karn.parse.generic_arg_count` | Wrong number of generic type arguments. | [`generic_type_ref`](grammar.md#rule-generic_type_ref) |
| `karn.parse.handler_in_agent` | A protocol handler (`on GET`/`schedule`/`message`) was declared in an agent. | [`handler`](grammar.md#rule-handler) |
| `karn.parse.malformed_float_literal` | A float literal is missing a digit on one side of the `.` (`1.`, `.5`). | [`float_literal`](grammar.md#rule-float_literal) |
| `karn.parse.non_associative` | A non-associative operator was chained (e.g. `a == b == c`). | [`binary_expr`](grammar.md#rule-binary_expr) |
| `karn.parse.orphan_doc_block` | A documentation block is not attached to a declaration (warning). |  |
| `karn.parse.reserved_keyword` | A reserved keyword was used as an identifier. | [`identifier`](grammar.md#rule-identifier) |
| `karn.parse.self_outside_method` | `self` used outside a method or handler. | [`self_expr`](grammar.md#rule-self_expr) |
| `karn.parse.unexpected_adapter` | An `adapter` appeared where it is not allowed. |  |
| `karn.parse.unexpected_context` | A `context` appeared where it is not allowed. | [`context_decl`](grammar.md#rule-context_decl) |
| `karn.parse.unexpected_eof` | Unexpected end of input. |  |
| `karn.parse.unexpected_test` | A `test` appeared where it is not allowed. | [`test_decl`](grammar.md#rule-test_decl) |
| `karn.parse.unknown_effect_method` | An unknown method on `Effect`. |  |
| `karn.parse.unknown_handler_kind` | An unknown handler form (expected `call`, an HTTP method, `schedule`, or `message`). | [`handler`](grammar.md#rule-handler) |
| `karn.parse.unknown_predicate` | An unknown refinement predicate. | [`predicate_name`](grammar.md#rule-predicate_name) |
| `karn.parse.uses_after_decls` | `uses` appears after other declarations. | [`uses_decl`](grammar.md#rule-uses_decl) |

## Project

| Code | Summary | Construct |
|---|---|---|
| `karn.project.file_and_directory` | A unit exists as both a file and a directory. |  |
| `karn.project.inconsistent_commons_name` | A source file's path does not match its declared name. |  |
| `karn.project.inconsistent_test_path` | A test file's path does not match its target's name. |  |
| `karn.project.kind_conflict` | A name is declared as both a commons and a context. |  |
| `karn.project.no_root` | No project root could be determined. |  |
| `karn.project.no_sources` | The project contains no source files. |  |
| `karn.project.read_failed` | A source file could not be read. |  |

## Providers

| Code | Summary | Construct |
|---|---|---|
| `karn.provider.dependency_cycle` | Providers form a capability dependency cycle through `given`. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.provider.extra_operation` | A `provides` block implements an operation not in the capability. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.provider.missing_operation` | A `provides` block is missing a capability operation. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.provider.outside_context` | `provides` was declared outside a context. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.provider.signature_mismatch` | A `provides` operation's signature does not match the capability. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.provider.unknown_capability` | `provides` names a capability that does not exist. | [`provider_decl`](grammar.md#rule-provider_decl) |

## Queue

| Code | Summary | Construct |
|---|---|---|
| `karn.queue.bad_params` | An `on message` handler does not take exactly one `message` parameter. | [`queue_handler`](grammar.md#rule-queue_handler) |
| `karn.queue.duplicate_consumer` | Two `on message` handlers consume the same queue. | [`queue_handler`](grammar.md#rule-queue_handler) |
| `karn.queue.invalid_name` | A `from queue("…")` binding has an empty queue name. | [`queue_handler`](grammar.md#rule-queue_handler) |
| `karn.queue.return_not_queue_result` | An `on message` handler does not return `Effect[QueueResult]`. | [`handler`](grammar.md#rule-handler) |

## Record spread

| Code | Summary | Construct |
|---|---|---|
| `karn.record_spread.field_type_mismatch` | A record-spread override has the wrong type for the field. | [`record_spread`](grammar.md#rule-record_spread) |
| `karn.record_spread.non_record_base` | The base of a record spread is not a record. | [`record_spread`](grammar.md#rule-record_spread) |
| `karn.record_spread.type_mismatch` | A record spread's base is a different record type. | [`record_spread`](grammar.md#rule-record_spread) |
| `karn.record_spread.unknown_field` | A record spread overrides a field the record does not have. | [`record_spread`](grammar.md#rule-record_spread) |

## Refinement

| Code | Summary | Construct |
|---|---|---|
| `karn.refine.literal_violates` | A literal does not satisfy the refined type's predicate. | [`refined_type`](grammar.md#rule-refined_type) |

## Resolution

| Code | Summary | Construct |
|---|---|---|
| `karn.resolve.ambiguous_variant` | A variant name is ambiguous across several sum types. |  |
| `karn.resolve.arity_mismatch` | A function was called with the wrong number of arguments. | [`call`](grammar.md#rule-call) |
| `karn.resolve.duplicate_actor` | Two actors share a name. |  |
| `karn.resolve.duplicate_agent` | Two agents share a name. | [`agent_decl`](grammar.md#rule-agent_decl) |
| `karn.resolve.duplicate_capability` | Two capabilities share a name. | [`capability_decl`](grammar.md#rule-capability_decl) |
| `karn.resolve.duplicate_field` | A record declares a field twice. | [`record_type`](grammar.md#rule-record_type) |
| `karn.resolve.duplicate_field_init` | A record construction initialises a field twice. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.resolve.duplicate_fn` | Two functions share a name. | [`fn_decl`](grammar.md#rule-fn_decl) |
| `karn.resolve.duplicate_method` | Two methods share a name. | [`fn_decl`](grammar.md#rule-fn_decl) |
| `karn.resolve.duplicate_param` | A parameter name is repeated. | [`param`](grammar.md#rule-param) |
| `karn.resolve.duplicate_provider` | A capability is provided more than once. | [`provider_decl`](grammar.md#rule-provider_decl) |
| `karn.resolve.duplicate_service` | Two services share a name. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.resolve.duplicate_type` | Two types share a name. | [`type_decl`](grammar.md#rule-type_decl) |
| `karn.resolve.duplicate_variant` | A sum type declares a variant twice. | [`sum_type`](grammar.md#rule-sum_type) |
| `karn.resolve.fn_without_call` | A function was referenced without being called. |  |
| `karn.resolve.let_shadows_fn` | A `let` binding shadows a function. | [`let_stmt`](grammar.md#rule-let_stmt) |
| `karn.resolve.let_shadows_type` | A `let` binding shadows a type. | [`let_stmt`](grammar.md#rule-let_stmt) |
| `karn.resolve.method_unknown_type` | A method is defined on an unknown type. |  |
| `karn.resolve.missing_field` | A record construction omits a required field. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.resolve.name_conflict` | Two declarations share a name. |  |
| `karn.resolve.not_a_record_type` | Record syntax was used on a non-record type. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.resolve.opaque_record_construction` | An opaque type was constructed with record syntax. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.resolve.param_as_function` | A value (such as a parameter) was called as a function. | [`call`](grammar.md#rule-call) |
| `karn.resolve.recursive_record_field` | A record directly contains a field of its own type. | [`record_type`](grammar.md#rule-record_type) |
| `karn.resolve.self_outside_method` | `self` referenced outside a method or handler. | [`self_expr`](grammar.md#rule-self_expr) |
| `karn.resolve.type_as_function` | A type name was called as if it were a function. | [`call`](grammar.md#rule-call) |
| `karn.resolve.type_in_expr` | A type name was used where a value is expected. |  |
| `karn.resolve.unconsumed_context` | A context's service was called without a `consumes` declaration. | [`consumes_decl`](grammar.md#rule-consumes_decl) |
| `karn.resolve.unknown_field` | Accessed a field the record does not have. | [`field_access`](grammar.md#rule-field_access) |
| `karn.resolve.unknown_function` | Called a function that does not exist. | [`call`](grammar.md#rule-call) |
| `karn.resolve.unknown_name` | Referenced a name that is not in scope. |  |
| `karn.resolve.unknown_static_member` | Referenced an unknown static member (e.g. `T.x`). | [`field_access`](grammar.md#rule-field_access) |
| `karn.resolve.unknown_type` | Referenced a type that does not exist. |  |

## Services

| Code | Summary | Construct |
|---|---|---|
| `karn.service.missing_from` | A `from`-less service has a handler other than `on call`. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.service.mixed_protocols` | A service mixes handler forms that do not match its `from <protocol>`. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.service.outside_context` | A `service` was declared outside a context. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.service.return_not_effect` | A service handler's return type is not an `Effect`. | [`service_decl`](grammar.md#rule-service_decl) |
| `karn.service.unknown_protocol` | A `from <protocol>` names an unknown protocol (e.g. a transport like Kafka). | [`service_decl`](grammar.md#rule-service_decl) |

## Tests

| Code | Summary | Construct |
|---|---|---|
| `karn.test.duplicate_case_name` | Two test cases share a description. | [`test_case`](grammar.md#rule-test_case) |
| `karn.test.unknown_target` | A `test` block targets a unit that does not exist. | [`test_decl`](grammar.md#rule-test_decl) |

## Type checking

| Code | Summary | Construct |
|---|---|---|
| `karn.types.ambiguous_constructor` | `Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it. |  |
| `karn.types.argument_mismatch` | A function argument has the wrong type. | [`call`](grammar.md#rule-call) |
| `karn.types.call_arity` | A function value was applied with the wrong number of arguments. | [`call`](grammar.md#rule-call) |
| `karn.types.cannot_infer_option_type_param` | The value type of `None` could not be inferred. | [`none_expr`](grammar.md#rule-none_expr) |
| `karn.types.cannot_infer_result_type_params` | The type parameters of a `Result` could not be inferred. |  |
| `karn.types.constructor_arity` | A variant constructor got the wrong number of arguments. |  |
| `karn.types.constructor_base_mismatch` | A `.of` constructor was given an argument of the wrong base type. |  |
| `karn.types.duplicate_variant_arm` | A `match` has two arms for the same variant. | [`match_arm`](grammar.md#rule-match_arm) |
| `karn.types.empty_refinement` | A refinement admits no values (contradictory predicates). | [`refinement`](grammar.md#rule-refinement) |
| `karn.types.err_value_mismatch` | An `Err` payload has the wrong type. | [`err_expr`](grammar.md#rule-err_expr) |
| `karn.types.field_access_on_non_record` | Field access on a value that is not a record. | [`field_access`](grammar.md#rule-field_access) |
| `karn.types.field_refinement_not_base` | An inline field refinement requires a base or refined type. | [`record_field`](grammar.md#rule-record_field) |
| `karn.types.field_value_mismatch` | A record field was given a value of the wrong type. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.types.function_at_boundary` | A function type appeared in a serialisable or boundary position (a record field, sum payload, service/agent handler signature, capability operation signature, agent state field, or agent key); functions cannot serialise or cross a boundary. | [`function_type_ref`](grammar.md#rule-function_type_ref) |
| `karn.types.if_branch_mismatch` | The branches of an `if` have different types. | [`if_expr`](grammar.md#rule-if_expr) |
| `karn.types.if_non_bool_cond` | An `if` condition is not a `Bool`. | [`if_expr`](grammar.md#rule-if_expr) |
| `karn.types.interpolation_non_scalar` | An interpolation hole holds a value with no string form. |  |
| `karn.types.invalid_regex` | A `Matches` predicate contains an invalid regular expression. | [`refinement`](grammar.md#rule-refinement) |
| `karn.types.inverted_range` | An `InRange` predicate has its bounds inverted. | [`refinement`](grammar.md#rule-refinement) |
| `karn.types.is_base_mismatch` | An `is` refinement check is applied to a value of the wrong base type. | [`is_expr`](grammar.md#rule-is_expr) |
| `karn.types.is_non_sum` | `is` was applied to a value that is not a sum type. | [`is_expr`](grammar.md#rule-is_expr) |
| `karn.types.is_unknown_variant` | `is` names a variant the type does not have. | [`is_expr`](grammar.md#rule-is_expr) |
| `karn.types.json_uncodable` | A `Json.encode`/`Json.decode` target type cannot pass through the typed JSON codec (functions, effects, error builtins). | [`method_call`](grammar.md#rule-method_call) |
| `karn.types.lambda_mismatch` | A lambda's parameter count, parameter annotations, or body type do not match the expected function type. | [`lambda_expr`](grammar.md#rule-lambda_expr) |
| `karn.types.let_annotation_mismatch` | A `let` value does not match its type annotation. | [`let_stmt`](grammar.md#rule-let_stmt) |
| `karn.types.list_element_mismatch` | A list-literal element has a different type from the list's element type. | [`list_literal`](grammar.md#rule-list_literal) |
| `karn.types.match_arm_mismatch` | A `match` arm has a different type from the others. | [`match_arm`](grammar.md#rule-match_arm) |
| `karn.types.match_non_sum_discriminant` | `match` was applied to a value that is not a sum type. | [`match_expr`](grammar.md#rule-match_expr) |
| `karn.types.method_arity` | A method was called with the wrong number of arguments. | [`method_call`](grammar.md#rule-method_call) |
| `karn.types.method_not_found` | Called a method the type does not have. | [`method_call`](grammar.md#rule-method_call) |
| `karn.types.method_on_non_named_type` | A method was called on a built-in type that has no methods. | [`method_call`](grammar.md#rule-method_call) |
| `karn.types.mixed_pattern_bindings` | A pattern mixes named and positional bindings. | [`variant_pattern`](grammar.md#rule-variant_pattern) |
| `karn.types.negative_length` | A length predicate was given a negative value. | [`refinement`](grammar.md#rule-refinement) |
| `karn.types.no_numeric_coercion` | `Int` and `Float` were mixed without an explicit conversion — in an operation or in refinement bounds. | [`binary_expr`](grammar.md#rule-binary_expr), [`refinement`](grammar.md#rule-refinement) |
| `karn.types.non_exhaustive_match` | A `match` does not cover every variant. | [`match_expr`](grammar.md#rule-match_expr) |
| `karn.types.ok_value_mismatch` | An `Ok` payload has the wrong type. | [`ok_expr`](grammar.md#rule-ok_expr) |
| `karn.types.opaque_raw_outside` | `.raw` on an opaque type was used outside its defining commons. | [`field_access`](grammar.md#rule-field_access) |
| `karn.types.opaque_record_construction` | An opaque type was constructed with record syntax. | [`record_construction`](grammar.md#rule-record_construction) |
| `karn.types.opaque_unsafe_outside` | `.unsafe` on an opaque type was used outside its defining context. | [`field_access`](grammar.md#rule-field_access) |
| `karn.types.pattern_arity` | A pattern binds the wrong number of payload fields. | [`variant_pattern`](grammar.md#rule-variant_pattern) |
| `karn.types.pattern_type_mismatch` | A pattern's type does not match the matched value. | [`variant_pattern`](grammar.md#rule-variant_pattern) |
| `karn.types.predicate_base_mismatch` | A predicate does not apply to the type's base (e.g. a string predicate on an `Int`). | [`refinement`](grammar.md#rule-refinement) |
| `karn.types.question_error_mismatch` | `?` propagates an error type incompatible with the function's. | [`question_expr`](grammar.md#rule-question_expr) |
| `karn.types.question_on_non_result` | `?` was applied to a non-`Result` value. | [`question_expr`](grammar.md#rule-question_expr) |
| `karn.types.question_outside_result` | `?` used in a function that does not return a `Result`. | [`question_expr`](grammar.md#rule-question_expr) |
| `karn.types.return_mismatch` | A returned value does not match the declared return type. |  |
| `karn.types.some_value_mismatch` | A `Some` payload has the wrong type. | [`some_expr`](grammar.md#rule-some_expr) |
| `karn.types.type_mismatch` | Two types that were required to match did not. |  |
| `karn.types.uninferable_element_type` | An empty `[]` (or `List.empty()` / `Map.empty()`) has no expected type to infer its element type from. | [`list_literal`](grammar.md#rule-list_literal) |
| `karn.types.unkeyable_map_key` | A `Map` key type is not value-keyable (`String`, `Int`, or a refined/opaque type over them). | [`generic_type_ref`](grammar.md#rule-generic_type_ref) |
| `karn.types.unknown_field` | Referenced a field the record type does not declare. | [`field_access`](grammar.md#rule-field_access) |
| `karn.types.unknown_pattern_field` | A pattern names a field the variant does not have. | [`variant_pattern`](grammar.md#rule-variant_pattern) |
| `karn.types.unknown_static_member` | Referenced an unknown static member on a type. | [`field_access`](grammar.md#rule-field_access) |
| `karn.types.unknown_variant_in_pattern` | A pattern names a variant the sum type does not have. | [`variant_pattern`](grammar.md#rule-variant_pattern) |
| `karn.types.unreachable_arm` | A `match` arm is unreachable. | [`match_arm`](grammar.md#rule-match_arm) |
| `karn.types.variant_arity` | A variant constructor got the wrong number of payload values. |  |
| `karn.types.variant_missing_payload` | A variant requiring a payload was used without one. |  |
| `karn.types.variant_payload_mismatch` | A variant payload has the wrong type. |  |

## Uses

| Code | Summary | Construct |
|---|---|---|
| `karn.uses.name_conflict` | A `uses` name collides with another name. | [`uses_decl`](grammar.md#rule-uses_decl) |
| `karn.uses.self_reference` | A commons `uses` itself. | [`uses_decl`](grammar.md#rule-uses_decl) |
| `karn.uses.target_is_context` | `uses` targets a context instead of a commons. | [`uses_decl`](grammar.md#rule-uses_decl) |
| `karn.uses.unknown_commons` | `uses` names a commons that does not exist. | [`uses_decl`](grammar.md#rule-uses_decl) |
