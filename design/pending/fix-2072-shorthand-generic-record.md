---
level: minor
changelog: A refined literal in a field of a generic record passed to a cross-context call (`Page { items: [250] }` for a `Page[Cents]` parameter), or in a record literal that also has a shorthand field, is admitted against the callee's own declaration, as other nested literals are, so it crosses from a caller with its own type of that name; a shorthand field is checked by its binding's type
---
