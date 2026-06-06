# Grammar

<!-- GENERATED FILE — do not edit by hand.
     Source: tree-sitter-karn/src/grammar.json, via karnc/tests/grammar_reference.rs.
     Regenerate with: KARN_BLESS=1 cargo test -p karnc --test grammar_reference -->

The complete Karn grammar, generated from the `tree-sitter-karn` grammar.

**Notation.** `"x"` a literal token · `/x/` a regular expression · `( … )?` optional · `( … )*` zero or more · `( … )+` one or more · `a | b` choice · `ε` empty. Rule names beginning with `_` are internal helper rules (inlined into the syntax tree). `doc_block` is an external token — a `--- … ---` documentation block.

```ebnf
source_file ::= (commons_decl | context_decl | test_decl)+ | _item_fragment+ | _expr_fragment
_item_fragment ::= _context_body_item | handler | state_decl | key_decl
_expr_fragment ::= _statement+ _expression? | _expression
commons_decl ::= "commons" qualified_name ("{" _commons_body_item* "}" | _commons_body_item*)
context_decl ::= "context" qualified_name ("{" _context_body_item* "}" | _context_body_item*)
test_decl ::= "test" qualified_name ("{" _test_body_item* "}" | _test_body_item*)
qualified_name ::= identifier ("." identifier)*
_commons_body_item ::= uses_decl | type_decl | fn_decl | capability_decl | provider_decl | service_decl | agent_decl
_context_body_item ::= uses_decl | consumes_decl | exports_decl | type_decl | fn_decl | capability_decl | provider_decl | service_decl | agent_decl
_test_body_item ::= uses_decl | consumes_decl | mocks_decl | test_case
uses_decl ::= "uses" qualified_name
consumes_decl ::= "consumes" qualified_name ("as" identifier)?
exports_decl ::= "exports" ("opaque" | "transparent") "{" (identifier ("," identifier)*)? ","? "}"
type_decl ::= "type" identifier "=" _type_body
_type_body ::= opaque_type | refined_type | record_type | sum_type | enum_type
opaque_type ::= "opaque" _base_type ("where" refinement)?
refined_type ::= _base_type ("where" refinement)?
record_type ::= "{" (record_field ("," record_field)*)? ","? "}"
record_field ::= identifier ":" _type_ref ("where" refinement)?
sum_type ::= sum_variant+
sum_variant ::= "|" constant_name ("(" (variant_payload_field ("," variant_payload_field)*)? ","? ")")?
variant_payload_field ::= identifier ":" _type_ref
enum_type ::= "enum" "{" (constant_name ("," constant_name)*)? ","? "}"
refinement ::= _refinement_pred ("and" _refinement_pred)*
_refinement_pred ::= pred_call | pred_atom
pred_call ::= predicate_name "(" (_pred_arg ("," _pred_arg)*)? ")"
pred_atom ::= predicate_name
predicate_name ::= "Matches" | "InRange" | "MinLength" | "MaxLength" | "Length" | "NonNegative" | "Positive" | "NonEmpty"
_pred_arg ::= number_literal | string_literal
_base_type ::= base_type
base_type ::= "Int" | "String" | "Bool"
_type_ref ::= _base_type | unit_type | validation_error_type | generic_type_ref | identifier
unit_type ::= "(" ")"
validation_error_type ::= "ValidationError"
generic_type_ref ::= ("Result" | "Option" | "Effect" | "HttpResult") "[" _type_ref ("," _type_ref)* "]"
fn_decl ::= "fn" (method_name | identifier) "(" _params? ")" "->" _type_ref block
method_name ::= identifier "." identifier
_params ::= (self_param | param) ("," param)* ","?
self_param ::= "self"
param ::= identifier ":" _type_ref
capability_decl ::= "capability" identifier "{" capability_op* "}"
capability_op ::= "fn" identifier "(" (param ("," param)*)? ","? ")" "->" _type_ref
provider_decl ::= "provides" identifier "=" identifier "{" provider_op* "}"
provider_op ::= "fn" identifier "(" (param ("," param)*)? ","? ")" "->" _type_ref block
service_decl ::= "service" identifier "{" handler* "}"
agent_decl ::= "agent" identifier "{" key_decl state_decl handler* "}"
key_decl ::= "key" identifier ":" _type_ref
state_decl ::= "state" "{" (record_field ("," record_field)*)? ","? "}"
handler ::= call_handler | http_handler | cron_handler
call_handler ::= "on" "call" identifier? "(" (param ("," param)*)? ","? ")" "->" _type_ref given_clause? block
http_handler ::= "on" "http" http_method string_literal "(" (param ("," param)*)? ","? ")" "->" _type_ref given_clause? block
http_method ::= "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
cron_handler ::= "on" "cron" string_literal "(" (param ("," param)*)? ","? ")" "->" _type_ref given_clause? block
given_clause ::= "given" identifier ("," identifier)*
mocks_decl ::= "mocks" identifier "=" identifier "{" provider_op* "}"
test_case ::= "test" string_literal block
block ::= "{" _statement* _expression? "}"
_statement ::= let_stmt | effect_let_stmt | commit_stmt | assert_expr
let_stmt ::= "let" _binding_name (":" _type_ref)? "=" _expression
effect_let_stmt ::= "let" _binding_name (":" _type_ref)? "<-" _expression
commit_stmt ::= "commit" _expression
_binding_name ::= identifier | "_"
_expression ::= if_expr | match_expr | is_expr | assert_expr | binary_expr | unary_expr | _primary
assert_expr ::= "assert" _expression
if_expr ::= "if" _expression block "else" (if_expr | block)
match_expr ::= "match" _expression "{" match_arm* "}"
match_arm ::= _pattern "=>" _expression ","?
_pattern ::= wildcard_pattern | variant_pattern
wildcard_pattern ::= "_"
variant_pattern ::= (identifier ".")? identifier ("(" (_pattern_binding ("," _pattern_binding)*)? ","? ")")?
_pattern_binding ::= named_binding | positional_binding
named_binding ::= identifier ":" (identifier | "_")
positional_binding ::= identifier | "_"
is_expr ::= _expression "is" _pattern
binary_expr ::= _expression "||" _expression | _expression "&&" _expression | _expression ("==" | "!=") _expression | _expression ("<" | "<=" | ">" | ">=") _expression | _expression ("+" | "-") _expression | _expression ("*" | "/") _expression
unary_expr ::= ("!" | "-") _expression
_primary ::= paren_expr | method_call | field_access | call | record_construction | record_spread | question_expr | ok_expr | err_expr | some_expr | none_expr | effect_pure_expr | mock_expr | block | number_literal | string_literal | boolean_literal | unit_literal | self_expr | identifier
paren_expr ::= "(" _expression ")"
method_call ::= _primary "." identifier "(" (_expression ("," _expression)*)? ","? ")"
field_access ::= _primary "." identifier
call ::= identifier "(" (_expression ("," _expression)*)? ","? ")"
record_construction ::= identifier "{" (field_init ("," field_init)*)? ","? "}"
field_init ::= identifier ":" _expression | identifier
record_spread ::= identifier "{" "..." _expression ("," field_init)* ","? "}" | "{" "..." _expression ("," field_init)* ","? "}"
question_expr ::= _expression "?"
ok_expr ::= "Ok" "(" _expression ")"
err_expr ::= "Err" "(" _expression ")"
some_expr ::= "Some" "(" _expression ")"
none_expr ::= "None"
effect_pure_expr ::= "Effect" "." "pure" "(" _expression ")"
mock_expr ::= "Mock" "[" _type_ref "]" mock_arg?
mock_arg ::= "(" _expression ("," _expression)* ","? ")" | "{" (field_init ("," field_init)*)? ","? "}"
self_expr ::= "self"
identifier ::= /[A-Za-z][A-Za-z0-9_]*/
constant_name ::= /[A-Z][A-Za-z0-9_]*/
number_literal ::= /[0-9]+/
string_literal ::= """ (/[^"\\\n]/ | /\\[nt"\\]/)* """
boolean_literal ::= "true" | "false"
unit_literal ::= "(" ")"
line_comment ::= "--" /[^\n]*/
```

## Tokens & trivia

- **Word token:** `identifier`
- **Ignored between tokens:** `/\s+/`, `line_comment`, `doc_block`
- **External tokens:** `doc_block`
