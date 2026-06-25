# Complete grammar (appendix)

<!-- GENERATED FILE — do not edit by hand.
     Source: tree-sitter-bynk/src/grammar.json, via bynkc/tests/grammar_reference.rs.
     Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test grammar_reference -->

The complete Bynk grammar, generated from the `tree-sitter-bynk` grammar. For the annotated, per-construct reference see [Syntax & grammar](grammar.md).

**Notation.** `"x"` a literal token · `/x/` a regular expression · `( … )?` optional · `( … )*` zero or more · `( … )+` one or more · `a | b` choice · `ε` empty. Rule names are the readable display names (a leading `_` denotes an internal helper rule; trivial wrappers are collapsed). `doc_block` is an external token — a `--- … ---` documentation block.

```ebnf
source_file ::= (commons_decl | context_decl | adapter_decl | integration_decl | test_decl)+ | item_fragment+ | expr_fragment
item_fragment ::= context_body_item | handler | state_decl | key_decl
expr_fragment ::= statement+ expression? | expression
commons_decl ::= "commons" qualified_name ("{" commons_body_item* "}" | commons_body_item*)
context_decl ::= "context" qualified_name ("{" context_body_item* "}" | context_body_item*)
adapter_decl ::= "adapter" qualified_name ("{" adapter_body_item* "}" | adapter_body_item*)
test_decl ::= "test" qualified_name ("{" test_body_item* "}" | test_body_item*)
integration_decl ::= "test" "integration" string_literal ("{" wires_decl integration_body_item* "}" | wires_decl integration_body_item*)
wires_decl ::= "wires" qualified_name ("," qualified_name)*
integration_body_item ::= uses_decl | test_case
qualified_name ::= identifier ("." identifier)*
commons_body_item ::= uses_decl | type_decl | fn_decl | capability_decl | provider_decl | service_decl | agent_decl | actor_decl
context_body_item ::= uses_decl | consumes_decl | exports_decl | type_decl | fn_decl | capability_decl | provider_decl | service_decl | agent_decl | actor_decl
adapter_body_item ::= binding_decl | uses_decl | consumes_decl | exports_decl | type_decl | fn_decl | capability_decl | provider_decl | service_decl | agent_decl | actor_decl
test_body_item ::= uses_decl | consumes_decl | mocks_decl | test_case
uses_decl ::= "uses" qualified_name
consumes_decl ::= "consumes" qualified_name ("as" identifier | "{" (identifier ("," identifier)*)? ","? "}")?
binding_decl ::= "binding" string_literal ("requires" "{" (binding_requirement ("," binding_requirement)*)? ","? "}")?
binding_requirement ::= string_literal ":" string_literal
exports_decl ::= "exports" ("opaque" | "transparent" | "capability") "{" (identifier ("," identifier)*)? ","? "}"
type_decl ::= "type" identifier "=" type_body
type_body ::= opaque_type | refined_type | record_type | sum_type | enum_type
opaque_type ::= "opaque" base_type ("where" refinement)?
refined_type ::= base_type ("where" refinement)?
record_type ::= "{" (record_field ("," record_field)*)? ","? "}"
record_field ::= identifier ":" type_ref ("where" refinement)? ("=" expression)?
sum_type ::= sum_variant+
sum_variant ::= "|" constant_name ("(" (variant_payload_field ("," variant_payload_field)*)? ","? ")")?
variant_payload_field ::= identifier ":" type_ref
enum_type ::= "enum" "{" (constant_name ("," constant_name)*)? ","? "}"
refinement ::= refinement_pred ("and" refinement_pred)*
refinement_pred ::= pred_call | predicate_name
pred_call ::= predicate_name "(" (pred_arg ("," pred_arg)*)? ")"
predicate_name ::= "Matches" | "InRange" | "MinLength" | "MaxLength" | "Length" | "NonNegative" | "Positive" | "NonEmpty"
pred_arg ::= number_literal | float_literal | string_literal
base_type ::= "Int" | "String" | "Bool" | "Float"
type_ref ::= function_type_ref | base_type | unit_type | validation_error_type | generic_type_ref | identifier
function_type_ref ::= (base_type | unit_type | validation_error_type | generic_type_ref | identifier | "(" type_ref ("," type_ref)* ","? ")") "->" type_ref
unit_type ::= "(" ")"
validation_error_type ::= "ValidationError"
generic_type_ref ::= ("Result" | "Option" | "Effect" | "HttpResult" | "List" | "Map") "[" type_ref ("," type_ref)* "]"
fn_decl ::= "fn" (method_name | identifier) ("[" identifier ("," identifier)* "]")? "(" params? ")" "->" type_ref block
method_name ::= identifier "." identifier
params ::= (self_param | param) ("," param)* ","?
self_param ::= "self"
param ::= identifier ":" type_ref
capability_decl ::= "capability" identifier "{" capability_op* "}"
capability_op ::= "fn" identifier "(" (param ("," param)*)? ","? ")" "->" type_ref
provider_decl ::= "provides" identifier "=" identifier given_clause? ("{" provider_op* "}")?
provider_op ::= "fn" identifier "(" (param ("," param)*)? ","? ")" "->" type_ref block
service_decl ::= "service" identifier service_protocol? "{" handler* "}"
service_protocol ::= "from" ("http" | "cron" | "queue" "(" string_literal ")")
agent_decl ::= "agent" identifier "{" key_decl state_decl? store_field* invariant_decl* handler* "}"
invariant_decl ::= "invariant" identifier ":" expression
key_decl ::= "key" identifier ":" type_ref
state_decl ::= "state" "{" (record_field ("," record_field)*)? ","? "}"
store_field ::= "store" identifier ":" store_kind ("=" expression)?
store_kind ::= identifier ("[" type_ref ("," type_ref)* "]")?
handler ::= call_handler | http_handler | cron_handler | queue_handler
call_handler ::= "on" "call" identifier? by_clause? "(" (param ("," param)*)? ","? ")" "->" type_ref given_clause? block
http_handler ::= "on" http_method "(" string_literal ")" by_clause? "(" (param ("," param)*)? ","? ")" "->" type_ref given_clause? block
http_method ::= "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
cron_handler ::= "on" "schedule" "(" string_literal ")" by_clause? "(" (param ("," param)*)? ","? ")" "->" type_ref given_clause? block
queue_handler ::= "on" "message" by_clause? "(" (param ("," param)*)? ","? ")" "->" type_ref given_clause? block
given_clause ::= "given" qualified_name ("," qualified_name)*
actor_decl ::= "actor" identifier ("{" "auth" "=" scheme scheme_config? ("," "identity" "=" type_ref)? "}" | "=" identifier "where" refinement)
scheme ::= "None" | "Internal" | "Bearer" | "Signature"
scheme_config ::= "(" scheme_arg ("," scheme_arg)* ")"
scheme_arg ::= identifier "=" (string_literal | number_literal)
by_clause ::= "by" (identifier ":")? identifier ("|" identifier)*
mocks_decl ::= "mocks" identifier "=" identifier "{" provider_op* "}"
test_case ::= "test" string_literal block
block ::= "{" statement* expression? "}"
statement ::= let_stmt | effect_let_stmt | effect_send_stmt | commit_stmt | assign_stmt | assert_expr
let_stmt ::= "let" binding_name (":" type_ref)? "=" expression
effect_let_stmt ::= "let" binding_name (":" type_ref)? "<-" expression
effect_send_stmt ::= "~>" expression
commit_stmt ::= "commit" expression
assign_stmt ::= identifier ":=" expression
binding_name ::= identifier | "_"
expression ::= if_expr | match_expr | is_expr | assert_expr | binary_expr | unary_expr | primary
assert_expr ::= "assert" expression
if_expr ::= "if" expression block "else" (if_expr | block)
match_expr ::= "match" expression "{" match_arm* "}"
match_arm ::= pattern "=>" expression ","?
pattern ::= wildcard_pattern | variant_pattern
wildcard_pattern ::= "_"
variant_pattern ::= (identifier ".")? identifier ("(" (pattern_binding ("," pattern_binding)*)? ","? ")")?
pattern_binding ::= named_binding | positional_binding
named_binding ::= identifier ":" (identifier | "_")
positional_binding ::= identifier | "_"
is_expr ::= expression "is" pattern
binary_expr ::= expression "implies" expression | expression "||" expression | expression "&&" expression | expression ("==" | "!=") expression | expression ("<" | "<=" | ">" | ">=") expression | expression ("+" | "-") expression | expression ("*" | "/") expression
unary_expr ::= ("!" | "-") expression
primary ::= lambda_expr | paren_expr | method_call | field_access | call | record_construction | record_spread | question_expr | ok_expr | err_expr | some_expr | none_expr | effect_pure_expr | mock_expr | list_literal | block | number_literal | float_literal | string_literal | boolean_literal | unit_literal | self_expr | identifier
lambda_expr ::= "(" (lambda_param ("," lambda_param)*)? ")" "=>" (expression | block)
lambda_param ::= identifier (":" type_ref)?
paren_expr ::= "(" expression ")"
method_call ::= primary "." identifier ("[" type_ref ("," type_ref)* "]")? "(" (expression ("," expression)*)? ","? ")"
field_access ::= primary "." identifier
call ::= identifier ("[" type_ref ("," type_ref)* "]")? "(" (expression ("," expression)*)? ","? ")"
list_literal ::= "[" (expression ("," expression)* ","?)? "]"
record_construction ::= identifier "{" (field_init ("," field_init)*)? ","? "}"
field_init ::= identifier ":" expression | identifier
record_spread ::= identifier "{" "..." expression ("," field_init)* ","? "}" | "{" "..." expression ("," field_init)* ","? "}"
question_expr ::= expression "?"
ok_expr ::= "Ok" "(" expression ")"
err_expr ::= "Err" "(" expression ")"
some_expr ::= "Some" "(" expression ")"
none_expr ::= "None"
effect_pure_expr ::= "Effect" "." "pure" "(" expression ")"
mock_expr ::= "Mock" "[" type_ref "]" mock_arg?
mock_arg ::= "(" expression ("," expression)* ","? ")" | "{" (field_init ("," field_init)*)? ","? "}"
self_expr ::= "self"
identifier ::= /[A-Za-z][A-Za-z0-9_]*/
constant_name ::= /[A-Z][A-Za-z0-9_]*/
number_literal ::= /[0-9]+/
float_literal ::= /[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+/
string_literal ::= """ (/[^"\\\n]/ | /\\[nt"\\]/ | string_interpolation)* """
string_interpolation ::= "\(" expression ")"
boolean_literal ::= "true" | "false"
unit_literal ::= "(" ")"
line_comment ::= "--" /[^\n]*/
```

## Tokens & trivia

- **Word token:** `identifier`
- **Ignored between tokens:** `/\s+/`, `line_comment`, `doc_block`
- **External tokens:** `doc_block`
