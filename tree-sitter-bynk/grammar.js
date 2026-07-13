/**
 * @file Tree-sitter grammar for Bynk.
 *
 * Covers the syntactic surface defined by the normative specification
 * (the Book's spec/ section, §3–§4), which is generated from this grammar and kept
 * current per increment. Implements the highlighting / structural shape the
 * editor needs; semantic rules (type checking, exhaustiveness, effect
 * propagation, `given` matching) are intentionally left to the LSP.
 *
 * The grammar is permissive in the places where the type checker would
 * reject code anyway — e.g., `capability` declarations parse inside any
 * declaration body; the LSP surfaces the placement error.
 *
 * Validated by parsing the bynkc fixture corpus (tests/fixtures) to zero
 * ERROR/MISSING nodes.
 *
 * @author Bynk project
 * @license see project root
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  expect: 0,
  // v0.80: `implies` is the lowest-precedence binary operator (below `||`).
  implies: 1,
  or: 2,
  and: 3,
  is: 4,
  cmp: 5,
  rel: 6,
  add: 7,
  mul: 8,
  unary: 9,
  postfix: 10,
};

module.exports = grammar({
  name: "bynk",

  externals: ($) => [$.doc_block],

  extras: ($) => [/\s+/, $.line_comment, $.doc_block],

  // The DSL allows reserved words to be referenced inside specific rules via
  // tokens; we declare keywords so they take precedence over `identifier`.
  word: ($) => $.identifier,

  conflicts: ($) => [
    // `match e { … }` / `if e { … }`: after a bare identifier `e`,
    // the `{` is ambiguous between opening the match/if body and opening a
    // record construction/spread `e { … }`. Keep all parses alive; the body
    // content (match arms / block statements vs field inits) decides which one
    // survives at parse time.
    [$.record_construction, $.record_spread, $._primary],
    // v0.20a: `(x …` is ambiguous between a lambda parameter list and a
    // parenthesised expression (and `()` the unit literal) until the
    // closing `)` is followed — or not — by `=>`.
    [$.lambda_param, $._primary],
    [$._primary, $.lambda_expr],
    // v0.145: a match-arm `if`-guard is an `_expression` immediately followed by
    // the arm's `=>`. When the guard opens with `(`, one token can't tell a unit
    // literal `()` (a guard, with `=>` opening the arm) from a zero-param lambda
    // `() =>` (the guard itself). GLR keeps both alive; only the unit parse
    // completes the arm.
    [$.lambda_expr, $.unit_literal],
    // v0.117: after `expect Cap.op called`, one token of lookahead can't tell a
    // following `<n> times` count from the end of the observation — GLR keeps
    // both parses alive until the next token decides.
    [$.observation_expr],
    // The subject `Cap.op` overlaps a `field_access` (start of an ordinary
    // predicate); both parses survive until the token after the op reveals
    // whether a matcher (`called`/`never`/`before`) or an ordinary continuation
    // (`(`, an operator, …) follows.
    [$.observation_expr, $._primary],
  ],

  rules: {
    // -- Top level --

    // A source file is normally one or more top-level units. To keep the
    // highlighter from painting every documentation snippet as one big ERROR,
    // we also parse *fragments* — pieces lifted out of their enclosing unit,
    // which never occur in a real `.bynk` file but appear throughout the docs
    // and in editor scratch buffers. The LSP still flags the structural
    // placement error semantically.
    //
    // The three branches are disjoint on their first token, so no real input
    // is ambiguous between them: a unit opens with `commons`/`context`/`test`;
    // an item fragment opens with another declaration keyword (`type`, `fn`,
    // `service`, `on`, …); and a statement/expression fragment opens with a
    // value keyword (`let`, `if`, `match`, `assert`) or a bare term. Keeping
    // item and expression fragments in separate branches (rather than one
    // mixed `repeat1`) avoids spurious boundary conflicts where a completed
    // declaration abuts a following parenthesised expression.
    source_file: ($) =>
      choice(
        repeat1(
          choice(
            $.commons_decl,
            $.context_decl,
            $.adapter_decl,
            $.suite_decl,
          ),
        ),
        repeat1($._item_fragment),
        $._expr_fragment,
      ),

    _item_fragment: ($) =>
      choice($._context_body_item, $.handler, $.store_field, $.key_decl),

    // The body of a fragment block: statements then an optional tail value,
    // mirroring `block` so multi-statement and bare-expression snippets parse.
    // Spelled as a non-empty choice so the rule never matches the empty input.
    _expr_fragment: ($) =>
      choice(
        seq(repeat1($._statement), optional(field("tail", $._expression))),
        field("tail", $._expression),
      ),

    commons_decl: ($) =>
      seq(
        "commons",
        field("name", $.qualified_name),
        choice(
          seq("{", repeat($._commons_body_item), "}"),
          // Fragment form: header followed by items to EOF (or the next
          // top-level header).
          repeat($._commons_body_item),
        ),
      ),

    context_decl: ($) =>
      seq(
        "context",
        field("name", $.qualified_name),
        choice(
          seq("{", repeat($._context_body_item), "}"),
          repeat($._context_body_item),
        ),
      ),

    // v0.17: an adapter — the host boundary (capability contract + binding).
    // Brace or fragment form, like commons/context.
    adapter_decl: ($) =>
      seq(
        "adapter",
        field("name", $.qualified_name),
        choice(
          seq("{", repeat($._adapter_body_item), "}"),
          repeat($._adapter_body_item),
        ),
      ),

    // v0.7: test unit targeting a commons or context. Brace or fragment form
    // (a test case is `test "string" …`, so it never collides with a new
    // `test <qualified_name>` unit — but the disambiguation needs one extra
    // token of lookahead, handled by a declared conflict).
    // v0.118 (testing track slice 6): an optional `as <tier>` classifier —
    // `unit` | `integration` | `system`. The tier words are contextual (they
    // lex as ordinary identifiers except right after `as` in a suite/case
    // header), like the observation matcher words.
    suite_decl: ($) =>
      prec.right(
        seq(
          "suite",
          field("target", $.qualified_name),
          optional(seq("as", field("tier", choice("unit", "integration", "system")))),
          choice(
            seq("{", repeat($._test_body_item), "}"),
            repeat($._test_body_item),
          ),
        ),
      ),

    qualified_name: ($) => sep1($.identifier, "."),

    _commons_body_item: ($) =>
      choice(
        $.uses_decl,
        $.type_decl,
        $.fn_decl,
        // Permissive: capability/service/etc. can syntactically appear
        // anywhere — the LSP reports semantic placement errors.
        $.capability_decl,
        $.provider_decl,
        $.service_decl,
        $.agent_decl,
        $.actor_decl,
      ),

    _context_body_item: ($) =>
      choice(
        $.uses_decl,
        $.consumes_decl,
        $.exports_decl,
        $.type_decl,
        $.fn_decl,
        $.capability_decl,
        $.provider_decl,
        $.service_decl,
        $.agent_decl,
        $.actor_decl,
      ),

    // v0.17: adapter items — a binding clause, capabilities, boundary types,
    // inline pure helpers and `uses`, external providers, and `exports`.
    // v0.18 adds `consumes` (adapter-to-adapter capability dependencies).
    // Permissive (service/agent parse here too); the LSP reports the semantic
    // placement error.
    _adapter_body_item: ($) =>
      choice(
        $.binding_decl,
        $.uses_decl,
        $.consumes_decl,
        $.exports_decl,
        $.type_decl,
        $.fn_decl,
        $.capability_decl,
        $.provider_decl,
        $.service_decl,
        $.agent_decl,
        $.actor_decl,
      ),

    _test_body_item: ($) =>
      choice($.uses_decl, $.consumes_decl, $.provides_clause, $.case, $.property_decl),

    // -- Headers / clauses --

    uses_decl: ($) => seq("uses", field("target", $.qualified_name)),
    consumes_decl: ($) =>
      seq(
        "consumes",
        field("target", $.qualified_name),
        optional(
          choice(
            // v0.6: `consumes a.b as Alias`.
            seq("as", field("alias", $.identifier)),
            // v0.17: `consumes a.b { Cap, … }` — selected capabilities.
            seq(
              "{",
              optional(sep1(field("capability", $.identifier), ",")),
              optional(","),
              "}",
            ),
          ),
        ),
      ),

    // v0.17: an adapter's binding module + optional npm dependency map.
    binding_decl: ($) =>
      seq(
        "binding",
        field("module", $.string_literal),
        optional(
          seq(
            "requires",
            "{",
            optional(sep1($.binding_requirement, ",")),
            optional(","),
            "}",
          ),
        ),
      ),
    binding_requirement: ($) =>
      seq(field("package", $.string_literal), ":", field("range", $.string_literal)),
    exports_decl: ($) =>
      seq(
        "exports",
        // v0.15: `capability` joins the type-visibility keywords here.
        field("kind", choice("opaque", "transparent", "capability")),
        "{",
        optional(sep1(field("name", $.identifier), ",")),
        optional(","),
        "}",
      ),

    // -- Type declarations --

    type_decl: ($) =>
      seq(
        "type",
        field("name", $.identifier),
        // v0.157 (ADR 0183): optional `[A, B]` type parameters — only a record
        // body may be generic (enforced by the checker, not the grammar).
        optional(seq("[", sep1(field("type_param", $.identifier), ","), "]")),
        "=",
        field("body", $._type_body),
      ),

    _type_body: ($) =>
      choice(
        $.opaque_type,
        $.refined_type,
        $.record_type,
        $.sum_type,
        $.enum_type,
      ),

    opaque_type: ($) =>
      seq(
        "opaque",
        field("base", $._base_type),
        optional(seq("where", field("refinement", $.refinement))),
      ),

    refined_type: ($) =>
      prec(
        1,
        seq(
          field("base", $._base_type),
          optional(seq("where", field("refinement", $.refinement))),
        ),
      ),

    record_type: ($) =>
      seq(
        "{",
        optional(sep1($.record_field, ",")),
        optional(","),
        "}",
      ),

    record_field: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", $._type_ref),
        optional(seq("where", field("refinement", $.refinement))),
        // v0.11: an optional initial-value expression. Meaningful on agent
        // `store` fields; the checker restricts where it applies.
        optional(seq("=", field("init", $._expression))),
      ),

    // v0.154 (ADR 0178): a sum body may carry a trailing `embeds E as V, …`
    // clause declaring error embeddings the `?` operator auto-converts.
    // `embeds`/`as` are contextual — matched only in this position.
    sum_type: ($) =>
      prec.right(
        seq(
          repeat1($.sum_variant),
          optional(
            seq(
              "embeds",
              sep1(
                seq(
                  field("source", $._type_ref),
                  "as",
                  field("variant", $.constant_name),
                ),
                ",",
              ),
            ),
          ),
        ),
      ),
    sum_variant: ($) =>
      seq(
        "|",
        field("name", $.constant_name),
        optional(
          seq(
            "(",
            optional(sep1($.variant_payload_field, ",")),
            optional(","),
            ")",
          ),
        ),
      ),
    variant_payload_field: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", $._type_ref),
      ),

    enum_type: ($) =>
      seq(
        "enum",
        "{",
        optional(sep1(field("variant", $.constant_name), ",")),
        optional(","),
        "}",
      ),

    refinement: ($) => sep1($._refinement_pred, "and"),
    _refinement_pred: ($) =>
      choice(
        $.pred_call,
        $.pred_atom,
      ),
    pred_call: ($) =>
      seq(field("name", $.predicate_name), "(", optional(sep1($._pred_arg, ",")), ")"),
    pred_atom: ($) => $.predicate_name,
    predicate_name: ($) =>
      choice(
        "Matches",
        "InRange",
        "MinLength",
        "MaxLength",
        "Length",
        "NonNegative",
        "Positive",
        "NonEmpty",
      ),
    _pred_arg: ($) => choice($.number_literal, $.float_literal, $.string_literal),

    _base_type: ($) => $.base_type,
    base_type: () => choice("Int", "String", "Bool", "Float", "Duration", "Instant"),

    _type_ref: ($) =>
      choice(
        $.function_type_ref,
        $._base_type,
        $.unit_type,
        $.validation_error_type,
        $.generic_type_ref,
        $.applied_type_ref,
        $.identifier,
      ),

    // v0.157 (ADR 0183): `Name[Arg, …]` — an application of a user-declared
    // generic type. Distinct from `generic_type_ref` (the fixed built-in
    // generics); the head is a user identifier.
    applied_type_ref: ($) =>
      seq(
        field("name", $.identifier),
        "[",
        sep1(field("arg", $._type_ref), ","),
        "]",
      ),

    // v0.20a: a function type — `A -> B`, `(A, B) -> C`, `() -> B`,
    // right-associative (prec.right). A parenthesised parameter list is only
    // a function type when followed by `->`; bare `()` stays the unit type.
    function_type_ref: ($) =>
      prec.right(
        seq(
          field(
            "params",
            choice(
              $._base_type,
              $.unit_type,
              $.validation_error_type,
              $.generic_type_ref,
              $.applied_type_ref,
              $.identifier,
              seq("(", sep1($._type_ref, ","), optional(","), ")"),
            ),
          ),
          "->",
          field("return_type", $._type_ref),
        ),
      ),

    unit_type: () => seq("(", ")"),
    validation_error_type: () => "ValidationError",
    generic_type_ref: ($) =>
      seq(
        field(
          "name",
          choice(
            alias("Result", $.builtin_type),
            alias("Option", $.builtin_type),
            alias("Effect", $.builtin_type),
            // v0.9: HTTP result type.
            alias("HttpResult", $.builtin_type),
            // v0.20b: the built-in collection types.
            alias("List", $.builtin_type),
            alias("Map", $.builtin_type),
            // v0.100/v0.92/v0.102: stream, lazy storage query, held connection.
            alias("Stream", $.builtin_type),
            alias("Query", $.builtin_type),
            alias("Connection", $.builtin_type),
            // v0.119 (testing track slice 7): `History[Agent]` — a generated,
            // driven call-history (test-only generator).
            alias("History", $.builtin_type),
          ),
        ),
        "[",
        sep1(field("arg", $._type_ref), ","),
        "]",
      ),

    // -- Function declarations --

    fn_decl: ($) =>
      seq(
        "fn",
        field("name", choice($.method_name, $.identifier)),
        // v0.20a: optional `[A, B]` type parameters (functions only).
        optional(seq("[", sep1(field("type_param", $.identifier), ","), "]")),
        "(",
        optional($._params),
        ")",
        "->",
        field("return_type", $._type_ref),
        // v0.115: contract clauses between the return type and the body.
        repeat($.requires_clause),
        repeat($.ensures_clause),
        field("body", $.block),
      ),
    method_name: ($) =>
      seq(field("type", $.identifier), ".", field("method", $.identifier)),

    // v0.115 (testing track slice 3): a function contract clause. `requires` is a
    // precondition over the parameters; `ensures` is a postcondition over the
    // parameters and the contextual `result` binding. The predicate is the one
    // predicate surface (same grammar as an `invariant_decl`).
    requires_clause: ($) =>
      seq("requires", field("name", $.identifier), ":", field("predicate", $._expression)),
    ensures_clause: ($) =>
      seq("ensures", field("name", $.identifier), ":", field("predicate", $._expression)),

    _params: ($) =>
      seq(
        choice($.self_param, $.param),
        repeat(seq(",", $.param)),
        optional(","),
      ),
    self_param: () => "self",
    param: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", $._type_ref),
      ),

    // -- v0.5: capabilities, providers, services, agents --

    capability_decl: ($) =>
      seq(
        "capability",
        field("name", $.identifier),
        "{",
        repeat($.capability_op),
        "}",
      ),
    capability_op: ($) =>
      seq(
        "fn",
        field("name", $.identifier),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
      ),

    provider_decl: ($) =>
      seq(
        "provides",
        field("capability", $.identifier),
        "=",
        field("provider", $.identifier),
        // v0.12: a provider may depend on other capabilities.
        optional(field("given", $.given_clause)),
        // v0.17: a bodiless provider is *external* — supplied by an adapter's
        // binding. The absence of the brace block (not an empty one) is the
        // signal; the LSP reports placement errors (body in an adapter, or
        // bodiless outside one).
        optional(seq("{", repeat($.provider_op), "}")),
      ),
    provider_op: ($) =>
      seq(
        "fn",
        field("name", $.identifier),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        field("body", $.block),
      ),

    service_decl: ($) =>
      seq(
        "service",
        field("name", $.identifier),
        optional(field("protocol", $.service_protocol)),
        // v0.155: optional service-level `by`/`given` defaults on the header —
        // the ambient contract every handler inherits unless it declares its own.
        optional(field("default_by", $.by_clause)),
        optional(field("default_given", $.given_clause)),
        "{",
        // v0.131 (ADR 0158): the optional `cors { }` policy, in header position
        // before the handlers (a `from http` concern; the checker restricts it).
        optional(field("cors", $.cors_policy)),
        // v0.141 (ADR 0164): the optional `security { }` policy, likewise a
        // header-position `from http` concern restricted by the checker.
        optional(field("security", $.security_policy)),
        // v0.142 (ADR 0166): the optional `limits { }` policy, likewise a
        // header-position `from http` concern restricted by the checker.
        optional(field("limits", $.limits_policy)),
        repeat($.handler),
        "}",
      ),
    // v0.131: `cors { <name>: <value>, … }`. `cors` is a contextual keyword (like
    // `store`/`key` — also a valid identifier); field names are ordinary
    // identifiers matched against the closed set by the checker.
    cors_policy: ($) =>
      seq(
        "cors",
        "{",
        repeat(seq($.cors_field, optional(","))),
        "}",
      ),
    cors_field: ($) =>
      seq(field("name", $.identifier), ":", field("value", $._expression)),
    // v0.141: `security { <name>: <value>, … }`. `security` is a contextual
    // keyword (like `cors`); field names (`hsts`/`nosniff`) are ordinary
    // identifiers matched against the closed set by the checker.
    security_policy: ($) =>
      seq(
        "security",
        "{",
        repeat(seq($.security_field, optional(","))),
        "}",
      ),
    security_field: ($) =>
      seq(field("name", $.identifier), ":", field("value", $._expression)),
    // v0.142: `limits { <name>: <value>, … }`. `limits` is a contextual
    // keyword (like `cors`/`security`); field names are ordinary identifiers
    // matched against the closed set by the checker.
    limits_policy: ($) =>
      seq(
        "limits",
        "{",
        repeat(seq($.limits_field, optional(","))),
        "}",
      ),
    limits_field: ($) =>
      seq(field("name", $.identifier), ":", field("value", $._expression)),
    // v0.44: `from <protocol>` on the service header.
    // v0.103: `from WebSocket(in: I, out: O)` binds the inbound/outbound frame
    // types. `WebSocket`, `in`, and `out` are contextual (resolved by `word`).
    service_protocol: ($) =>
      seq(
        "from",
        choice(
          "http",
          "cron",
          seq("queue", "(", field("name", $.string_literal), ")"),
          seq(
            "WebSocket",
            "(",
            "in",
            ":",
            field("in_frame", $._type_ref),
            ",",
            "out",
            ":",
            field("out_frame", $._type_ref),
            optional(","),
            ")",
          ),
        ),
      ),

    agent_decl: ($) =>
      seq(
        "agent",
        field("name", $.identifier),
        "{",
        field("key", $.key_decl),
        // v0.81 (storage track): the agent's `store` fields (ADR 0108).
        repeat($.store_field),
        repeat(choice($.invariant_decl, $.transition_decl)),
        repeat($.handler),
        "}",
      ),
    // v0.80: an agent invariant — `invariant <name>: <predicate>`. Sits between
    // the store fields and the handlers.
    invariant_decl: ($) =>
      seq(
        "invariant",
        field("name", $.identifier),
        ":",
        field("predicate", $._expression),
      ),
    // v0.116 (testing track slice 4): an agent step invariant —
    // `transition <name>: <predicate over old/new>`. Sits beside the snapshot
    // invariants, between the store fields and the handlers. `old`/`new` are
    // ordinary identifiers in the predicate (contextual, not keywords).
    transition_decl: ($) =>
      seq(
        "transition",
        field("name", $.identifier),
        ":",
        field("predicate", $._expression),
      ),
    key_decl: ($) =>
      seq("key", field("name", $.identifier), ":", field("type", $._type_ref)),
    // v0.81 (storage track): `store <name>: <Kind>[…] [= init]`. `store` is a
    // contextual keyword (also a valid identifier — e.g. a `cache.store`
    // context); the `word` directive resolves it by context.
    store_field: ($) =>
      seq(
        "store",
        field("name", $.identifier),
        ":",
        field("kind", $.store_kind),
        repeat(field("annotation", $.store_annotation)),
        optional(seq("=", field("init", $._expression))),
      ),
    store_kind: ($) =>
      seq(
        field("head", $.identifier),
        optional(seq("[", sep1($._type_ref, ","), "]")),
      ),

    // v0.85 (storage track; ADR 0111): `@<name>(<args>)` field annotations.
    // `@` appears only here; the name is an ordinary identifier (the checker
    // matches the closed registry). An argument is an optional `label:` then a
    // value expression — `by: orderId`, `5.minutes`.
    store_annotation: ($) =>
      seq(
        "@",
        field("name", $.identifier),
        optional(
          seq("(", optional(sep1($.annotation_arg, ",")), optional(","), ")"),
        ),
      ),
    annotation_arg: ($) =>
      seq(
        optional(seq(field("label", $.identifier), ":")),
        field("value", $._expression),
      ),

    // v0.5 `on call`; v0.44 `on GET("path")` / `on schedule("expr")` /
    // `on message(...)` — the protocol lives on the service header.
    handler: ($) =>
      choice(
        $.call_handler,
        $.http_handler,
        $.cron_handler,
        $.queue_handler,
        // v0.103/v0.106: WebSocket lifecycle handlers. `on message` reuses the
        // queue-handler shape (the protocol on the service header disambiguates).
        $.ws_open_handler,
        $.ws_close_handler,
      ),
    call_handler: ($) =>
      seq(
        "on",
        "call",
        optional(field("method", $.identifier)),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    // v0.44: `on GET("/path") (params) -> …` — the method-builder collapses
    // verb+route into one config expression in the handler-config slot.
    http_handler: ($) =>
      seq(
        "on",
        field("method", $.http_method),
        "(",
        field("path", $.string_literal),
        ")",
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    http_method: () => choice("GET", "POST", "PUT", "PATCH", "DELETE"),
    // v0.44: `on schedule("<expr>") (at: Int?) -> Effect[Result[(), E]] { … }`.
    cron_handler: ($) =>
      seq(
        "on",
        "schedule",
        "(",
        field("schedule", $.string_literal),
        ")",
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    // v0.44: `on message(m: T) -> Effect[QueueResult] { … }`. The queue binding
    // lives on the service header (`from queue("name")`).
    queue_handler: ($) =>
      seq(
        "on",
        "message",
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    // v0.103/v0.106: `on open` / `on close` lifecycle handlers of a
    // `from WebSocket` service. `open`/`close` are contextual (resolved by
    // `word`). `on message` (the inbound-frame handler) reuses queue_handler.
    ws_open_handler: ($) =>
      seq(
        "on",
        "open",
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    ws_close_handler: ($) =>
      seq(
        "on",
        "close",
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("by", $.by_clause)),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    given_clause: ($) =>
      // v0.15: a capability may be a bare local name or a dotted cross-context
      // reference (`B.Cap` / `platform.time.Clock`).
      seq("given", sep1(field("capability", $.qualified_name), ",")),

    // v0.45: an actor declaration — a boundary contract. Normal form
    // `actor Name { auth = Scheme (, identity = Type)? }`; the reserved
    // refinement form `actor Admin = Base where <predicate>` is parsed (and
    // rejected by the checker in Foundations).
    actor_decl: ($) =>
      seq(
        "actor",
        field("name", $.identifier),
        choice(
          seq(
            "{",
            "auth",
            "=",
            field("scheme", $.scheme),
            // v0.47: the Bearer scheme takes a `(secret = "<env>")` config.
            // v0.51: generalised to keyed args — `Scheme(key = value, …)` with
            // string or integer values (Signature: secret/header/timestamp +
            // integer tolerance).
            optional(field("config", $.scheme_config)),
            optional(seq(",", "identity", "=", field("identity", $._type_ref))),
            "}",
          ),
          seq(
            "=",
            field("base", $.identifier),
            "where",
            field("predicate", $.refinement),
          ),
        ),
      ),
    // The closed, compiler-known scheme set. `None`/`Internal` are zero-crypto;
    // `Bearer` (JWT/HS256), `Signature` (HMAC over the body), and `Oidc`
    // (JWKS/RS256+ES256) are compiler-generated verifiers.
    scheme: () => choice("None", "Internal", "Bearer", "Signature", "Oidc"),
    // v0.51: the keyed-args scheme config — `(key = value, …)`. Values are
    // string or integer literals; the checker validates which keys each scheme
    // admits (Bearer: secret; Signature: secret/header/timestamp/tolerance).
    scheme_config: ($) =>
      seq(
        "(",
        sep1($.scheme_arg, ","),
        ")",
      ),
    scheme_arg: ($) =>
      seq(
        field("key", $.identifier),
        "=",
        field("value", choice($.string_literal, $.number_literal)),
      ),
    // v0.45: the handler `by <binder>: <Actor>` clause — sits after the protocol
    // config and before the parameters.
    // v0.50: the binder is optional — `by <Actor>` (verify, don't capture the
    // identity) or `by <binder>: <Actor>` (capture it).
    // v0.52: the actor position is a `|`-separated list. One name is the
    // ordinary single-actor handler; more than one is an ordered sum of peer
    // actors (`by who: User | Visitor`), resolved first-wins.
    by_clause: ($) =>
      seq(
        "by",
        optional(seq(field("binder", $.identifier), ":")),
        field("actor", $.identifier),
        repeat(seq("|", field("actor", $.identifier))),
      ),

    // -- v0.7: test bodies --

    // v0.118 (testing track slice 6): a test-scope stub for one capability
    // operation — `provides Cap.op(<arg pattern>, …) <rhs>`. Distinguished from
    // the production provider declaration (`provides Cap = Impl …`) by the
    // `.op(` shape, and only admitted inside a suite/case body. An arg pattern
    // is `_` (any) or a value expression; the right-hand side is
    // `returns <expr>`, `returns each [ <outcome>, … ]` (a scripted sequence,
    // where an outcome is `fails` or an expr), or `fails`. `returns`/`each`/
    // `fails` are contextual words.
    provides_clause: ($) =>
      seq(
        "provides",
        field("cap", $.identifier),
        ".",
        field("op", $.identifier),
        "(",
        optional(
          seq(
            sep1(field("arg", choice(alias("_", $.wildcard), $._expression)), ","),
            optional(","),
          ),
        ),
        ")",
        choice(
          seq(
            "returns",
            "each",
            "[",
            optional(
              seq(
                sep1(field("outcome", choice("fails", $._expression)), ","),
                optional(","),
              ),
            ),
            "]",
          ),
          seq("returns", field("value", $._expression)),
          "fails",
        ),
      ),
    // v0.118: an optional `as <tier>` classifier plus a body of leading
    // `provides` stubs, then the ordinary statements and tail. The tier words
    // (`unit`/`integration`/`system`) are contextual, as on `suite_decl`.
    case: ($) =>
      seq(
        "case",
        field("name", $.string_literal),
        optional(seq("as", field("tier", choice("unit", "integration", "system")))),
        "{",
        repeat($.provides_clause),
        repeat($._statement),
        optional(field("tail", $._expression)),
        "}",
      ),

    // v0.114 (testing track slice 2): a generative `property` — its body is a
    // single `for all` binder over generated inhabitants of each binding's type.
    property_decl: ($) =>
      seq(
        "property",
        field("name", $.string_literal),
        "{",
        field("forall", $.for_all),
        "}",
      ),

    // `for all x: T, y: U where <pred> { … expect … }` — the generated bindings,
    // an optional `where` filter, and the predicate body.
    for_all: ($) =>
      seq(
        "for",
        "all",
        $.for_all_binding,
        repeat(seq(",", $.for_all_binding)),
        optional(seq("where", field("filter", $._expression))),
        field("body", $.block),
      ),

    for_all_binding: ($) =>
      seq(field("name", $.identifier), ":", field("type", $._type_ref)),

    // -- Block & statements --

    // A block is a run of statements ending in a tail expression (the block's
    // value). The tail is optional because a test body may end in a statement
    // such as a bare `assert`.
    block: ($) =>
      seq("{", repeat($._statement), optional(field("tail", $._expression)), "}"),

    // `assert` is an expression (v0.9.1) but also appears in statement
    // position within a test body (an expression-statement of type `()`).
    _statement: ($) =>
      choice(
        $.let_stmt,
        $.effect_let_stmt,
        $.effect_send_stmt,
        $.do_stmt,
        $.assign_stmt,
        prec(1, $.expect_expr),
      ),

    let_stmt: ($) =>
      seq(
        "let",
        field("name", $._binding_name),
        optional(seq(":", field("type", $._type_ref))),
        "=",
        field("value", $._expression),
      ),
    effect_let_stmt: ($) =>
      seq(
        "let",
        field("name", $._binding_name),
        optional(seq(":", field("type", $._type_ref))),
        "<-",
        field("value", $._expression),
      ),
    // v0.79: `~> expr` — an asynchronous fire-and-forget send (no binder).
    effect_send_stmt: ($) => seq("~>", field("value", $._expression)),
    // v0.146 (ADR 0170): `do expr` — perform a unit effect as a statement
    // (the binder-free `let _ <- expr` for an `Effect[()]`).
    do_stmt: ($) => seq("do", field("value", $._expression)),
    // v0.81 (storage track): `name := expr` — a `Cell` store write.
    assign_stmt: ($) =>
      seq(field("target", $.identifier), ":=", field("value", $._expression)),

    // A let/effect-let may bind the discard name `_`.
    _binding_name: ($) => choice($.identifier, alias("_", $.wildcard)),

    // -- Expressions --

    _expression: ($) =>
      choice(
        $.if_expr,
        $.match_expr,
        $.is_expr,
        $.expect_expr,
        $.binary_expr,
        $.unary_expr,
        $._primary,
      ),

    // v0.9.1: `assert` is an expression of type `()`.
    // v0.117 (testing track slice 5): `expect` also admits an observation over a
    // capability seam (`expect Cap.op called …`), sitting ahead of the ordinary
    // predicate. The subject `Cap.op` overlaps a `field_access`, so the matcher
    // words disambiguate contextually.
    expect_expr: ($) =>
      prec.right(
        PREC.expect,
        seq("expect", field("cond", choice($.observation_expr, $._expression))),
      ),

    // v0.117: an observation — a `Cap.op` seam reference followed by one of the
    // sugar matchers. `called` / `never` / `once` / `times` / `with` / `before`
    // are contextual (state-lexed as keywords only here; ordinary identifiers
    // elsewhere), mirroring the compiler's lookahead parse (DECISION F3).
    observation_expr: ($) =>
      seq(
        field("cap", $.identifier),
        ".",
        field("op", $.identifier),
        choice(
          seq(
            "called",
            // An optional call-count matcher: `once` or `<n> times`.
            optional(choice("once", seq(field("count", $.number_literal), "times"))),
            optional(seq("with", field("predicate", $._expression))),
          ),
          seq("never", "called"),
          seq(
            "before",
            field("other_cap", $.identifier),
            ".",
            field("other_op", $.identifier),
          ),
        ),
      ),

    // v0.117: `trace(Cap.op)` — the escape hatch yielding the recorded calls as a
    // `List` of per-op records. A test-only builtin recognised by its
    // `trace ( Ident . Ident )` shape.
    trace_expr: ($) =>
      prec(
        PREC.postfix + 1,
        seq(
          "trace",
          "(",
          field("cap", $.identifier),
          ".",
          field("op", $.identifier),
          ")",
        ),
      ),

    // v0.146 (ADR 0170): `else` is optional. A missing `else` defaults to a
    // unit branch (legal only for a unit then-branch, enforced by the checker).
    if_expr: ($) =>
      seq(
        "if",
        field("cond", $._expression),
        field("then", $.block),
        optional(seq("else", field("else", choice($.if_expr, $.block)))),
      ),

    match_expr: ($) =>
      seq(
        "match",
        field("disc", $._expression),
        "{",
        repeat($.match_arm),
        "}",
      ),
    // Arms are newline-separated; a trailing comma is permitted but not
    // required.
    match_arm: ($) =>
      prec.right(
        seq(
          field("pattern", $._pattern),
          // v0.145 (ADR 0169): an optional trailing `if <Bool-expr>` guard,
          // between the pattern and `=>`, gating the arm over its bindings. The
          // `if` reuses the existing keyword token (already highlighted).
          optional(seq("if", field("guard", $._expression))),
          "=>",
          // `_expression` already includes `block` via `_primary`, so a
          // `{ … }` arm body is covered without a separate alternative.
          field("body", $._expression),
          optional(","),
        ),
      ),

    _pattern: ($) =>
      choice($.wildcard_pattern, $.literal_pattern, $.variant_pattern),
    wildcard_pattern: () => "_",
    // v0.130 §2.3.4: a literal pattern — an integer (optionally negated), a
    // string, or a boolean. A closed set (ADR 0001); no `Float`, no `()`.
    literal_pattern: ($) =>
      choice(
        seq(optional("-"), $.number_literal),
        $.string_literal,
        $.boolean_literal,
      ),
    // The variant name is an `identifier`, not `constant_name`: because
    // `word` unifies all word-shaped lexemes to `identifier`, a `constant_name`
    // here can never out-lex `identifier` after `is`/`=>`, which previously
    // mis-parsed `x is Miss` as a `type.` prefix awaiting a dot. Capitalisation
    // is recovered in the highlight query, not the grammar.
    variant_pattern: ($) =>
      prec.right(
        seq(
          optional(seq(field("type", $.identifier), ".")),
          field("variant", $.identifier),
          optional(
            seq(
              "(",
              optional(sep1($._pattern_binding, ",")),
              optional(","),
              ")",
            ),
          ),
        ),
      ),
    // v0.145 (ADR 0169, decision B): a variant payload matches its field against
    // a full sub-`_pattern` (one recursion point), so `Some(Ok(x))` /
    // `Err(PollClosed)` parse as nested `variant_pattern` nodes rather than flat
    // bindings. A bare lowercase identifier is a payload-less `variant_pattern`
    // (a binding, recovered in the highlight query by capitalisation); `_` is a
    // `wildcard_pattern`; an uppercase-led one discriminates a nested variant.
    _pattern_binding: ($) =>
      choice(
        $.named_binding,
        $._pattern,
      ),
    named_binding: ($) =>
      seq(field("field", $.identifier), ":", field("name", $._pattern)),

    is_expr: ($) =>
      prec.left(
        PREC.is,
        seq(field("value", $._expression), "is", field("pattern", $._pattern)),
      ),

    binary_expr: ($) =>
      choice(
        // v0.80: `P implies Q` — right-associative, lowest precedence.
        prec.right(PREC.implies, seq($._expression, "implies", $._expression)),
        prec.left(PREC.or, seq($._expression, "||", $._expression)),
        prec.left(PREC.and, seq($._expression, "&&", $._expression)),
        prec.left(PREC.cmp, seq($._expression, choice("==", "!="), $._expression)),
        prec.left(
          PREC.rel,
          seq($._expression, choice("<", "<=", ">", ">="), $._expression),
        ),
        prec.left(PREC.add, seq($._expression, choice("+", "-"), $._expression)),
        prec.left(PREC.mul, seq($._expression, choice("*", "/"), $._expression)),
      ),

    unary_expr: ($) =>
      prec.right(PREC.unary, seq(choice("!", "-"), $._expression)),

    _primary: ($) =>
      choice(
        $.lambda_expr,
        $.paren_expr,
        $.method_call,
        $.field_access,
        $.call,
        $.record_construction,
        $.record_spread,
        $.question_expr,
        $.ok_expr,
        $.err_expr,
        $.some_expr,
        $.none_expr,
        $.effect_pure_expr,
        $.val_expr,
        $.trace_expr,
        $.list_literal,
        $.block,
        $.number_literal,
        $.float_literal,
        $.string_literal,
        $.boolean_literal,
        $.unit_literal,
        $.self_expr,
        $.identifier,
      ),

    // v0.20a: a lambda — `(params) => expr | block`. Conflicts with
    // paren_expr/unit_literal until the `=>` disambiguates (GLR conflict
    // declared below).
    lambda_expr: ($) =>
      seq(
        "(",
        optional(sep1($.lambda_param, ",")),
        ")",
        "=>",
        field("body", choice($._expression, $.block)),
      ),

    lambda_param: ($) =>
      seq(field("name", $.identifier), optional(seq(":", field("type", $._type_ref)))),

    paren_expr: ($) => seq("(", $._expression, ")"),

    // Higher precedence than `field_access`: when a `(` follows `recv.name`,
    // shift into the call; when it doesn't, fall back to field access. This is
    // what lets `Reps.of(n)` be a call while `p.x` is a field access.
    // `method_call` also subsumes the old `constructor_call` (`Type.method(…)`).
    method_call: ($) =>
      prec.left(
        PREC.postfix + 1,
        seq(
          field("receiver", $._primary),
          ".",
          field("method", $.identifier),
          // v0.22b: optional explicit type arguments on a qualified static —
          // `Json.decode[T](…)` (same-line rule as `call` type application).
          optional(seq("[", sep1(field("type_arg", $._type_ref), ","), "]")),
          "(",
          optional(sep1(field("arg", $._expression), ",")),
          optional(","),
          ")",
        ),
      ),

    field_access: ($) =>
      prec.left(
        PREC.postfix,
        seq(field("receiver", $._primary), ".", field("field", $.identifier)),
      ),

    call: ($) =>
      prec.left(
        PREC.postfix,
        seq(
          field("name", $.identifier),
          // v0.20a: optional explicit type arguments — `name[T, U](…)`.
          optional(seq("[", sep1(field("type_arg", $._type_ref), ","), "]")),
          "(",
          optional(sep1(field("arg", $._expression), ",")),
          optional(","),
          ")",
        ),
      ),

    // v0.20b: a list literal — `[a, b, c]`, optional trailing comma. A
    // *leading* `[` only; type application stays a postfix form on a callee
    // identifier (see `call`).
    list_literal: ($) =>
      seq(
        "[",
        optional(seq(sep1(field("element", $._expression), ","), optional(","))),
        "]",
      ),

    record_construction: ($) =>
      seq(
        field("type", $.identifier),
        "{",
        optional(sep1($.field_init, ",")),
        optional(","),
        "}",
      ),
    field_init: ($) =>
      choice(
        seq(field("name", $.identifier), ":", field("value", $._expression)),
        field("shorthand", $.identifier),
      ),

    record_spread: ($) =>
      choice(
        seq(
          field("type", $.identifier),
          "{",
          "...",
          field("base", $._expression),
          repeat(seq(",", $.field_init)),
          optional(","),
          "}",
        ),
        seq(
          "{",
          "...",
          field("base", $._expression),
          repeat(seq(",", $.field_init)),
          optional(","),
          "}",
        ),
      ),

    question_expr: ($) =>
      prec.left(PREC.postfix, seq($._expression, "?")),

    ok_expr: ($) => seq("Ok", "(", $._expression, ")"),
    err_expr: ($) => seq("Err", "(", $._expression, ")"),
    some_expr: ($) => seq("Some", "(", $._expression, ")"),
    none_expr: () => "None",
    effect_pure_expr: ($) =>
      seq("Effect", ".", "pure", "(", $._expression, ")"),

    // v0.114 (was `Mock[T]`, v0.9.4): `Val[T]` test-context value fabrication.
    // The `[ … ]` here is the bracket syntax otherwise reserved for generics; in
    // expression position it carries the fabricated type. The optional argument
    // is either a parenthesised literal-/variant-pin or a brace record-override
    // (the latter reuses `field_init`, identical to record construction). The
    // test-context restriction is semantic and left to the LSP.
    val_expr: ($) =>
      prec.right(
        seq(
          "Val",
          "[",
          field("type", $._type_ref),
          "]",
          optional(field("arg", $.val_arg)),
        ),
      ),
    val_arg: ($) =>
      choice(
        seq("(", sep1(field("pin", $._expression), ","), optional(","), ")"),
        seq("{", optional(sep1($.field_init, ",")), optional(","), "}"),
      ),

    self_expr: () => "self",

    // -- Tokens --

    identifier: () => /[A-Za-z][A-Za-z0-9_]*/,

    // Constant-style names (capitalised idents used as variant names).
    // Tree-sitter cannot enforce capitalisation at the lex level without
    // overshadowing `identifier`; we use a regex.
    constant_name: () => /[A-Z][A-Za-z0-9_]*/,

    number_literal: () => /[0-9]+(_[0-9]+)*/,
    // v0.21: a float literal — fraction with a digit on both sides of the
    // `.`, an exponent, or both. Longest-match keeps `1.toFloat()` lexing
    // as a method call on an integer literal.
    float_literal: () =>
      /[0-9]+(_[0-9]+)*\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?|[0-9]+(_[0-9]+)*[eE][+-]?[0-9]+(_[0-9]+)*/,
    string_literal: ($) =>
      seq(
        '"',
        repeat(choice(/[^"\\\n]/, /\\[nt"\\]/, $.string_interpolation)),
        '"',
      ),
    // v0.43: an interpolation hole `\(expr)`. A bare `\(` was an invalid
    // escape, so this is backward-compatible; `\\(` is an escaped backslash
    // followed by a literal `(`, so it is not a hole.
    string_interpolation: ($) => seq('\\(', $._expression, ')'),
    boolean_literal: () => choice("true", "false"),
    unit_literal: () => seq("(", ")"),

    // `--` line comment. The external scanner consumes `---+`-starting
    // lines as `doc_block` tokens before the regex tokenizer runs, so
    // line_comment sees only `--` (or `-` chars followed by a non-third
    // dash sequence that's already been gobbled by the external scanner).
    line_comment: () => token(seq("--", /[^\n]*/)),

    // `doc_block` is provided by the external scanner — see src/scanner.c.
    // The grammar references it via the `externals` array above.
  },
});

function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}
