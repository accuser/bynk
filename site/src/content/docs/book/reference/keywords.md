---
title: Keywords
---
<!-- GENERATED FILE — do not edit by hand.
     Source: bynkc/src/keywords.rs (`render_markdown`).
     Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test keywords_reference -->

Every reserved keyword, with a one-line description. Reserved words cannot be used as identifiers.

There are **64** reserved keywords.

| Keyword | Meaning |
|---|---|
| `Bool` | The boolean base type. |
| `Bytes` | The binary base type — an immutable octet sequence, erased to `Uint8Array` (`Bytes.fromUtf8(s)`). |
| `Duration` | The time-span base type, in milliseconds (`5.minutes`). |
| `Effect` | The effectful-computation type, `Effect[T]`. |
| `Err` | The error variant of `Result`. |
| `Float` | The floating-point base type. |
| `Instant` | The absolute-time base type, in epoch milliseconds (`Clock.now()`). |
| `Int` | The integer base type. |
| `JsonError` | The JSON-decode error type, `Result[T, JsonError]` from `Json.decode`. |
| `None` | The empty variant of `Option`. |
| `Ok` | The success variant of `Result`. |
| `Option` | The optional-value type, `Option[T]`. |
| `Result` | The success-or-error type, `Result[T, E]`. |
| `Some` | The present variant of `Option`. |
| `String` | The string base type. |
| `ValidationError` | The error type returned by a refined type's `.of`. |
| `actor` | Declare an actor — a boundary contract a handler consumes via `by`. |
| `adapter` | Declare an adapter — the host boundary (capability contract + binding). |
| `agent` | Declare a stateful, keyed agent inside a context. |
| `and` | Combine refinement predicates (`where A and B`). |
| `as` | Alias a consumed context (`consumes X as Y`). |
| `binding` | Name an adapter's TypeScript binding module (`binding "<module>"`). |
| `by` | Name the actor a handler consumes, after the return type — or a service-level default on the header (`… -> T by <name>: <Actor>`). |
| `capability` | Declare a capability (a dependency interface) in a context. |
| `case` | Declare a test case inside a `suite` (`case "…" { … }`). |
| `commons` | Declare a pure, stateless module of types and functions. |
| `consumes` | Declare a dependency on another context's services. |
| `context` | Declare a deployable context (services, agents, capabilities). |
| `cron` | The cron protocol on a service header (`from cron`). |
| `do` | Perform a unit effect as a statement (`do e` — the binder-free `let _ <- e`). |
| `else` | The alternative branch of an `if` expression. |
| `ensures` | Declare a function postcondition — a pure `Bool` clause over the parameters and `result` (`ensures <name>: <pred>`). |
| `enum` | Declare a payloadless sum type (`enum { A, B }`). |
| `expect` | Assert a predicate inside a test case (`expect <bool-predicate>`). |
| `exports` | Declare which types a context exposes, and how. |
| `false` | The boolean literal `false`. |
| `fn` | Declare a function. |
| `from` | Name the protocol a service conforms to (`service X from http`). |
| `given` | Declare the capabilities a handler requires. |
| `http` | The HTTP protocol on a service header (`from http`). |
| `if` | A conditional expression. |
| `implies` | Logical implication (`P implies Q` ≡ `!P || Q`), used in invariant predicates. |
| `invariant` | Declare an agent invariant — a predicate that must hold of every committed state. |
| `is` | Test a value against a variant pattern, yielding a `Bool`. |
| `let` | Bind a local value (`let x = …`, or `let x <- …` for an effect). |
| `match` | Pattern-match over a sum type, `Result`, or `Option`. |
| `on` | Begin a handler declaration (`on call`, `on GET(…)`, `on message`, `on open`/`on close`). |
| `opaque` | Declare an opaque type, or export a type opaquely. |
| `property` | Declare a generative test inside a `suite` (`property "…" { for all … }`). |
| `protocol` | Reserved keyword (protocols are a closed, compiler-known set). |
| `provides` | Provide an implementation of a capability. |
| `queue` | The queue protocol on a service header (`from queue("name")`). |
| `record` | Reserved keyword (records are written `type X = { … }`). |
| `requires` | Declare a function precondition — a pure `Bool` clause over the parameters (`requires <name>: <pred>`). |
| `self` | The current agent instance, inside a handler. |
| `service` | Declare a service (a group of handlers) in a context. |
| `stub` | Stub a consumed capability operation at a test seam (`stub Cap.op(…) returns <v>` / `fails`). |
| `suite` | Declare a test suite targeting a unit (`suite <target> { case … }`). |
| `transition` | Declare an agent step invariant over the `old`/`new` state pair (`transition <name>: …`). |
| `transparent` | Export a type with its structure visible (`exports transparent { … }`). |
| `true` | The boolean literal `true`. |
| `type` | Declare a type: alias, record, sum, opaque, or refined. |
| `uses` | Bring a commons into scope. |
| `where` | Attach refinement predicates to a base type. |
