---
level: patch
changelog: A test-scaffold module's `Json.decode[T]`/`Json.encode` on a named record now generates its own `serialise_*`/`deserialise_*` closure and namespace-qualifies the type, instead of emitting a call to a codec the unit never exports and a bare type name the module never declares
---
