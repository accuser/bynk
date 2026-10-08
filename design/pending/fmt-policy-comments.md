---
level: patch
changelog: "`fmt` keeps comments inside a `cors`, `security` or `limits` policy (#1786): above a field, at the end of a field's line, before the closing `}` and after it. It used to refuse such a file with `bynk.fmt.comment_loss`, and before that deleted the comments silently. A `---` block inside a policy is kept as an orphan, and `bynk check` now warns `bynk.parse.orphan_doc_block` on it instead of dropping it with no warning"
---
