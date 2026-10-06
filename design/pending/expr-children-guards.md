---
level: minor
changelog: "Checks now see inside a `match` arm's guard. The shared walk over an expression's sub-expressions skipped guards, so a guard escaped every check built on it: a `:=` whose right side reads the written cell only in a guard is now `bynk.cell.self_reference`, and constructing another context's type in a guard is now `bynk.context.external_construction`, so code that compiled before can now fail. The same walk now also sees a call-site principal's identity (`by User(who)`). The editor's extract-to-function threads a name read only in a guard or an identity as a parameter (#1760)"
---
