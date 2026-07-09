//! Central registry of diagnostic codes.
//!
//! This is the single source of truth for the `bynk.*` codes the compiler can
//! emit. The reference page `site/src/content/docs/book/reference/diagnostics.md` is generated
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
    /// (e.g. `bynk.boundary.structural_mismatch`). Every non-empty name is
    /// checked against the grammar by `tests/diagnostics_registry.rs`.
    pub grammar_symbol: &'static [&'static str],
}

/// Every diagnostic code the compiler emits, sorted by code.
pub const REGISTRY: &[DiagnosticInfo] = &[
    d(
        "bynk.actor.bearer_identity_not_string_constructible",
        "A `Bearer` actor's identity is not a string-constructible type.",
    ),
    d(
        "bynk.actor.bearer_missing_secret",
        "A `Bearer` actor does not name its signing secret.",
    ),
    d(
        "bynk.actor.binder_shadows_param",
        "A `by` actor binder collides with a handler parameter of the same name.",
    ),
    d(
        "bynk.actor.by_on_agent",
        "A `by` actor clause was placed on an agent `on call` handler, which has no actor.",
    ),
    d(
        "bynk.actor.duplicate_sum_scheme",
        "Two peers in a multi-actor sum share an authentication scheme.",
    ),
    d(
        "bynk.actor.identity_not_sealed",
        "An actor identity type is not a context-ownable (sealed) value type.",
    ),
    d(
        "bynk.actor.missing_by_on_http",
        "An HTTP handler lacks the required `by` actor clause.",
    ),
    d(
        "bynk.actor.oidc_identity_not_string_constructible",
        "An `Oidc` actor's identity is not a string-constructible type.",
    ),
    d(
        "bynk.actor.oidc_missing_audience",
        "An `Oidc` actor does not name its `audience`.",
    ),
    d(
        "bynk.actor.oidc_missing_issuer",
        "An `Oidc` actor does not name its `issuer`.",
    ),
    d(
        "bynk.actor.oidc_missing_jwks",
        "An `Oidc` actor does not name its `jwks` endpoint.",
    ),
    d(
        "bynk.actor.oidc_not_in_sum",
        "An `Oidc` actor appears as a member of a multi-actor sum.",
    ),
    d(
        "bynk.actor.outside_context",
        "An `actor` was declared outside a context (e.g. in a commons).",
    ),
    d(
        "bynk.actor.refinement_base_unsupported",
        "A refinement actor's base is not a `Bearer` actor (no claims to authorise against).",
    ),
    d(
        "bynk.actor.refinement_in_sum",
        "A refinement actor appears as a member of a multi-actor sum.",
    ),
    d(
        "bynk.actor.refinement_predicate_unsupported",
        "A refinement actor's `where` predicate is outside the closed claim-predicate set.",
    ),
    d(
        "bynk.actor.scheme_not_admissible",
        "An actor's scheme is not admissible on this handler's protocol.",
    ),
    d(
        "bynk.actor.signature_identity_unsupported",
        "A `Signature` actor declared an `identity`, which is not yet supported.",
    ),
    d(
        "bynk.actor.signature_missing_header",
        "A `Signature` actor does not name its signature header.",
    ),
    d(
        "bynk.actor.signature_missing_secret",
        "A `Signature` actor does not name its signing secret.",
    ),
    d(
        "bynk.actor.signature_requires_body",
        "A `Signature` handler does not take a `body` parameter.",
    ),
    d(
        "bynk.actor.signature_tolerance_without_timestamp",
        "A `Signature` actor set `tolerance` without a `timestamp` header.",
    ),
    d(
        "bynk.actor.sum_requires_binder",
        "A multi-actor sum `by` clause has no binder to match the resolved actor.",
    ),
    d(
        "bynk.actor.unknown_actor",
        "A handler's `by` clause names an actor that is not declared.",
    ),
    d(
        "bynk.actor.unknown_scheme",
        "An actor declares an authentication scheme that is not compiler-known.",
    ),
    d(
        "bynk.actor.unreachable_sum_arm",
        "A multi-actor sum has an arm unreachable after a catch-all (`None`) peer.",
    ),
    dg(
        "bynk.adapter.consumes_context",
        "An `adapter` consumed a context; adapter dependencies are adapter-to-adapter.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.adapter.consumes_requires_selection",
        "An `adapter` used a whole-unit or aliased `consumes`; adapters must select capabilities with `consumes U { Cap, … }`.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.adapter.disallowed_item",
        "An `adapter` declared a `service`, `agent`, or other item it may not contain.",
        &["adapter_decl"],
    ),
    dg(
        "bynk.adapter.duplicate_binding",
        "An `adapter` declared more than one `binding` clause.",
        &["binding_decl"],
    ),
    dg(
        "bynk.adapter.no_binding",
        "An `adapter` declares an external provider but no `binding` module to supply it.",
        &["adapter_decl"],
    ),
    dg(
        "bynk.adapter.provider_has_body",
        "A provider inside an `adapter` has a Bynk body; adapter providers must be external.",
        &["provider_decl"],
    ),
    dg(
        "bynk.agent.construction_arity",
        "An agent was constructed with the wrong number of key arguments.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agent.handler_arity",
        "An agent handler was called with the wrong number of arguments.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agent.handler_not_found",
        "Called a handler the agent does not declare.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agent.key_mismatch",
        "An agent key argument has the wrong type.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agent.outside_context",
        "An `agent` was declared outside a context.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agent.return_not_effect",
        "An agent handler's return type is not an `Effect`.",
        &["agent_decl"],
    ),
    dg(
        "bynk.agents.bad_state_initialiser",
        "An agent `store` field initialiser is not a static value of the field's type.",
        &["store_field"],
    ),
    dg(
        "bynk.agents.non_zeroable_state_field",
        "An agent `store` field has no initialiser and no implicit zero value.",
        &["store_field"],
    ),
    d(
        "bynk.boundary.structural_mismatch",
        "Data crossing a context boundary did not match the expected shape.",
    ),
    dg(
        "bynk.capability.op_arity",
        "A capability operation was called with the wrong number of arguments.",
        &["capability_decl"],
    ),
    dg(
        "bynk.capability.outside_context",
        "A `capability` was declared outside a context.",
        &["capability_decl"],
    ),
    dg(
        "bynk.capability.unknown_operation",
        "Referenced an operation the capability does not declare.",
        &["capability_decl"],
    ),
    d(
        "bynk.cell.invalid_target",
        "A `:=` write targets something that is not a `store Cell` field.",
    ),
    d(
        "bynk.cell.self_reference",
        "A `:=` right-hand side reads the cell being written (a read-modify-write); use `.update`.",
    ),
    dg(
        "bynk.consumes.alias_conflict",
        "Two `consumes` aliases collide.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.capability_name_clash",
        "Two flattened `consumes U { Cap }` capabilities collide, or one clashes with a local capability.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.in_commons",
        "`consumes` appears in a `commons` (it is only valid in a context).",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.name_conflict",
        "A `consumes` name collides with another name in scope.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.self_reference",
        "A context `consumes` itself.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.service_arity",
        "A consumed service was called with the wrong number of arguments.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.target_is_commons",
        "`consumes` targets a `commons` instead of a context.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.unknown_context",
        "`consumes` names a context that does not exist.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.consumes.unknown_service",
        "Called a service the consumed context does not declare.",
        &["consumes_decl"],
    ),
    d(
        "bynk.context.consumes_cycle",
        "Contexts form a `consumes` dependency cycle.",
    ),
    d(
        "bynk.context.external_construction",
        "A context-owned type was constructed from outside that context.",
    ),
    dg(
        "bynk.context.external_provider",
        "A bodiless (external) provider was declared outside an `adapter`.",
        &["provider_decl"],
    ),
    d(
        "bynk.context.opaque_inspection",
        "An opaquely-exported type was inspected from outside its context.",
    ),
    d(
        "bynk.contract.duplicate_name",
        "A function declares two contract clauses (`requires`/`ensures`) with the same name.",
    ),
    d(
        "bynk.contract.impure_predicate",
        "A contract predicate uses an effectful or test-only construct; a contract clause must be pure.",
    ),
    d(
        "bynk.contract.not_bool",
        "A contract predicate does not have type `Bool`.",
    ),
    d(
        "bynk.contract.restated_by_test",
        "A `case`/`property` merely restates a contract clause already declared at the function; the test is redundant.",
    ),
    d(
        "bynk.contract.result_in_requires",
        "A precondition (`requires`) references `result`; the return value is only in scope inside an `ensures`.",
    ),
    dg(
        "bynk.cron.bad_params",
        "A cron handler declares more than one parameter, or a non-`Int` one.",
        &["cron_handler"],
    ),
    dg(
        "bynk.cron.duplicate_schedule",
        "Two cron handlers declare the same schedule.",
        &["cron_handler"],
    ),
    dg(
        "bynk.cron.invalid_schedule",
        "A cron expression is not five whitespace-separated fields.",
        &["cron_handler"],
    ),
    dg(
        "bynk.cron.return_not_effect_result",
        "A cron handler does not return `Effect[Result[(), E]]`.",
        &["cron_handler"],
    ),
    d(
        "bynk.duration.literal_overflow",
        "A `Duration` literal (`<int>.<unit>`) exceeds the representable millisecond range.",
    ),
    dg(
        "bynk.effect.bind_in_pure_context",
        "An `<-` bind was used in a pure (non-effectful) context.",
        &["effect_let_stmt"],
    ),
    dg(
        "bynk.effect.bind_on_non_effect",
        "An `<-` bind was applied to a non-`Effect` value.",
        &["effect_let_stmt"],
    ),
    d(
        "bynk.effect.capability_in_pure_context",
        "A capability was used in a pure context.",
    ),
    d(
        "bynk.effect.cross_context_in_pure_context",
        "A cross-context call was made in a pure context.",
    ),
    dg(
        "bynk.effect.do_in_pure_context",
        "A `do` statement was used in a pure (non-effectful) context.",
        &["do_stmt"],
    ),
    dg(
        "bynk.effect.do_on_non_effect",
        "A `do` statement was applied to a non-`Effect` value.",
        &["do_stmt"],
    ),
    dg(
        "bynk.effect.do_requires_unit",
        "A `do` statement was applied to a valued `Effect[T]`; `do` performs a unit effect, so a real result would be dropped — use `let _ <- e` instead.",
        &["do_stmt"],
    ),
    dg(
        "bynk.effect.fn_value_in_pure_context",
        "An effectful function value was called in a pure context; like a capability call, it is legal only where the enclosing body is effectful.",
        &["call"],
    ),
    dg(
        "bynk.expect.not_bool",
        "`expect` was given a non-`Bool` predicate.",
        &["expect_expr"],
    ),
    dg(
        "bynk.expect.outside_case",
        "`expect` was used outside a `case` body.",
        &["expect_expr"],
    ),
    dg(
        "bynk.exports.capability_not_provided",
        "An exported capability has no provider in its context.",
        &["exports_decl"],
    ),
    dg(
        "bynk.exports.conflicting_visibility",
        "A type is exported with conflicting visibilities.",
        &["exports_decl"],
    ),
    dg(
        "bynk.exports.duplicate_export",
        "The same name is exported more than once.",
        &["exports_decl"],
    ),
    dg(
        "bynk.exports.duplicate_in_clause",
        "A name appears twice in one `exports` clause.",
        &["exports_decl"],
    ),
    dg(
        "bynk.exports.undeclared_capability",
        "`exports capability` names a capability that is not declared.",
        &["exports_decl"],
    ),
    dg(
        "bynk.exports.undeclared_type",
        "`exports` names a type that is not declared.",
        &["exports_decl"],
    ),
    dg(
        "bynk.generics.no_bounds",
        "A type parameter carries a bound (`[A: …]`); bounded generics are not in v0.20a.",
        &["fn_decl"],
    ),
    dg(
        "bynk.generics.no_generic_types",
        "A `type` declaration carries a type-parameter list; generic type declarations are not in v0.20a (type parameters belong to functions).",
        &["type_decl"],
    ),
    dg(
        "bynk.generics.type_arg_mismatch",
        "Inferred or explicit type arguments conflict, have the wrong arity, target a non-generic function, or a type parameter shadows a declared type.",
        &["call"],
    ),
    dg(
        "bynk.generics.uninferable_type_arg",
        "A generic function's type parameter could not be inferred from the arguments and was not given explicitly (`name[T](…)`); a bare generic function also cannot be passed as a value in v0.20a.",
        &["call"],
    ),
    dg(
        "bynk.given.cross_context_unknown_capability",
        "`given B.Cap` names a capability the consumed context does not export.",
        &["given_clause"],
    ),
    dg(
        "bynk.given.undeclared_capability",
        "A handler uses a capability it did not declare with `given`.",
        &["given_clause"],
    ),
    dg(
        "bynk.given.unknown_capability",
        "`given` names a capability that does not exist.",
        &["given_clause"],
    ),
    dg(
        "bynk.given.unused_capability",
        "A `given` capability is never used (warning).",
        &["given_clause"],
    ),
    d(
        "bynk.held.branch_divergence",
        "Branches of a conditional leave a held value (e.g. `Connection[F]`) in inconsistent ownership states — one consumes or stores it, another leaves it owned (§2.9.5, real-time track slice 2).",
    ),
    d(
        "bynk.held.consume_on_borrow",
        "A consuming operation (`close`/`put`/`take`) is called on a *borrowed* held reference — borrows admit only non-consuming operations like `send` (§2.9.3, real-time track slice 2).",
    ),
    d(
        "bynk.held.leak",
        "A held value (`Connection[F]`) is still owned at scope exit — it must be disposed (stored, closed, or transferred) before the handler returns (§2.9.1, real-time track slice 2).",
    ),
    d(
        "bynk.held.unsupported_map_op",
        "A held `Map[K, Connection]` is given an `update`/`upsert` — a held resource cannot be transformed by a `(Connection) -> Connection` function; use `put`/`get`/`remove` (real-time track slice 3b-ii).",
    ),
    d(
        "bynk.held.unsupported_storage",
        "A held value (`Connection[F]`) is stored in a `Set`/`Log`/`Cache` — held values may only live in `Cell[Option[Connection]]` or `Map[K, Connection]` (§2.9.3, real-time track slice 2).",
    ),
    d(
        "bynk.held.use_after_consume",
        "A held value (`Connection[F]`) is used after a consuming operation (`close`/`put`/`take`) ended its lifetime (§2.9.2, real-time track slice 2).",
    ),
    d(
        "bynk.history.not_an_agent",
        "A `for all run: History[T]` names a `T` that is not an agent — only an agent has handlers to sequence and reachable states to observe (testing track slice 7, ADR 0155).",
    ),
    d(
        "bynk.history.not_generable",
        "A `for all run: History[Agent]` targets an agent with a handler parameter whose type cannot be generated (e.g. a `Matches` refinement), so its call-history cannot be driven (testing track slice 7, ADR 0155).",
    ),
    d(
        "bynk.history.outside_property",
        "`History[Agent]` appears outside a `property`'s `for all` binding — it is a test-only generator, not a value type (testing track slice 7, ADR 0155).",
    ),
    d(
        "bynk.history.restates_invariant",
        "A history property merely re-checks a guarantee a declared `invariant`/`transition` already enforces on every reached state (testing track slice 7, ADR 0155).",
    ),
    dg(
        "bynk.http.body_on_get_or_delete",
        "A GET or DELETE handler declares a `body` parameter.",
        &["http_handler"],
    ),
    d(
        "bynk.http.cache_bad_max_age",
        "A `@cache` annotation's `maxAge` is missing or not a positive `Duration` literal.",
    ),
    d(
        "bynk.http.cache_bad_scope",
        "A `@cache` annotation's `scope` is not `public` or `private`.",
    ),
    d(
        "bynk.http.cache_duplicate",
        "A handler carries more than one `@cache` annotation.",
    ),
    d(
        "bynk.http.cache_on_non_get",
        "A `@cache` annotation is placed on a handler that is not `on http GET`.",
    ),
    d(
        "bynk.http.cache_unknown_arg",
        "A `@cache` annotation has an argument outside the closed set (`maxAge`/`scope`).",
    ),
    d(
        "bynk.http.cors_invalid_field",
        "A `cors` policy field (`headers`/`credentials`/`maxAge`) has the wrong value shape.",
    ),
    d(
        "bynk.http.cors_invalid_origins",
        "A `cors` policy's `origins` is missing, empty, or not a list of string literals.",
    ),
    d(
        "bynk.http.cors_not_http",
        "A `cors { }` policy appears on a service that is not `from http`.",
    ),
    d(
        "bynk.http.cors_unknown_field",
        "A `cors { }` policy declares a field outside the closed set.",
    ),
    d(
        "bynk.http.cors_wildcard_credentials",
        "A `cors` policy combines `credentials: true` with the wildcard origin `[\"*\"]`.",
    ),
    dg(
        "bynk.http.duplicate_route",
        "Two handlers share the same method and route.",
        &["http_handler"],
    ),
    dg(
        "bynk.http.extra_param",
        "A handler parameter is neither a path parameter nor `body`.",
        &["http_handler"],
    ),
    dg(
        "bynk.http.invalid_path",
        "An HTTP route path is malformed.",
        &["http_handler"],
    ),
    d(
        "bynk.http.limit_bad_max_body",
        "A `@limit` annotation's `maxBody` is missing or not a positive `Int` literal.",
    ),
    d(
        "bynk.http.limit_duplicate",
        "A handler carries more than one `@limit` annotation.",
    ),
    d(
        "bynk.http.limit_on_bodyless",
        "A `@limit` annotation is placed on a handler that takes no body (a GET or DELETE).",
    ),
    d(
        "bynk.http.limit_unknown_arg",
        "A `@limit` annotation has an argument outside the closed set (`maxBody`).",
    ),
    d(
        "bynk.http.limits_invalid_field",
        "A `limits` policy field (`maxBody`) has the wrong value shape.",
    ),
    d(
        "bynk.http.limits_not_http",
        "A `limits { }` policy appears on a service that is not `from http`.",
    ),
    d(
        "bynk.http.limits_unknown_field",
        "A `limits { }` policy declares a field outside the closed set.",
    ),
    dg(
        "bynk.http.path_param_not_stringy",
        "A path parameter's type is not constructible from a string.",
        &["http_handler"],
    ),
    dg(
        "bynk.http.reserved_prefix",
        "A route uses the reserved `/_bynk/` prefix.",
        &["http_handler"],
    ),
    dg(
        "bynk.http.return_not_effect_http_result",
        "An HTTP handler does not return `Effect[HttpResult[T]]`.",
        &["http_handler"],
    ),
    d(
        "bynk.http.security_invalid_field",
        "A `security` policy field (`hsts`/`nosniff`) has the wrong value shape.",
    ),
    d(
        "bynk.http.security_not_http",
        "A `security { }` policy appears on a service that is not `from http`.",
    ),
    d(
        "bynk.http.security_unknown_field",
        "A `security { }` policy declares a field outside the closed set.",
    ),
    dg(
        "bynk.http.unbound_path_param",
        "A `:name` route segment has no matching handler parameter.",
        &["http_handler"],
    ),
    d(
        "bynk.http.unknown_handler_annotation",
        "A handler carries an annotation outside the closed set (`@cache`/`@limit`).",
    ),
    d(
        "bynk.index.bad_argument",
        "An `@indexed` argument is not a `by: <field>` label.",
    ),
    d(
        "bynk.index.missing",
        "A query filters a map by equality on a field that is not `@indexed` (a perf-hint warning).",
    ),
    d(
        "bynk.index.unkeyable_key",
        "An `@indexed(by: k)` field is not value-keyable.",
    ),
    d(
        "bynk.index.unknown_key",
        "An `@indexed(by: k)` field is not a field of the map's value type.",
    ),
    d(
        "bynk.index.unused",
        "A declared `@indexed(by: k)` is never used by an equality filter (a hygiene warning).",
    ),
    d(
        "bynk.invariant.cross_agent_reference",
        "An invariant predicate references another agent; invariants are per-agent.",
    ),
    d(
        "bynk.invariant.duplicate_name",
        "An agent declares two invariants with the same name.",
    ),
    d(
        "bynk.invariant.impure_predicate",
        "An invariant predicate uses an effectful or test-only construct.",
    ),
    d(
        "bynk.invariant.not_bool",
        "An invariant predicate does not have type `Bool`.",
    ),
    dg(
        "bynk.lambda.unannotated_param",
        "A lambda parameter has no type annotation in a position where no function type is expected to infer it from.",
        &["lambda_expr"],
    ),
    dg(
        "bynk.lex.bad_escape",
        "An invalid escape sequence in a string literal.",
        &["string_literal"],
    ),
    dg(
        "bynk.lex.float_literal_overflow",
        "A float literal does not fit a finite 64-bit float.",
        &["float_literal"],
    ),
    dg(
        "bynk.lex.integer_overflow",
        "An integer literal is out of range.",
        &["number_literal"],
    ),
    d(
        "bynk.lex.unclosed_doc_block",
        "A documentation block is not closed.",
    ),
    d(
        "bynk.lex.unexpected_character",
        "An unexpected character in the source.",
    ),
    dg(
        "bynk.lex.unterminated_interpolation",
        "An interpolation hole `\\(…)` is not closed on its line.",
        &["string_literal"],
    ),
    dg(
        "bynk.lex.unterminated_string",
        "A string literal is not terminated.",
        &["string_literal"],
    ),
    d(
        "bynk.list.deprecated_function",
        "A `bynk.list` free function (`map`/`filter`/`find`/`any`/`all`) is deprecated in favour of the `List` method form (warning; auto-fixable).",
    ),
    d(
        "bynk.namespace.reserved",
        "A user unit is named `bynk` or `bynk.*`; the `bynk` root is reserved for the toolchain.",
    ),
    d(
        "bynk.observe.bad_count",
        "An observation call count is not a non-negative integer literal (`called once` / `called <n> times`).",
    ),
    d(
        "bynk.observe.impure_with",
        "A `with` predicate uses an effectful or test-only construct; it must be pure.",
    ),
    d(
        "bynk.observe.not_a_seam",
        "An observation targets a capability the unit under test does not consume.",
    ),
    d(
        "bynk.observe.outside_case",
        "An observation appears outside a `case` body.",
    ),
    d(
        "bynk.observe.trace_outside_test",
        "`trace(Cap.op)` appears outside a `case` body.",
    ),
    d(
        "bynk.observe.unknown_op",
        "An observation names an operation the capability does not declare.",
    ),
    d(
        "bynk.observe.with_not_bool",
        "A `with` predicate does not have type `Bool`.",
    ),
    dg(
        "bynk.parse.consumes_after_decls",
        "`consumes` appears after other declarations.",
        &["consumes_decl"],
    ),
    d(
        "bynk.parse.dangling_handler_annotation",
        "A handler-position annotation (e.g. `@cache`) is not followed by an `on` handler.",
    ),
    dg(
        "bynk.parse.duplicate_cors",
        "A service declares more than one `cors { }` policy.",
        &["service_decl"],
    ),
    dg(
        "bynk.parse.duplicate_limits",
        "A service declares more than one `limits { }` policy.",
        &["service_decl"],
    ),
    dg(
        "bynk.parse.duplicate_security",
        "A service declares more than one `security { }` policy.",
        &["service_decl"],
    ),
    dg(
        "bynk.parse.empty_agent",
        "An `agent` body is empty.",
        &["agent_decl"],
    ),
    dg(
        "bynk.parse.empty_capability",
        "A `capability` body is empty.",
        &["capability_decl"],
    ),
    d(
        "bynk.parse.empty_interpolation",
        "An interpolation hole `\\(…)` contains no expression.",
    ),
    dg(
        "bynk.parse.empty_match",
        "A `match` has no arms.",
        &["match_expr"],
    ),
    dg(
        "bynk.parse.empty_service",
        "A `service` body is empty.",
        &["service_decl"],
    ),
    dg(
        "bynk.parse.expected_agent_key",
        "Expected a `key` declaration in an agent.",
        &["agent_decl"],
    ),
    d(
        "bynk.parse.expected_agent_storage",
        "An agent declares no storage — it has no `store` fields.",
    ),
    dg(
        "bynk.parse.expected_base_type",
        "Expected a base type.",
        &["base_type"],
    ),
    dg(
        "bynk.parse.expected_capability_op",
        "Expected a capability operation.",
        &["capability_op"],
    ),
    d("bynk.parse.expected_expression", "Expected an expression."),
    dg(
        "bynk.parse.expected_handler",
        "Expected a handler.",
        &["handler"],
    ),
    d("bynk.parse.expected_item", "Expected a declaration."),
    dg(
        "bynk.parse.expected_predicate",
        "Expected a refinement predicate.",
        &["refinement"],
    ),
    dg(
        "bynk.parse.expected_provider_op",
        "Expected a provider operation.",
        &["provider_op"],
    ),
    d("bynk.parse.expected_token", "Expected a specific token."),
    d("bynk.parse.expected_type", "Expected a type."),
    d(
        "bynk.parse.expected_unit_header",
        "Expected a `commons` or `context` header.",
    ),
    dg(
        "bynk.parse.expected_visibility",
        "Expected a visibility keyword.",
        &["exports_decl"],
    ),
    dg(
        "bynk.parse.exports_after_decls",
        "`exports` appears after other declarations.",
        &["exports_decl"],
    ),
    d(
        "bynk.parse.extra_tokens",
        "Unexpected tokens after an otherwise complete construct.",
    ),
    dg(
        "bynk.parse.generic_arg_count",
        "Wrong number of generic type arguments.",
        &["generic_type_ref"],
    ),
    dg(
        "bynk.parse.handler_in_agent",
        "A protocol handler (`on GET`/`schedule`/`message`) was declared in an agent.",
        &["handler"],
    ),
    d(
        "bynk.parse.invariant_after_handler",
        "An `invariant` was declared after a handler; invariants precede handlers.",
    ),
    dg(
        "bynk.parse.malformed_float_literal",
        "A float literal is missing a digit on one side of the `.` (`1.`, `.5`).",
        &["float_literal"],
    ),
    dg(
        "bynk.parse.non_associative",
        "A non-associative operator was chained (e.g. `a == b == c`).",
        &["binary_expr"],
    ),
    d(
        "bynk.parse.orphan_doc_block",
        "A documentation block is not attached to a declaration (warning).",
    ),
    dg(
        "bynk.parse.reserved_keyword",
        "A reserved keyword was used as an identifier.",
        &["identifier"],
    ),
    dg(
        "bynk.parse.self_outside_method",
        "`self` used outside a method or handler.",
        &["self_expr"],
    ),
    d(
        "bynk.parse.storage_after_phase",
        "Agent storage (`state` / `store`) is declared after the invariants or handlers.",
    ),
    d(
        "bynk.parse.transition_after_handler",
        "A `transition` is declared after an agent handler; step invariants precede the handlers.",
    ),
    d(
        "bynk.parse.unexpected_adapter",
        "An `adapter` appeared where it is not allowed.",
    ),
    dg(
        "bynk.parse.unexpected_context",
        "A `context` appeared where it is not allowed.",
        &["context_decl"],
    ),
    d("bynk.parse.unexpected_eof", "Unexpected end of input."),
    dg(
        "bynk.parse.unexpected_suite",
        "A `suite` appeared where it is not allowed.",
        &["suite_decl"],
    ),
    d(
        "bynk.parse.unknown_effect_method",
        "An unknown method on `Effect`.",
    ),
    dg(
        "bynk.parse.unknown_handler_kind",
        "An unknown handler form (expected `call`, an HTTP method, `schedule`, or `message`).",
        &["handler"],
    ),
    dg(
        "bynk.parse.unknown_predicate",
        "An unknown refinement predicate.",
        &["predicate_name"],
    ),
    d(
        "bynk.parse.unknown_tier",
        "A `case`/`suite` `as <tier>` clause names something other than `unit`, `integration`, or `system`.",
    ),
    dg(
        "bynk.parse.uses_after_decls",
        "`uses` appears after other declarations.",
        &["uses_decl"],
    ),
    d(
        "bynk.project.file_and_directory",
        "A unit exists as both a file and a directory.",
    ),
    d(
        "bynk.project.inconsistent_commons_name",
        "A source file's path does not match its declared name.",
    ),
    d(
        "bynk.project.kind_conflict",
        "A name is declared as both a commons and a context.",
    ),
    d(
        "bynk.project.no_root",
        "No project root could be determined.",
    ),
    d(
        "bynk.project.no_sources",
        "The project contains no source files.",
    ),
    d(
        "bynk.project.read_failed",
        "A source file could not be read.",
    ),
    dg(
        "bynk.property.restates_refinement",
        "A `property` merely re-checks a refinement its type already guarantees.",
        &["for_all"],
    ),
    dg(
        "bynk.property.where_not_bool",
        "A `for all ... where` filter does not type to `Bool`.",
        &["for_all"],
    ),
    dg(
        "bynk.provider.dependency_cycle",
        "Providers form a capability dependency cycle through `given`.",
        &["provider_decl"],
    ),
    dg(
        "bynk.provider.extra_operation",
        "A `provides` block implements an operation not in the capability.",
        &["provider_decl"],
    ),
    dg(
        "bynk.provider.missing_operation",
        "A `provides` block is missing a capability operation.",
        &["provider_decl"],
    ),
    dg(
        "bynk.provider.outside_context",
        "`provides` was declared outside a context.",
        &["provider_decl"],
    ),
    dg(
        "bynk.provider.signature_mismatch",
        "A `provides` operation's signature does not match the capability.",
        &["provider_decl"],
    ),
    dg(
        "bynk.provider.unknown_capability",
        "`provides` names a capability that does not exist.",
        &["provider_decl"],
    ),
    d(
        "bynk.provides.bad_sequence",
        "A `provides … returns each […]` sequence is malformed (e.g. empty).",
    ),
    d(
        "bynk.provides.not_a_seam",
        "A test `provides` overrides a capability the unit under test does not consume.",
    ),
    d(
        "bynk.provides.rhs_type",
        "A test `provides … returns <value>` right-hand side does not match the operation's return type.",
    ),
    d(
        "bynk.provides.unknown_op",
        "A test `provides` names an operation the capability does not declare.",
    ),
    d(
        "bynk.query.join_key_mismatch",
        "A `joinOn`/`leftJoin` left and right key function return different types.",
    ),
    dg(
        "bynk.query.sum_needs_numeric",
        "A `sum`/`average` key function does not return a numeric type (`Int`, `Float`, or `Duration`).",
        &[],
    ),
    dg(
        "bynk.queue.bad_params",
        "An `on message` handler does not take exactly one `message` parameter.",
        &["queue_handler"],
    ),
    dg(
        "bynk.queue.duplicate_consumer",
        "Two `on message` handlers consume the same queue.",
        &["queue_handler"],
    ),
    dg(
        "bynk.queue.invalid_name",
        "A `from queue(\"…\")` binding has an empty queue name.",
        &["queue_handler"],
    ),
    dg(
        "bynk.queue.return_not_queue_result",
        "An `on message` handler does not return `Effect[QueueResult]`.",
        &["handler"],
    ),
    dg(
        "bynk.record_spread.field_type_mismatch",
        "A record-spread override has the wrong type for the field.",
        &["record_spread"],
    ),
    dg(
        "bynk.record_spread.non_record_base",
        "The base of a record spread is not a record.",
        &["record_spread"],
    ),
    dg(
        "bynk.record_spread.type_mismatch",
        "A record spread's base is a different record type.",
        &["record_spread"],
    ),
    dg(
        "bynk.record_spread.unknown_field",
        "A record spread overrides a field the record does not have.",
        &["record_spread"],
    ),
    dg(
        "bynk.refine.literal_violates",
        "A literal does not satisfy the refined type's predicate.",
        &["refined_type"],
    ),
    dg(
        "bynk.requires.unpinned_dependency",
        "An adapter `binding … requires { … }` entry has an unpinned version range.",
        &["binding_decl"],
    ),
    d(
        "bynk.resolve.ambiguous_variant",
        "A variant name is ambiguous across several sum types.",
    ),
    dg(
        "bynk.resolve.arity_mismatch",
        "A function was called with the wrong number of arguments.",
        &["call"],
    ),
    d("bynk.resolve.duplicate_actor", "Two actors share a name."),
    dg(
        "bynk.resolve.duplicate_agent",
        "Two agents share a name.",
        &["agent_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_capability",
        "Two capabilities share a name.",
        &["capability_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_field",
        "A record declares a field twice.",
        &["record_type"],
    ),
    dg(
        "bynk.resolve.duplicate_field_init",
        "A record construction initialises a field twice.",
        &["record_construction"],
    ),
    dg(
        "bynk.resolve.duplicate_fn",
        "Two functions share a name.",
        &["fn_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_method",
        "Two methods share a name.",
        &["fn_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_param",
        "A parameter name is repeated.",
        &["param"],
    ),
    dg(
        "bynk.resolve.duplicate_provider",
        "A capability is provided more than once.",
        &["provider_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_service",
        "Two services share a name.",
        &["service_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_type",
        "Two types share a name.",
        &["type_decl"],
    ),
    dg(
        "bynk.resolve.duplicate_variant",
        "A sum type declares a variant twice.",
        &["sum_type"],
    ),
    d(
        "bynk.resolve.fn_without_call",
        "A function was referenced without being called.",
    ),
    dg(
        "bynk.resolve.let_shadows_fn",
        "A `let` binding shadows a function.",
        &["let_stmt"],
    ),
    dg(
        "bynk.resolve.let_shadows_type",
        "A `let` binding shadows a type.",
        &["let_stmt"],
    ),
    d(
        "bynk.resolve.method_unknown_type",
        "A method is defined on an unknown type.",
    ),
    dg(
        "bynk.resolve.missing_field",
        "A record construction omits a required field.",
        &["record_construction"],
    ),
    d(
        "bynk.resolve.name_conflict",
        "Two declarations share a name.",
    ),
    dg(
        "bynk.resolve.not_a_record_type",
        "Record syntax was used on a non-record type.",
        &["record_construction"],
    ),
    dg(
        "bynk.resolve.opaque_record_construction",
        "An opaque type was constructed with record syntax.",
        &["record_construction"],
    ),
    dg(
        "bynk.resolve.param_as_function",
        "A value (such as a parameter) was called as a function.",
        &["call"],
    ),
    dg(
        "bynk.resolve.recursive_record_field",
        "A record directly contains a field of its own type.",
        &["record_type"],
    ),
    dg(
        "bynk.resolve.self_outside_method",
        "`self` referenced outside a method or handler.",
        &["self_expr"],
    ),
    dg(
        "bynk.resolve.type_as_function",
        "A type name was called as if it were a function.",
        &["call"],
    ),
    d(
        "bynk.resolve.type_in_expr",
        "A type name was used where a value is expected.",
    ),
    dg(
        "bynk.resolve.unconsumed_context",
        "A context's service was called without a `consumes` declaration.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.resolve.unknown_field",
        "Accessed a field the record does not have.",
        &["field_access"],
    ),
    dg(
        "bynk.resolve.unknown_function",
        "Called a function that does not exist.",
        &["call"],
    ),
    d(
        "bynk.resolve.unknown_name",
        "Referenced a name that is not in scope.",
    ),
    dg(
        "bynk.resolve.unknown_static_member",
        "Referenced an unknown static member (e.g. `T.x`).",
        &["field_access"],
    ),
    d(
        "bynk.resolve.unknown_type",
        "Referenced a type that does not exist.",
    ),
    dg(
        "bynk.send.in_pure_context",
        "A `~>` send was used in a pure (non-effectful) context.",
        &["effect_send_stmt"],
    ),
    dg(
        "bynk.send.non_effect",
        "A `~>` send was applied to a non-`Effect` value.",
        &["effect_send_stmt"],
    ),
    dg(
        "bynk.send.requires_unit",
        "A `~>` send targets an operation whose reply is not `Effect[()]`.",
        &["effect_send_stmt"],
    ),
    dg(
        "bynk.service.missing_from",
        "A `from`-less service has a handler other than `on call`.",
        &["service_decl"],
    ),
    dg(
        "bynk.service.mixed_protocols",
        "A service mixes handler forms that do not match its `from <protocol>`.",
        &["service_decl"],
    ),
    dg(
        "bynk.service.outside_context",
        "A `service` was declared outside a context.",
        &["service_decl"],
    ),
    dg(
        "bynk.service.return_not_effect",
        "A service handler's return type is not an `Effect`.",
        &["service_decl"],
    ),
    dg(
        "bynk.service.unknown_protocol",
        "A `from <protocol>` names an unknown protocol (e.g. a transport like Kafka).",
        &["service_decl"],
    ),
    d(
        "bynk.service.websocket_header",
        "The `from WebSocket` header is malformed — it binds frame types as `WebSocket(in: <type>, out: <type>)` (real-time track slice 3).",
    ),
    d(
        "bynk.service.websocket_multiple",
        "A context holds more than one `from WebSocket` service — at v1 the Workers upgrade routes by the `Upgrade: websocket` header alone, so one WebSocket service per context (real-time track slice 3b).",
    ),
    d(
        "bynk.service.websocket_open_arity",
        "A `from WebSocket` service must hold exactly one `on open` handler (the edge upgrade), and at most one `on message` (inbound) and one `on close` (real-time track slice 3/3b-iii).",
    ),
    d(
        "bynk.store.annotation_kind_mismatch",
        "A storage annotation is used on a kind it does not apply to (e.g. `@ttl` on a `Map`).",
    ),
    d(
        "bynk.store.annotation_unsupported",
        "A known storage annotation (`@ttl`/`@retain`/`@indexed`/`@bounded`) is used before the slice that supports it.",
    ),
    d(
        "bynk.store.cache_needs_clock",
        "A handler performs a `Cache` operation (TTL expiry reads the clock) without declaring `given Clock`.",
    ),
    d(
        "bynk.store.cache_ttl_required",
        "A `Cache` field is missing its required `@ttl(<duration>)` annotation (a keyed store with no expiry is a `Map`).",
    ),
    d(
        "bynk.store.kind_arity",
        "A storage kind was applied to the wrong number of type arguments (e.g. `Cell[A, B]`).",
    ),
    d(
        "bynk.store.kind_unsupported",
        "A known storage kind (`Queue`) is used before the slice that supports it.",
    ),
    d(
        "bynk.store.log_needs_clock",
        "A handler calls `Log.append` (which stamps the current time) without declaring `given Clock`.",
    ),
    d(
        "bynk.store.unknown_annotation",
        "A `store` field carries an annotation outside the closed `@indexed`/`@ttl`/`@retain`/`@bounded` set.",
    ),
    d(
        "bynk.store.unknown_kind",
        "A `store` field's type is not a known storage kind.",
    ),
    d(
        "bynk.store.unknown_op",
        "A storage-`Map`/`Set` operation is not a recognised entry/membership method.",
    ),
    dg(
        "bynk.suite.duplicate_case_name",
        "Two `case`s share a description.",
        &["case"],
    ),
    dg(
        "bynk.suite.unknown_target",
        "A `suite` targets a unit that does not exist.",
        &["suite_decl"],
    ),
    d(
        "bynk.target.browser_bundle_only",
        "The `browser` platform builds only the in-process `Bundle` topology; `--target workers` is not a browser build.",
    ),
    dg(
        "bynk.target.vendor_conflict",
        "One deployment unit's in-process closure uses platform-native capabilities from two mutually-exclusive platforms.",
        &["consumes_decl"],
    ),
    dg(
        "bynk.target.vendor_required",
        "A deployment unit uses a platform-native capability but the build selects another `--platform`.",
        &["consumes_decl"],
    ),
    d(
        "bynk.tier.property_has_tier",
        "A `property` carries an `as <tier>` clause; tiers are a `case`-only affordance.",
    ),
    d(
        "bynk.tier.system_needs_wire",
        "An `as system` test stands up fewer than two contexts; the system tier wires across contexts.",
    ),
    d(
        "bynk.transition.cross_agent_reference",
        "A transition predicate references another agent; step invariants are per-agent.",
    ),
    d(
        "bynk.transition.duplicate_name",
        "An agent declares two transitions with the same name.",
    ),
    d(
        "bynk.transition.impure_predicate",
        "A transition predicate uses an effectful or test-only construct; a step invariant must be pure.",
    ),
    d(
        "bynk.transition.no_step_reference",
        "A transition references neither `old` nor `new`; it constrains one state, so it is an `invariant`, not a step.",
    ),
    d(
        "bynk.transition.not_bool",
        "A transition predicate does not have type `Bool`.",
    ),
    d(
        "bynk.types.ambiguous_constructor",
        "`Ok`/`Err` is ambiguous between `Result` and `HttpResult`; qualify it.",
    ),
    dg(
        "bynk.types.argument_mismatch",
        "A function argument has the wrong type.",
        &["call"],
    ),
    d(
        "bynk.types.bytes_at_workers_boundary",
        "A bare `Bytes` appears in a `workers` wire signature — the erased cross-context boundary does not base64-encode it, so v1 diagnoses it rather than mis-encode. The typed paths (`bundle` calls, `store`/record fields) round-trip a `Bytes` fine (ADR 0142 D8).",
    ),
    dg(
        "bynk.types.call_arity",
        "A function value was applied with the wrong number of arguments.",
        &["call"],
    ),
    dg(
        "bynk.types.cannot_infer_option_type_param",
        "The value type of `None` could not be inferred.",
        &["none_expr"],
    ),
    d(
        "bynk.types.cannot_infer_result_type_params",
        "The type parameters of a `Result` could not be inferred.",
    ),
    d(
        "bynk.types.constructor_arity",
        "A variant constructor got the wrong number of arguments.",
    ),
    d(
        "bynk.types.constructor_base_mismatch",
        "A `.of` constructor was given an argument of the wrong base type.",
    ),
    dg(
        "bynk.types.duplicate_literal_arm",
        "A `match` has two arms for the same literal value.",
        &["match_arm"],
    ),
    dg(
        "bynk.types.duplicate_variant_arm",
        "A `match` has two arms for the same variant.",
        &["match_arm"],
    ),
    dg(
        "bynk.types.empty_refinement",
        "A refinement admits no values (contradictory predicates).",
        &["refinement"],
    ),
    dg(
        "bynk.types.err_value_mismatch",
        "An `Err` payload has the wrong type.",
        &["err_expr"],
    ),
    dg(
        "bynk.types.field_access_on_non_record",
        "Field access on a value that is not a record.",
        &["field_access"],
    ),
    dg(
        "bynk.types.field_refinement_not_base",
        "An inline field refinement requires a base or refined type.",
        &["record_field"],
    ),
    dg(
        "bynk.types.field_value_mismatch",
        "A record field was given a value of the wrong type.",
        &["record_construction"],
    ),
    dg(
        "bynk.types.function_at_boundary",
        "A function type appeared in a serialisable or boundary position (a record field, sum payload, service/agent handler signature, capability operation signature, agent state field, or agent key); functions cannot serialise or cross a boundary.",
        &["function_type_ref"],
    ),
    dg(
        "bynk.types.guard_not_bool",
        "A match-arm `if` guard is not a `Bool` expression.",
        &["match_arm"],
    ),
    d(
        "bynk.types.held_at_boundary",
        "A held value (`Connection[F]`) appears in a serialisable or boundary position — a held resource is built and disposed in place, never persisted or sent across a boundary (§2.9, real-time track slice 2).",
    ),
    d(
        "bynk.types.held_not_comparable",
        "A held value (`Connection[F]`) is compared with `==`/`!=` — held values have identity, not value-equality (§2.9.3, real-time track slice 2).",
    ),
    dg(
        "bynk.types.if_branch_mismatch",
        "The branches of an `if` have different types.",
        &["if_expr"],
    ),
    dg(
        "bynk.types.if_non_bool_cond",
        "An `if` condition is not a `Bool`.",
        &["if_expr"],
    ),
    dg(
        "bynk.types.if_without_else_requires_unit",
        "An `if` with no `else` branch has a non-unit then-branch; the missing else defaults to `()`, so the branch must be `()` or `Effect[()]`.",
        &["if_expr"],
    ),
    d(
        "bynk.types.interpolation_non_scalar",
        "An interpolation hole holds a value with no string form.",
    ),
    dg(
        "bynk.types.invalid_regex",
        "A `Matches` predicate contains an invalid regular expression.",
        &["refinement"],
    ),
    dg(
        "bynk.types.inverted_range",
        "An `InRange` predicate has its bounds inverted.",
        &["refinement"],
    ),
    dg(
        "bynk.types.is_base_mismatch",
        "An `is` refinement check is applied to a value of the wrong base type.",
        &["is_expr"],
    ),
    dg(
        "bynk.types.is_literal_pattern",
        "A literal was used on the right of `is`; `is` tests type/refinement, not value equality (use `==`).",
        &["is_expr"],
    ),
    dg(
        "bynk.types.is_non_sum",
        "`is` was applied to a value that is not a sum type.",
        &["is_expr"],
    ),
    dg(
        "bynk.types.is_unknown_variant",
        "`is` names a variant the type does not have.",
        &["is_expr"],
    ),
    dg(
        "bynk.types.json_uncodable",
        "A `Json.encode`/`Json.decode` target type cannot pass through the typed JSON codec (functions, effects, error builtins).",
        &["method_call"],
    ),
    dg(
        "bynk.types.key_not_orderable",
        "A `sortBy`/`min`/`max` key function does not return an orderable type (`Int`, `Float`, `String`, `Duration`, or `Instant`).",
        &[],
    ),
    dg(
        "bynk.types.lambda_mismatch",
        "A lambda's parameter count, parameter annotations, or body type do not match the expected function type.",
        &["lambda_expr"],
    ),
    dg(
        "bynk.types.let_annotation_mismatch",
        "A `let` value does not match its type annotation.",
        &["let_stmt"],
    ),
    dg(
        "bynk.types.list_element_mismatch",
        "A list-literal element has a different type from the list's element type.",
        &["list_literal"],
    ),
    dg(
        "bynk.types.match_arm_mismatch",
        "A `match` arm has a different type from the others.",
        &["match_arm"],
    ),
    dg(
        "bynk.types.match_non_sum_discriminant",
        "`match` was applied to a value that is not a sum type.",
        &["match_expr"],
    ),
    dg(
        "bynk.types.method_arity",
        "A method was called with the wrong number of arguments.",
        &["method_call"],
    ),
    dg(
        "bynk.types.method_not_found",
        "Called a method the type does not have.",
        &["method_call"],
    ),
    dg(
        "bynk.types.method_on_non_named_type",
        "A method was called on a built-in type that has no methods.",
        &["method_call"],
    ),
    dg(
        "bynk.types.mixed_pattern_bindings",
        "A pattern mixes named and positional bindings.",
        &["variant_pattern"],
    ),
    dg(
        "bynk.types.negative_length",
        "A length predicate was given a negative value.",
        &["refinement"],
    ),
    dg(
        "bynk.types.no_numeric_coercion",
        "`Int` and `Float` were mixed without an explicit conversion — in an operation or in refinement bounds.",
        &["binary_expr", "refinement"],
    ),
    dg(
        "bynk.types.non_exhaustive_match",
        "A `match` does not cover every variant.",
        &["match_expr"],
    ),
    dg(
        "bynk.types.ok_value_mismatch",
        "An `Ok` payload has the wrong type.",
        &["ok_expr"],
    ),
    dg(
        "bynk.types.opaque_raw_outside",
        "`.raw` on an opaque type was used outside its defining commons.",
        &["field_access"],
    ),
    dg(
        "bynk.types.opaque_record_construction",
        "An opaque type was constructed with record syntax.",
        &["record_construction"],
    ),
    dg(
        "bynk.types.opaque_unsafe_outside",
        "`.unsafe` on an opaque type was used outside its defining context.",
        &["field_access"],
    ),
    dg(
        "bynk.types.pattern_arity",
        "A pattern binds the wrong number of payload fields.",
        &["variant_pattern"],
    ),
    dg(
        "bynk.types.pattern_type_mismatch",
        "A pattern's type does not match the matched value.",
        &["variant_pattern"],
    ),
    dg(
        "bynk.types.predicate_base_mismatch",
        "A predicate does not apply to the type's base (e.g. a string predicate on an `Int`).",
        &["refinement"],
    ),
    d(
        "bynk.types.query_at_boundary",
        "A `Query` type appears in a storable or boundary-crossing position — a query is built and executed in place, never persisted or sent (ADR 0115).",
    ),
    dg(
        "bynk.types.question_error_mismatch",
        "`?` propagates an error type incompatible with the function's.",
        &["question_expr"],
    ),
    dg(
        "bynk.types.question_on_non_result",
        "`?` was applied to a non-`Result` value.",
        &["question_expr"],
    ),
    dg(
        "bynk.types.question_outside_result",
        "`?` used in a function that does not return a `Result`.",
        &["question_expr"],
    ),
    d(
        "bynk.types.return_mismatch",
        "A returned value does not match the declared return type.",
    ),
    dg(
        "bynk.types.some_value_mismatch",
        "A `Some` payload has the wrong type.",
        &["some_expr"],
    ),
    d(
        "bynk.types.stream_at_boundary",
        "A `Stream` type appears in a storable or boundary-crossing position — a stream is a live value-over-time source, never persisted or sent across a boundary (real-time track slice 0).",
    ),
    d(
        "bynk.types.stream_not_comparable",
        "A `Stream` value is compared with `==`/`!=` — a stream is a live value-over-time source, not a comparable value (real-time track slice 0).",
    ),
    d(
        "bynk.types.type_mismatch",
        "Two types that were required to match did not.",
    ),
    dg(
        "bynk.types.uninferable_element_type",
        "An empty `[]` (or `List.empty()` / `Map.empty()`) has no expected type to infer its element type from.",
        &["list_literal"],
    ),
    dg(
        "bynk.types.unkeyable_distinct",
        "A `distinct`/`distinctBy` element or key is not value-keyable (`String`, `Int`, or a refined/opaque type over them).",
        &[],
    ),
    dg(
        "bynk.types.unkeyable_map_key",
        "A `Map` key type is not value-keyable (`String`, `Int`, or a refined/opaque type over them).",
        &["generic_type_ref"],
    ),
    dg(
        "bynk.types.unknown_field",
        "Referenced a field the record type does not declare.",
        &["field_access"],
    ),
    dg(
        "bynk.types.unknown_pattern_field",
        "A pattern names a field the variant does not have.",
        &["variant_pattern"],
    ),
    dg(
        "bynk.types.unknown_static_member",
        "Referenced an unknown static member on a type.",
        &["field_access"],
    ),
    dg(
        "bynk.types.unknown_variant_in_pattern",
        "A pattern names a variant the sum type does not have.",
        &["variant_pattern"],
    ),
    dg(
        "bynk.types.unreachable_arm",
        "A `match` arm is unreachable.",
        &["match_arm"],
    ),
    d(
        "bynk.types.variant_arity",
        "A variant constructor got the wrong number of payload values.",
    ),
    d(
        "bynk.types.variant_missing_payload",
        "A variant requiring a payload was used without one.",
    ),
    d(
        "bynk.types.variant_payload_mismatch",
        "A variant payload has the wrong type.",
    ),
    dg(
        "bynk.uses.name_conflict",
        "A `uses` name collides with another name.",
        &["uses_decl"],
    ),
    dg(
        "bynk.uses.self_reference",
        "A commons `uses` itself.",
        &["uses_decl"],
    ),
    dg(
        "bynk.uses.target_is_context",
        "`uses` targets a context instead of a commons.",
        &["uses_decl"],
    ),
    dg(
        "bynk.uses.unknown_commons",
        "`uses` names a commons that does not exist.",
        &["uses_decl"],
    ),
    dg(
        "bynk.val.agent_not_generable",
        "A `for all`/`Val` cannot generate an agent — fabricated agent states need not be reachable.",
        &["for_all"],
    ),
    dg(
        "bynk.val.arity",
        "`Val[T]` was given the wrong number of pin arguments.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.literal_violates",
        "A pinned `Val[T]` value violates the type's refinement.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.needs_pin",
        "A bare `Val[T]` cannot generate a value (e.g. a `Matches` string); pin one.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.outside_test",
        "`Val[T]` was used outside a test case body.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.pin_not_literal",
        "A `Val[T]` pin argument is not a compile-time literal.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.pin_unsupported",
        "A pin was given for a type kind that does not support pinning.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.unknown_type",
        "`Val[T]` names a type that does not resolve.",
        &["val_expr"],
    ),
    dg(
        "bynk.val.unsupported_kind",
        "`Val[T]` cannot fabricate a value for this kind of type.",
        &["val_expr"],
    ),
    d(
        "bynk.ws.message_frame_param",
        "A WebSocket `on message` handler does not have exactly one parameter of the service's inbound (`in:`) frame type — the decoded frame (real-time track slice 3b-iii).",
    ),
    d(
        "bynk.ws.open_given_unsupported",
        "A WebSocket `on open` handler declares `given` capabilities — unsupported at v1, since on Workers the handler runs inside the connection-hosting Durable Object, which has no composition root to supply them (real-time track slice 3b).",
    ),
    d(
        "bynk.ws.open_transfer_shape",
        "A WebSocket `on open` handler does not transfer its `connection` into exactly one agent, so the Workers upgrade has no single Durable Object to route to (real-time track slice 3b).",
    ),
    d(
        "bynk.ws.route_param_mismatch",
        "A WebSocket `on message`/`on close` route parameter does not match the `on open` parameter at the same position — route values are recovered positionally from the connection, so they must be a type-compatible prefix of the `on open` parameters (real-time track slice 3b-iii).",
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
/// `"types"` for `"bynk.types.type_mismatch"`.
pub fn category(code: &str) -> &str {
    code.split('.').nth(1).unwrap_or("")
}

/// A human-readable heading for a category segment.
fn category_title(cat: &str) -> &'static str {
    match cat {
        "agent" | "agents" => "Agents",
        "boundary" => "Boundaries",
        "capability" => "Capabilities",
        "consumes" => "Consumes",
        "context" => "Contexts",
        "contract" => "Contracts",
        "cron" => "Cron",
        "effect" => "Effects",
        "expect" => "Expectations",
        "exports" => "Exports",
        "given" => "Given capabilities",
        "http" => "HTTP",
        "lex" => "Lexer",
        "mock" => "Mocks (collaborators)",
        "observe" => "Observation",
        "parse" => "Parser",
        "project" => "Project",
        "property" => "Properties (generative tests)",
        "provider" => "Providers",
        "queue" => "Queue",
        "record_spread" => "Record spread",
        "refine" => "Refinement",
        "resolve" => "Resolution",
        "service" => "Services",
        "suite" => "Suites and cases",
        "transition" => "Transitions (step invariants)",
        "types" => "Type checking",
        "uses" => "Uses",
        "val" => "Value fabrication",
        _ => "Other",
    }
}

/// Render the diagnostic index as a Markdown reference page, grouped by
/// category. This is the generator behind
/// `site/src/content/docs/book/reference/diagnostics.md`.
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
         Source: bynkc/src/diagnostics.rs (`render_markdown`).\n     \
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test diagnostics_registry -->\n\n",
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
            "Generated from the grammar_symbol field of bynkc/src/diagnostics.rs. \
             Do not edit by hand. Regenerate with: BYNK_BLESS=1 cargo test -p \
             bynkc --test diagnostics_registry"
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
