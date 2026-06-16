# Keywords

<!-- GENERATED FILE — do not edit by hand.
     Source: karnc/src/keywords.rs (`render_markdown`).
     Regenerate with: KARN_BLESS=1 cargo test -p karnc --test keywords_reference -->

Every reserved keyword, with a one-line description. Reserved words cannot be used as identifiers.

There are **55** reserved keywords.

| Keyword | Meaning |
|---|---|
| `Bool` | The boolean base type. |
| `Effect` | The effectful-computation type, `Effect[T]`. |
| `Err` | The error variant of `Result`. |
| `Float` | The floating-point base type. |
| `Int` | The integer base type. |
| `JsonError` | The JSON-decode error type, `Result[T, JsonError]` from `Json.decode`. |
| `None` | The empty variant of `Option`. |
| `Ok` | The success variant of `Result`. |
| `Option` | The optional-value type, `Option[T]`. |
| `Result` | The success-or-error type, `Result[T, E]`. |
| `Some` | The present variant of `Option`. |
| `String` | The string base type. |
| `ValidationError` | The error type returned by a refined type's `.of`. |
| `adapter` | Declare an adapter — the host boundary (capability contract + binding). |
| `agent` | Declare a stateful, keyed agent inside a context. |
| `and` | Combine refinement predicates (`where A and B`). |
| `as` | Alias a consumed context (`consumes X as Y`). |
| `assert` | Assert a condition inside a test case. |
| `binding` | Name an adapter's TypeScript binding module (`binding "<module>"`). |
| `capability` | Declare a capability (a dependency interface) in a context. |
| `commit` | Persist new agent state from within a handler. |
| `commons` | Declare a pure, stateless module of types and functions. |
| `consumes` | Declare a dependency on another context's services. |
| `context` | Declare a deployable context (services, agents, capabilities). |
| `cron` | The cron protocol on a service header (`from cron`). |
| `else` | The alternative branch of an `if` expression. |
| `enum` | Declare a payloadless sum type (`enum { A, B }`). |
| `expect` | Reserved keyword. |
| `exports` | Declare which types a context exposes, and how. |
| `false` | The boolean literal `false`. |
| `fn` | Declare a function. |
| `from` | Name the protocol a service conforms to (`service X from http`). |
| `given` | Declare the capabilities a handler requires. |
| `http` | The HTTP protocol on a service header (`from http`). |
| `if` | A conditional expression. |
| `is` | Test a value against a variant pattern, yielding a `Bool`. |
| `let` | Bind a local value (`let x = …`, or `let x <- …` for an effect). |
| `match` | Pattern-match over a sum type, `Result`, or `Option`. |
| `mocks` | Provide a mock capability implementation in a test. |
| `on` | Begin a handler declaration (`on call`, `on GET(…)`, `on message`). |
| `opaque` | Declare an opaque type, or export a type opaquely. |
| `protocol` | Reserved keyword (protocols are a closed, compiler-known set). |
| `provides` | Provide an implementation of a capability. |
| `queue` | The queue protocol on a service header (`from queue("name")`). |
| `record` | Reserved keyword (records are written `type X = { … }`). |
| `self` | The current agent instance, inside a handler. |
| `service` | Declare a service (a group of handlers) in a context. |
| `state` | Declare an agent's persistent state block. |
| `test` | Declare a test block or a test case. |
| `transparent` | Export a type with its structure visible (`exports transparent { … }`). |
| `true` | The boolean literal `true`. |
| `type` | Declare a type: alias, record, sum, opaque, or refined. |
| `uses` | Bring a commons into scope. |
| `where` | Attach refinement predicates to a base type. |
| `wires` | List the contexts a `test integration` stands up as Workers. |
