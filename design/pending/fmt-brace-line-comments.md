---
level: patch
changelog: "`fmt` keeps a `--` comment on the same line as an opening `{` (#1788), moving it onto its own line under the brace. It used to refuse such a file with `bynk.fmt.comment_loss`; now the comment is kept on a unit, service, agent, capability, provider or adapter body, a policy, a `fn` block and a record type. Comments inside a record `type` (or `event`) body, and above or at the end of an agent's `key` line, are kept too; those were refused wherever they sat"
---
