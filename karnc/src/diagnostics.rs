//! Central registry of diagnostic codes.
//!
//! This is the single source of truth for the `karn.*` codes the compiler can
//! emit. The reference page `docs/src/reference/diagnostics.md` is generated
//! from [`render_markdown`], and the test `tests/diagnostics_registry.rs`
//! asserts that this table matches exactly the set of codes used across the
//! compiler source — so a new code cannot be introduced without documenting it
//! here, and a removed code cannot linger in the docs.
//!
//! Each entry is a `(code, summary)` pair. The category shown in the generated
//! reference is derived from the second dotted segment of the code.

/// One documented diagnostic: its stable code and a one-line summary of the
/// cause. Richer "cause and fix" material for the common diagnostics lives in
/// the troubleshooting how-to guides.
pub struct DiagnosticInfo {
    pub code: &'static str,
    pub summary: &'static str,
}

/// Every diagnostic code the compiler emits, sorted by code.
pub const REGISTRY: &[DiagnosticInfo] = &[
    d("karn.agent.construction_arity", "An agent was constructed with the wrong number of key arguments."),
    d("karn.agent.handler_arity", "An agent handler was called with the wrong number of arguments."),
    d("karn.agent.handler_not_found", "Called a handler the agent does not declare."),
    d("karn.agent.key_mismatch", "An agent key argument has the wrong type."),
    d("karn.agent.outside_context", "An `agent` was declared outside a context."),
    d("karn.agent.return_not_effect", "An agent handler's return type is not an `Effect`."),
    d("karn.agents.non_zeroable_state_field", "An agent state field has no zero value, so a fresh key cannot be initialised."),
    d("karn.assert.non_bool", "`assert` was given a non-`Bool` expression."),
    d("karn.assert.outside_test", "`assert` was used outside a test case body."),
    d("karn.boundary.structural_mismatch", "Data crossing a context boundary did not match the expected shape."),
    d("karn.capability.op_arity", "A capability operation was called with the wrong number of arguments."),
    d("karn.capability.outside_context", "A `capability` was declared outside a context."),
    d("karn.capability.unknown_operation", "Referenced an operation the capability does not declare."),
    d("karn.commit.outside_agent", "`commit` was used outside an agent handler."),
    d("karn.commit.two_reachable_commits", "Two `commit` statements are reachable on the same execution path."),
    d("karn.commit.wrong_state_type", "A `commit` value does not match the agent's state type."),
    d("karn.consumes.alias_conflict", "Two `consumes` aliases collide."),
    d("karn.consumes.in_commons", "`consumes` appears in a `commons` (it is only valid in a context)."),
    d("karn.consumes.name_conflict", "A `consumes` name collides with another name in scope."),
    d("karn.consumes.self_reference", "A context `consumes` itself."),
    d("karn.consumes.service_arity", "A consumed service was called with the wrong number of arguments."),
    d("karn.consumes.target_is_commons", "`consumes` targets a `commons` instead of a context."),
    d("karn.consumes.unknown_context", "`consumes` names a context that does not exist."),
    d("karn.consumes.unknown_service", "Called a service the consumed context does not declare."),
    d("karn.context.consumes_cycle", "Contexts form a `consumes` dependency cycle."),
    d("karn.context.external_construction", "A context-owned type was constructed from outside that context."),
    d("karn.context.opaque_inspection", "An opaquely-exported type was inspected from outside its context."),
    d("karn.cron.bad_params", "An `on cron` handler declares more than one parameter, or a non-`Int` one."),
    d("karn.cron.duplicate_schedule", "Two `on cron` handlers declare the same schedule."),
    d("karn.cron.invalid_schedule", "A cron expression is not five whitespace-separated fields."),
    d("karn.cron.return_not_effect_result", "An `on cron` handler does not return `Effect[Result[(), E]]`."),
    d("karn.effect.bind_in_pure_context", "An `<-` bind was used in a pure (non-effectful) context."),
    d("karn.effect.bind_on_non_effect", "An `<-` bind was applied to a non-`Effect` value."),
    d("karn.effect.capability_in_pure_context", "A capability was used in a pure context."),
    d("karn.effect.cross_context_in_pure_context", "A cross-context call was made in a pure context."),
    d("karn.exports.conflicting_visibility", "A type is exported with conflicting visibilities."),
    d("karn.exports.duplicate_export", "The same name is exported more than once."),
    d("karn.exports.duplicate_in_clause", "A name appears twice in one `exports` clause."),
    d("karn.exports.undeclared_type", "`exports` names a type that is not declared."),
    d("karn.given.undeclared_capability", "A handler uses a capability it did not declare with `given`."),
    d("karn.given.unknown_capability", "`given` names a capability that does not exist."),
    d("karn.given.unused_capability", "A `given` capability is never used (warning)."),
    d("karn.http.body_on_get_or_delete", "A GET or DELETE handler declares a `body` parameter."),
    d("karn.http.duplicate_route", "Two handlers share the same method and route."),
    d("karn.http.extra_param", "A handler parameter is neither a path parameter nor `body`."),
    d("karn.http.invalid_path", "An HTTP route path is malformed."),
    d("karn.http.path_param_not_stringy", "A path parameter's type is not constructible from a string."),
    d("karn.http.reserved_prefix", "A route uses the reserved `/_karn/` prefix."),
    d("karn.http.return_not_effect_http_result", "An HTTP handler does not return `Effect[HttpResult[T]]`."),
    d("karn.http.unbound_path_param", "A `:name` route segment has no matching handler parameter."),
    d("karn.lex.bad_escape", "An invalid escape sequence in a string literal."),
    d("karn.lex.integer_overflow", "An integer literal is out of range."),
    d("karn.lex.unclosed_doc_block", "A documentation block is not closed."),
    d("karn.lex.unexpected_character", "An unexpected character in the source."),
    d("karn.lex.unterminated_string", "A string literal is not terminated."),
    d("karn.mock.arity", "`Mock[T]` was given the wrong number of pin arguments."),
    d("karn.mock.duplicate_target", "A `mocks` target is declared more than once."),
    d("karn.mock.in_commons_test", "`mocks` used in a commons test, where there is no dependency to inject."),
    d("karn.mock.literal_violates", "A pinned `Mock[T]` value violates the type's refinement."),
    d("karn.mock.needs_pin", "A bare `Mock[T]` cannot generate a value (e.g. a `Matches` string); pin one."),
    d("karn.mock.outside_test", "`Mock[T]` was used outside a test case body."),
    d("karn.mock.pin_not_literal", "A `Mock[T]` pin argument is not a compile-time literal."),
    d("karn.mock.pin_unsupported", "A pin was given for a type kind that does not support pinning."),
    d("karn.mock.signature_mismatch", "A `mocks` implementation's signature does not match the capability."),
    d("karn.mock.unknown_target", "`mocks` names a capability that is not in scope."),
    d("karn.mock.unknown_type", "`Mock[T]` names a type that does not resolve."),
    d("karn.mock.unsupported_kind", "`Mock[T]` cannot fabricate a value for this kind of type."),
    d("karn.parse.consumes_after_decls", "`consumes` appears after other declarations."),
    d("karn.parse.cron_in_agent", "An `on cron` handler was declared in an agent."),
    d("karn.parse.empty_agent", "An `agent` body is empty."),
    d("karn.parse.empty_capability", "A `capability` body is empty."),
    d("karn.parse.empty_match", "A `match` has no arms."),
    d("karn.parse.empty_mock_body", "A `mocks` body is empty."),
    d("karn.parse.empty_service", "A `service` body is empty."),
    d("karn.parse.expected_agent_key", "Expected a `key` declaration in an agent."),
    d("karn.parse.expected_base_type", "Expected a base type."),
    d("karn.parse.expected_capability_op", "Expected a capability operation."),
    d("karn.parse.expected_expression", "Expected an expression."),
    d("karn.parse.expected_handler", "Expected a handler."),
    d("karn.parse.expected_item", "Expected a declaration."),
    d("karn.parse.expected_predicate", "Expected a refinement predicate."),
    d("karn.parse.expected_provider_op", "Expected a provider operation."),
    d("karn.parse.expected_token", "Expected a specific token."),
    d("karn.parse.expected_type", "Expected a type."),
    d("karn.parse.expected_unit_header", "Expected a `commons` or `context` header."),
    d("karn.parse.expected_visibility", "Expected a visibility keyword."),
    d("karn.parse.exports_after_decls", "`exports` appears after other declarations."),
    d("karn.parse.extra_tokens", "Unexpected tokens after an otherwise complete construct."),
    d("karn.parse.generic_arg_count", "Wrong number of generic type arguments."),
    d("karn.parse.http_in_agent", "An `on http` handler was declared in an agent."),
    d("karn.parse.non_associative", "A non-associative operator was chained (e.g. `a == b == c`)."),
    d("karn.parse.orphan_doc_block", "A documentation block is not attached to a declaration (warning)."),
    d("karn.parse.reserved_keyword", "A reserved keyword was used as an identifier."),
    d("karn.parse.reserved_syntax", "Use of syntax reserved for a future version (e.g. `[` for generics)."),
    d("karn.parse.self_outside_method", "`self` used outside a method or handler."),
    d("karn.parse.unexpected_context", "A `context` appeared where it is not allowed."),
    d("karn.parse.unexpected_eof", "Unexpected end of input."),
    d("karn.parse.unexpected_test", "A `test` appeared where it is not allowed."),
    d("karn.parse.unknown_effect_method", "An unknown method on `Effect`."),
    d("karn.parse.unknown_handler_kind", "An unknown handler kind (expected `call` or `http`)."),
    d("karn.parse.unknown_http_method", "An unknown HTTP method."),
    d("karn.parse.unknown_predicate", "An unknown refinement predicate."),
    d("karn.parse.uses_after_decls", "`uses` appears after other declarations."),
    d("karn.project.file_and_directory", "A unit exists as both a file and a directory."),
    d("karn.project.inconsistent_commons_name", "A source file's path does not match its declared name."),
    d("karn.project.inconsistent_test_path", "A test file's path does not match its target's name."),
    d("karn.project.kind_conflict", "A name is declared as both a commons and a context."),
    d("karn.project.no_root", "No project root could be determined."),
    d("karn.project.no_sources", "The project contains no source files."),
    d("karn.project.read_failed", "A source file could not be read."),
    d("karn.provider.extra_operation", "A `provides` block implements an operation not in the capability."),
    d("karn.provider.missing_operation", "A `provides` block is missing a capability operation."),
    d("karn.provider.outside_context", "`provides` was declared outside a context."),
    d("karn.provider.signature_mismatch", "A `provides` operation's signature does not match the capability."),
    d("karn.provider.unknown_capability", "`provides` names a capability that does not exist."),
    d("karn.record_spread.field_type_mismatch", "A record-spread override has the wrong type for the field."),
    d("karn.record_spread.non_record_base", "The base of a record spread is not a record."),
    d("karn.record_spread.type_mismatch", "A record spread's base is a different record type."),
    d("karn.record_spread.unknown_field", "A record spread overrides a field the record does not have."),
    d("karn.refine.literal_violates", "A literal does not satisfy the refined type's predicate."),
    d("karn.resolve.ambiguous_variant", "A variant name is ambiguous across several sum types."),
    d("karn.resolve.arity_mismatch", "A function was called with the wrong number of arguments."),
    d("karn.resolve.duplicate_agent", "Two agents share a name."),
    d("karn.resolve.duplicate_capability", "Two capabilities share a name."),
    d("karn.resolve.duplicate_field", "A record declares a field twice."),
    d("karn.resolve.duplicate_field_init", "A record construction initialises a field twice."),
    d("karn.resolve.duplicate_fn", "Two functions share a name."),
    d("karn.resolve.duplicate_method", "Two methods share a name."),
    d("karn.resolve.duplicate_param", "A parameter name is repeated."),
    d("karn.resolve.duplicate_provider", "A capability is provided more than once."),
    d("karn.resolve.duplicate_service", "Two services share a name."),
    d("karn.resolve.duplicate_type", "Two types share a name."),
    d("karn.resolve.duplicate_variant", "A sum type declares a variant twice."),
    d("karn.resolve.fn_without_call", "A function was referenced without being called."),
    d("karn.resolve.let_shadows_fn", "A `let` binding shadows a function."),
    d("karn.resolve.let_shadows_type", "A `let` binding shadows a type."),
    d("karn.resolve.method_unknown_type", "A method is defined on an unknown type."),
    d("karn.resolve.missing_field", "A record construction omits a required field."),
    d("karn.resolve.name_conflict", "Two declarations share a name."),
    d("karn.resolve.not_a_record_type", "Record syntax was used on a non-record type."),
    d("karn.resolve.opaque_record_construction", "An opaque type was constructed with record syntax."),
    d("karn.resolve.param_as_function", "A value (such as a parameter) was called as a function."),
    d("karn.resolve.recursive_record_field", "A record directly contains a field of its own type."),
    d("karn.resolve.self_outside_method", "`self` referenced outside a method or handler."),
    d("karn.resolve.type_as_function", "A type name was called as if it were a function."),
    d("karn.resolve.type_in_expr", "A type name was used where a value is expected."),
    d("karn.resolve.unconsumed_context", "A context's service was called without a `consumes` declaration."),
    d("karn.resolve.unknown_field", "Accessed a field the record does not have."),
    d("karn.resolve.unknown_function", "Called a function that does not exist."),
    d("karn.resolve.unknown_name", "Referenced a name that is not in scope."),
    d("karn.resolve.unknown_static_member", "Referenced an unknown static member (e.g. `T.x`)."),
    d("karn.resolve.unknown_type", "Referenced a type that does not exist."),
    d("karn.service.outside_context", "A `service` was declared outside a context."),
    d("karn.service.return_not_effect", "A service handler's return type is not an `Effect`."),
    d("karn.test.duplicate_case_name", "Two test cases share a description."),
    d("karn.test.unknown_target", "A `test` block targets a unit that does not exist."),
    d("karn.types.ambiguous_constructor", "`Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it."),
    d("karn.types.argument_mismatch", "A function argument has the wrong type."),
    d("karn.types.cannot_infer_option_type_param", "The value type of `None` could not be inferred."),
    d("karn.types.cannot_infer_result_type_params", "The type parameters of a `Result` could not be inferred."),
    d("karn.types.constructor_arity", "A variant constructor got the wrong number of arguments."),
    d("karn.types.constructor_base_mismatch", "A `.of` constructor was given an argument of the wrong base type."),
    d("karn.types.duplicate_variant_arm", "A `match` has two arms for the same variant."),
    d("karn.types.empty_refinement", "A refinement admits no values (contradictory predicates)."),
    d("karn.types.err_value_mismatch", "An `Err` payload has the wrong type."),
    d("karn.types.field_access_on_non_record", "Field access on a value that is not a record."),
    d("karn.types.field_refinement_not_base", "An inline field refinement requires a base or refined type."),
    d("karn.types.field_value_mismatch", "A record field was given a value of the wrong type."),
    d("karn.types.if_branch_mismatch", "The branches of an `if` have different types."),
    d("karn.types.if_non_bool_cond", "An `if` condition is not a `Bool`."),
    d("karn.types.invalid_regex", "A `Matches` predicate contains an invalid regular expression."),
    d("karn.types.inverted_range", "An `InRange` predicate has its bounds inverted."),
    d("karn.types.is_non_sum", "`is` was applied to a value that is not a sum type."),
    d("karn.types.is_unknown_variant", "`is` names a variant the type does not have."),
    d("karn.types.let_annotation_mismatch", "A `let` value does not match its type annotation."),
    d("karn.types.match_arm_mismatch", "A `match` arm has a different type from the others."),
    d("karn.types.match_non_sum_discriminant", "`match` was applied to a value that is not a sum type."),
    d("karn.types.method_arity", "A method was called with the wrong number of arguments."),
    d("karn.types.method_not_found", "Called a method the type does not have."),
    d("karn.types.method_on_non_named_type", "A method was called on a built-in type that has no methods."),
    d("karn.types.mixed_pattern_bindings", "A pattern mixes named and positional bindings."),
    d("karn.types.negative_length", "A length predicate was given a negative value."),
    d("karn.types.non_exhaustive_match", "A `match` does not cover every variant."),
    d("karn.types.ok_value_mismatch", "An `Ok` payload has the wrong type."),
    d("karn.types.opaque_raw_outside", "`.raw` on an opaque type was used outside its defining commons."),
    d("karn.types.opaque_record_construction", "An opaque type was constructed with record syntax."),
    d("karn.types.opaque_unsafe_outside", "`.unsafe` on an opaque type was used outside its defining context."),
    d("karn.types.pattern_arity", "A pattern binds the wrong number of payload fields."),
    d("karn.types.pattern_type_mismatch", "A pattern's type does not match the matched value."),
    d("karn.types.predicate_base_mismatch", "A predicate does not apply to the type's base (e.g. a string predicate on an `Int`)."),
    d("karn.types.question_error_mismatch", "`?` propagates an error type incompatible with the function's."),
    d("karn.types.question_on_non_result", "`?` was applied to a non-`Result` value."),
    d("karn.types.question_outside_result", "`?` used in a function that does not return a `Result`."),
    d("karn.types.return_mismatch", "A returned value does not match the declared return type."),
    d("karn.types.some_value_mismatch", "A `Some` payload has the wrong type."),
    d("karn.types.type_mismatch", "Two types that were required to match did not."),
    d("karn.types.unknown_field", "Referenced a field the record type does not declare."),
    d("karn.types.unknown_pattern_field", "A pattern names a field the variant does not have."),
    d("karn.types.unknown_static_member", "Referenced an unknown static member on a type."),
    d("karn.types.unknown_variant_in_pattern", "A pattern names a variant the sum type does not have."),
    d("karn.types.unreachable_arm", "A `match` arm is unreachable."),
    d("karn.types.variant_arity", "A variant constructor got the wrong number of payload values."),
    d("karn.types.variant_missing_payload", "A variant requiring a payload was used without one."),
    d("karn.types.variant_payload_mismatch", "A variant payload has the wrong type."),
    d("karn.uses.name_conflict", "A `uses` name collides with another name."),
    d("karn.uses.self_reference", "A commons `uses` itself."),
    d("karn.uses.target_is_context", "`uses` targets a context instead of a commons."),
    d("karn.uses.unknown_commons", "`uses` names a commons that does not exist."),
];

const fn d(code: &'static str, summary: &'static str) -> DiagnosticInfo {
    DiagnosticInfo { code, summary }
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
         on the most common ones, see the [troubleshooting guides](../how-to/troubleshooting/index.md).\n\n",
    );
    out.push_str(&format!("There are **{}** codes in total.\n", REGISTRY.len()));

    for (title, infos) in &by_category {
        out.push_str(&format!("\n## {title}\n\n"));
        out.push_str("| Code | Summary |\n|---|---|\n");
        for info in infos {
            out.push_str(&format!("| `{}` | {} |\n", info.code, info.summary));
        }
    }

    out
}
