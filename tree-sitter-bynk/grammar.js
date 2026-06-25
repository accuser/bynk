/**
 * @file Tree-sitter grammar for Bynk.
 *
 * Covers the syntactic surface defined by the normative specification
 * (docs/src/spec/, §3–§4), which is generated from this grammar and kept
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
  assert: 0,
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
    // `match e { … }` / `if e { … }` / `commit e`: after a bare identifier `e`,
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
            $.integration_decl,
            $.test_decl,
          ),
        ),
        repeat1($._item_fragment),
        $._expr_fragment,
      ),

    _item_fragment: ($) =>
      choice($._context_body_item, $.handler, $.state_decl, $.key_decl),

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
    test_decl: ($) =>
      prec.right(
        seq(
          "test",
          field("target", $.qualified_name),
          choice(
            seq("{", repeat($._test_body_item), "}"),
            repeat($._test_body_item),
          ),
        ),
      ),

    // v0.16: a multi-Worker integration test. `integration` is contextual
    // after `test` and before the suite-name string; `wires` lists the
    // participating contexts. Body holds `uses` and `test "name"` cases (no
    // `mocks` — integration tests wire real implementations).
    integration_decl: ($) =>
      prec.right(
        seq(
          "test",
          "integration",
          field("name", $.string_literal),
          choice(
            seq("{", $.wires_decl, repeat($._integration_body_item), "}"),
            seq($.wires_decl, repeat($._integration_body_item)),
          ),
        ),
      ),

    wires_decl: ($) => seq("wires", sep1(field("participant", $.qualified_name), ",")),

    _integration_body_item: ($) => choice($.uses_decl, $.test_case),

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
      choice($.uses_decl, $.consumes_decl, $.mocks_decl, $.test_case),

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
        // `state` fields; the checker restricts where it applies.
        optional(seq("=", field("init", $._expression))),
      ),

    sum_type: ($) => prec.right(repeat1($.sum_variant)),
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
    base_type: () => choice("Int", "String", "Bool", "Float"),

    _type_ref: ($) =>
      choice(
        $.function_type_ref,
        $._base_type,
        $.unit_type,
        $.validation_error_type,
        $.generic_type_ref,
        $.identifier,
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
        field("body", $.block),
      ),
    method_name: ($) =>
      seq(field("type", $.identifier), ".", field("method", $.identifier)),

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
        "{",
        repeat($.handler),
        "}",
      ),
    // v0.44: `from <protocol>` on the service header.
    service_protocol: ($) =>
      seq(
        "from",
        choice(
          "http",
          "cron",
          seq("queue", "(", field("name", $.string_literal), ")"),
        ),
      ),

    agent_decl: ($) =>
      seq(
        "agent",
        field("name", $.identifier),
        "{",
        field("key", $.key_decl),
        // v0.81 (storage track): the legacy `state { }` block and the successor
        // `store` fields coexist (ADR 0108 D3), so both are optional/repeatable.
        optional(field("state", $.state_decl)),
        repeat($.store_field),
        repeat($.invariant_decl),
        repeat($.handler),
        "}",
      ),
    // v0.80: an agent invariant — `invariant <name>: <predicate>`. Sits between
    // the state block and the handlers.
    invariant_decl: ($) =>
      seq(
        "invariant",
        field("name", $.identifier),
        ":",
        field("predicate", $._expression),
      ),
    key_decl: ($) =>
      seq("key", field("name", $.identifier), ":", field("type", $._type_ref)),
    state_decl: ($) =>
      seq(
        "state",
        "{",
        optional(sep1($.record_field, ",")),
        optional(","),
        "}",
      ),
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
      choice($.call_handler, $.http_handler, $.cron_handler, $.queue_handler),
    call_handler: ($) =>
      seq(
        "on",
        "call",
        optional(field("method", $.identifier)),
        optional(field("by", $.by_clause)),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
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
        optional(field("by", $.by_clause)),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
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
        optional(field("by", $.by_clause)),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
        optional(field("given", $.given_clause)),
        field("body", $.block),
      ),
    // v0.44: `on message(m: T) -> Effect[QueueResult] { … }`. The queue binding
    // lives on the service header (`from queue("name")`).
    queue_handler: ($) =>
      seq(
        "on",
        "message",
        optional(field("by", $.by_clause)),
        "(",
        optional(sep1($.param, ",")),
        optional(","),
        ")",
        "->",
        field("return_type", $._type_ref),
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
    // The closed, compiler-known scheme set. `None`/`Internal` are admitted in
    // Foundations; `Bearer`/`Signature` are reserved-and-rejected.
    scheme: () => choice("None", "Internal", "Bearer", "Signature"),
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

    mocks_decl: ($) =>
      seq(
        "mocks",
        field("capability", $.identifier),
        "=",
        field("impl", $.identifier),
        "{",
        repeat($.provider_op),
        "}",
      ),
    test_case: ($) =>
      seq("test", field("name", $.string_literal), field("body", $.block)),

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
        $.commit_stmt,
        $.assign_stmt,
        prec(1, $.assert_expr),
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
    commit_stmt: ($) => seq("commit", field("value", $._expression)),
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
        $.assert_expr,
        $.binary_expr,
        $.unary_expr,
        $._primary,
      ),

    // v0.9.1: `assert` is an expression of type `()`.
    assert_expr: ($) =>
      prec.right(PREC.assert, seq("assert", field("cond", $._expression))),

    if_expr: ($) =>
      seq(
        "if",
        field("cond", $._expression),
        field("then", $.block),
        "else",
        field("else", choice($.if_expr, $.block)),
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
          "=>",
          // `_expression` already includes `block` via `_primary`, so a
          // `{ … }` arm body is covered without a separate alternative.
          field("body", $._expression),
          optional(","),
        ),
      ),

    _pattern: ($) => choice($.wildcard_pattern, $.variant_pattern),
    wildcard_pattern: () => "_",
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
    _pattern_binding: ($) =>
      choice(
        $.named_binding,
        $.positional_binding,
      ),
    named_binding: ($) =>
      seq(field("field", $.identifier), ":", field("name", choice($.identifier, "_"))),
    positional_binding: ($) => choice($.identifier, "_"),

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
        $.mock_expr,
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

    // v0.9.4 Part B: `Mock[T]` test-context construction. The `[ … ]` here is
    // the bracket syntax otherwise reserved for generics; in expression
    // position it carries the mocked type. The optional argument is either a
    // parenthesised literal-/variant-pin or a brace record-override (the latter
    // reuses `field_init`, identical to record construction). The test-context
    // restriction is semantic and left to the LSP.
    mock_expr: ($) =>
      prec.right(
        seq(
          "Mock",
          "[",
          field("type", $._type_ref),
          "]",
          optional(field("arg", $.mock_arg)),
        ),
      ),
    mock_arg: ($) =>
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

    number_literal: () => /[0-9]+/,
    // v0.21: a float literal — fraction with a digit on both sides of the
    // `.`, an exponent, or both. Longest-match keeps `1.toFloat()` lexing
    // as a method call on an integer literal.
    float_literal: () =>
      /[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+/,
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
