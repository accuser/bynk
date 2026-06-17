//! Central registry of diagnostic codes.
//!
//! This is the single source of truth for the `karn.*` codes the compiler can
//! emit. The reference page `docs/src/reference/diagnostics.md` is generated
//! from [`render_markdown`], and the test `tests/diagnostics_registry.rs`
//! asserts that this table matches exactly the set of codes used across the
//! compiler source — so a new code cannot be introduced without documenting it
//! here, and a removed code cannot linger in the docs.
//!
//! Each entry is a `(code, summary)` pair, optionally tagged with the grammar
//! production(s) it constrains (`grammar_symbol`). The category shown in the
//! generated reference is derived from the second dotted segment of the code;
//! the grammar weave (`docs/grammar-semantics.json`, the
//! `{{#grammar-semantics}}` directive, and the diagnostics page's Construct
//! column) is generated from `grammar_symbol`.

/// One documented diagnostic: its stable code and a one-line summary of the
/// cause. Richer "cause and fix" material for the common diagnostics lives in
/// the troubleshooting how-to guides.
pub struct DiagnosticInfo {
    pub code: &'static str,
    pub summary: &'static str,
    /// The grammar production(s) this diagnostic constrains, by `tree-sitter`
    /// rule name (e.g. `http_handler`). This is the single source of the
    /// "static semantics" weave: a grammar-reference entry embeds the
    /// diagnostics for a rule via `{{#grammar-semantics <rule>}}`, generated
    /// from here. Empty for diagnostics with no single governing construct
    /// (e.g. `karn.boundary.structural_mismatch`). Every non-empty name is
    /// checked against the grammar by `tests/diagnostics_registry.rs`.
    pub grammar_symbol: &'static [&'static str],
}

/// Every diagnostic code the compiler emits, sorted by code.
pub const REGISTRY: &[DiagnosticInfo] = &[
    d(
        "karn.actor.bearer_identity_not_string_constructible",
        "A `Bearer` actor's identity is not a string-constructible type.",
    ),
    d(
        "karn.actor.bearer_missing_secret",
        "A `Bearer` actor does not name its signing secret.",
    ),
    d(
        "karn.actor.binder_shadows_param",
        "A `by` actor binder collides with a handler parameter of the same name.",
    ),
    d(
        "karn.actor.identity_not_sealed",
        "An actor identity type is not a context-ownable (sealed) value type.",
    ),
    d(
        "karn.actor.missing_by_on_http",
        "An HTTP handler lacks the required `by` actor clause.",
    ),
    d(
        "karn.actor.outside_context",
        "An `actor` was declared outside a context (e.g. in a commons).",
    ),
    d(
        "karn.actor.refinement_unsupported",
        "The actor refinement form (`actor A = B where …`) is not yet supported.",
    ),
    d(
        "karn.actor.scheme_not_admissible",
        "An actor's scheme is not admissible on this handler's protocol.",
    ),
    d(
        "karn.actor.scheme_unsupported",
        "An actor's authentication scheme is reserved but not yet supported.",
    ),
    d(
        "karn.actor.unknown_actor",
        "A handler's `by` clause names an actor that is not declared.",
    ),
    d(
        "karn.actor.unknown_scheme",
        "An actor declares an authentication scheme that is not compiler-known.",
    ),
    dg(
        "karn.adapter.consumes_context",
        "An `adapter` consumed a context; adapter dependencies are adapter-to-adapter.",
        &["consumes_decl"],
    ),
    dg(
        "karn.adapter.consumes_requires_selection",
        "An `adapter` used a whole-unit or aliased `consumes`; adapters must select capabilities with `consumes U { Cap, … }`.",
        &["consumes_decl"],
    ),
    dg(
        "karn.adapter.disallowed_item",
        "An `adapter` declared a `service`, `agent`, or other item it may not contain.",
        &["adapter_decl"],
    ),
    dg(
        "karn.adapter.duplicate_binding",
        "An `adapter` declared more than one `binding` clause.",
        &["binding_decl"],
    ),
    dg(
        "karn.adapter.no_binding",
        "An `adapter` declares an external provider but no `binding` module to supply it.",
        &["adapter_decl"],
    ),
    dg(
        "karn.adapter.provider_has_body",
        "A provider inside an `adapter` has a Karn body; adapter providers must be external.",
        &["provider_decl"],
    ),
    dg(
        "karn.agent.construction_arity",
        "An agent was constructed with the wrong number of key arguments.",
        &["agent_decl"],
    ),
    dg(
        "karn.agent.handler_arity",
        "An agent handler was called with the wrong number of arguments.",
        &["agent_decl"],
    ),
    dg(
        "karn.agent.handler_not_found",
        "Called a handler the agent does not declare.",
        &["agent_decl"],
    ),
    dg(
        "karn.agent.key_mismatch",
        "An agent key argument has the wrong type.",
        &["agent_decl"],
    ),
    dg(
        "karn.agent.outside_context",
        "An `agent` was declared outside a context.",
        &["agent_decl"],
    ),
    dg(
        "karn.agent.return_not_effect",
        "An agent handler's return type is not an `Effect`.",
        &["agent_decl"],
    ),
    dg(
        "karn.agents.bad_state_initialiser",
        "An agent state-field initialiser is not a static value of the field's type.",
        &["state_decl"],
    ),
    dg(
        "karn.agents.non_zeroable_state_field",
        "An agent state field has no initialiser and no implicit zero value.",
        &["state_decl"],
    ),
    dg(
        "karn.assert.non_bool",
        "`assert` was given a non-`Bool` expression.",
        &["assert_expr"],
    ),
    dg(
        "karn.assert.outside_test",
        "`assert` was used outside a test case body.",
        &["assert_expr"],
    ),
    d(
        "karn.boundary.structural_mismatch",
        "Data crossing a context boundary did not match the expected shape.",
    ),
    dg(
        "karn.capability.op_arity",
        "A capability operation was called with the wrong number of arguments.",
        &["capability_decl"],
    ),
    dg(
        "karn.capability.outside_context",
        "A `capability` was declared outside a context.",
        &["capability_decl"],
    ),
    dg(
        "karn.capability.unknown_operation",
        "Referenced an operation the capability does not declare.",
        &["capability_decl"],
    ),
    dg(
        "karn.commit.outside_agent",
        "`commit` was used outside an agent handler.",
        &["commit_stmt"],
    ),
    dg(
        "karn.commit.two_reachable_commits",
        "Two `commit` statements are reachable on the same execution path.",
        &["commit_stmt"],
    ),
    dg(
        "karn.commit.wrong_state_type",
        "A `commit` value does not match the agent's state type.",
        &["commit_stmt"],
    ),
    dg(
        "karn.consumes.alias_conflict",
        "Two `consumes` aliases collide.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.capability_name_clash",
        "Two flattened `consumes U { Cap }` capabilities collide, or one clashes with a local capability.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.in_commons",
        "`consumes` appears in a `commons` (it is only valid in a context).",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.name_conflict",
        "A `consumes` name collides with another name in scope.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.self_reference",
        "A context `consumes` itself.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.service_arity",
        "A consumed service was called with the wrong number of arguments.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.target_is_commons",
        "`consumes` targets a `commons` instead of a context.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.unknown_context",
        "`consumes` names a context that does not exist.",
        &["consumes_decl"],
    ),
    dg(
        "karn.consumes.unknown_service",
        "Called a service the consumed context does not declare.",
        &["consumes_decl"],
    ),
    d(
        "karn.context.consumes_cycle",
        "Contexts form a `consumes` dependency cycle.",
    ),
    d(
        "karn.context.external_construction",
        "A context-owned type was constructed from outside that context.",
    ),
    dg(
        "karn.context.external_provider",
        "A bodiless (external) provider was declared outside an `adapter`.",
        &["provider_decl"],
    ),
    d(
        "karn.context.opaque_inspection",
        "An opaquely-exported type was inspected from outside its context.",
    ),
    dg(
        "karn.cron.bad_params",
        "A cron handler declares more than one parameter, or a non-`Int` one.",
        &["cron_handler"],
    ),
    dg(
        "karn.cron.duplicate_schedule",
        "Two cron handlers declare the same schedule.",
        &["cron_handler"],
    ),
    dg(
        "karn.cron.invalid_schedule",
        "A cron expression is not five whitespace-separated fields.",
        &["cron_handler"],
    ),
    dg(
        "karn.cron.return_not_effect_result",
        "A cron handler does not return `Effect[Result[(), E]]`.",
        &["cron_handler"],
    ),
    dg(
        "karn.effect.bind_in_pure_context",
        "An `<-` bind was used in a pure (non-effectful) context.",
        &["effect_let_stmt"],
    ),
    dg(
        "karn.effect.bind_on_non_effect",
        "An `<-` bind was applied to a non-`Effect` value.",
        &["effect_let_stmt"],
    ),
    d(
        "karn.effect.capability_in_pure_context",
        "A capability was used in a pure context.",
    ),
    d(
        "karn.effect.cross_context_in_pure_context",
        "A cross-context call was made in a pure context.",
    ),
    dg(
        "karn.effect.fn_value_in_pure_context",
        "An effectful function value was called in a pure context; like a capability call, it is legal only where the enclosing body is effectful.",
        &["call"],
    ),
    dg(
        "karn.exports.capability_not_provided",
        "An exported capability has no provider in its context.",
        &["exports_decl"],
    ),
    dg(
        "karn.exports.conflicting_visibility",
        "A type is exported with conflicting visibilities.",
        &["exports_decl"],
    ),
    dg(
        "karn.exports.duplicate_export",
        "The same name is exported more than once.",
        &["exports_decl"],
    ),
    dg(
        "karn.exports.duplicate_in_clause",
        "A name appears twice in one `exports` clause.",
        &["exports_decl"],
    ),
    dg(
        "karn.exports.undeclared_capability",
        "`exports capability` names a capability that is not declared.",
        &["exports_decl"],
    ),
    dg(
        "karn.exports.undeclared_type",
        "`exports` names a type that is not declared.",
        &["exports_decl"],
    ),
    dg(
        "karn.generics.no_bounds",
        "A type parameter carries a bound (`[A: …]`); bounded generics are not in v0.20a.",
        &["fn_decl"],
    ),
    dg(
        "karn.generics.no_generic_types",
        "A `type` declaration carries a type-parameter list; generic type declarations are not in v0.20a (type parameters belong to functions).",
        &["type_decl"],
    ),
    dg(
        "karn.generics.type_arg_mismatch",
        "Inferred or explicit type arguments conflict, have the wrong arity, target a non-generic function, or a type parameter shadows a declared type.",
        &["call"],
    ),
    dg(
        "karn.generics.uninferable_type_arg",
        "A generic function's type parameter could not be inferred from the arguments and was not given explicitly (`name[T](…)`); a bare generic function also cannot be passed as a value in v0.20a.",
        &["call"],
    ),
    dg(
        "karn.given.cross_context_unknown_capability",
        "`given B.Cap` names a capability the consumed context does not export.",
        &["given_clause"],
    ),
    dg(
        "karn.given.undeclared_capability",
        "A handler uses a capability it did not declare with `given`.",
        &["given_clause"],
    ),
    dg(
        "karn.given.unknown_capability",
        "`given` names a capability that does not exist.",
        &["given_clause"],
    ),
    dg(
        "karn.given.unused_capability",
        "A `given` capability is never used (warning).",
        &["given_clause"],
    ),
    dg(
        "karn.http.body_on_get_or_delete",
        "A GET or DELETE handler declares a `body` parameter.",
        &["http_handler"],
    ),
    dg(
        "karn.http.duplicate_route",
        "Two handlers share the same method and route.",
        &["http_handler"],
    ),
    dg(
        "karn.http.extra_param",
        "A handler parameter is neither a path parameter nor `body`.",
        &["http_handler"],
    ),
    dg(
        "karn.http.invalid_path",
        "An HTTP route path is malformed.",
        &["http_handler"],
    ),
    dg(
        "karn.http.path_param_not_stringy",
        "A path parameter's type is not constructible from a string.",
        &["http_handler"],
    ),
    dg(
        "karn.http.reserved_prefix",
        "A route uses the reserved `/_karn/` prefix.",
        &["http_handler"],
    ),
    dg(
        "karn.http.return_not_effect_http_result",
        "An HTTP handler does not return `Effect[HttpResult[T]]`.",
        &["http_handler"],
    ),
    dg(
        "karn.http.unbound_path_param",
        "A `:name` route segment has no matching handler parameter.",
        &["http_handler"],
    ),
    dg(
        "karn.integration.duplicate_participant",
        "A context is listed more than once in a `wires` clause.",
        &["wires_decl"],
    ),
    dg(
        "karn.integration.duplicate_suite",
        "Two integration tests share the same suite name.",
        &["integration_decl"],
    ),
    dg(
        "karn.integration.mock_in_integration",
        "`mocks` is not allowed in an integration test.",
        &["mocks_decl"],
    ),
    dg(
        "karn.integration.too_few_participants",
        "An integration test wires fewer than two contexts.",
        &["wires_decl"],
    ),
    dg(
        "karn.integration.unknown_participant",
        "A `wires` clause names something that is not a declared context.",
        &["wires_decl"],
    ),
    dg(
        "karn.integration.unwired_dependency",
        "A participant consumes a context that is not wired into the integration test.",
        &["integration_decl"],
    ),
    dg(
        "karn.lambda.unannotated_param",
        "A lambda parameter has no type annotation in a position where no function type is expected to infer it from.",
        &["lambda_expr"],
    ),
    dg(
        "karn.lex.bad_escape",
        "An invalid escape sequence in a string literal.",
        &["string_literal"],
    ),
    dg(
        "karn.lex.float_literal_overflow",
        "A float literal does not fit a finite 64-bit float.",
        &["float_literal"],
    ),
    dg(
        "karn.lex.integer_overflow",
        "An integer literal is out of range.",
        &["number_literal"],
    ),
    d(
        "karn.lex.unclosed_doc_block",
        "A documentation block is not closed.",
    ),
    d(
        "karn.lex.unexpected_character",
        "An unexpected character in the source.",
    ),
    dg(
        "karn.lex.unterminated_interpolation",
        "An interpolation hole `\\(…)` is not closed on its line.",
        &["string_literal"],
    ),
    dg(
        "karn.lex.unterminated_string",
        "A string literal is not terminated.",
        &["string_literal"],
    ),
    dg(
        "karn.mock.arity",
        "`Mock[T]` was given the wrong number of pin arguments.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.duplicate_target",
        "A `mocks` target is declared more than once.",
        &["mocks_decl"],
    ),
    dg(
        "karn.mock.in_commons_test",
        "`mocks` used in a commons test, where there is no dependency to inject.",
        &["mocks_decl"],
    ),
    dg(
        "karn.mock.literal_violates",
        "A pinned `Mock[T]` value violates the type's refinement.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.needs_pin",
        "A bare `Mock[T]` cannot generate a value (e.g. a `Matches` string); pin one.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.outside_test",
        "`Mock[T]` was used outside a test case body.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.pin_not_literal",
        "A `Mock[T]` pin argument is not a compile-time literal.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.pin_unsupported",
        "A pin was given for a type kind that does not support pinning.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.signature_mismatch",
        "A `mocks` implementation's signature does not match the capability.",
        &["mocks_decl"],
    ),
    dg(
        "karn.mock.unknown_target",
        "`mocks` names a capability that is not in scope.",
        &["mocks_decl"],
    ),
    dg(
        "karn.mock.unknown_type",
        "`Mock[T]` names a type that does not resolve.",
        &["mock_expr"],
    ),
    dg(
        "karn.mock.unsupported_kind",
        "`Mock[T]` cannot fabricate a value for this kind of type.",
        &["mock_expr"],
    ),
    d(
        "karn.namespace.reserved",
        "A user unit is named `karn` or `karn.*`; the `karn` root is reserved for the toolchain.",
    ),
    dg(
        "karn.parse.consumes_after_decls",
        "`consumes` appears after other declarations.",
        &["consumes_decl"],
    ),
    dg(
        "karn.parse.empty_agent",
        "An `agent` body is empty.",
        &["agent_decl"],
    ),
    dg(
        "karn.parse.empty_capability",
        "A `capability` body is empty.",
        &["capability_decl"],
    ),
    d(
        "karn.parse.empty_interpolation",
        "An interpolation hole `\\(…)` contains no expression.",
    ),
    dg(
        "karn.parse.empty_match",
        "A `match` has no arms.",
        &["match_expr"],
    ),
    dg(
        "karn.parse.empty_mock_body",
        "A `mocks` body is empty.",
        &["mocks_decl"],
    ),
    dg(
        "karn.parse.empty_service",
        "A `service` body is empty.",
        &["service_decl"],
    ),
    dg(
        "karn.parse.expected_agent_key",
        "Expected a `key` declaration in an agent.",
        &["agent_decl"],
    ),
    dg(
        "karn.parse.expected_base_type",
        "Expected a base type.",
        &["base_type"],
    ),
    dg(
        "karn.parse.expected_capability_op",
        "Expected a capability operation.",
        &["capability_op"],
    ),
    d("karn.parse.expected_expression", "Expected an expression."),
    dg(
        "karn.parse.expected_handler",
        "Expected a handler.",
        &["handler"],
    ),
    d("karn.parse.expected_item", "Expected a declaration."),
    dg(
        "karn.parse.expected_predicate",
        "Expected a refinement predicate.",
        &["refinement"],
    ),
    dg(
        "karn.parse.expected_provider_op",
        "Expected a provider operation.",
        &["provider_op"],
    ),
    d("karn.parse.expected_token", "Expected a specific token."),
    d("karn.parse.expected_type", "Expected a type."),
    d(
        "karn.parse.expected_unit_header",
        "Expected a `commons` or `context` header.",
    ),
    dg(
        "karn.parse.expected_visibility",
        "Expected a visibility keyword.",
        &["exports_decl"],
    ),
    dg(
        "karn.parse.exports_after_decls",
        "`exports` appears after other declarations.",
        &["exports_decl"],
    ),
    d(
        "karn.parse.extra_tokens",
        "Unexpected tokens after an otherwise complete construct.",
    ),
    dg(
        "karn.parse.generic_arg_count",
        "Wrong number of generic type arguments.",
        &["generic_type_ref"],
    ),
    dg(
        "karn.parse.handler_in_agent",
        "A protocol handler (`on GET`/`schedule`/`message`) was declared in an agent.",
        &["handler"],
    ),
    dg(
        "karn.parse.malformed_float_literal",
        "A float literal is missing a digit on one side of the `.` (`1.`, `.5`).",
        &["float_literal"],
    ),
    dg(
        "karn.parse.non_associative",
        "A non-associative operator was chained (e.g. `a == b == c`).",
        &["binary_expr"],
    ),
    d(
        "karn.parse.orphan_doc_block",
        "A documentation block is not attached to a declaration (warning).",
    ),
    dg(
        "karn.parse.reserved_keyword",
        "A reserved keyword was used as an identifier.",
        &["identifier"],
    ),
    dg(
        "karn.parse.self_outside_method",
        "`self` used outside a method or handler.",
        &["self_expr"],
    ),
    d(
        "karn.parse.unexpected_adapter",
        "An `adapter` appeared where it is not allowed.",
    ),
    dg(
        "karn.parse.unexpected_context",
        "A `context` appeared where it is not allowed.",
        &["context_decl"],
    ),
    d("karn.parse.unexpected_eof", "Unexpected end of input."),
    dg(
        "karn.parse.unexpected_test",
        "A `test` appeared where it is not allowed.",
        &["test_decl"],
    ),
    d(
        "karn.parse.unknown_effect_method",
        "An unknown method on `Effect`.",
    ),
    dg(
        "karn.parse.unknown_handler_kind",
        "An unknown handler form (expected `call`, an HTTP method, `schedule`, or `message`).",
        &["handler"],
    ),
    dg(
        "karn.parse.unknown_predicate",
        "An unknown refinement predicate.",
        &["predicate_name"],
    ),
    dg(
        "karn.parse.uses_after_decls",
        "`uses` appears after other declarations.",
        &["uses_decl"],
    ),
    d(
        "karn.project.file_and_directory",
        "A unit exists as both a file and a directory.",
    ),
    d(
        "karn.project.inconsistent_commons_name",
        "A source file's path does not match its declared name.",
    ),
    d(
        "karn.project.inconsistent_test_path",
        "A test file's path does not match its target's name.",
    ),
    d(
        "karn.project.kind_conflict",
        "A name is declared as both a commons and a context.",
    ),
    d(
        "karn.project.no_root",
        "No project root could be determined.",
    ),
    d(
        "karn.project.no_sources",
        "The project contains no source files.",
    ),
    d(
        "karn.project.read_failed",
        "A source file could not be read.",
    ),
    dg(
        "karn.provider.dependency_cycle",
        "Providers form a capability dependency cycle through `given`.",
        &["provider_decl"],
    ),
    dg(
        "karn.provider.extra_operation",
        "A `provides` block implements an operation not in the capability.",
        &["provider_decl"],
    ),
    dg(
        "karn.provider.missing_operation",
        "A `provides` block is missing a capability operation.",
        &["provider_decl"],
    ),
    dg(
        "karn.provider.outside_context",
        "`provides` was declared outside a context.",
        &["provider_decl"],
    ),
    dg(
        "karn.provider.signature_mismatch",
        "A `provides` operation's signature does not match the capability.",
        &["provider_decl"],
    ),
    dg(
        "karn.provider.unknown_capability",
        "`provides` names a capability that does not exist.",
        &["provider_decl"],
    ),
    dg(
        "karn.queue.bad_params",
        "An `on message` handler does not take exactly one `message` parameter.",
        &["queue_handler"],
    ),
    dg(
        "karn.queue.duplicate_consumer",
        "Two `on message` handlers consume the same queue.",
        &["queue_handler"],
    ),
    dg(
        "karn.queue.invalid_name",
        "A `from queue(\"…\")` binding has an empty queue name.",
        &["queue_handler"],
    ),
    dg(
        "karn.queue.return_not_queue_result",
        "An `on message` handler does not return `Effect[QueueResult]`.",
        &["handler"],
    ),
    dg(
        "karn.record_spread.field_type_mismatch",
        "A record-spread override has the wrong type for the field.",
        &["record_spread"],
    ),
    dg(
        "karn.record_spread.non_record_base",
        "The base of a record spread is not a record.",
        &["record_spread"],
    ),
    dg(
        "karn.record_spread.type_mismatch",
        "A record spread's base is a different record type.",
        &["record_spread"],
    ),
    dg(
        "karn.record_spread.unknown_field",
        "A record spread overrides a field the record does not have.",
        &["record_spread"],
    ),
    dg(
        "karn.refine.literal_violates",
        "A literal does not satisfy the refined type's predicate.",
        &["refined_type"],
    ),
    dg(
        "karn.requires.unpinned_dependency",
        "An adapter `binding … requires { … }` entry has an unpinned version range.",
        &["binding_decl"],
    ),
    d(
        "karn.resolve.ambiguous_variant",
        "A variant name is ambiguous across several sum types.",
    ),
    dg(
        "karn.resolve.arity_mismatch",
        "A function was called with the wrong number of arguments.",
        &["call"],
    ),
    d("karn.resolve.duplicate_actor", "Two actors share a name."),
    dg(
        "karn.resolve.duplicate_agent",
        "Two agents share a name.",
        &["agent_decl"],
    ),
    dg(
        "karn.resolve.duplicate_capability",
        "Two capabilities share a name.",
        &["capability_decl"],
    ),
    dg(
        "karn.resolve.duplicate_field",
        "A record declares a field twice.",
        &["record_type"],
    ),
    dg(
        "karn.resolve.duplicate_field_init",
        "A record construction initialises a field twice.",
        &["record_construction"],
    ),
    dg(
        "karn.resolve.duplicate_fn",
        "Two functions share a name.",
        &["fn_decl"],
    ),
    dg(
        "karn.resolve.duplicate_method",
        "Two methods share a name.",
        &["fn_decl"],
    ),
    dg(
        "karn.resolve.duplicate_param",
        "A parameter name is repeated.",
        &["param"],
    ),
    dg(
        "karn.resolve.duplicate_provider",
        "A capability is provided more than once.",
        &["provider_decl"],
    ),
    dg(
        "karn.resolve.duplicate_service",
        "Two services share a name.",
        &["service_decl"],
    ),
    dg(
        "karn.resolve.duplicate_type",
        "Two types share a name.",
        &["type_decl"],
    ),
    dg(
        "karn.resolve.duplicate_variant",
        "A sum type declares a variant twice.",
        &["sum_type"],
    ),
    d(
        "karn.resolve.fn_without_call",
        "A function was referenced without being called.",
    ),
    dg(
        "karn.resolve.let_shadows_fn",
        "A `let` binding shadows a function.",
        &["let_stmt"],
    ),
    dg(
        "karn.resolve.let_shadows_type",
        "A `let` binding shadows a type.",
        &["let_stmt"],
    ),
    d(
        "karn.resolve.method_unknown_type",
        "A method is defined on an unknown type.",
    ),
    dg(
        "karn.resolve.missing_field",
        "A record construction omits a required field.",
        &["record_construction"],
    ),
    d(
        "karn.resolve.name_conflict",
        "Two declarations share a name.",
    ),
    dg(
        "karn.resolve.not_a_record_type",
        "Record syntax was used on a non-record type.",
        &["record_construction"],
    ),
    dg(
        "karn.resolve.opaque_record_construction",
        "An opaque type was constructed with record syntax.",
        &["record_construction"],
    ),
    dg(
        "karn.resolve.param_as_function",
        "A value (such as a parameter) was called as a function.",
        &["call"],
    ),
    dg(
        "karn.resolve.recursive_record_field",
        "A record directly contains a field of its own type.",
        &["record_type"],
    ),
    dg(
        "karn.resolve.self_outside_method",
        "`self` referenced outside a method or handler.",
        &["self_expr"],
    ),
    dg(
        "karn.resolve.type_as_function",
        "A type name was called as if it were a function.",
        &["call"],
    ),
    d(
        "karn.resolve.type_in_expr",
        "A type name was used where a value is expected.",
    ),
    dg(
        "karn.resolve.unconsumed_context",
        "A context's service was called without a `consumes` declaration.",
        &["consumes_decl"],
    ),
    dg(
        "karn.resolve.unknown_field",
        "Accessed a field the record does not have.",
        &["field_access"],
    ),
    dg(
        "karn.resolve.unknown_function",
        "Called a function that does not exist.",
        &["call"],
    ),
    d(
        "karn.resolve.unknown_name",
        "Referenced a name that is not in scope.",
    ),
    dg(
        "karn.resolve.unknown_static_member",
        "Referenced an unknown static member (e.g. `T.x`).",
        &["field_access"],
    ),
    d(
        "karn.resolve.unknown_type",
        "Referenced a type that does not exist.",
    ),
    dg(
        "karn.service.missing_from",
        "A `from`-less service has a handler other than `on call`.",
        &["service_decl"],
    ),
    dg(
        "karn.service.mixed_protocols",
        "A service mixes handler forms that do not match its `from <protocol>`.",
        &["service_decl"],
    ),
    dg(
        "karn.service.outside_context",
        "A `service` was declared outside a context.",
        &["service_decl"],
    ),
    dg(
        "karn.service.return_not_effect",
        "A service handler's return type is not an `Effect`.",
        &["service_decl"],
    ),
    dg(
        "karn.service.unknown_protocol",
        "A `from <protocol>` names an unknown protocol (e.g. a transport like Kafka).",
        &["service_decl"],
    ),
    dg(
        "karn.target.vendor_conflict",
        "One deployment unit's in-process closure uses platform-native capabilities from two mutually-exclusive platforms.",
        &["consumes_decl"],
    ),
    dg(
        "karn.target.vendor_required",
        "A deployment unit uses a platform-native capability but the build selects another `--platform`.",
        &["consumes_decl"],
    ),
    dg(
        "karn.test.duplicate_case_name",
        "Two test cases share a description.",
        &["test_case"],
    ),
    dg(
        "karn.test.unknown_target",
        "A `test` block targets a unit that does not exist.",
        &["test_decl"],
    ),
    d(
        "karn.types.ambiguous_constructor",
        "`Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it.",
    ),
    dg(
        "karn.types.argument_mismatch",
        "A function argument has the wrong type.",
        &["call"],
    ),
    dg(
        "karn.types.call_arity",
        "A function value was applied with the wrong number of arguments.",
        &["call"],
    ),
    dg(
        "karn.types.cannot_infer_option_type_param",
        "The value type of `None` could not be inferred.",
        &["none_expr"],
    ),
    d(
        "karn.types.cannot_infer_result_type_params",
        "The type parameters of a `Result` could not be inferred.",
    ),
    d(
        "karn.types.constructor_arity",
        "A variant constructor got the wrong number of arguments.",
    ),
    d(
        "karn.types.constructor_base_mismatch",
        "A `.of` constructor was given an argument of the wrong base type.",
    ),
    dg(
        "karn.types.duplicate_variant_arm",
        "A `match` has two arms for the same variant.",
        &["match_arm"],
    ),
    dg(
        "karn.types.empty_refinement",
        "A refinement admits no values (contradictory predicates).",
        &["refinement"],
    ),
    dg(
        "karn.types.err_value_mismatch",
        "An `Err` payload has the wrong type.",
        &["err_expr"],
    ),
    dg(
        "karn.types.field_access_on_non_record",
        "Field access on a value that is not a record.",
        &["field_access"],
    ),
    dg(
        "karn.types.field_refinement_not_base",
        "An inline field refinement requires a base or refined type.",
        &["record_field"],
    ),
    dg(
        "karn.types.field_value_mismatch",
        "A record field was given a value of the wrong type.",
        &["record_construction"],
    ),
    dg(
        "karn.types.function_at_boundary",
        "A function type appeared in a serialisable or boundary position (a record field, sum payload, service/agent handler signature, capability operation signature, agent state field, or agent key); functions cannot serialise or cross a boundary.",
        &["function_type_ref"],
    ),
    dg(
        "karn.types.if_branch_mismatch",
        "The branches of an `if` have different types.",
        &["if_expr"],
    ),
    dg(
        "karn.types.if_non_bool_cond",
        "An `if` condition is not a `Bool`.",
        &["if_expr"],
    ),
    d(
        "karn.types.interpolation_non_scalar",
        "An interpolation hole holds a value with no string form.",
    ),
    dg(
        "karn.types.invalid_regex",
        "A `Matches` predicate contains an invalid regular expression.",
        &["refinement"],
    ),
    dg(
        "karn.types.inverted_range",
        "An `InRange` predicate has its bounds inverted.",
        &["refinement"],
    ),
    dg(
        "karn.types.is_base_mismatch",
        "An `is` refinement check is applied to a value of the wrong base type.",
        &["is_expr"],
    ),
    dg(
        "karn.types.is_non_sum",
        "`is` was applied to a value that is not a sum type.",
        &["is_expr"],
    ),
    dg(
        "karn.types.is_unknown_variant",
        "`is` names a variant the type does not have.",
        &["is_expr"],
    ),
    dg(
        "karn.types.json_uncodable",
        "A `Json.encode`/`Json.decode` target type cannot pass through the typed JSON codec (functions, effects, error builtins).",
        &["method_call"],
    ),
    dg(
        "karn.types.lambda_mismatch",
        "A lambda's parameter count, parameter annotations, or body type do not match the expected function type.",
        &["lambda_expr"],
    ),
    dg(
        "karn.types.let_annotation_mismatch",
        "A `let` value does not match its type annotation.",
        &["let_stmt"],
    ),
    dg(
        "karn.types.list_element_mismatch",
        "A list-literal element has a different type from the list's element type.",
        &["list_literal"],
    ),
    dg(
        "karn.types.match_arm_mismatch",
        "A `match` arm has a different type from the others.",
        &["match_arm"],
    ),
    dg(
        "karn.types.match_non_sum_discriminant",
        "`match` was applied to a value that is not a sum type.",
        &["match_expr"],
    ),
    dg(
        "karn.types.method_arity",
        "A method was called with the wrong number of arguments.",
        &["method_call"],
    ),
    dg(
        "karn.types.method_not_found",
        "Called a method the type does not have.",
        &["method_call"],
    ),
    dg(
        "karn.types.method_on_non_named_type",
        "A method was called on a built-in type that has no methods.",
        &["method_call"],
    ),
    dg(
        "karn.types.mixed_pattern_bindings",
        "A pattern mixes named and positional bindings.",
        &["variant_pattern"],
    ),
    dg(
        "karn.types.negative_length",
        "A length predicate was given a negative value.",
        &["refinement"],
    ),
    dg(
        "karn.types.no_numeric_coercion",
        "`Int` and `Float` were mixed without an explicit conversion — in an operation or in refinement bounds.",
        &["binary_expr", "refinement"],
    ),
    dg(
        "karn.types.non_exhaustive_match",
        "A `match` does not cover every variant.",
        &["match_expr"],
    ),
    dg(
        "karn.types.ok_value_mismatch",
        "An `Ok` payload has the wrong type.",
        &["ok_expr"],
    ),
    dg(
        "karn.types.opaque_raw_outside",
        "`.raw` on an opaque type was used outside its defining commons.",
        &["field_access"],
    ),
    dg(
        "karn.types.opaque_record_construction",
        "An opaque type was constructed with record syntax.",
        &["record_construction"],
    ),
    dg(
        "karn.types.opaque_unsafe_outside",
        "`.unsafe` on an opaque type was used outside its defining context.",
        &["field_access"],
    ),
    dg(
        "karn.types.pattern_arity",
        "A pattern binds the wrong number of payload fields.",
        &["variant_pattern"],
    ),
    dg(
        "karn.types.pattern_type_mismatch",
        "A pattern's type does not match the matched value.",
        &["variant_pattern"],
    ),
    dg(
        "karn.types.predicate_base_mismatch",
        "A predicate does not apply to the type's base (e.g. a string predicate on an `Int`).",
        &["refinement"],
    ),
    dg(
        "karn.types.question_error_mismatch",
        "`?` propagates an error type incompatible with the function's.",
        &["question_expr"],
    ),
    dg(
        "karn.types.question_on_non_result",
        "`?` was applied to a non-`Result` value.",
        &["question_expr"],
    ),
    dg(
        "karn.types.question_outside_result",
        "`?` used in a function that does not return a `Result`.",
        &["question_expr"],
    ),
    d(
        "karn.types.return_mismatch",
        "A returned value does not match the declared return type.",
    ),
    dg(
        "karn.types.some_value_mismatch",
        "A `Some` payload has the wrong type.",
        &["some_expr"],
    ),
    d(
        "karn.types.type_mismatch",
        "Two types that were required to match did not.",
    ),
    dg(
        "karn.types.uninferable_element_type",
        "An empty `[]` (or `List.empty()` / `Map.empty()`) has no expected type to infer its element type from.",
        &["list_literal"],
    ),
    dg(
        "karn.types.unkeyable_map_key",
        "A `Map` key type is not value-keyable (`String`, `Int`, or a refined/opaque type over them).",
        &["generic_type_ref"],
    ),
    dg(
        "karn.types.unknown_field",
        "Referenced a field the record type does not declare.",
        &["field_access"],
    ),
    dg(
        "karn.types.unknown_pattern_field",
        "A pattern names a field the variant does not have.",
        &["variant_pattern"],
    ),
    dg(
        "karn.types.unknown_static_member",
        "Referenced an unknown static member on a type.",
        &["field_access"],
    ),
    dg(
        "karn.types.unknown_variant_in_pattern",
        "A pattern names a variant the sum type does not have.",
        &["variant_pattern"],
    ),
    dg(
        "karn.types.unreachable_arm",
        "A `match` arm is unreachable.",
        &["match_arm"],
    ),
    d(
        "karn.types.variant_arity",
        "A variant constructor got the wrong number of payload values.",
    ),
    d(
        "karn.types.variant_missing_payload",
        "A variant requiring a payload was used without one.",
    ),
    d(
        "karn.types.variant_payload_mismatch",
        "A variant payload has the wrong type.",
    ),
    dg(
        "karn.uses.name_conflict",
        "A `uses` name collides with another name.",
        &["uses_decl"],
    ),
    dg(
        "karn.uses.self_reference",
        "A commons `uses` itself.",
        &["uses_decl"],
    ),
    dg(
        "karn.uses.target_is_context",
        "`uses` targets a context instead of a commons.",
        &["uses_decl"],
    ),
    dg(
        "karn.uses.unknown_commons",
        "`uses` names a commons that does not exist.",
        &["uses_decl"],
    ),
];

/// A diagnostic with no single governing grammar construct.
const fn d(code: &'static str, summary: &'static str) -> DiagnosticInfo {
    DiagnosticInfo {
        code,
        summary,
        grammar_symbol: &[],
    }
}

/// A diagnostic that constrains one or more grammar productions.
const fn dg(
    code: &'static str,
    summary: &'static str,
    grammar_symbol: &'static [&'static str],
) -> DiagnosticInfo {
    DiagnosticInfo {
        code,
        summary,
        grammar_symbol,
    }
}

/// The category segment of a code (the part between the first two dots), e.g.
/// `"types"` for `"karn.types.type_mismatch"`.
pub fn category(code: &str) -> &str {
    code.split('.').nth(1).unwrap_or("")
}

/// A human-readable heading for a category segment.
fn category_title(cat: &str) -> &'static str {
    match cat {
        "agent" | "agents" => "Agents",
        "assert" => "Assertions",
        "boundary" => "Boundaries",
        "capability" => "Capabilities",
        "commit" => "Commit",
        "consumes" => "Consumes",
        "context" => "Contexts",
        "cron" => "Cron",
        "effect" => "Effects",
        "exports" => "Exports",
        "given" => "Given capabilities",
        "http" => "HTTP",
        "lex" => "Lexer",
        "mock" => "Mock and mocks",
        "parse" => "Parser",
        "project" => "Project",
        "provider" => "Providers",
        "queue" => "Queue",
        "record_spread" => "Record spread",
        "refine" => "Refinement",
        "resolve" => "Resolution",
        "service" => "Services",
        "test" => "Tests",
        "types" => "Type checking",
        "uses" => "Uses",
        _ => "Other",
    }
}

/// Render the diagnostic index as a Markdown reference page, grouped by
/// category. This is the generator behind `docs/src/reference/diagnostics.md`.
pub fn render_markdown() -> String {
    use std::collections::BTreeMap;

    // Group codes by their category title, preserving sorted code order.
    let mut by_category: BTreeMap<&str, Vec<&DiagnosticInfo>> = BTreeMap::new();
    for info in REGISTRY {
        by_category
            .entry(category_title(category(info.code)))
            .or_default()
            .push(info);
    }

    let mut out = String::new();
    out.push_str("# Diagnostic index\n\n");
    out.push_str(
        "<!-- GENERATED FILE — do not edit by hand.\n     \
         Source: karnc/src/diagnostics.rs (`render_markdown`).\n     \
         Regenerate with: KARN_BLESS=1 cargo test -p karnc --test diagnostics_registry -->\n\n",
    );
    out.push_str(
        "Every diagnostic code the compiler can emit, with a one-line summary of \
         the cause, grouped by category. For step-by-step cause-and-fix guidance \
         on the most common ones, see the [troubleshooting guides](../troubleshooting/index.md).\n\n",
    );
    out.push_str(&format!(
        "There are **{}** codes in total.\n",
        REGISTRY.len()
    ));

    for (title, infos) in &by_category {
        out.push_str(&format!("\n## {title}\n\n"));
        out.push_str("| Code | Summary | Construct |\n|---|---|---|\n");
        for info in infos {
            // The construct column deep-links each governing production to its
            // entry in the annotated grammar reference; generated from
            // `grammar_symbol` (each value is an embeddable rule, so the
            // `#rule-<raw>` anchor resolves — enforced in diagnostics_registry).
            let construct = info
                .grammar_symbol
                .iter()
                .map(|sym| format!("[`{sym}`](grammar.md#rule-{sym})"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                info.code, info.summary, construct
            ));
        }
    }

    out
}

/// Invert the registry into a `{ "<rule>": [ { code, summary }, … ], … }` map,
/// serialised as pretty JSON with sorted keys and sorted codes. Only rules with
/// at least one diagnostic appear. This is the generator behind
/// `docs/grammar-semantics.json`, which the `{{#grammar-semantics <rule>}}`
/// preprocessor directive consumes.
pub fn render_grammar_semantics_json() -> String {
    use std::collections::BTreeMap;

    // REGISTRY is sorted by code, so each rule's vector comes out code-sorted;
    // the BTreeMap gives sorted rule names.
    let mut by_symbol: BTreeMap<&str, Vec<&DiagnosticInfo>> = BTreeMap::new();
    for info in REGISTRY {
        for sym in info.grammar_symbol {
            by_symbol.entry(sym).or_default().push(info);
        }
    }

    let mut map = serde_json::Map::new();
    map.insert(
        "_generated".to_string(),
        serde_json::Value::String(
            "Generated from the grammar_symbol field of karnc/src/diagnostics.rs. \
             Do not edit by hand. Regenerate with: KARN_BLESS=1 cargo test -p \
             karnc --test diagnostics_registry"
                .to_string(),
        ),
    );
    for (sym, infos) in by_symbol {
        let arr: Vec<serde_json::Value> = infos
            .iter()
            .map(|info| serde_json::json!({ "code": info.code, "summary": info.summary }))
            .collect();
        map.insert(sym.to_string(), serde_json::Value::Array(arr));
    }

    let mut s =
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).expect("serialise semantics");
    s.push('\n');
    s
}
