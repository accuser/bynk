---
level: minor
changelog: A refined literal nested in a cross-context argument, in an `Option`/`Result` payload, a list element or a record field (`t.pay.maybe(Some(250))`), is admitted against the callee's own declaration, as a direct literal is, so it crosses from a caller with no `uses` of the declaring commons or with its own type of that name; an empty `[]`, `None`, `List.empty()` or `Map.empty()` passed positionally takes its parameter's type and crosses from such a caller too
---
