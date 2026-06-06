# Diagnostic index

<!-- GENERATED FILE — do not edit by hand.
     Source: karnc/src/diagnostics.rs (`render_markdown`).
     Regenerate with: KARN_BLESS=1 cargo test -p karnc --test diagnostics_registry -->

Every diagnostic code the compiler can emit, with a one-line summary of the cause, grouped by category. For step-by-step cause-and-fix guidance on the most common ones, see the [troubleshooting guides](../how-to/troubleshooting/index.md).

There are **216** codes in total.

## Agents

| Code | Summary |
|---|---|
| `karn.agent.construction_arity` | An agent was constructed with the wrong number of key arguments. |
| `karn.agent.handler_arity` | An agent handler was called with the wrong number of arguments. |
| `karn.agent.handler_not_found` | Called a handler the agent does not declare. |
| `karn.agent.key_mismatch` | An agent key argument has the wrong type. |
| `karn.agent.outside_context` | An `agent` was declared outside a context. |
| `karn.agent.return_not_effect` | An agent handler's return type is not an `Effect`. |
| `karn.agents.bad_state_initialiser` | An agent state-field initialiser is not a static value of the field's type. |
| `karn.agents.non_zeroable_state_field` | An agent state field has no initialiser and no implicit zero value. |

## Assertions

| Code | Summary |
|---|---|
| `karn.assert.non_bool` | `assert` was given a non-`Bool` expression. |
| `karn.assert.outside_test` | `assert` was used outside a test case body. |

## Boundaries

| Code | Summary |
|---|---|
| `karn.boundary.structural_mismatch` | Data crossing a context boundary did not match the expected shape. |

## Capabilities

| Code | Summary |
|---|---|
| `karn.capability.op_arity` | A capability operation was called with the wrong number of arguments. |
| `karn.capability.outside_context` | A `capability` was declared outside a context. |
| `karn.capability.unknown_operation` | Referenced an operation the capability does not declare. |

## Commit

| Code | Summary |
|---|---|
| `karn.commit.outside_agent` | `commit` was used outside an agent handler. |
| `karn.commit.two_reachable_commits` | Two `commit` statements are reachable on the same execution path. |
| `karn.commit.wrong_state_type` | A `commit` value does not match the agent's state type. |

## Consumes

| Code | Summary |
|---|---|
| `karn.consumes.alias_conflict` | Two `consumes` aliases collide. |
| `karn.consumes.in_commons` | `consumes` appears in a `commons` (it is only valid in a context). |
| `karn.consumes.name_conflict` | A `consumes` name collides with another name in scope. |
| `karn.consumes.self_reference` | A context `consumes` itself. |
| `karn.consumes.service_arity` | A consumed service was called with the wrong number of arguments. |
| `karn.consumes.target_is_commons` | `consumes` targets a `commons` instead of a context. |
| `karn.consumes.unknown_context` | `consumes` names a context that does not exist. |
| `karn.consumes.unknown_service` | Called a service the consumed context does not declare. |

## Contexts

| Code | Summary |
|---|---|
| `karn.context.consumes_cycle` | Contexts form a `consumes` dependency cycle. |
| `karn.context.external_construction` | A context-owned type was constructed from outside that context. |
| `karn.context.opaque_inspection` | An opaquely-exported type was inspected from outside its context. |

## Cron

| Code | Summary |
|---|---|
| `karn.cron.bad_params` | An `on cron` handler declares more than one parameter, or a non-`Int` one. |
| `karn.cron.duplicate_schedule` | Two `on cron` handlers declare the same schedule. |
| `karn.cron.invalid_schedule` | A cron expression is not five whitespace-separated fields. |
| `karn.cron.return_not_effect_result` | An `on cron` handler does not return `Effect[Result[(), E]]`. |

## Effects

| Code | Summary |
|---|---|
| `karn.effect.bind_in_pure_context` | An `<-` bind was used in a pure (non-effectful) context. |
| `karn.effect.bind_on_non_effect` | An `<-` bind was applied to a non-`Effect` value. |
| `karn.effect.capability_in_pure_context` | A capability was used in a pure context. |
| `karn.effect.cross_context_in_pure_context` | A cross-context call was made in a pure context. |

## Exports

| Code | Summary |
|---|---|
| `karn.exports.conflicting_visibility` | A type is exported with conflicting visibilities. |
| `karn.exports.duplicate_export` | The same name is exported more than once. |
| `karn.exports.duplicate_in_clause` | A name appears twice in one `exports` clause. |
| `karn.exports.undeclared_type` | `exports` names a type that is not declared. |

## Given capabilities

| Code | Summary |
|---|---|
| `karn.given.undeclared_capability` | A handler uses a capability it did not declare with `given`. |
| `karn.given.unknown_capability` | `given` names a capability that does not exist. |
| `karn.given.unused_capability` | A `given` capability is never used (warning). |

## HTTP

| Code | Summary |
|---|---|
| `karn.http.body_on_get_or_delete` | A GET or DELETE handler declares a `body` parameter. |
| `karn.http.duplicate_route` | Two handlers share the same method and route. |
| `karn.http.extra_param` | A handler parameter is neither a path parameter nor `body`. |
| `karn.http.invalid_path` | An HTTP route path is malformed. |
| `karn.http.path_param_not_stringy` | A path parameter's type is not constructible from a string. |
| `karn.http.reserved_prefix` | A route uses the reserved `/_karn/` prefix. |
| `karn.http.return_not_effect_http_result` | An HTTP handler does not return `Effect[HttpResult[T]]`. |
| `karn.http.unbound_path_param` | A `:name` route segment has no matching handler parameter. |

## Lexer

| Code | Summary |
|---|---|
| `karn.lex.bad_escape` | An invalid escape sequence in a string literal. |
| `karn.lex.integer_overflow` | An integer literal is out of range. |
| `karn.lex.unclosed_doc_block` | A documentation block is not closed. |
| `karn.lex.unexpected_character` | An unexpected character in the source. |
| `karn.lex.unterminated_string` | A string literal is not terminated. |

## Mock and mocks

| Code | Summary |
|---|---|
| `karn.mock.arity` | `Mock[T]` was given the wrong number of pin arguments. |
| `karn.mock.duplicate_target` | A `mocks` target is declared more than once. |
| `karn.mock.in_commons_test` | `mocks` used in a commons test, where there is no dependency to inject. |
| `karn.mock.literal_violates` | A pinned `Mock[T]` value violates the type's refinement. |
| `karn.mock.needs_pin` | A bare `Mock[T]` cannot generate a value (e.g. a `Matches` string); pin one. |
| `karn.mock.outside_test` | `Mock[T]` was used outside a test case body. |
| `karn.mock.pin_not_literal` | A `Mock[T]` pin argument is not a compile-time literal. |
| `karn.mock.pin_unsupported` | A pin was given for a type kind that does not support pinning. |
| `karn.mock.signature_mismatch` | A `mocks` implementation's signature does not match the capability. |
| `karn.mock.unknown_target` | `mocks` names a capability that is not in scope. |
| `karn.mock.unknown_type` | `Mock[T]` names a type that does not resolve. |
| `karn.mock.unsupported_kind` | `Mock[T]` cannot fabricate a value for this kind of type. |

## Parser

| Code | Summary |
|---|---|
| `karn.parse.consumes_after_decls` | `consumes` appears after other declarations. |
| `karn.parse.cron_in_agent` | An `on cron` handler was declared in an agent. |
| `karn.parse.empty_agent` | An `agent` body is empty. |
| `karn.parse.empty_capability` | A `capability` body is empty. |
| `karn.parse.empty_match` | A `match` has no arms. |
| `karn.parse.empty_mock_body` | A `mocks` body is empty. |
| `karn.parse.empty_service` | A `service` body is empty. |
| `karn.parse.expected_agent_key` | Expected a `key` declaration in an agent. |
| `karn.parse.expected_base_type` | Expected a base type. |
| `karn.parse.expected_capability_op` | Expected a capability operation. |
| `karn.parse.expected_expression` | Expected an expression. |
| `karn.parse.expected_handler` | Expected a handler. |
| `karn.parse.expected_item` | Expected a declaration. |
| `karn.parse.expected_predicate` | Expected a refinement predicate. |
| `karn.parse.expected_provider_op` | Expected a provider operation. |
| `karn.parse.expected_token` | Expected a specific token. |
| `karn.parse.expected_type` | Expected a type. |
| `karn.parse.expected_unit_header` | Expected a `commons` or `context` header. |
| `karn.parse.expected_visibility` | Expected a visibility keyword. |
| `karn.parse.exports_after_decls` | `exports` appears after other declarations. |
| `karn.parse.extra_tokens` | Unexpected tokens after an otherwise complete construct. |
| `karn.parse.generic_arg_count` | Wrong number of generic type arguments. |
| `karn.parse.http_in_agent` | An `on http` handler was declared in an agent. |
| `karn.parse.non_associative` | A non-associative operator was chained (e.g. `a == b == c`). |
| `karn.parse.orphan_doc_block` | A documentation block is not attached to a declaration (warning). |
| `karn.parse.queue_in_agent` | An `on queue` handler was declared in an agent. |
| `karn.parse.reserved_keyword` | A reserved keyword was used as an identifier. |
| `karn.parse.reserved_syntax` | Use of syntax reserved for a future version (e.g. `[` for generics). |
| `karn.parse.self_outside_method` | `self` used outside a method or handler. |
| `karn.parse.unexpected_context` | A `context` appeared where it is not allowed. |
| `karn.parse.unexpected_eof` | Unexpected end of input. |
| `karn.parse.unexpected_test` | A `test` appeared where it is not allowed. |
| `karn.parse.unknown_effect_method` | An unknown method on `Effect`. |
| `karn.parse.unknown_handler_kind` | An unknown handler kind (expected `call` or `http`). |
| `karn.parse.unknown_http_method` | An unknown HTTP method. |
| `karn.parse.unknown_predicate` | An unknown refinement predicate. |
| `karn.parse.uses_after_decls` | `uses` appears after other declarations. |

## Project

| Code | Summary |
|---|---|
| `karn.project.file_and_directory` | A unit exists as both a file and a directory. |
| `karn.project.inconsistent_commons_name` | A source file's path does not match its declared name. |
| `karn.project.inconsistent_test_path` | A test file's path does not match its target's name. |
| `karn.project.kind_conflict` | A name is declared as both a commons and a context. |
| `karn.project.no_root` | No project root could be determined. |
| `karn.project.no_sources` | The project contains no source files. |
| `karn.project.read_failed` | A source file could not be read. |

## Providers

| Code | Summary |
|---|---|
| `karn.provider.dependency_cycle` | Providers form a capability dependency cycle through `given`. |
| `karn.provider.extra_operation` | A `provides` block implements an operation not in the capability. |
| `karn.provider.missing_operation` | A `provides` block is missing a capability operation. |
| `karn.provider.outside_context` | `provides` was declared outside a context. |
| `karn.provider.signature_mismatch` | A `provides` operation's signature does not match the capability. |
| `karn.provider.unknown_capability` | `provides` names a capability that does not exist. |

## Queue

| Code | Summary |
|---|---|
| `karn.queue.bad_params` | An `on queue` handler does not take exactly one `message` parameter. |
| `karn.queue.duplicate_consumer` | Two `on queue` handlers consume the same queue. |
| `karn.queue.invalid_name` | An `on queue` handler has an empty queue name. |
| `karn.queue.return_not_effect_result` | An `on queue` handler does not return `Effect[Result[(), E]]`. |

## Record spread

| Code | Summary |
|---|---|
| `karn.record_spread.field_type_mismatch` | A record-spread override has the wrong type for the field. |
| `karn.record_spread.non_record_base` | The base of a record spread is not a record. |
| `karn.record_spread.type_mismatch` | A record spread's base is a different record type. |
| `karn.record_spread.unknown_field` | A record spread overrides a field the record does not have. |

## Refinement

| Code | Summary |
|---|---|
| `karn.refine.literal_violates` | A literal does not satisfy the refined type's predicate. |

## Resolution

| Code | Summary |
|---|---|
| `karn.resolve.ambiguous_variant` | A variant name is ambiguous across several sum types. |
| `karn.resolve.arity_mismatch` | A function was called with the wrong number of arguments. |
| `karn.resolve.duplicate_agent` | Two agents share a name. |
| `karn.resolve.duplicate_capability` | Two capabilities share a name. |
| `karn.resolve.duplicate_field` | A record declares a field twice. |
| `karn.resolve.duplicate_field_init` | A record construction initialises a field twice. |
| `karn.resolve.duplicate_fn` | Two functions share a name. |
| `karn.resolve.duplicate_method` | Two methods share a name. |
| `karn.resolve.duplicate_param` | A parameter name is repeated. |
| `karn.resolve.duplicate_provider` | A capability is provided more than once. |
| `karn.resolve.duplicate_service` | Two services share a name. |
| `karn.resolve.duplicate_type` | Two types share a name. |
| `karn.resolve.duplicate_variant` | A sum type declares a variant twice. |
| `karn.resolve.fn_without_call` | A function was referenced without being called. |
| `karn.resolve.let_shadows_fn` | A `let` binding shadows a function. |
| `karn.resolve.let_shadows_type` | A `let` binding shadows a type. |
| `karn.resolve.method_unknown_type` | A method is defined on an unknown type. |
| `karn.resolve.missing_field` | A record construction omits a required field. |
| `karn.resolve.name_conflict` | Two declarations share a name. |
| `karn.resolve.not_a_record_type` | Record syntax was used on a non-record type. |
| `karn.resolve.opaque_record_construction` | An opaque type was constructed with record syntax. |
| `karn.resolve.param_as_function` | A value (such as a parameter) was called as a function. |
| `karn.resolve.recursive_record_field` | A record directly contains a field of its own type. |
| `karn.resolve.self_outside_method` | `self` referenced outside a method or handler. |
| `karn.resolve.type_as_function` | A type name was called as if it were a function. |
| `karn.resolve.type_in_expr` | A type name was used where a value is expected. |
| `karn.resolve.unconsumed_context` | A context's service was called without a `consumes` declaration. |
| `karn.resolve.unknown_field` | Accessed a field the record does not have. |
| `karn.resolve.unknown_function` | Called a function that does not exist. |
| `karn.resolve.unknown_name` | Referenced a name that is not in scope. |
| `karn.resolve.unknown_static_member` | Referenced an unknown static member (e.g. `T.x`). |
| `karn.resolve.unknown_type` | Referenced a type that does not exist. |

## Services

| Code | Summary |
|---|---|
| `karn.service.outside_context` | A `service` was declared outside a context. |
| `karn.service.return_not_effect` | A service handler's return type is not an `Effect`. |

## Tests

| Code | Summary |
|---|---|
| `karn.test.duplicate_case_name` | Two test cases share a description. |
| `karn.test.unknown_target` | A `test` block targets a unit that does not exist. |

## Type checking

| Code | Summary |
|---|---|
| `karn.types.ambiguous_constructor` | `Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it. |
| `karn.types.argument_mismatch` | A function argument has the wrong type. |
| `karn.types.cannot_infer_option_type_param` | The value type of `None` could not be inferred. |
| `karn.types.cannot_infer_result_type_params` | The type parameters of a `Result` could not be inferred. |
| `karn.types.constructor_arity` | A variant constructor got the wrong number of arguments. |
| `karn.types.constructor_base_mismatch` | A `.of` constructor was given an argument of the wrong base type. |
| `karn.types.duplicate_variant_arm` | A `match` has two arms for the same variant. |
| `karn.types.empty_refinement` | A refinement admits no values (contradictory predicates). |
| `karn.types.err_value_mismatch` | An `Err` payload has the wrong type. |
| `karn.types.field_access_on_non_record` | Field access on a value that is not a record. |
| `karn.types.field_refinement_not_base` | An inline field refinement requires a base or refined type. |
| `karn.types.field_value_mismatch` | A record field was given a value of the wrong type. |
| `karn.types.if_branch_mismatch` | The branches of an `if` have different types. |
| `karn.types.if_non_bool_cond` | An `if` condition is not a `Bool`. |
| `karn.types.invalid_regex` | A `Matches` predicate contains an invalid regular expression. |
| `karn.types.inverted_range` | An `InRange` predicate has its bounds inverted. |
| `karn.types.is_base_mismatch` | An `is` refinement check is applied to a value of the wrong base type. |
| `karn.types.is_non_sum` | `is` was applied to a value that is not a sum type. |
| `karn.types.is_unknown_variant` | `is` names a variant the type does not have. |
| `karn.types.let_annotation_mismatch` | A `let` value does not match its type annotation. |
| `karn.types.match_arm_mismatch` | A `match` arm has a different type from the others. |
| `karn.types.match_non_sum_discriminant` | `match` was applied to a value that is not a sum type. |
| `karn.types.method_arity` | A method was called with the wrong number of arguments. |
| `karn.types.method_not_found` | Called a method the type does not have. |
| `karn.types.method_on_non_named_type` | A method was called on a built-in type that has no methods. |
| `karn.types.mixed_pattern_bindings` | A pattern mixes named and positional bindings. |
| `karn.types.negative_length` | A length predicate was given a negative value. |
| `karn.types.non_exhaustive_match` | A `match` does not cover every variant. |
| `karn.types.ok_value_mismatch` | An `Ok` payload has the wrong type. |
| `karn.types.opaque_raw_outside` | `.raw` on an opaque type was used outside its defining commons. |
| `karn.types.opaque_record_construction` | An opaque type was constructed with record syntax. |
| `karn.types.opaque_unsafe_outside` | `.unsafe` on an opaque type was used outside its defining context. |
| `karn.types.pattern_arity` | A pattern binds the wrong number of payload fields. |
| `karn.types.pattern_type_mismatch` | A pattern's type does not match the matched value. |
| `karn.types.predicate_base_mismatch` | A predicate does not apply to the type's base (e.g. a string predicate on an `Int`). |
| `karn.types.question_error_mismatch` | `?` propagates an error type incompatible with the function's. |
| `karn.types.question_on_non_result` | `?` was applied to a non-`Result` value. |
| `karn.types.question_outside_result` | `?` used in a function that does not return a `Result`. |
| `karn.types.return_mismatch` | A returned value does not match the declared return type. |
| `karn.types.some_value_mismatch` | A `Some` payload has the wrong type. |
| `karn.types.type_mismatch` | Two types that were required to match did not. |
| `karn.types.unknown_field` | Referenced a field the record type does not declare. |
| `karn.types.unknown_pattern_field` | A pattern names a field the variant does not have. |
| `karn.types.unknown_static_member` | Referenced an unknown static member on a type. |
| `karn.types.unknown_variant_in_pattern` | A pattern names a variant the sum type does not have. |
| `karn.types.unreachable_arm` | A `match` arm is unreachable. |
| `karn.types.variant_arity` | A variant constructor got the wrong number of payload values. |
| `karn.types.variant_missing_payload` | A variant requiring a payload was used without one. |
| `karn.types.variant_payload_mismatch` | A variant payload has the wrong type. |

## Uses

| Code | Summary |
|---|---|
| `karn.uses.name_conflict` | A `uses` name collides with another name. |
| `karn.uses.self_reference` | A commons `uses` itself. |
| `karn.uses.target_is_context` | `uses` targets a context instead of a commons. |
| `karn.uses.unknown_commons` | `uses` names a commons that does not exist. |
