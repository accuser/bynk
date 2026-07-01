; Bynk highlighting queries.
;
; Maps grammar nodes to standard tree-sitter highlighting groups. See
; design/bynk-tree-sitter-spec.md §2 for the group inventory.

; -- Keywords --

[
  "if"
  "else"
  "let"
  "match"
  "is"
  "expect"
  "on"
  "given"
  "call"
  "from"
  "http"
  "cron"
  "queue"
  "schedule"
  "message"
] @keyword

[
  "uses"
  "consumes"
  "as"
] @keyword.import

[
  "commons"
  "context"
  "type"
  "fn"
  "capability"
  "provides"
  "service"
  "agent"
  "store"
  "exports"
  "key"
  "suite"
  "case"
  "mocks"
  "integration"
  "wires"
  "invariant"
] @keyword.declaration

[
  "opaque"
  "transparent"
] @keyword.modifier

[
  "where"
  "and"
  "implies"
  "enum"
] @keyword.operator

; HTTP method on `on http METHOD "path"` handlers.
(http_method) @keyword

; -- Types --

(base_type) @type.builtin
(validation_error_type) @type.builtin

(builtin_type) @type.builtin

; User-defined type names appear in type positions.
(type_decl name: (identifier) @type)
(opaque_type) @type
(generic_type_ref arg: (identifier) @type)
(record_field type: (identifier) @type)
(variant_payload_field type: (identifier) @type)
(param type: (identifier) @type)
(method_name type: (identifier) @type)
(record_construction type: (identifier) @type)
(record_spread type: (identifier) @type)
(variant_pattern type: (identifier) @type)
(key_decl type: (identifier) @type)

; -- Functions --

(fn_decl name: (identifier) @function)
(fn_decl name: (method_name method: (identifier) @function.method))
(call name: (identifier) @function)
(method_call method: (identifier) @function.method)
(capability_op name: (identifier) @function.method)
(provider_op name: (identifier) @function.method)
(call_handler method: (identifier) @function.method)

["Ok" "Err" "Some"] @function.builtin
(none_expr) @constant.builtin
(effect_pure_expr "Effect" @type.builtin
                  "pure" @function.builtin)
(mock_expr "Mock" @function.builtin)

; -- Variables / parameters --

(param name: (identifier) @variable.parameter)
(let_stmt name: (identifier) @variable)
(effect_let_stmt name: (identifier) @variable)
(self_expr) @variable.builtin
(self_param) @variable.builtin
(key_decl name: (identifier) @variable.parameter)

; -- Fields --

(record_field name: (identifier) @field)
(variant_payload_field name: (identifier) @field)
(field_access field: (identifier) @field)
(field_init name: (identifier) @field)
(field_init shorthand: (identifier) @field)
(named_binding field: (identifier) @field)
(positional_binding (identifier) @variable)

; -- Variants & constants --

; Sum/enum variant declarations keep the dedicated `constant_name` node.
(constant_name) @constant
; Variant names in patterns are plain identifiers; treat a capitalised
; pattern variant as a constant.
((variant_pattern variant: (identifier) @constant)
 (#match? @constant "^[A-Z]"))
(boolean_literal) @constant.builtin

; -- Literals --

(string_literal) @string
(number_literal) @number
(float_literal) @number
(unit_literal) @constant.builtin

; -- Refinement predicates --

(predicate_name) @attribute

; -- Module references --

(qualified_name (identifier) @module)
(uses_decl target: (qualified_name) @module)
(consumes_decl target: (qualified_name) @module)
(suite_decl target: (qualified_name) @module)

; -- Operators & punctuation --

[
  "+" "-" "*" "/"
  "==" "!=" "<" "<=" ">" ">="
  "&&" "||" "!"
  "<-" "~>" ":=" "->" "=" "?"
] @operator

[
  "(" ")" "{" "}" "[" "]"
] @punctuation.bracket

[
  "," ":" "|"
] @punctuation.delimiter

[
  "=>" "..."
] @punctuation.special

; -- Comments --

(line_comment) @comment
(doc_block) @comment.documentation

; -- Errors (tree-sitter recovery) --

(ERROR) @error
