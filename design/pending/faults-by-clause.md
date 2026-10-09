---
level: minor
changelog: "**A fault claim carries a call-site principal** ([#1812](https://github.com/accuser/bynk/issues/1812)). `expect api.POST(\"/orders\", order) by User(\"bob\") faults` drives an identity-carrying handler as `let r <- … by User(\"bob\")` does, so its fault path is testable; the principal is checked against the addressed handler (an absent `by` is still `bynk.test.principal_required`) and lowered around the claimed call. A fault claim driven `by Nobody` is `bynk.test.faults_needs_in_process` at any tier. Grammar (`faults_expr`), formatter and tree-sitter updated."
---
